use std::collections::BTreeMap;

use ferro_babe::model::{
    ConstantPoolIndex, ConstantRef, InstructionOperand, LdcValueRef, MemberReference,
    StackMapFrameKind, VerificationType,
};
use ferro_babe::{Class, Disassembler, RecoveryMode};

use crate::ir::{
    ClassIr, ConstantKind, ExceptionHandlerIr, InstructionIr, InstructionOperandIr, MemberRefIr,
    MethodIr, VerificationFrameIr,
};
use crate::{
    ClassName, DescriptorError, Error, InferredType, MethodDescriptor, PrimitiveType,
    ReferenceType, TypeDescriptor,
};

pub(crate) fn parse_and_lower(bytes: &[u8]) -> Result<ClassIr, Error> {
    let disassembler = Disassembler::builder()
        .recovery(RecoveryMode::BestEffort)
        .build();
    let disassembly = disassembler.parse(bytes)?;
    let class = disassembly.class().ok_or(Error::IncompleteClass)?;
    lower_class(class)
}

pub(crate) fn lower_class(class: &Class) -> Result<ClassIr, Error> {
    let name = parse_class_name(class.name())?;
    let methods = class
        .methods()
        .map(|method| lower_method(class, &name, method))
        .collect::<Result<Vec<_>, _>>()?;

    Ok(ClassIr { name, methods })
}

fn lower_method(
    class: &Class,
    owner: &ClassName,
    method: ferro_babe::model::Method<'_>,
) -> Result<MethodIr, Error> {
    let descriptor = MethodDescriptor::parse(method.descriptor())?;
    let instructions = method
        .instructions()
        .map(|instructions| {
            instructions
                .map(|instruction| lower_instruction(class, instruction))
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
    let verification_frames = lower_verification_frames(owner, method, &descriptor, &instructions);

    Ok(MethodIr {
        name: method.name().to_owned(),
        descriptor_text: method.descriptor().to_owned(),
        descriptor,
        access_flags: method.access_flags(),
        max_stack: method.max_stack(),
        max_locals: method.max_locals(),
        instructions,
        exception_handlers,
        verification_frames,
    })
}

fn lower_verification_frames(
    owner: &ClassName,
    method: ferro_babe::model::Method<'_>,
    descriptor: &MethodDescriptor,
    instructions: &[InstructionIr],
) -> BTreeMap<u16, VerificationFrameIr> {
    let Some(frames) = method.stack_map_frames() else {
        return BTreeMap::new();
    };

    let mut locals = initial_verification_locals(owner, descriptor, method.access_flags());
    let mut previous_offset = -1_i32;
    let mut lowered = BTreeMap::new();

    for frame in frames {
        let offset = previous_offset + i32::from(frame.offset_delta()) + 1;
        let Ok(offset) = u16::try_from(offset) else {
            break;
        };
        previous_offset = i32::from(offset);

        let stack = match frame.kind() {
            StackMapFrameKind::Same | StackMapFrameKind::SameExtended => Vec::new(),
            StackMapFrameKind::SameLocalsOneStackItem => {
                verification_types(frame.stack(), owner, instructions)
            }
            StackMapFrameKind::Chop { count } => {
                locals.truncate(locals.len().saturating_sub(usize::from(count)));
                Vec::new()
            }
            StackMapFrameKind::Append => {
                locals.extend(verification_types(frame.locals(), owner, instructions));
                Vec::new()
            }
            StackMapFrameKind::Full => {
                locals = verification_types(frame.locals(), owner, instructions);
                verification_types(frame.stack(), owner, instructions)
            }
        };

        lowered.insert(
            offset,
            VerificationFrameIr {
                locals: expand_local_slots(&locals),
                stack,
            },
        );
    }

    lowered
}

fn initial_verification_locals(
    owner: &ClassName,
    descriptor: &MethodDescriptor,
    access_flags: u16,
) -> Vec<InferredType> {
    let mut locals = Vec::new();
    if access_flags & 0x0008 == 0 {
        locals.push(InferredType::Reference(ReferenceType::Exact(owner.clone())));
    }
    locals.extend(descriptor.parameters().iter().map(inferred_from_descriptor));
    locals
}

fn verification_types(
    values: Option<ferro_babe::model::VerificationTypes<'_>>,
    owner: &ClassName,
    instructions: &[InstructionIr],
) -> Vec<InferredType> {
    let Some(values) = values else {
        return Vec::new();
    };

    values
        .iter()
        .map(|value| lower_verification_type(value, owner, instructions))
        .collect()
}

fn lower_verification_type(
    value: VerificationType<'_>,
    owner: &ClassName,
    instructions: &[InstructionIr],
) -> InferredType {
    match value {
        VerificationType::Top => InferredType::Bottom,
        VerificationType::Integer => InferredType::Int,
        VerificationType::Float => InferredType::Float,
        VerificationType::Long => InferredType::Long,
        VerificationType::Double => InferredType::Double,
        VerificationType::Null => InferredType::Reference(ReferenceType::Null),
        VerificationType::UninitializedThis => {
            InferredType::Reference(ReferenceType::Exact(owner.clone()))
        }
        VerificationType::Object { internal_name, .. } => internal_name
            .and_then(reference_type_from_stack_map_name)
            .map(InferredType::Reference)
            .unwrap_or(InferredType::Reference(ReferenceType::Unknown)),
        VerificationType::Uninitialized { offset } => instructions
            .iter()
            .find(|instruction| instruction.offset == offset && instruction.opcode == 0xbb)
            .and_then(type_name_from_instruction)
            .and_then(|name| ClassName::parse(name).ok())
            .map(|class_name| InferredType::Uninitialized {
                class_name,
                allocation_offset: offset,
            })
            .unwrap_or(InferredType::Reference(ReferenceType::Unknown)),
    }
}

fn reference_type_from_stack_map_name(name: &str) -> Option<ReferenceType> {
    if name.starts_with('[') {
        let descriptor = TypeDescriptor::parse(name).ok()?;
        return matches!(descriptor, TypeDescriptor::Array { .. })
            .then_some(ReferenceType::Array(descriptor));
    }

    ClassName::parse(name).ok().map(ReferenceType::Exact)
}

fn type_name_from_instruction(instruction: &InstructionIr) -> Option<&str> {
    match &instruction.operand {
        InstructionOperandIr::Type { type_name, .. }
        | InstructionOperandIr::MultiArray { type_name, .. } => type_name.as_deref(),
        _ => None,
    }
}

fn expand_local_slots(values: &[InferredType]) -> Vec<InferredType> {
    let mut slots = Vec::with_capacity(values.len());
    for value in values {
        slots.push(value.clone());
        if matches!(value, InferredType::Long | InferredType::Double) {
            slots.push(InferredType::Bottom);
        }
    }
    slots
}

fn inferred_from_descriptor(descriptor: &TypeDescriptor) -> InferredType {
    match descriptor {
        TypeDescriptor::Primitive(primitive) => match primitive {
            PrimitiveType::Long => InferredType::Long,
            PrimitiveType::Float => InferredType::Float,
            PrimitiveType::Double => InferredType::Double,
            PrimitiveType::Boolean
            | PrimitiveType::Byte
            | PrimitiveType::Char
            | PrimitiveType::Short
            | PrimitiveType::Int => InferredType::Int,
        },
        TypeDescriptor::Reference(class_name) => {
            InferredType::Reference(ReferenceType::Exact(class_name.clone()))
        }
        TypeDescriptor::Array { .. } => {
            InferredType::Reference(ReferenceType::Array(descriptor.clone()))
        }
    }
}

fn lower_instruction(
    class: &Class,
    instruction: ferro_babe::model::Instruction<'_>,
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
        Some(
            ConstantRef::Unusable
            | ConstantRef::Utf8(_)
            | ConstantRef::FieldReference { .. }
            | ConstantRef::MethodReference { .. }
            | ConstantRef::InterfaceMethodReference { .. }
            | ConstantRef::NameAndType { .. }
            | ConstantRef::MethodHandle { .. }
            | ConstantRef::MethodType { .. }
            | ConstantRef::Dynamic { .. }
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
