mod classfile;
mod lower;
mod model;

pub(crate) use classfile::strip_stack_map_tables;
pub(crate) use lower::parse_and_lower;
pub(crate) use model::{
    ClassIr, ConstantKind, ExceptionHandlerIr, InstructionIr, InstructionOperandIr, MemberRefIr,
    MethodIr,
};
