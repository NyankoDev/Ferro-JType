use std::collections::{BTreeMap, HashMap, VecDeque};

use crate::cfg::{EdgeKind, build_cfg};
use crate::ir::{ClassIr, MethodIr};
use crate::solver::frame::{Frame, inferred_from_descriptor};
use crate::solver::transfer::transfer;
use crate::{
    ClassInference, Diagnostic, DiagnosticKind, DiagnosticLocation, DiagnosticSeverity, Error,
    InferenceConfig, InferredType, InstructionInference, MethodInference,
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
            .find(|diagnostic| diagnostic.severity() == DiagnosticSeverity::Error)
    {
        return Err(Error::StrictAnalysis {
            message: diagnostic.message().to_owned(),
        });
    }

    Ok(ClassInference::new(
        class.name.clone(),
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
    let mut diagnostics = cfg_result.diagnostics;
    let entry_frame = Frame::entry(
        owner,
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
    let mut observations = BTreeMap::new();

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
        for instruction in &method.instructions[block.instruction_range.clone()] {
            let before = frame.clone();
            transfer(method, instruction, &mut frame, &mut diagnostics);
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
            observations.insert(
                instruction.offset,
                InstructionInference::new(
                    instruction.offset,
                    before.locals,
                    before.stack,
                    frame.stack.clone(),
                ),
            );
        }

        for edge in &block.successors {
            let outgoing = match &edge.kind {
                EdgeKind::Exception { catch_type } => frame.exception_frame(catch_type.clone()),
                EdgeKind::FallThrough | EdgeKind::Branch | EdgeKind::Switch => frame.clone(),
            };
            let changed = merge_frame(
                &mut incoming,
                edge.target,
                outgoing,
                method,
                block.start_offset,
                &mut diagnostics,
            );
            if changed {
                worklist.push_back(edge.target);
            }
        }
    }

    let local_types = collect_local_types(&incoming, &observations, entry_frame.locals);
    let instructions = observations.into_values().collect();
    (
        MethodInference::new(
            method.name.clone(),
            method.descriptor.clone(),
            parameter_types,
            method.descriptor.return_type().clone(),
            local_types,
            instructions,
        ),
        diagnostics,
    )
}

fn merge_frame(
    incoming: &mut HashMap<crate::cfg::BlockId, Frame>,
    target: crate::cfg::BlockId,
    outgoing: Frame,
    method: &MethodIr,
    offset: u16,
    diagnostics: &mut Vec<Diagnostic>,
) -> bool {
    let Some(existing) = incoming.get_mut(&target) else {
        incoming.insert(target, outgoing);
        return true;
    };

    let outcome = existing.merge_from(&outgoing);
    if outcome.stack_height_mismatch {
        diagnostics.push(Diagnostic::new(
            DiagnosticSeverity::Warning,
            DiagnosticKind::StackHeightMismatch,
            DiagnosticLocation::method(&method.name, &method.descriptor_text).at_offset(offset),
            "control-flow paths reached a block with different operand-stack heights",
        ));
    }
    outcome.changed
}

fn collect_local_types(
    incoming: &HashMap<crate::cfg::BlockId, Frame>,
    observations: &BTreeMap<u16, InstructionInference>,
    entry_locals: Vec<InferredType>,
) -> Vec<InferredType> {
    let mut locals = entry_locals;
    for frame in incoming.values() {
        merge_locals(&mut locals, &frame.locals);
    }
    for observation in observations.values() {
        merge_locals(&mut locals, observation.local_types());
    }
    locals
}

fn merge_locals(destination: &mut Vec<InferredType>, source: &[InferredType]) {
    destination.resize(destination.len().max(source.len()), InferredType::Bottom);
    for (destination, source) in destination.iter_mut().zip(source) {
        *destination = destination.join(source);
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
