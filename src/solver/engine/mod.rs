use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
};

use crate::cfg::build_cfg;
use crate::ir::MethodIr;
use crate::result::MethodHeader;
use crate::solver::frame::{Frame, inferred_from_descriptor};
use crate::solver::transfer::transfer;
use crate::{
    Diagnostic, DiagnosticKind, DiagnosticLocation, DiagnosticSeverity, InferenceConfig,
    MethodInference, MethodSummaryResolver,
};

mod observe;
mod propagate;

use observe::{
    collect_inferred_return_type, collect_local_types, collect_returned_parameter_index,
    observe_final_frames,
};
use propagate::{
    BranchFact, Propagation, branch_edge_is_feasible, branch_fact, instanceof_true_edge,
    instruction_may_throw, limit_diagnostic, merge_frame, propagate_exception_edges,
    propagate_subroutine_return,
};

pub(super) fn analyze_method(
    owner: &crate::ClassName,
    method: &MethodIr,
    config: &InferenceConfig,
    method_summaries: Option<&dyn MethodSummaryResolver>,
    field_summaries: Option<&dyn crate::FieldSummaryResolver>,
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
        if config.uses_local_variable_metadata() {
            Arc::from(method.local_variable_hints.clone())
        } else {
            Arc::default()
        },
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
                MethodHeader {
                    descriptor: method.descriptor.clone(),
                    generic_signature: method.generic_signature.clone(),
                    analysis_complete: true,
                    parameter_types,
                    return_type: method.descriptor.return_type().clone(),
                    inferred_return_type: None,
                    returned_parameter_index: None,
                },
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
    let mut analysis_complete = true;

    while let Some(block_id) = worklist.pop_front() {
        total_work_items += 1;
        if !config.unbounded_analysis() && total_work_items > config.max_work_items() {
            diagnostics.push(limit_diagnostic(method, "work-item budget"));
            analysis_complete = false;
            break;
        }

        let visits_for_block = visits.entry(block_id).or_insert(0_usize);
        *visits_for_block += 1;
        if !config.unbounded_analysis() && *visits_for_block > config.max_block_iterations() {
            diagnostics.push(limit_diagnostic(method, "per-block iteration budget"));
            analysis_complete = false;
            continue;
        }

        let block = &graph.blocks[block_id];
        let mut frame = incoming[&block_id].clone();
        let mut terminator_branch_fact = None;
        for instruction in &method.instructions[block.instruction_range.clone()] {
            let before = frame.clone();
            terminator_branch_fact = branch_fact(instruction.opcode, &before);
            transfer(
                method,
                instruction,
                &mut frame,
                &mut diagnostics,
                method_summaries,
                field_summaries,
            );
            if instruction_may_throw(instruction.opcode) {
                let mut propagation =
                    Propagation::new(method, &mut diagnostics, &mut worklist, hierarchy);
                propagate_exception_edges(
                    &block.exception_successors,
                    instruction.offset,
                    before,
                    &mut incoming,
                    &mut propagation,
                );
            }
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
            let mut propagation =
                Propagation::new(method, &mut diagnostics, &mut worklist, hierarchy);
            if merge_frame(
                &mut incoming,
                edge.target,
                outgoing,
                block.start_offset,
                &mut propagation,
            ) {
                propagation.enqueue(edge.target);
            }
        }

        let last_instruction = &method.instructions[block.instruction_range.end - 1];
        let mut propagation = Propagation::new(method, &mut diagnostics, &mut worklist, hierarchy);
        propagate_subroutine_return(
            &graph,
            last_instruction,
            &frame,
            &mut incoming,
            &mut propagation,
        );
    }

    let observations =
        observe_final_frames(method, &graph, &incoming, method_summaries, field_summaries);
    let local_types = collect_local_types(
        &incoming,
        &observations.instructions,
        entry_frame.locals,
        method,
        hierarchy,
    );
    let inferred_return_type = analysis_complete
        .then(|| collect_inferred_return_type(method, &observations.instructions, hierarchy))
        .flatten();
    let returned_parameter_index = inferred_return_type
        .as_ref()
        .and_then(|_| collect_returned_parameter_index(method, &observations.return_origins));
    let instructions = observations.instructions.into_values().collect();
    (
        MethodInference::new(
            method.name.clone(),
            MethodHeader {
                descriptor: method.descriptor.clone(),
                generic_signature: method.generic_signature.clone(),
                analysis_complete,
                parameter_types,
                return_type: method.descriptor.return_type().clone(),
                inferred_return_type,
                returned_parameter_index,
            },
            local_types,
            instructions,
        ),
        diagnostics,
    )
}
