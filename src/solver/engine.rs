use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};

use crate::cfg::{EdgeKind, build_cfg};
use crate::ir::{ClassIr, InstructionIr, InstructionOperandIr, MethodIr};
use crate::solver::frame::{Frame, inferred_from_descriptor};
use crate::solver::transfer::transfer;
use crate::types::join_local_types;
use crate::{
    ClassInference, ClassName, Diagnostic, DiagnosticKind, DiagnosticLocation, DiagnosticSeverity,
    Error, InferenceConfig, InferredType, InstructionInference, MethodInference, ReferenceType,
};

pub(crate) fn analyze_class(
    class: &ClassIr,
    config: &InferenceConfig,
) -> Result<ClassInference, Error> {
    let mut diagnostics = Vec::new();
    let methods = class
        .methods
        .iter()
        .map(|method| {
            let (inference, method_diagnostics) = analyze_method(&class.name, method, config);
            diagnostics.extend(method_diagnostics);
            inference
        })
        .collect();

    if config.strict()
        && let Some(diagnostic) = diagnostics
            .iter()
            .find(|diagnostic| diagnostic.severity() != DiagnosticSeverity::Note)
    {
        return Err(Error::StrictAnalysis {
            message: diagnostic.message().to_owned(),
        });
    }

    Ok(ClassInference::new(
        class.name.clone(),
        class.generic_signature.clone(),
        methods,
        diagnostics,
    ))
}

fn analyze_method(
    owner: &crate::ClassName,
    method: &MethodIr,
    config: &InferenceConfig,
) -> (MethodInference, Vec<Diagnostic>) {
    let cfg_result = build_cfg(method);
    let graph = cfg_result.graph;
    let hierarchy = config.type_hierarchy();
    let mut diagnostics = cfg_result.diagnostics;
    let entry_frame = Frame::entry(
        owner,
        method.name == "<init>",
        &method.descriptor,
        method.access_flags,
        method.max_locals,
    );
    let parameter_types = method
        .descriptor
        .parameters()
        .iter()
        .map(inferred_from_descriptor)
        .collect();

    let Some(entry) = graph.entry else {
        return (
            MethodInference::new(
                method.name.clone(),
                method.descriptor.clone(),
                method.generic_signature.clone(),
                parameter_types,
                method.descriptor.return_type().clone(),
                entry_frame.locals,
                Vec::new(),
            ),
            diagnostics,
        );
    };

    let mut incoming = HashMap::from([(entry, entry_frame.clone())]);
    let mut worklist = VecDeque::from([entry]);
    let mut visits = HashMap::new();
    let mut total_work_items = 0_usize;

    while let Some(block_id) = worklist.pop_front() {
        total_work_items += 1;
        if total_work_items > config.max_work_items() {
            diagnostics.push(limit_diagnostic(method, "work-item budget"));
            break;
        }

        let visits_for_block = visits.entry(block_id).or_insert(0_usize);
        *visits_for_block += 1;
        if *visits_for_block > config.max_block_iterations() {
            diagnostics.push(limit_diagnostic(method, "per-block iteration budget"));
            continue;
        }

        let block = &graph.blocks[block_id];
        let mut frame = incoming[&block_id].clone();
        let mut terminator_branch_fact = None;
        for instruction in &method.instructions[block.instruction_range.clone()] {
            let before = frame.clone();
            terminator_branch_fact = branch_fact(instruction.opcode, &before);
            transfer(method, instruction, &mut frame, &mut diagnostics);
            let mut propagation = Propagation {
                method,
                diagnostics: &mut diagnostics,
                worklist: &mut worklist,
                hierarchy,
            };
            propagate_exception_edges(
                &block.exception_successors,
                instruction.offset,
                before,
                &mut incoming,
                &mut propagation,
            );
            if frame.stack.len() > usize::from(method.max_stack) {
                diagnostics.push(Diagnostic::new(
                    DiagnosticSeverity::Warning,
                    DiagnosticKind::StackHeightMismatch,
                    DiagnosticLocation::method(&method.name, &method.descriptor_text)
                        .at_offset(instruction.offset),
                    format!(
                        "inferred operand stack height {} exceeds declared max_stack {}",
                        frame.stack.len(),
                        method.max_stack
                    ),
                ));
            }
        }

        for edge in &block.successors {
            let mut outgoing = frame.clone();
            let terminator = method.instructions[block.instruction_range.end - 1].opcode;
            if !branch_edge_is_feasible(terminator, &edge.kind, terminator_branch_fact.as_ref()) {
                continue;
            }
            if let Some(BranchFact::InstanceOf(fact)) = &terminator_branch_fact
                && instanceof_true_edge(terminator, &edge.kind)
            {
                outgoing.refine_origin(fact.origin, fact.reference.clone());
            }
            let mut propagation = Propagation {
                method,
                diagnostics: &mut diagnostics,
                worklist: &mut worklist,
                hierarchy,
            };
            if merge_frame(
                &mut incoming,
                edge.target,
                outgoing,
                block.start_offset,
                &mut propagation,
            ) {
                propagation.worklist.push_back(edge.target);
            }
        }

        let last_instruction = &method.instructions[block.instruction_range.end - 1];
        let mut propagation = Propagation {
            method,
            diagnostics: &mut diagnostics,
            worklist: &mut worklist,
            hierarchy,
        };
        propagate_subroutine_return(
            &graph,
            last_instruction,
            &frame,
            &mut incoming,
            &mut propagation,
        );
    }

    let observations = observe_final_frames(method, &graph, &incoming);
    let local_types = collect_local_types(
        &incoming,
        &observations,
        entry_frame.locals,
        method,
        hierarchy,
    );
    let instructions = observations.into_values().collect();
    (
        MethodInference::new(
            method.name.clone(),
            method.descriptor.clone(),
            method.generic_signature.clone(),
            parameter_types,
            method.descriptor.return_type().clone(),
            local_types,
            instructions,
        ),
        diagnostics,
    )
}

#[derive(Debug, Clone)]
enum BranchFact {
    InstanceOf(crate::solver::frame::InstanceOfFact),
    Null(bool),
}

fn branch_fact(opcode: u8, frame: &Frame) -> Option<BranchFact> {
    match opcode {
        0x99 | 0x9a => frame.top_instanceof_fact().map(BranchFact::InstanceOf),
        0xc6 | 0xc7 => known_nullness(frame.top_value()?.value).map(BranchFact::Null),
        _ => None,
    }
}

fn known_nullness(value: InferredType) -> Option<bool> {
    match value {
        InferredType::Reference(ReferenceType::Null) => Some(true),
        InferredType::Reference(ReferenceType::Exact(_) | ReferenceType::Array(_))
        | InferredType::Uninitialized { .. }
        | InferredType::UninitializedThis { .. } => Some(false),
        InferredType::Bottom
        | InferredType::Int
        | InferredType::Float
        | InferredType::Long
        | InferredType::Double
        | InferredType::Reference(ReferenceType::Unknown)
        | InferredType::ReturnAddress
        | InferredType::Alternatives(_)
        | InferredType::Conflict => None,
    }
}

fn branch_edge_is_feasible(opcode: u8, edge_kind: &EdgeKind, fact: Option<&BranchFact>) -> bool {
    let Some(BranchFact::Null(is_null)) = fact else {
        return true;
    };
    let branch_taken = matches!(edge_kind, EdgeKind::Branch);
    match opcode {
        0xc6 => branch_taken == *is_null,
        0xc7 => branch_taken != *is_null,
        _ => true,
    }
}

const fn instanceof_true_edge(opcode: u8, edge_kind: &EdgeKind) -> bool {
    matches!(
        (opcode, edge_kind),
        (0x99, EdgeKind::FallThrough) | (0x9a, EdgeKind::Branch)
    )
}

struct Propagation<'a> {
    method: &'a MethodIr,
    diagnostics: &'a mut Vec<Diagnostic>,
    worklist: &'a mut VecDeque<crate::cfg::BlockId>,
    hierarchy: Option<&'a dyn crate::TypeHierarchy>,
}

fn propagate_exception_edges(
    edges: &[crate::cfg::ExceptionEdge],
    instruction_offset: u16,
    before: Frame,
    incoming: &mut HashMap<crate::cfg::BlockId, Frame>,
    propagation: &mut Propagation<'_>,
) {
    for edge in edges
        .iter()
        .filter(|edge| edge.instruction_offset == instruction_offset)
    {
        let outgoing = before.exception_frame(edge.catch_type.clone());
        if merge_frame(
            incoming,
            edge.target,
            outgoing,
            instruction_offset,
            propagation,
        ) {
            propagation.worklist.push_back(edge.target);
        }
    }
}

fn propagate_subroutine_return(
    graph: &crate::cfg::ControlFlowGraph,
    instruction: &InstructionIr,
    frame: &Frame,
    incoming: &mut HashMap<crate::cfg::BlockId, Frame>,
    propagation: &mut Propagation<'_>,
) {
    if instruction.opcode != 0xa9 {
        return;
    }

    let InstructionOperandIr::Local(local) = instruction.operand else {
        propagation.diagnostics.push(Diagnostic::new(
            DiagnosticSeverity::Warning,
            DiagnosticKind::InvalidControlFlow,
            location(propagation.method, instruction.offset),
            "ret instruction does not identify a local-variable slot",
        ));
        return;
    };
    let Some(targets) = frame.local_return_targets(local) else {
        propagation.diagnostics.push(Diagnostic::new(
            DiagnosticSeverity::Warning,
            DiagnosticKind::InvalidControlFlow,
            location(propagation.method, instruction.offset),
            format!("ret local slot {local} has no known return address"),
        ));
        return;
    };

    for target_offset in targets {
        let Some(target) = graph.block_at_offset(*target_offset) else {
            propagation.diagnostics.push(Diagnostic::new(
                DiagnosticSeverity::Warning,
                DiagnosticKind::InvalidControlFlow,
                location(propagation.method, instruction.offset),
                format!("ret target {target_offset} does not identify an instruction"),
            ));
            continue;
        };
        if merge_frame(
            incoming,
            target,
            frame.clone(),
            instruction.offset,
            propagation,
        ) {
            propagation.worklist.push_back(target);
        }
    }
}

fn observe_final_frames(
    method: &MethodIr,
    graph: &crate::cfg::ControlFlowGraph,
    incoming: &HashMap<crate::cfg::BlockId, Frame>,
) -> BTreeMap<u16, InstructionInference> {
    let mut observations = BTreeMap::new();
    let mut ignored_diagnostics = Vec::new();

    for (block_id, block) in graph.blocks.iter() {
        let Some(entry_frame) = incoming.get(&block_id) else {
            continue;
        };
        let mut frame = entry_frame.clone();

        for instruction in &method.instructions[block.instruction_range.clone()] {
            let before = frame.clone();
            transfer(method, instruction, &mut frame, &mut ignored_diagnostics);
            observations.insert(
                instruction.offset,
                InstructionInference::new(
                    instruction.offset,
                    dynamic_call_kind(instruction),
                    before.locals,
                    before.stack,
                    frame.stack.clone(),
                ),
            );
        }
    }

    observations
}

fn dynamic_call_kind(instruction: &InstructionIr) -> Option<crate::DynamicCallKind> {
    let InstructionOperandIr::InvokeDynamic { kind, .. } = instruction.operand else {
        return None;
    };
    Some(kind)
}

fn merge_frame(
    incoming: &mut HashMap<crate::cfg::BlockId, Frame>,
    target: crate::cfg::BlockId,
    outgoing: Frame,
    offset: u16,
    propagation: &mut Propagation<'_>,
) -> bool {
    let Some(existing) = incoming.get_mut(&target) else {
        incoming.insert(target, outgoing);
        return true;
    };

    let previous = existing.clone();
    let outcome = existing.merge_from(&outgoing, propagation.hierarchy);
    if outcome.stack_height_mismatch {
        propagation.diagnostics.push(Diagnostic::new(
            DiagnosticSeverity::Warning,
            DiagnosticKind::StackHeightMismatch,
            DiagnosticLocation::method(
                &propagation.method.name,
                &propagation.method.descriptor_text,
            )
            .at_offset(offset),
            "control-flow paths reached a block with different operand-stack heights",
        ));
    }
    *existing != previous
}

fn collect_local_types(
    incoming: &HashMap<crate::cfg::BlockId, Frame>,
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
    incoming: &HashMap<crate::cfg::BlockId, Frame>,
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
    incoming: &HashMap<crate::cfg::BlockId, Frame>,
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

fn limit_diagnostic(method: &MethodIr, limit: &str) -> Diagnostic {
    Diagnostic::new(
        DiagnosticSeverity::Error,
        DiagnosticKind::AnalysisLimitReached,
        DiagnosticLocation::method(&method.name, &method.descriptor_text),
        format!("analysis stopped after reaching the {limit}"),
    )
}

fn location(method: &MethodIr, offset: u16) -> DiagnosticLocation {
    DiagnosticLocation::method(&method.name, &method.descriptor_text).at_offset(offset)
}
