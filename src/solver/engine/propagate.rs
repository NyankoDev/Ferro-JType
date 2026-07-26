use std::collections::{HashMap, VecDeque};

use crate::cfg::{BlockId, ControlFlowGraph, EdgeKind, ExceptionEdge};
use crate::ir::{InstructionIr, InstructionOperandIr, MethodIr};
use crate::solver::frame::Frame;
use crate::{
    Diagnostic, DiagnosticKind, DiagnosticLocation, DiagnosticSeverity, InferredType,
    ReferenceType, TypeHierarchy,
};
use rust_asm::opcodes as op;

pub(super) const fn instruction_may_throw(opcode: u8) -> bool {
    matches!(
        opcode,
        op::LDC..=op::LDC2_W
            | op::IALOAD..=op::SALOAD
            | op::IASTORE..=op::SASTORE
            | op::IDIV
            | op::LDIV
            | op::IREM
            | op::LREM
            | op::GETSTATIC..=op::INVOKEDYNAMIC
            | op::NEW..=op::MONITOREXIT
            | op::MULTIANEWARRAY
    )
}

#[derive(Debug, Clone)]
pub(super) enum BranchFact {
    InstanceOf(crate::solver::frame::InstanceOfFact),
    Null(bool),
}

pub(super) fn branch_fact(opcode: u8, frame: &Frame) -> Option<BranchFact> {
    match opcode {
        op::IFEQ | op::IFNE => frame.top_instanceof_fact().map(BranchFact::InstanceOf),
        op::IFNULL | op::IFNONNULL => {
            known_nullness(frame.top_value()?.value).map(BranchFact::Null)
        }
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
        | InferredType::Integral(_)
        | InferredType::Float
        | InferredType::Long
        | InferredType::Double
        | InferredType::Reference(ReferenceType::Unknown)
        | InferredType::ReturnAddress
        | InferredType::Alternatives(_)
        | InferredType::Conflict => None,
    }
}

pub(super) fn branch_edge_is_feasible(
    opcode: u8,
    edge_kind: &EdgeKind,
    fact: Option<&BranchFact>,
) -> bool {
    let Some(BranchFact::Null(is_null)) = fact else {
        return true;
    };
    let branch_taken = matches!(edge_kind, EdgeKind::Branch);
    match opcode {
        op::IFNULL => branch_taken == *is_null,
        op::IFNONNULL => branch_taken != *is_null,
        _ => true,
    }
}

pub(super) const fn instanceof_true_edge(opcode: u8, edge_kind: &EdgeKind) -> bool {
    matches!(
        (opcode, edge_kind),
        (op::IFEQ, EdgeKind::FallThrough) | (op::IFNE, EdgeKind::Branch)
    )
}

pub(super) struct Propagation<'a> {
    method: &'a MethodIr,
    diagnostics: &'a mut Vec<Diagnostic>,
    worklist: &'a mut VecDeque<BlockId>,
    hierarchy: Option<&'a dyn TypeHierarchy>,
}

impl<'a> Propagation<'a> {
    pub(super) fn new(
        method: &'a MethodIr,
        diagnostics: &'a mut Vec<Diagnostic>,
        worklist: &'a mut VecDeque<BlockId>,
        hierarchy: Option<&'a dyn TypeHierarchy>,
    ) -> Self {
        Self {
            method,
            diagnostics,
            worklist,
            hierarchy,
        }
    }

    pub(super) fn enqueue(&mut self, block_id: BlockId) {
        self.worklist.push_back(block_id);
    }
}

pub(super) fn propagate_exception_edges(
    edges: &[ExceptionEdge],
    instruction_offset: u16,
    before: Frame,
    incoming: &mut HashMap<BlockId, Frame>,
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
            propagation.enqueue(edge.target);
        }
    }
}

pub(super) fn propagate_subroutine_return(
    graph: &ControlFlowGraph,
    instruction: &InstructionIr,
    frame: &Frame,
    incoming: &mut HashMap<BlockId, Frame>,
    propagation: &mut Propagation<'_>,
) {
    if instruction.opcode != op::RET {
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
            propagation.enqueue(target);
        }
    }
}

pub(super) fn merge_frame(
    incoming: &mut HashMap<BlockId, Frame>,
    target: BlockId,
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

pub(super) fn limit_diagnostic(method: &MethodIr, limit: &str) -> Diagnostic {
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
