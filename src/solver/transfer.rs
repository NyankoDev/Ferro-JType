use crate::ir::{ConstantKind, InstructionIr, InstructionOperandIr, MemberRefIr, MethodIr};
use crate::solver::frame::{Frame, FrameValue, inferred_from_descriptor};
use crate::{
    ClassName, Diagnostic, DiagnosticKind, DiagnosticLocation, DiagnosticSeverity, InferredType,
    MethodDescriptor, ReferenceType, ReturnType, TypeDescriptor,
};

pub(crate) fn transfer(
    method: &MethodIr,
    instruction: &InstructionIr,
    frame: &mut Frame,
    diagnostics: &mut Vec<Diagnostic>,
) {
    match instruction.opcode {
        0x00 => {}
        0x01 => frame.push(InferredType::Reference(ReferenceType::Null)),
        0x02..=0x08 | 0x10 | 0x11 => frame.push(InferredType::Int),
        0x09..=0x0a => frame.push(InferredType::Long),
        0x0b..=0x0d => frame.push(InferredType::Float),
        0x0e..=0x0f => frame.push(InferredType::Double),
        0x12..=0x14 => push_constant(instruction, frame),
        0x15 | 0x1a..=0x1d => load_local(instruction, frame, 0x15, 0x1a),
        0x16 | 0x1e..=0x21 => load_local(instruction, frame, 0x16, 0x1e),
        0x17 | 0x22..=0x25 => load_local(instruction, frame, 0x17, 0x22),
        0x18 | 0x26..=0x29 => load_local(instruction, frame, 0x18, 0x26),
        0x19 | 0x2a..=0x2d => load_local(instruction, frame, 0x19, 0x2a),
        0x2e | 0x33..=0x35 => {
            array_load(frame, InferredType::Int, method, instruction, diagnostics)
        }
        0x2f => array_load(frame, InferredType::Long, method, instruction, diagnostics),
        0x30 => array_load(frame, InferredType::Float, method, instruction, diagnostics),
        0x31 => array_load(
            frame,
            InferredType::Double,
            method,
            instruction,
            diagnostics,
        ),
        0x32 => reference_array_load(frame, method, instruction, diagnostics),
        0x36 | 0x3b..=0x3e => store_local(instruction, frame, 0x36, 0x3b, method, diagnostics),
        0x37 | 0x3f..=0x42 => store_local(instruction, frame, 0x37, 0x3f, method, diagnostics),
        0x38 | 0x43..=0x46 => store_local(instruction, frame, 0x38, 0x43, method, diagnostics),
        0x39 | 0x47..=0x4a => store_local(instruction, frame, 0x39, 0x47, method, diagnostics),
        0x3a | 0x4b..=0x4e => store_local(instruction, frame, 0x3a, 0x4b, method, diagnostics),
        0x4f..=0x56 => array_store(frame, method, instruction, diagnostics),
        0x57 => discard(frame, method, instruction, diagnostics),
        0x58 => discard_two_slots(frame, method, instruction, diagnostics),
        0x59 => duplicate_top(frame, method, instruction, diagnostics),
        0x5a => duplicate_x1(frame, method, instruction, diagnostics),
        0x5b => duplicate_x2(frame, method, instruction, diagnostics),
        0x5c => duplicate_two(frame, method, instruction, diagnostics),
        0x5d => duplicate_two_x1(frame, method, instruction, diagnostics),
        0x5e => duplicate_two_x2(frame, method, instruction, diagnostics),
        0x5f => swap(frame, method, instruction, diagnostics),
        0x60 | 0x64 | 0x68 | 0x6c | 0x70 | 0x78 | 0x7a | 0x7c | 0x7e | 0x80 | 0x82 => {
            binary(frame, InferredType::Int, method, instruction, diagnostics)
        }
        0x61 | 0x65 | 0x69 | 0x6d | 0x71 | 0x79 | 0x7b | 0x7d | 0x7f | 0x81 | 0x83 => {
            binary(frame, InferredType::Long, method, instruction, diagnostics)
        }
        0x62 | 0x66 | 0x6a | 0x6e | 0x72 => {
            binary(frame, InferredType::Float, method, instruction, diagnostics)
        }
        0x63 | 0x67 | 0x6b | 0x6f | 0x73 => binary(
            frame,
            InferredType::Double,
            method,
            instruction,
            diagnostics,
        ),
        0x74 | 0x76 | 0x77 => unary(frame, method, instruction, diagnostics),
        0x75 => unary(frame, method, instruction, diagnostics),
        0x84 => increment_local(instruction, frame),
        0x85 => convert(frame, InferredType::Long, method, instruction, diagnostics),
        0x86 => convert(frame, InferredType::Float, method, instruction, diagnostics),
        0x87 => convert(
            frame,
            InferredType::Double,
            method,
            instruction,
            diagnostics,
        ),
        0x88 | 0x8b | 0x8e | 0x91 | 0x92 | 0x93 => {
            convert(frame, InferredType::Int, method, instruction, diagnostics)
        }
        0x89 | 0x8c | 0x8f => convert(frame, InferredType::Long, method, instruction, diagnostics),
        0x8a | 0x8d | 0x90 => convert(frame, InferredType::Float, method, instruction, diagnostics),
        0x94..=0x98 => binary(frame, InferredType::Int, method, instruction, diagnostics),
        0x99..=0x9e | 0xc6 | 0xc7 => discard(frame, method, instruction, diagnostics),
        0x9f..=0xa6 => {
            discard(frame, method, instruction, diagnostics);
            discard(frame, method, instruction, diagnostics);
        }
        0xa8 | 0xc9 => push_subroutine_return_address(method, instruction, frame),
        0xaa | 0xab => discard(frame, method, instruction, diagnostics),
        0xac..=0xb0 => discard(frame, method, instruction, diagnostics),
        0xb1 | 0xa7 | 0xa9 | 0xc8 => {}
        0xb2 => field_get(instruction, frame, method, diagnostics, false),
        0xb3 => field_put(instruction, frame, method, diagnostics, false),
        0xb4 => field_get(instruction, frame, method, diagnostics, true),
        0xb5 => field_put(instruction, frame, method, diagnostics, true),
        0xb6..=0xb9 => invoke_member(instruction, frame, method, diagnostics),
        0xba => invoke_dynamic(instruction, frame, method, diagnostics),
        0xbb => allocate_object(instruction, frame),
        0xbc => allocate_primitive_array(instruction, frame, method, diagnostics),
        0xbd => allocate_reference_array(instruction, frame, method, diagnostics),
        0xbe => {
            discard(frame, method, instruction, diagnostics);
            frame.push(InferredType::Int);
        }
        0xbf => discard(frame, method, instruction, diagnostics),
        0xc0 => cast_reference(instruction, frame, method, diagnostics),
        0xc1 => {
            discard(frame, method, instruction, diagnostics);
            frame.push(InferredType::Int);
        }
        0xc2 | 0xc3 => discard(frame, method, instruction, diagnostics),
        0xc5 => allocate_multi_array(instruction, frame, method, diagnostics),
        0xca | 0xfe | 0xff => unsupported(method, instruction, diagnostics),
        _ => unsupported(method, instruction, diagnostics),
    }
}

fn load_local(instruction: &InstructionIr, frame: &mut Frame, wide_opcode: u8, short_base: u8) {
    let local = local_index(instruction, wide_opcode, short_base).unwrap_or_default();
    frame.push_value(frame.get_local_value(local));
}

fn store_local(
    instruction: &InstructionIr,
    frame: &mut Frame,
    wide_opcode: u8,
    short_base: u8,
    method: &MethodIr,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let value = pop_value(frame, method, instruction, diagnostics);
    let local = local_index(instruction, wide_opcode, short_base).unwrap_or_default();
    frame.set_local_value(local, value);
}

fn local_index(instruction: &InstructionIr, wide_opcode: u8, short_base: u8) -> Option<u16> {
    if instruction.opcode == wide_opcode {
        let InstructionOperandIr::Local(local) = instruction.operand else {
            return None;
        };
        return Some(local);
    }

    instruction
        .opcode
        .checked_sub(short_base)
        .filter(|index| *index < 4)
        .map(u16::from)
}

fn array_load(
    frame: &mut Frame,
    result: InferredType,
    method: &MethodIr,
    instruction: &InstructionIr,
    diagnostics: &mut Vec<Diagnostic>,
) {
    discard(frame, method, instruction, diagnostics);
    discard(frame, method, instruction, diagnostics);
    frame.push(result);
}

fn reference_array_load(
    frame: &mut Frame,
    method: &MethodIr,
    instruction: &InstructionIr,
    diagnostics: &mut Vec<Diagnostic>,
) {
    discard(frame, method, instruction, diagnostics);
    let array = pop(frame, method, instruction, diagnostics);
    frame.push(reference_array_element_type(&array));
}

fn reference_array_element_type(array: &InferredType) -> InferredType {
    let InferredType::Reference(ReferenceType::Array(TypeDescriptor::Array {
        dimensions,
        element,
    })) = array
    else {
        return InferredType::Reference(ReferenceType::Unknown);
    };

    if *dimensions == 1 {
        return inferred_from_descriptor(element);
    }

    InferredType::Reference(ReferenceType::Array(TypeDescriptor::Array {
        dimensions: dimensions - 1,
        element: element.clone(),
    }))
}

fn array_store(
    frame: &mut Frame,
    method: &MethodIr,
    instruction: &InstructionIr,
    diagnostics: &mut Vec<Diagnostic>,
) {
    discard(frame, method, instruction, diagnostics);
    discard(frame, method, instruction, diagnostics);
    discard(frame, method, instruction, diagnostics);
}

fn binary(
    frame: &mut Frame,
    result: InferredType,
    method: &MethodIr,
    instruction: &InstructionIr,
    diagnostics: &mut Vec<Diagnostic>,
) {
    discard(frame, method, instruction, diagnostics);
    discard(frame, method, instruction, diagnostics);
    frame.push(result);
}

fn unary(
    frame: &mut Frame,
    method: &MethodIr,
    instruction: &InstructionIr,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let value = pop(frame, method, instruction, diagnostics);
    frame.push(value);
}

fn convert(
    frame: &mut Frame,
    result: InferredType,
    method: &MethodIr,
    instruction: &InstructionIr,
    diagnostics: &mut Vec<Diagnostic>,
) {
    discard(frame, method, instruction, diagnostics);
    frame.push(result);
}

fn increment_local(instruction: &InstructionIr, frame: &mut Frame) {
    let InstructionOperandIr::Increment { local, .. } = instruction.operand else {
        return;
    };
    frame.set_local(local, InferredType::Int);
}

fn field_get(
    instruction: &InstructionIr,
    frame: &mut Frame,
    method: &MethodIr,
    diagnostics: &mut Vec<Diagnostic>,
    has_receiver: bool,
) {
    if has_receiver {
        discard(frame, method, instruction, diagnostics);
    }
    frame.push(field_type(instruction, method, diagnostics));
}

fn field_put(
    instruction: &InstructionIr,
    frame: &mut Frame,
    method: &MethodIr,
    diagnostics: &mut Vec<Diagnostic>,
    has_receiver: bool,
) {
    discard(frame, method, instruction, diagnostics);
    if has_receiver {
        discard(frame, method, instruction, diagnostics);
    }
}

fn invoke_member(
    instruction: &InstructionIr,
    frame: &mut Frame,
    method: &MethodIr,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let Some((descriptor, member)) = method_call_descriptor(instruction, method, diagnostics)
    else {
        frame.clear_stack();
        return;
    };

    for _ in descriptor.parameters() {
        discard(frame, method, instruction, diagnostics);
    }

    let receiver =
        (instruction.opcode != 0xb8).then(|| pop(frame, method, instruction, diagnostics));
    if let (
        Some(MemberRefIr::Resolved { name, owner, .. }),
        Some(InferredType::Uninitialized {
            allocation_offset, ..
        }),
    ) = (member, receiver)
        && name == "<init>"
    {
        frame.replace_uninitialized(allocation_offset, owner.clone());
    }

    push_return_type(&descriptor, frame);
}

fn invoke_dynamic(
    instruction: &InstructionIr,
    frame: &mut Frame,
    method: &MethodIr,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let InstructionOperandIr::InvokeDynamic { descriptor, .. } = &instruction.operand else {
        return;
    };
    let Some(descriptor) = descriptor
        .as_deref()
        .and_then(|descriptor| MethodDescriptor::parse(descriptor).ok())
    else {
        unsupported(method, instruction, diagnostics);
        frame.clear_stack();
        return;
    };

    for _ in descriptor.parameters() {
        discard(frame, method, instruction, diagnostics);
    }
    push_return_type(&descriptor, frame);
}

fn method_call_descriptor<'a>(
    instruction: &'a InstructionIr,
    method: &MethodIr,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<(MethodDescriptor, Option<&'a MemberRefIr>)> {
    let member = match &instruction.operand {
        InstructionOperandIr::Member(member) => member,
        InstructionOperandIr::InvokeInterface { method, .. } => method,
        _ => return None,
    };
    let MemberRefIr::Resolved { descriptor, .. } = member else {
        unsupported(method, instruction, diagnostics);
        return None;
    };

    match MethodDescriptor::parse(descriptor) {
        Ok(descriptor) => Some((descriptor, Some(member))),
        Err(_) => {
            unsupported(method, instruction, diagnostics);
            None
        }
    }
}

fn push_return_type(descriptor: &MethodDescriptor, frame: &mut Frame) {
    if let ReturnType::Type(return_type) = descriptor.return_type() {
        frame.push(inferred_from_descriptor(return_type));
    }
}

fn field_type(
    instruction: &InstructionIr,
    method: &MethodIr,
    diagnostics: &mut Vec<Diagnostic>,
) -> InferredType {
    let InstructionOperandIr::Member(MemberRefIr::Resolved { descriptor, .. }) =
        &instruction.operand
    else {
        unsupported(method, instruction, diagnostics);
        return InferredType::Reference(ReferenceType::Unknown);
    };

    TypeDescriptor::parse(descriptor)
        .map(|descriptor| inferred_from_descriptor(&descriptor))
        .unwrap_or_else(|_| {
            unsupported(method, instruction, diagnostics);
            InferredType::Reference(ReferenceType::Unknown)
        })
}

fn allocate_object(instruction: &InstructionIr, frame: &mut Frame) {
    let class_name = type_name(instruction).and_then(|name| ClassName::parse(name).ok());
    match class_name {
        Some(class_name) => frame.push(InferredType::Uninitialized {
            class_name,
            allocation_offset: instruction.offset,
        }),
        None => frame.push(InferredType::Reference(ReferenceType::Unknown)),
    }
}

fn allocate_primitive_array(
    instruction: &InstructionIr,
    frame: &mut Frame,
    method: &MethodIr,
    diagnostics: &mut Vec<Diagnostic>,
) {
    discard(frame, method, instruction, diagnostics);
    let primitive = match instruction.operand {
        InstructionOperandIr::Immediate(4) => crate::PrimitiveType::Boolean,
        InstructionOperandIr::Immediate(5) => crate::PrimitiveType::Char,
        InstructionOperandIr::Immediate(6) => crate::PrimitiveType::Float,
        InstructionOperandIr::Immediate(7) => crate::PrimitiveType::Double,
        InstructionOperandIr::Immediate(8) => crate::PrimitiveType::Byte,
        InstructionOperandIr::Immediate(9) => crate::PrimitiveType::Short,
        InstructionOperandIr::Immediate(10) => crate::PrimitiveType::Int,
        InstructionOperandIr::Immediate(11) => crate::PrimitiveType::Long,
        _ => {
            frame.push(InferredType::Reference(ReferenceType::Unknown));
            return;
        }
    };
    frame.push(InferredType::Reference(ReferenceType::Array(
        TypeDescriptor::Array {
            dimensions: 1,
            element: Box::new(TypeDescriptor::Primitive(primitive)),
        },
    )));
}

fn allocate_reference_array(
    instruction: &InstructionIr,
    frame: &mut Frame,
    method: &MethodIr,
    diagnostics: &mut Vec<Diagnostic>,
) {
    discard(frame, method, instruction, diagnostics);
    let reference = type_name(instruction)
        .and_then(array_element_descriptor)
        .and_then(array_of)
        .map(ReferenceType::Array)
        .unwrap_or(ReferenceType::Unknown);
    frame.push(InferredType::Reference(reference));
}

fn cast_reference(
    instruction: &InstructionIr,
    frame: &mut Frame,
    method: &MethodIr,
    diagnostics: &mut Vec<Diagnostic>,
) {
    discard(frame, method, instruction, diagnostics);
    let reference = type_name(instruction)
        .and_then(reference_descriptor)
        .map(|descriptor| match descriptor {
            TypeDescriptor::Reference(class_name) => ReferenceType::Exact(class_name),
            descriptor @ TypeDescriptor::Array { .. } => ReferenceType::Array(descriptor),
            TypeDescriptor::Primitive(_) => ReferenceType::Unknown,
        })
        .unwrap_or(ReferenceType::Unknown);
    frame.push(InferredType::Reference(reference));
}

fn allocate_multi_array(
    instruction: &InstructionIr,
    frame: &mut Frame,
    method: &MethodIr,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let dimensions = match instruction.operand {
        InstructionOperandIr::MultiArray { dimensions, .. } => dimensions,
        _ => 0,
    };
    for _ in 0..dimensions {
        discard(frame, method, instruction, diagnostics);
    }
    let reference = type_name(instruction)
        .and_then(|name| TypeDescriptor::parse(name).ok())
        .and_then(|descriptor| match descriptor {
            descriptor @ TypeDescriptor::Array { .. } => Some(ReferenceType::Array(descriptor)),
            TypeDescriptor::Primitive(_) | TypeDescriptor::Reference(_) => None,
        })
        .unwrap_or(ReferenceType::Unknown);
    frame.push(InferredType::Reference(reference));
}

fn type_name(instruction: &InstructionIr) -> Option<&str> {
    match &instruction.operand {
        InstructionOperandIr::Type { type_name, .. }
        | InstructionOperandIr::MultiArray { type_name, .. } => type_name.as_deref(),
        _ => None,
    }
}

fn array_element_descriptor(name: &str) -> Option<TypeDescriptor> {
    reference_descriptor(name)
        .or_else(|| ClassName::parse(name).ok().map(TypeDescriptor::Reference))
}

fn array_of(component: TypeDescriptor) -> Option<TypeDescriptor> {
    match component {
        TypeDescriptor::Array {
            dimensions,
            element,
        } => Some(TypeDescriptor::Array {
            dimensions: dimensions.checked_add(1)?,
            element,
        }),
        element => Some(TypeDescriptor::Array {
            dimensions: 1,
            element: Box::new(element),
        }),
    }
}

fn reference_descriptor(name: &str) -> Option<TypeDescriptor> {
    if name.starts_with('[') {
        TypeDescriptor::parse(name).ok()
    } else {
        ClassName::parse(name).ok().map(TypeDescriptor::Reference)
    }
}

fn push_constant(instruction: &InstructionIr, frame: &mut Frame) {
    let value = match instruction.operand {
        InstructionOperandIr::Constant(ConstantKind::Integer) => InferredType::Int,
        InstructionOperandIr::Constant(ConstantKind::Float) => InferredType::Float,
        InstructionOperandIr::Constant(ConstantKind::Long) => InferredType::Long,
        InstructionOperandIr::Constant(ConstantKind::Double) => InferredType::Double,
        InstructionOperandIr::Constant(ConstantKind::String) => {
            InferredType::Reference(ReferenceType::Exact(ClassName::java_lang_string()))
        }
        InstructionOperandIr::Constant(ConstantKind::Type) => {
            InferredType::Reference(ReferenceType::Exact(ClassName::java_lang_class()))
        }
        InstructionOperandIr::Constant(ConstantKind::MethodHandle) => InferredType::Reference(
            ReferenceType::Exact(ClassName::java_lang_invoke_method_handle()),
        ),
        InstructionOperandIr::Constant(ConstantKind::MethodType) => InferredType::Reference(
            ReferenceType::Exact(ClassName::java_lang_invoke_method_type()),
        ),
        InstructionOperandIr::Constant(ConstantKind::Unresolved) => {
            InferredType::Reference(ReferenceType::Unknown)
        }
        _ => InferredType::Reference(ReferenceType::Unknown),
    };
    frame.push(value);
}

fn push_subroutine_return_address(
    method: &MethodIr,
    instruction: &InstructionIr,
    frame: &mut Frame,
) {
    let return_target = method
        .instructions
        .iter()
        .skip_while(|candidate| candidate.offset != instruction.offset)
        .nth(1)
        .map(|candidate| candidate.offset);

    match return_target {
        Some(return_target) => frame.push_return_address(return_target),
        None => frame.push(InferredType::ReturnAddress),
    }
}

fn discard(
    frame: &mut Frame,
    method: &MethodIr,
    instruction: &InstructionIr,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let _ = pop(frame, method, instruction, diagnostics);
}

fn discard_two_slots(
    frame: &mut Frame,
    method: &MethodIr,
    instruction: &InstructionIr,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let top = pop(frame, method, instruction, diagnostics);
    if !is_category_two(&top) {
        discard(frame, method, instruction, diagnostics);
    }
}

fn pop(
    frame: &mut Frame,
    method: &MethodIr,
    instruction: &InstructionIr,
    diagnostics: &mut Vec<Diagnostic>,
) -> InferredType {
    pop_value(frame, method, instruction, diagnostics).value
}

fn pop_value(
    frame: &mut Frame,
    method: &MethodIr,
    instruction: &InstructionIr,
    diagnostics: &mut Vec<Diagnostic>,
) -> FrameValue {
    frame.pop_value().unwrap_or_else(|| {
        diagnostics.push(Diagnostic::new(
            DiagnosticSeverity::Warning,
            DiagnosticKind::StackUnderflow,
            location(method, instruction.offset),
            "instruction consumed a value from an empty operand stack",
        ));
        FrameValue::plain(InferredType::Conflict)
    })
}

fn duplicate_top(
    frame: &mut Frame,
    method: &MethodIr,
    instruction: &InstructionIr,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let value = pop_value(frame, method, instruction, diagnostics);
    frame.push_value(value.clone());
    frame.push_value(value);
}

fn duplicate_x1(
    frame: &mut Frame,
    method: &MethodIr,
    instruction: &InstructionIr,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let first = pop_value(frame, method, instruction, diagnostics);
    let second = pop_value(frame, method, instruction, diagnostics);
    frame.push_value(first.clone());
    frame.push_value(second);
    frame.push_value(first);
}

fn duplicate_x2(
    frame: &mut Frame,
    method: &MethodIr,
    instruction: &InstructionIr,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let first = pop_value(frame, method, instruction, diagnostics);
    let second = pop_value(frame, method, instruction, diagnostics);
    if is_category_two(&second.value) {
        frame.push_value(first.clone());
        frame.push_value(second);
        frame.push_value(first);
    } else {
        let third = pop_value(frame, method, instruction, diagnostics);
        frame.push_value(first.clone());
        frame.push_value(third);
        frame.push_value(second);
        frame.push_value(first);
    }
}

fn duplicate_two(
    frame: &mut Frame,
    method: &MethodIr,
    instruction: &InstructionIr,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let first = pop_value(frame, method, instruction, diagnostics);
    if is_category_two(&first.value) {
        frame.push_value(first.clone());
        frame.push_value(first);
    } else {
        let second = pop_value(frame, method, instruction, diagnostics);
        frame.push_value(second.clone());
        frame.push_value(first.clone());
        frame.push_value(second);
        frame.push_value(first);
    }
}

fn duplicate_two_x1(
    frame: &mut Frame,
    method: &MethodIr,
    instruction: &InstructionIr,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let first = pop_value(frame, method, instruction, diagnostics);
    let second = pop_value(frame, method, instruction, diagnostics);
    if is_category_two(&first.value) {
        frame.push_value(first.clone());
        frame.push_value(second);
        frame.push_value(first);
    } else {
        let third = pop_value(frame, method, instruction, diagnostics);
        frame.push_value(second.clone());
        frame.push_value(first.clone());
        frame.push_value(third);
        frame.push_value(second);
        frame.push_value(first);
    }
}

fn duplicate_two_x2(
    frame: &mut Frame,
    method: &MethodIr,
    instruction: &InstructionIr,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let first = pop_value(frame, method, instruction, diagnostics);
    let second = pop_value(frame, method, instruction, diagnostics);
    if is_category_two(&first.value) && is_category_two(&second.value) {
        frame.push_value(first.clone());
        frame.push_value(second);
        frame.push_value(first);
    } else if is_category_two(&first.value) {
        let third = pop_value(frame, method, instruction, diagnostics);
        frame.push_value(first.clone());
        frame.push_value(third);
        frame.push_value(second);
        frame.push_value(first);
    } else {
        let third = pop_value(frame, method, instruction, diagnostics);
        if is_category_two(&third.value) {
            frame.push_value(second.clone());
            frame.push_value(first.clone());
            frame.push_value(third);
            frame.push_value(second);
            frame.push_value(first);
        } else {
            let fourth = pop_value(frame, method, instruction, diagnostics);
            frame.push_value(second.clone());
            frame.push_value(first.clone());
            frame.push_value(fourth);
            frame.push_value(third);
            frame.push_value(second);
            frame.push_value(first);
        }
    }
}

fn swap(
    frame: &mut Frame,
    method: &MethodIr,
    instruction: &InstructionIr,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let first = pop_value(frame, method, instruction, diagnostics);
    let second = pop_value(frame, method, instruction, diagnostics);
    frame.push_value(first);
    frame.push_value(second);
}

fn unsupported(method: &MethodIr, instruction: &InstructionIr, diagnostics: &mut Vec<Diagnostic>) {
    let details = operand_details(&instruction.operand);
    diagnostics.push(Diagnostic::new(
        DiagnosticSeverity::Warning,
        DiagnosticKind::UnsupportedInstruction,
        location(method, instruction.offset),
        format!(
            "opcode 0x{:02x} is not modeled{details}",
            instruction.opcode
        ),
    ));
}

fn operand_details(operand: &InstructionOperandIr) -> String {
    match operand {
        InstructionOperandIr::ConstantPool(index) => format!(" (constant-pool index {index})"),
        InstructionOperandIr::Type {
            constant_pool_index,
            ..
        }
        | InstructionOperandIr::MultiArray {
            constant_pool_index,
            ..
        } => format!(" (type constant-pool index {constant_pool_index})"),
        InstructionOperandIr::InvokeInterface { count, .. } => {
            format!(" (invokeinterface argument count {count})")
        }
        InstructionOperandIr::InvokeDynamic {
            constant_pool_index,
            ..
        } => format!(" (invokedynamic constant-pool index {constant_pool_index})"),
        InstructionOperandIr::Increment { amount, .. } => format!(" (increment {amount})"),
        InstructionOperandIr::Member(MemberRefIr::Unresolved {
            constant_pool_index,
        }) => format!(" (unresolved member constant-pool index {constant_pool_index})"),
        _ => String::new(),
    }
}

fn location(method: &MethodIr, offset: u16) -> DiagnosticLocation {
    DiagnosticLocation::method(&method.name, &method.descriptor_text).at_offset(offset)
}

const fn is_category_two(value: &InferredType) -> bool {
    matches!(value, InferredType::Long | InferredType::Double)
}
