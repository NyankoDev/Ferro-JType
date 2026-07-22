use std::collections::{BTreeMap, BTreeSet};

use la_arena::Arena;

use crate::cfg::{BasicBlock, BlockId, ControlFlowGraph, Edge, EdgeKind};
use crate::ir::{InstructionOperandIr, MethodIr};
use crate::{Diagnostic, DiagnosticKind, DiagnosticLocation, DiagnosticSeverity};

pub(crate) struct CfgBuildResult {
    pub(crate) graph: ControlFlowGraph,
    pub(crate) diagnostics: Vec<Diagnostic>,
}

pub(crate) fn build_cfg(method: &MethodIr) -> CfgBuildResult {
    if method.instructions.is_empty() {
        return CfgBuildResult {
            graph: ControlFlowGraph {
                blocks: Arena::new(),
                entry: None,
            },
            diagnostics: Vec::new(),
        };
    }

    let instruction_positions = method
        .instructions
        .iter()
        .enumerate()
        .map(|(index, instruction)| (instruction.offset, index))
        .collect::<BTreeMap<_, _>>();
    let mut starts = BTreeSet::from([method.instructions[0].offset]);
    let mut diagnostics = Vec::new();

    for offset in method.verification_frames.keys() {
        if instruction_positions.contains_key(offset) {
            starts.insert(*offset);
        }
    }

    for (index, instruction) in method.instructions.iter().enumerate() {
        let has_next = method.instructions.get(index + 1).map(|next| next.offset);
        let control_targets = instruction_targets(instruction);

        for target in control_targets {
            add_start_if_instruction(
                &mut starts,
                &instruction_positions,
                target,
                method,
                instruction.offset,
                &mut diagnostics,
            );
        }

        if ends_block(instruction.opcode)
            && let Some(next_offset) = has_next
        {
            starts.insert(next_offset);
        }
    }

    for handler in &method.exception_handlers {
        add_start_if_instruction(
            &mut starts,
            &instruction_positions,
            i32::from(handler.start_offset),
            method,
            handler.start_offset,
            &mut diagnostics,
        );
        add_start_if_instruction(
            &mut starts,
            &instruction_positions,
            i32::from(handler.handler_offset),
            method,
            handler.handler_offset,
            &mut diagnostics,
        );
        if instruction_positions.contains_key(&handler.end_offset) {
            starts.insert(handler.end_offset);
        }
    }

    let ordered_starts = starts.into_iter().collect::<Vec<_>>();
    let mut blocks = Arena::with_capacity(ordered_starts.len());
    let mut block_by_offset = BTreeMap::new();

    for (index, start_offset) in ordered_starts.iter().copied().enumerate() {
        let start_index = instruction_positions[&start_offset];
        let end_index = ordered_starts
            .get(index + 1)
            .and_then(|next_offset| instruction_positions.get(next_offset).copied())
            .unwrap_or(method.instructions.len());
        let block = blocks.alloc(BasicBlock {
            start_offset,
            instruction_range: start_index..end_index,
            successors: Vec::new(),
        });
        block_by_offset.insert(start_offset, block);
    }

    for (_, block) in blocks.iter_mut() {
        let last_index = block.instruction_range.end - 1;
        let last_instruction = &method.instructions[last_index];
        let successors = ordinary_successors(last_instruction, &instruction_positions);

        for (target, kind) in successors {
            if let Some(target) = resolve_block(&block_by_offset, target) {
                push_edge(&mut block.successors, target, kind);
            } else {
                diagnostics.push(invalid_target_diagnostic(
                    method,
                    last_instruction.offset,
                    target,
                ));
            }
        }

        for handler in &method.exception_handlers {
            if block.start_offset >= handler.start_offset
                && block.start_offset < handler.end_offset
                && let Some(target) = block_by_offset.get(&handler.handler_offset).copied()
            {
                push_edge(
                    &mut block.successors,
                    target,
                    EdgeKind::Exception {
                        catch_type: handler.catch_type.clone(),
                    },
                );
            }
        }
    }

    let entry = block_by_offset.get(&method.instructions[0].offset).copied();
    CfgBuildResult {
        graph: ControlFlowGraph { blocks, entry },
        diagnostics,
    }
}

fn add_start_if_instruction(
    starts: &mut BTreeSet<u16>,
    instruction_positions: &BTreeMap<u16, usize>,
    target: i32,
    method: &MethodIr,
    source_offset: u16,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let Ok(target) = u16::try_from(target) else {
        diagnostics.push(invalid_target_diagnostic(method, source_offset, target));
        return;
    };

    if instruction_positions.contains_key(&target) {
        starts.insert(target);
    } else {
        diagnostics.push(invalid_target_diagnostic(
            method,
            source_offset,
            i32::from(target),
        ));
    }
}

fn instruction_targets(instruction: &crate::ir::InstructionIr) -> Vec<i32> {
    match &instruction.operand {
        InstructionOperandIr::Branch { target } => vec![*target],
        InstructionOperandIr::TableSwitch {
            default_target,
            targets,
        } => std::iter::once(*default_target)
            .chain(targets.iter().copied())
            .collect(),
        InstructionOperandIr::LookupSwitch {
            default_target,
            targets,
        } => std::iter::once(*default_target)
            .chain(targets.iter().map(|(_, target)| *target))
            .collect(),
        _ => Vec::new(),
    }
}

fn ordinary_successors(
    instruction: &crate::ir::InstructionIr,
    instruction_positions: &BTreeMap<u16, usize>,
) -> Vec<(i32, EdgeKind)> {
    let fall_through = instruction_positions
        .range((
            std::ops::Bound::Excluded(instruction.offset),
            std::ops::Bound::Unbounded,
        ))
        .next()
        .map(|(offset, _)| i32::from(*offset));

    match &instruction.operand {
        InstructionOperandIr::Branch { target } if is_unconditional_branch(instruction.opcode) => {
            vec![(*target, EdgeKind::Branch)]
        }
        InstructionOperandIr::Branch { target } if is_subroutine_branch(instruction.opcode) => {
            let mut successors = vec![(*target, EdgeKind::Branch)];
            if let Some(fall_through) = fall_through {
                successors.push((fall_through, EdgeKind::FallThrough));
            }
            successors
        }
        InstructionOperandIr::Branch { target } => {
            let mut successors = vec![(*target, EdgeKind::Branch)];
            if let Some(fall_through) = fall_through {
                successors.push((fall_through, EdgeKind::FallThrough));
            }
            successors
        }
        InstructionOperandIr::TableSwitch {
            default_target,
            targets,
        } => std::iter::once((*default_target, EdgeKind::Switch))
            .chain(
                targets
                    .iter()
                    .copied()
                    .map(|target| (target, EdgeKind::Switch)),
            )
            .collect(),
        InstructionOperandIr::LookupSwitch {
            default_target,
            targets,
        } => std::iter::once((*default_target, EdgeKind::Switch))
            .chain(
                targets
                    .iter()
                    .map(|(_, target)| (*target, EdgeKind::Switch)),
            )
            .collect(),
        _ if terminates_execution(instruction.opcode) => Vec::new(),
        _ => fall_through
            .map(|target| vec![(target, EdgeKind::FallThrough)])
            .unwrap_or_default(),
    }
}

fn resolve_block(block_by_offset: &BTreeMap<u16, BlockId>, offset: i32) -> Option<BlockId> {
    u16::try_from(offset)
        .ok()
        .and_then(|offset| block_by_offset.get(&offset).copied())
}

fn push_edge(edges: &mut Vec<Edge>, target: BlockId, kind: EdgeKind) {
    if !edges
        .iter()
        .any(|edge| edge.target == target && edge.kind == kind)
    {
        edges.push(Edge { target, kind });
    }
}

fn invalid_target_diagnostic(method: &MethodIr, source_offset: u16, target: i32) -> Diagnostic {
    Diagnostic::new(
        DiagnosticSeverity::Warning,
        DiagnosticKind::InvalidControlFlow,
        DiagnosticLocation::method(&method.name, &method.descriptor_text).at_offset(source_offset),
        format!("control-flow target {target} does not identify an instruction"),
    )
}

const fn ends_block(opcode: u8) -> bool {
    matches!(opcode, 0x99..=0xa8 | 0xaa | 0xab | 0xac..=0xb1 | 0xbf | 0xc6..=0xc9)
}

const fn terminates_execution(opcode: u8) -> bool {
    matches!(opcode, 0xac..=0xb1 | 0xbf)
}

const fn is_unconditional_branch(opcode: u8) -> bool {
    matches!(opcode, 0xa7 | 0xc8)
}

const fn is_subroutine_branch(opcode: u8) -> bool {
    matches!(opcode, 0xa8 | 0xc9)
}
