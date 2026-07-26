use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::cfg::{BlockId, ControlFlowGraph};
use crate::ir::{InstructionIr, InstructionOperandIr, MethodIr};
use crate::solver::frame::{Frame, ValueOrigin};
use crate::solver::transfer::transfer;
use crate::types::join_local_types;
use crate::{
    ClassName, InferredType, InstructionInference, MethodSummaryResolver, OperandConstraint,
    OperandExpectation, ReferenceType, ReturnType, TypeDescriptor,
};

pub(super) fn observe_final_frames(
    method: &MethodIr,
    graph: &ControlFlowGraph,
    incoming: &HashMap<BlockId, Frame>,
    method_summaries: Option<&dyn MethodSummaryResolver>,
    field_summaries: Option<&dyn crate::FieldSummaryResolver>,
) -> FinalObservations {
    let mut observations = BTreeMap::new();
    let mut return_origins = BTreeMap::new();
    let mut ignored_diagnostics = Vec::new();

    for (block_id, block) in graph.blocks.iter() {
        let Some(entry_frame) = incoming.get(&block_id) else {
            continue;
        };
        let mut frame = entry_frame.clone();

        for instruction in &method.instructions[block.instruction_range.clone()] {
            let before = frame.clone();
            if matches!(instruction.opcode, 0xac..=0xb0) {
                return_origins.insert(
                    instruction.offset,
                    before.top_value().and_then(|value| value.local_origin),
                );
            }
            transfer(
                method,
                instruction,
                &mut frame,
                &mut ignored_diagnostics,
                method_summaries,
                field_summaries,
            );
            observations.insert(
                instruction.offset,
                InstructionInference::new(
                    instruction.offset,
                    dynamic_call_kind(instruction),
                    operand_expectations(method, instruction, &before.stack),
                    before.local_types_at(instruction.offset),
                    before.stack,
                    frame.stack.clone(),
                ),
            );
        }
    }

    FinalObservations {
        instructions: observations,
        return_origins,
    }
}

fn operand_expectations(
    method: &MethodIr,
    instruction: &InstructionIr,
    stack_before: &[InferredType],
) -> Vec<OperandExpectation> {
    match instruction.opcode {
        0xb3 | 0xb5 => field_put_expectations(instruction, stack_before.len()),
        0xb6..=0xb9 => invocation_expectations(instruction, stack_before.len()),
        0xac..=0xb0 => return_expectations(method, stack_before.len()),
        _ => Vec::new(),
    }
}

fn field_put_expectations(
    instruction: &InstructionIr,
    stack_depth: usize,
) -> Vec<OperandExpectation> {
    let Some((owner, descriptor)) = resolved_member_reference(instruction) else {
        return Vec::new();
    };
    let Ok(descriptor) = TypeDescriptor::parse(descriptor) else {
        return Vec::new();
    };

    let mut constraints = Vec::with_capacity(2);
    if instruction.opcode == 0xb5 {
        constraints.push(OperandConstraint::ReceiverAssignableTo(owner.clone()));
    }
    constraints.push(OperandConstraint::Descriptor(descriptor));
    stack_expectations(stack_depth, constraints)
}

fn invocation_expectations(
    instruction: &InstructionIr,
    stack_depth: usize,
) -> Vec<OperandExpectation> {
    let Some((owner, descriptor)) = resolved_member_reference(instruction) else {
        return Vec::new();
    };
    let Ok(descriptor) = crate::MethodDescriptor::parse(descriptor) else {
        return Vec::new();
    };

    let mut constraints =
        Vec::with_capacity(descriptor.parameters().len() + usize::from(instruction.opcode != 0xb8));
    if instruction.opcode != 0xb8 {
        constraints.push(OperandConstraint::ReceiverAssignableTo(owner.clone()));
    }
    constraints.extend(
        descriptor
            .parameters()
            .iter()
            .cloned()
            .map(OperandConstraint::Descriptor),
    );
    stack_expectations(stack_depth, constraints)
}

fn return_expectations(method: &MethodIr, stack_depth: usize) -> Vec<OperandExpectation> {
    let ReturnType::Type(descriptor) = method.descriptor.return_type() else {
        return Vec::new();
    };
    stack_expectations(
        stack_depth,
        vec![OperandConstraint::Descriptor(descriptor.clone())],
    )
}

fn stack_expectations(
    stack_depth: usize,
    constraints: Vec<OperandConstraint>,
) -> Vec<OperandExpectation> {
    let Some(start_index) = stack_depth.checked_sub(constraints.len()) else {
        return Vec::new();
    };
    constraints
        .into_iter()
        .enumerate()
        .map(|(offset, constraint)| OperandExpectation::new(start_index + offset, constraint))
        .collect()
}

fn resolved_member_reference(instruction: &InstructionIr) -> Option<(&ClassName, &str)> {
    let member = match &instruction.operand {
        InstructionOperandIr::Member(member) => member,
        InstructionOperandIr::InvokeInterface { method, .. } => method,
        _ => return None,
    };
    let crate::ir::MemberRefIr::Resolved {
        owner, descriptor, ..
    } = member
    else {
        return None;
    };
    Some((owner, descriptor))
}

pub(super) struct FinalObservations {
    pub(super) instructions: BTreeMap<u16, InstructionInference>,
    pub(super) return_origins: BTreeMap<u16, Option<ValueOrigin>>,
}

pub(super) fn collect_inferred_return_type(
    method: &MethodIr,
    observations: &BTreeMap<u16, InstructionInference>,
    hierarchy: Option<&dyn crate::TypeHierarchy>,
) -> Option<InferredType> {
    let ReturnType::Type(declared_return_type) = method.descriptor.return_type() else {
        return None;
    };
    let mut inferred_return_type = None;

    for instruction in method
        .instructions
        .iter()
        .filter(|instruction| matches!(instruction.opcode, 0xac..=0xb0))
    {
        if !return_opcode_matches_descriptor(instruction.opcode, declared_return_type) {
            return None;
        }
        let Some(return_type) = observations
            .get(&instruction.offset)
            .and_then(|instruction| instruction.stack_before().last())
        else {
            continue;
        };
        if !return_value_matches_opcode(instruction.opcode, return_type) {
            return None;
        }
        inferred_return_type = Some(match inferred_return_type {
            Some(existing) => join_local_types(&existing, return_type, hierarchy),
            None => return_type.clone(),
        });
    }

    inferred_return_type
}

pub(super) fn collect_returned_parameter_index(
    method: &MethodIr,
    return_origins: &BTreeMap<u16, Option<ValueOrigin>>,
) -> Option<usize> {
    let mut parameter_index = None;
    for instruction in method
        .instructions
        .iter()
        .filter(|instruction| matches!(instruction.opcode, 0xac..=0xb0))
    {
        let Some(origin) = return_origins.get(&instruction.offset) else {
            continue;
        };
        let Some(ValueOrigin::Entry(slot)) = origin else {
            return None;
        };
        let index = parameter_index_for_local_slot(method, *slot)?;
        if let Some(previous) = parameter_index
            && previous != index
        {
            return None;
        }
        parameter_index = Some(index);
    }
    parameter_index
}

fn parameter_index_for_local_slot(method: &MethodIr, local_slot: u16) -> Option<usize> {
    let mut slot = u16::from(method.access_flags & 0x0008 == 0);
    for (index, parameter) in method.descriptor.parameters().iter().enumerate() {
        if slot == local_slot {
            return Some(index);
        }
        slot = slot.checked_add(u16::from(parameter.slot_width()))?;
    }
    None
}

fn return_opcode_matches_descriptor(opcode: u8, descriptor: &TypeDescriptor) -> bool {
    matches!(
        (opcode, descriptor),
        (
            0xac,
            TypeDescriptor::Primitive(
                crate::PrimitiveType::Boolean
                    | crate::PrimitiveType::Byte
                    | crate::PrimitiveType::Char
                    | crate::PrimitiveType::Short
                    | crate::PrimitiveType::Int
            )
        ) | (0xad, TypeDescriptor::Primitive(crate::PrimitiveType::Long))
            | (0xae, TypeDescriptor::Primitive(crate::PrimitiveType::Float))
            | (
                0xaf,
                TypeDescriptor::Primitive(crate::PrimitiveType::Double)
            )
            | (
                0xb0,
                TypeDescriptor::Reference(_) | TypeDescriptor::Array { .. }
            )
    )
}

fn return_value_matches_opcode(opcode: u8, value: &InferredType) -> bool {
    (opcode == 0xac && value.integral_types().is_some())
        || matches!(
            (opcode, value),
            (0xad, InferredType::Long) | (0xae, InferredType::Float) | (0xaf, InferredType::Double)
        )
        || (opcode == 0xb0 && reference_value(value))
}

fn reference_value(value: &InferredType) -> bool {
    match value {
        InferredType::Reference(_) => true,
        InferredType::Alternatives(values) => values.iter().all(reference_value),
        _ => false,
    }
}

fn dynamic_call_kind(instruction: &InstructionIr) -> Option<crate::DynamicCallKind> {
    let InstructionOperandIr::InvokeDynamic { kind, .. } = instruction.operand else {
        return None;
    };
    Some(kind)
}

pub(super) fn collect_local_types(
    incoming: &HashMap<BlockId, Frame>,
    observations: &BTreeMap<u16, InstructionInference>,
    entry_locals: Vec<InferredType>,
    method: &MethodIr,
    hierarchy: Option<&dyn crate::TypeHierarchy>,
) -> Vec<InferredType> {
    let mut locals = entry_locals;
    for frame in incoming.values() {
        merge_locals(&mut locals, &frame.locals, hierarchy);
    }
    for observation in observations.values() {
        merge_locals(&mut locals, observation.local_types(), hierarchy);
    }
    refine_catch_local_types(&mut locals, incoming, observations, method);
    locals
}

fn refine_catch_local_types(
    locals: &mut [InferredType],
    incoming: &HashMap<BlockId, Frame>,
    observations: &BTreeMap<u16, InstructionInference>,
    method: &MethodIr,
) {
    for (slot, catch_types) in catch_local_types(method) {
        let Some(local) = locals.get_mut(usize::from(slot)) else {
            continue;
        };
        if catch_types.len() > 1
            && matches!(local, InferredType::Reference(ReferenceType::Unknown))
            && local_values_are_catch_types(slot, &catch_types, incoming, observations)
        {
            *local =
                InferredType::Reference(ReferenceType::Exact(ClassName::java_lang_throwable()));
        }
    }
}

fn catch_local_types(method: &MethodIr) -> BTreeMap<u16, BTreeSet<ClassName>> {
    let mut catch_locals = BTreeMap::new();
    for handler in &method.exception_handlers {
        let Some(instruction) = method
            .instructions
            .iter()
            .find(|instruction| instruction.offset == handler.handler_offset)
        else {
            continue;
        };
        let Some(slot) = reference_store_local(instruction) else {
            continue;
        };

        catch_locals
            .entry(slot)
            .or_insert_with(BTreeSet::new)
            .insert(
                handler
                    .catch_type
                    .clone()
                    .unwrap_or_else(ClassName::java_lang_throwable),
            );
    }
    catch_locals
}

fn reference_store_local(instruction: &InstructionIr) -> Option<u16> {
    match instruction {
        InstructionIr {
            opcode: 0x3a,
            operand: InstructionOperandIr::Local(slot),
            ..
        } => Some(*slot),
        InstructionIr {
            opcode: 0x4b..=0x4e,
            ..
        } => Some(u16::from(instruction.opcode - 0x4b)),
        _ => None,
    }
}

fn local_values_are_catch_types(
    slot: u16,
    catch_types: &BTreeSet<ClassName>,
    incoming: &HashMap<BlockId, Frame>,
    observations: &BTreeMap<u16, InstructionInference>,
) -> bool {
    let mut saw_catch_value = false;
    for values in incoming
        .values()
        .map(|frame| frame.locals.as_slice())
        .chain(
            observations
                .values()
                .map(|instruction| instruction.local_types()),
        )
    {
        let Some(value) = values.get(usize::from(slot)) else {
            continue;
        };
        match value {
            InferredType::Bottom => {}
            InferredType::Reference(ReferenceType::Exact(class_name))
                if catch_types.contains(class_name) =>
            {
                saw_catch_value = true;
            }
            _ => return false,
        }
    }
    saw_catch_value
}

fn merge_locals(
    destination: &mut Vec<InferredType>,
    source: &[InferredType],
    hierarchy: Option<&dyn crate::TypeHierarchy>,
) {
    destination.resize(destination.len().max(source.len()), InferredType::Bottom);
    for (destination, source) in destination.iter_mut().zip(source) {
        *destination = join_local_types(destination, source, hierarchy);
    }
}
