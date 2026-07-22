mod lower;
mod model;

pub(crate) use lower::parse_and_lower;
pub(crate) use model::{
    ClassIr, ConstantKind, ExceptionHandlerIr, InstructionIr, InstructionOperandIr, MemberRefIr,
    MethodIr,
};
