use crate::ir::{ConstantKind, InstructionIr, InstructionOperandIr, MethodIr};
use crate::solver::frame::{Frame, InstanceOfFact, inferred_from_descriptor};
use crate::summary::{FieldSummaryResolver, MethodSummaryResolver};
use crate::{ClassName, Diagnostic, InferredType, IntegralTypeSet, ReferenceType, TypeDescriptor};

mod array;
mod member;
mod stack;

use array::*;
use member::*;
use stack::*;

pub(crate) fn transfer(
    method: &MethodIr,
    instruction: &InstructionIr,
    frame: &mut Frame,
    diagnostics: &mut Vec<Diagnostic>,
    method_summaries: Option<&dyn MethodSummaryResolver>,
    field_summaries: Option<&dyn FieldSummaryResolver>,
) {
    match instruction.opcode {
        0x00 => {}
        0x01 => frame.push(InferredType::Reference(ReferenceType::Null)),
        0x02..=0x08 | 0x10 | 0x11 => frame.push(InferredType::Integral(IntegralTypeSet::ALL)),
        0x09..=0x0a => frame.push(InferredType::Long),
        0x0b..=0x0d => frame.push(InferredType::Float),
        0x0e..=0x0f => frame.push(InferredType::Double),
        0x12..=0x14 => push_constant(instruction, frame),
        0x15 | 0x1a..=0x1d => load_local(instruction, frame, 0x15, 0x1a),
        0x16 | 0x1e..=0x21 => load_local(instruction, frame, 0x16, 0x1e),
        0x17 | 0x22..=0x25 => load_local(instruction, frame, 0x17, 0x22),
        0x18 | 0x26..=0x29 => load_local(instruction, frame, 0x18, 0x26),
        0x19 | 0x2a..=0x2d => load_local(instruction, frame, 0x19, 0x2a),
        0x2e => integral_array_load(
            frame,
            IntegralTypeSet::INT,
            method,
            instruction,
            diagnostics,
        ),
        0x33 => integral_array_load(
            frame,
            IntegralTypeSet::BOOLEAN.union(IntegralTypeSet::BYTE),
            method,
            instruction,
            diagnostics,
        ),
        0x34 => integral_array_load(
            frame,
            IntegralTypeSet::CHAR,
            method,
            instruction,
            diagnostics,
        ),
        0x35 => integral_array_load(
            frame,
            IntegralTypeSet::SHORT,
            method,
            instruction,
            diagnostics,
        ),
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
        0x88 | 0x8b | 0x8e => convert(frame, InferredType::Int, method, instruction, diagnostics),
        0x91 => convert(
            frame,
            InferredType::Integral(IntegralTypeSet::BYTE),
            method,
            instruction,
            diagnostics,
        ),
        0x92 => convert(
            frame,
            InferredType::Integral(IntegralTypeSet::CHAR),
            method,
            instruction,
            diagnostics,
        ),
        0x93 => convert(
            frame,
            InferredType::Integral(IntegralTypeSet::SHORT),
            method,
            instruction,
            diagnostics,
        ),
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
        0xb2 => field_get(
            instruction,
            frame,
            method,
            diagnostics,
            false,
            field_summaries,
        ),
        0xb3 => field_put(instruction, frame, method, diagnostics, false),
        0xb4 => field_get(
            instruction,
            frame,
            method,
            diagnostics,
            true,
            field_summaries,
        ),
        0xb5 => field_put(instruction, frame, method, diagnostics, true),
        0xb6..=0xb9 => invoke_member(instruction, frame, method, diagnostics, method_summaries),
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
        0xc1 => instance_of(instruction, frame, method, diagnostics),
        0xc2 | 0xc3 => discard(frame, method, instruction, diagnostics),
        0xc5 => allocate_multi_array(instruction, frame, method, diagnostics),
        0xca | 0xfe | 0xff => unsupported(method, instruction, diagnostics),
        _ => unsupported(method, instruction, diagnostics),
    }
}

fn load_local(instruction: &InstructionIr, frame: &mut Frame, wide_opcode: u8, short_base: u8) {
    let local = local_index(instruction, wide_opcode, short_base).unwrap_or_default();
    frame.push_local(local, instruction.offset);
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
    frame.store_local_value(local, value, instruction.offset);
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

fn allocate_object(instruction: &InstructionIr, frame: &mut Frame) {
    let class_name = type_name(instruction).and_then(|name| ClassName::parse(name).ok());
    match class_name {
        Some(class_name) => frame.push_allocation(class_name, instruction.offset),
        None => frame.push(InferredType::Reference(ReferenceType::Unknown)),
    }
}

fn cast_reference(
    instruction: &InstructionIr,
    frame: &mut Frame,
    method: &MethodIr,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let mut value = pop_value(frame, method, instruction, diagnostics);
    let reference = type_name(instruction)
        .and_then(reference_descriptor)
        .map(|descriptor| match descriptor {
            TypeDescriptor::Reference(class_name) => ReferenceType::Exact(class_name),
            descriptor @ TypeDescriptor::Array { .. } => ReferenceType::Array(descriptor),
            TypeDescriptor::Primitive(_) => ReferenceType::Unknown,
        })
        .unwrap_or(ReferenceType::Unknown);
    if !matches!(value.value, InferredType::Reference(ReferenceType::Null)) {
        value.value = InferredType::Reference(reference);
    }
    frame.push_value(value);
}

fn instance_of(
    instruction: &InstructionIr,
    frame: &mut Frame,
    method: &MethodIr,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let value = pop_value(frame, method, instruction, diagnostics);
    let reference = type_name(instruction)
        .and_then(reference_descriptor)
        .and_then(|descriptor| match descriptor {
            TypeDescriptor::Reference(class_name) => Some(ReferenceType::Exact(class_name)),
            descriptor @ TypeDescriptor::Array { .. } => Some(ReferenceType::Array(descriptor)),
            TypeDescriptor::Primitive(_) => None,
        });
    let fact = value
        .local_origin
        .zip(reference)
        .map(|(origin, reference)| InstanceOfFact { origin, reference });
    frame.push_instanceof_result(fact);
}

pub(super) fn type_name(instruction: &InstructionIr) -> Option<&str> {
    match &instruction.operand {
        InstructionOperandIr::Type { type_name, .. }
        | InstructionOperandIr::MultiArray { type_name, .. } => type_name.as_deref(),
        _ => None,
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
    let value = match &instruction.operand {
        InstructionOperandIr::Constant(ConstantKind::Integer) => {
            InferredType::Integral(IntegralTypeSet::ALL)
        }
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
        InstructionOperandIr::Constant(ConstantKind::Dynamic(descriptor)) => {
            inferred_from_descriptor(descriptor)
        }
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
