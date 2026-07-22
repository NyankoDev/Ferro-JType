use std::collections::BTreeMap;

use ferro_babe::model::{
    ConstantPoolIndex, ConstantRef, InstructionOperand, LdcValueRef, MemberReference,
};
use ferro_babe::{Class, Disassembler, RecoveryMode};
use rust_asm::class_reader::{AttributeInfo, read_class_file};
use rust_asm::constant_pool::CpInfo;

use crate::ir::{
    ClassIr, ConstantKind, ExceptionHandlerIr, InstructionIr, InstructionOperandIr, MemberRefIr,
    MethodIr, strip_stack_map_tables,
};
use crate::{
    ClassName, DescriptorError, Diagnostic, DiagnosticKind, DiagnosticLocation, DiagnosticSeverity,
    DynamicCallKind, Error, GenericSignature, MethodDescriptor, TypeDescriptor,
};

pub(crate) fn parse_and_lower(bytes: &[u8]) -> Result<ClassIr, Error> {
    match strip_stack_map_tables(bytes) {
        Ok(sanitized) => {
            parse_lowered_bytes(&sanitized, false).or_else(|_| parse_lowered_bytes(bytes, true))
        }
        Err(_) => parse_lowered_bytes(bytes, true),
    }
}

fn parse_lowered_bytes(bytes: &[u8], used_recovery: bool) -> Result<ClassIr, Error> {
    let generic_metadata = extract_generic_metadata(bytes);
    let disassembler = Disassembler::builder()
        .recovery(RecoveryMode::BestEffort)
        .build();
    let disassembly = disassembler.parse(bytes)?;
    let class = disassembly.class().ok_or(Error::IncompleteClass)?;
    let mut class = lower_class(class, &generic_metadata)?;
    if used_recovery {
        class.diagnostics.push(Diagnostic::new(
            DiagnosticSeverity::Warning,
            DiagnosticKind::ParserRecovery,
            DiagnosticLocation::class_level(),
            "StackMapTable sanitization could not be used; analyzed original bytes with best-effort recovery",
        ));
    }
    Ok(class)
}

fn lower_class(class: &Class, generic_metadata: &GenericMetadata) -> Result<ClassIr, Error> {
    let name = parse_class_name(class.name())?;
    let methods = class
        .methods()
        .map(|method| {
            let generic_signature = generic_metadata
                .method_signatures
                .get(&(method.name().to_owned(), method.descriptor().to_owned()))
                .cloned();
            lower_method(class, method, generic_signature, generic_metadata)
        })
        .collect::<Result<Vec<_>, _>>()?;

    Ok(ClassIr {
        name,
        access_flags: class.access_flags(),
        generic_signature: generic_metadata.class_signature.clone(),
        diagnostics: Vec::new(),
        methods,
    })
}

fn lower_method(
    class: &Class,
    method: ferro_babe::model::Method<'_>,
    generic_signature: Option<GenericSignature>,
    metadata: &GenericMetadata,
) -> Result<MethodIr, Error> {
    let descriptor = MethodDescriptor::parse(method.descriptor())?;
    let instructions = method
        .instructions()
        .map(|instructions| {
            instructions
                .map(|instruction| lower_instruction(class, instruction, metadata))
                .collect::<Result<Vec<_>, _>>()
        })
        .transpose()?
        .unwrap_or_default();
    let exception_handlers = method
        .exception_handlers()
        .map(|handler| {
            Ok(ExceptionHandlerIr {
                start_offset: handler.start().get(),
                end_offset: handler.end().get(),
                handler_offset: handler.handler().get(),
                catch_type: handler.catch_type().map(parse_class_name).transpose()?,
            })
        })
        .collect::<Result<Vec<_>, Error>>()?;
    Ok(MethodIr {
        name: method.name().to_owned(),
        descriptor_text: method.descriptor().to_owned(),
        descriptor,
        generic_signature,
        access_flags: method.access_flags(),
        max_stack: method.max_stack(),
        max_locals: method.max_locals(),
        instructions,
        exception_handlers,
    })
}

#[derive(Default)]
struct GenericMetadata {
    class_signature: Option<GenericSignature>,
    method_signatures: BTreeMap<(String, String), GenericSignature>,
    dynamic_call_kinds: BTreeMap<u16, DynamicCallKind>,
}

fn extract_generic_metadata(bytes: &[u8]) -> GenericMetadata {
    let Ok(class_file) = read_class_file(bytes) else {
        return GenericMetadata::default();
    };
    let class_signature = signature_from_attributes(&class_file, &class_file.attributes);
    let method_signatures = class_file
        .methods
        .iter()
        .filter_map(|method| {
            let signature = signature_from_attributes(&class_file, &method.attributes)?;
            let name = class_file.cp_utf8(method.name_index).ok()?.to_owned();
            let descriptor = class_file.cp_utf8(method.descriptor_index).ok()?.to_owned();
            Some(((name, descriptor), signature))
        })
        .collect();
    let dynamic_call_kinds = dynamic_call_kinds(&class_file);
    GenericMetadata {
        class_signature,
        method_signatures,
        dynamic_call_kinds,
    }
}

fn signature_from_attributes(
    class_file: &rust_asm::class_reader::ClassFile,
    attributes: &[AttributeInfo],
) -> Option<GenericSignature> {
    let AttributeInfo::Signature { signature_index } = attributes
        .iter()
        .find(|attribute| matches!(attribute, AttributeInfo::Signature { .. }))?
    else {
        return None;
    };
    class_file
        .cp_utf8(*signature_index)
        .ok()
        .map(str::to_owned)
        .map(GenericSignature::new)
}

fn dynamic_call_kinds(
    class_file: &rust_asm::class_reader::ClassFile,
) -> BTreeMap<u16, DynamicCallKind> {
    class_file
        .constant_pool
        .iter()
        .enumerate()
        .filter_map(|(index, constant)| {
            let CpInfo::InvokeDynamic {
                bootstrap_method_attr_index,
                ..
            } = constant
            else {
                return None;
            };
            let kind = bootstrap_kind(class_file, *bootstrap_method_attr_index)
                .unwrap_or(DynamicCallKind::OtherBootstrap);
            Some((u16::try_from(index).ok()?, kind))
        })
        .collect()
}

fn bootstrap_kind(
    class_file: &rust_asm::class_reader::ClassFile,
    bootstrap_index: u16,
) -> Option<DynamicCallKind> {
    let methods = class_file
        .attributes
        .iter()
        .find_map(|attribute| match attribute {
            AttributeInfo::BootstrapMethods { methods } => Some(methods),
            _ => None,
        })?;
    let bootstrap = methods.get(usize::from(bootstrap_index))?;
    let CpInfo::MethodHandle {
        reference_index, ..
    } = class_file
        .constant_pool
        .get(usize::from(bootstrap.bootstrap_method_ref))?
    else {
        return None;
    };
    let (class_index, name_and_type_index) = match class_file
        .constant_pool
        .get(usize::from(*reference_index))?
    {
        CpInfo::Methodref {
            class_index,
            name_and_type_index,
        }
        | CpInfo::InterfaceMethodref {
            class_index,
            name_and_type_index,
        } => (*class_index, *name_and_type_index),
        _ => return None,
    };
    let owner = class_file.class_name(class_index).ok()?;
    let CpInfo::NameAndType { name_index, .. } = class_file
        .constant_pool
        .get(usize::from(name_and_type_index))?
    else {
        return None;
    };
    let name = class_file.cp_utf8(*name_index).ok()?;
    match (owner, name) {
        ("java/lang/invoke/LambdaMetafactory", "metafactory" | "altMetafactory") => {
            Some(DynamicCallKind::LambdaMetafactory)
        }
        ("java/lang/invoke/StringConcatFactory", "makeConcat" | "makeConcatWithConstants") => {
            Some(DynamicCallKind::StringConcatFactory)
        }
        _ => Some(DynamicCallKind::OtherBootstrap),
    }
}

fn lower_instruction(
    class: &Class,
    instruction: ferro_babe::model::Instruction<'_>,
    metadata: &GenericMetadata,
) -> Result<InstructionIr, Error> {
    let operand = match instruction.operand() {
        InstructionOperand::None => InstructionOperandIr::None,
        InstructionOperand::Immediate(value) => InstructionOperandIr::Immediate(value),
        InstructionOperand::Local(local) => InstructionOperandIr::Local(local),
        InstructionOperand::ConstantPool(index) if is_type_operand(instruction.opcode()) => {
            InstructionOperandIr::Type {
                type_name: resolve_class_name_text(class, index),
                constant_pool_index: index.get(),
            }
        }
        InstructionOperand::ConstantPool(index) => InstructionOperandIr::ConstantPool(index.get()),
        InstructionOperand::Member(reference) => {
            InstructionOperandIr::Member(lower_member_reference(class, reference)?)
        }
        InstructionOperand::InvokeInterface { method, count } => {
            InstructionOperandIr::InvokeInterface {
                method: resolve_member_reference(class, method),
                count,
            }
        }
        InstructionOperand::InvokeDynamic { call_site } => InstructionOperandIr::InvokeDynamic {
            descriptor: resolve_dynamic_descriptor(class, call_site),
            constant_pool_index: call_site.get(),
            kind: metadata
                .dynamic_call_kinds
                .get(&call_site.get())
                .copied()
                .unwrap_or(DynamicCallKind::OtherBootstrap),
        },
        InstructionOperand::Branch { target, .. } => InstructionOperandIr::Branch { target },
        InstructionOperand::Ldc(value) => {
            InstructionOperandIr::Constant(lower_constant(class, value))
        }
        InstructionOperand::Increment { local, amount } => {
            InstructionOperandIr::Increment { local, amount }
        }
        InstructionOperand::TableSwitch {
            default, targets, ..
        } => InstructionOperandIr::TableSwitch {
            default_target: default.target(),
            targets: targets
                .iter()
                .map(|relative| i32::from(instruction.offset().get()) + relative)
                .collect(),
        },
        InstructionOperand::LookupSwitch { default, pairs, .. } => {
            InstructionOperandIr::LookupSwitch {
                default_target: default.target(),
                targets: pairs
                    .iter()
                    .map(|(key, relative)| (*key, i32::from(instruction.offset().get()) + relative))
                    .collect(),
            }
        }
        InstructionOperand::MultiArray {
            class: class_index,
            dimensions,
        } => InstructionOperandIr::MultiArray {
            type_name: resolve_class_name_text(class, class_index),
            dimensions,
            constant_pool_index: class_index.get(),
        },
    };

    Ok(InstructionIr {
        offset: instruction.offset().get(),
        opcode: instruction.opcode(),
        operand,
    })
}

fn lower_member_reference(
    class: &Class,
    reference: MemberReference<'_>,
) -> Result<MemberRefIr, Error> {
    match reference {
        MemberReference::ConstantPool(index) => Ok(resolve_member_reference(class, index)),
        MemberReference::Symbolic {
            owner,
            name,
            descriptor,
        } => Ok(MemberRefIr::Resolved {
            owner: parse_class_name(owner)?,
            name: name.to_owned(),
            descriptor: descriptor.to_owned(),
        }),
    }
}

fn resolve_member_reference(class: &Class, index: ConstantPoolIndex) -> MemberRefIr {
    let Some((class_index, name_and_type_index)) = member_indices(class.constant(index)) else {
        return MemberRefIr::Unresolved {
            constant_pool_index: index.get(),
        };
    };
    let Some(owner) = resolve_class_name(class, class_index) else {
        return MemberRefIr::Unresolved {
            constant_pool_index: index.get(),
        };
    };
    let Some((name, descriptor)) = resolve_name_and_type(class, name_and_type_index) else {
        return MemberRefIr::Unresolved {
            constant_pool_index: index.get(),
        };
    };

    MemberRefIr::Resolved {
        owner,
        name,
        descriptor,
    }
}

fn member_indices(
    reference: Option<ConstantRef<'_>>,
) -> Option<(ConstantPoolIndex, ConstantPoolIndex)> {
    match reference? {
        ConstantRef::FieldReference {
            class,
            name_and_type,
        }
        | ConstantRef::MethodReference {
            class,
            name_and_type,
        }
        | ConstantRef::InterfaceMethodReference {
            class,
            name_and_type,
        } => Some((class, name_and_type)),
        _ => None,
    }
}

fn resolve_dynamic_descriptor(class: &Class, index: ConstantPoolIndex) -> Option<String> {
    let ConstantRef::InvokeDynamic { name_and_type, .. } = class.constant(index)? else {
        return None;
    };
    let (_, descriptor) = resolve_name_and_type(class, name_and_type)?;
    Some(descriptor)
}

fn resolve_name_and_type(class: &Class, index: ConstantPoolIndex) -> Option<(String, String)> {
    let ConstantRef::NameAndType { name, descriptor } = class.constant(index)? else {
        return None;
    };
    Some((
        resolve_utf8(class, name)?.to_owned(),
        resolve_utf8(class, descriptor)?.to_owned(),
    ))
}

fn resolve_class_name(class: &Class, index: ConstantPoolIndex) -> Option<ClassName> {
    ClassName::parse(resolve_class_name_text(class, index)?).ok()
}

fn resolve_class_name_text(class: &Class, index: ConstantPoolIndex) -> Option<String> {
    let ConstantRef::Class { name } = class.constant(index)? else {
        return None;
    };
    Some(resolve_utf8(class, name)?.to_owned())
}

fn resolve_utf8(class: &Class, index: ConstantPoolIndex) -> Option<&str> {
    let ConstantRef::Utf8(value) = class.constant(index)? else {
        return None;
    };
    Some(value)
}

fn lower_constant(class: &Class, value: LdcValueRef<'_>) -> ConstantKind {
    match value {
        LdcValueRef::Integer(_) => ConstantKind::Integer,
        LdcValueRef::Float(_) => ConstantKind::Float,
        LdcValueRef::Long(_) => ConstantKind::Long,
        LdcValueRef::Double(_) => ConstantKind::Double,
        LdcValueRef::String(_) => ConstantKind::String,
        LdcValueRef::TypeDescriptor => ConstantKind::Type,
        LdcValueRef::ConstantPool(index) => lower_constant_pool(class, index),
    }
}

fn lower_constant_pool(class: &Class, index: ConstantPoolIndex) -> ConstantKind {
    match class.constant(index) {
        Some(ConstantRef::Integer(_)) => ConstantKind::Integer,
        Some(ConstantRef::Float(_)) => ConstantKind::Float,
        Some(ConstantRef::Long(_)) => ConstantKind::Long,
        Some(ConstantRef::Double(_)) => ConstantKind::Double,
        Some(ConstantRef::String { .. }) => ConstantKind::String,
        Some(ConstantRef::Class { .. }) => ConstantKind::Type,
        Some(ConstantRef::MethodHandle { .. }) => ConstantKind::MethodHandle,
        Some(ConstantRef::MethodType { .. }) => ConstantKind::MethodType,
        Some(ConstantRef::Dynamic { name_and_type, .. }) => {
            resolve_name_and_type(class, name_and_type)
                .and_then(|(_, descriptor)| TypeDescriptor::parse(&descriptor).ok())
                .map(ConstantKind::Dynamic)
                .unwrap_or(ConstantKind::Unresolved)
        }
        Some(
            ConstantRef::Unusable
            | ConstantRef::Utf8(_)
            | ConstantRef::FieldReference { .. }
            | ConstantRef::MethodReference { .. }
            | ConstantRef::InterfaceMethodReference { .. }
            | ConstantRef::NameAndType { .. }
            | ConstantRef::InvokeDynamic { .. }
            | ConstantRef::Module { .. }
            | ConstantRef::Package { .. },
        )
        | None => ConstantKind::Unresolved,
    }
}

fn parse_class_name(value: &str) -> Result<ClassName, Error> {
    ClassName::parse(value)
        .map_err(|error| Error::Descriptor(DescriptorError::InvalidClassName(error)))
}

const fn is_type_operand(opcode: u8) -> bool {
    matches!(opcode, 0xbb | 0xbd | 0xc0 | 0xc1)
}
