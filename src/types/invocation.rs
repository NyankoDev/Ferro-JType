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
            0xb6 => Some(Self::Virtual),
            0xb7 => Some(Self::Special),
            0xb8 => Some(Self::Static),
            0xb9 => Some(Self::Interface),
            _ => None,
        }
    }
}
