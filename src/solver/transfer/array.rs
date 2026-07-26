use crate::ir::{InstructionIr, InstructionOperandIr, MethodIr};
use crate::solver::frame::{Frame, inferred_from_descriptor};
use crate::{Diagnostic, InferredType, IntegralTypeSet, ReferenceType, TypeDescriptor};

use super::{discard, pop, type_name};

pub(super) fn array_load(
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

pub(super) fn integral_array_load(
    frame: &mut Frame,
    fallback: IntegralTypeSet,
    method: &MethodIr,
    instruction: &InstructionIr,
    diagnostics: &mut Vec<Diagnostic>,
) {
    discard(frame, method, instruction, diagnostics);
    let array = pop(frame, method, instruction, diagnostics);
    let result = primitive_array_element(&array)
        .filter(|element| {
            element
                .exact_type()
                .is_some_and(|primitive| fallback.contains(primitive))
        })
        .unwrap_or(fallback);
    frame.push(InferredType::from_integral_types(result));
}

fn primitive_array_element(array: &InferredType) -> Option<IntegralTypeSet> {
    let InferredType::Reference(ReferenceType::Array(TypeDescriptor::Array {
        dimensions: 1,
        element,
    })) = array
    else {
        return None;
    };

    let TypeDescriptor::Primitive(primitive) = element.as_ref() else {
        return None;
    };
    IntegralTypeSet::from_primitive(*primitive)
}

pub(super) fn reference_array_load(
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

pub(super) fn array_store(
    frame: &mut Frame,
    method: &MethodIr,
    instruction: &InstructionIr,
    diagnostics: &mut Vec<Diagnostic>,
) {
    discard(frame, method, instruction, diagnostics);
    discard(frame, method, instruction, diagnostics);
    discard(frame, method, instruction, diagnostics);
}

pub(super) fn allocate_primitive_array(
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

pub(super) fn allocate_reference_array(
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

pub(super) fn allocate_multi_array(
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

fn array_element_descriptor(name: &str) -> Option<TypeDescriptor> {
    reference_descriptor(name).or_else(|| {
        crate::ClassName::parse(name)
            .ok()
            .map(TypeDescriptor::Reference)
    })
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
        crate::ClassName::parse(name)
            .ok()
            .map(TypeDescriptor::Reference)
    }
}
