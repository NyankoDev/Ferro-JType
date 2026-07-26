/// JVM member-invocation dispatch encoded by a bytecode instruction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MethodInvocationKind {
    /// Virtual class-method dispatch through `invokevirtual`.
    Virtual,
    /// Exact dispatch through `invokespecial`.
    Special,
    /// Exact class-method dispatch through `invokestatic`.
    Static,
    /// Interface dispatch through `invokeinterface`.
    Interface,
}

impl MethodInvocationKind {
    pub(crate) const fn from_opcode(opcode: u8) -> Option<Self> {
        match opcode {
            op::INVOKEVIRTUAL => Some(Self::Virtual),
            op::INVOKESPECIAL => Some(Self::Special),
            op::INVOKESTATIC => Some(Self::Static),
            op::INVOKEINTERFACE => Some(Self::Interface),
            _ => None,
        }
    }
}
use rust_asm::opcodes as op;
