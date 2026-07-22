use std::collections::BTreeMap;
use std::ops::Range;

use la_arena::{Arena, Idx};

use crate::ClassName;

pub(crate) type BlockId = Idx<BasicBlock>;

#[derive(Debug, Clone)]
pub(crate) struct ControlFlowGraph {
    pub(crate) blocks: Arena<BasicBlock>,
    pub(crate) entry: Option<BlockId>,
    pub(crate) blocks_by_offset: BTreeMap<u16, BlockId>,
}

impl ControlFlowGraph {
    pub(crate) fn block_at_offset(&self, offset: u16) -> Option<BlockId> {
        self.blocks_by_offset.get(&offset).copied()
    }
}

#[derive(Debug, Clone)]
pub(crate) struct BasicBlock {
    pub(crate) start_offset: u16,
    pub(crate) instruction_range: Range<usize>,
    pub(crate) successors: Vec<Edge>,
    pub(crate) exception_successors: Vec<ExceptionEdge>,
}

#[derive(Debug, Clone)]
pub(crate) struct Edge {
    pub(crate) target: BlockId,
    pub(crate) kind: EdgeKind,
}

#[derive(Debug, Clone)]
pub(crate) struct ExceptionEdge {
    pub(crate) instruction_offset: u16,
    pub(crate) target: BlockId,
    pub(crate) catch_type: Option<ClassName>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum EdgeKind {
    FallThrough,
    Branch,
    Switch,
}
