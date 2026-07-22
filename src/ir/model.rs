use crate::{ClassName, MethodDescriptor};

#[derive(Debug, Clone)]
pub(crate) struct ClassIr {
    pub(crate) name: ClassName,
    pub(crate) methods: Vec<MethodIr>,
}

#[derive(Debug, Clone)]
pub(crate) struct MethodIr {
    pub(crate) name: String,
    pub(crate) descriptor_text: String,
    pub(crate) descriptor: MethodDescriptor,
    pub(crate) access_flags: u16,
    pub(crate) max_stack: u16,
    pub(crate) max_locals: u16,
    pub(crate) instructions: Vec<InstructionIr>,
    pub(crate) exception_handlers: Vec<ExceptionHandlerIr>,
}

#[derive(Debug, Clone)]
pub(crate) struct InstructionIr {
    pub(crate) offset: u16,
    pub(crate) opcode: u8,
    pub(crate) operand: InstructionOperandIr,
}

#[derive(Debug, Clone)]
pub(crate) enum InstructionOperandIr {
    None,
    Immediate(i32),
    Local(u16),
    ConstantPool(u16),
    Type {
        type_name: Option<String>,
        constant_pool_index: u16,
    },
    Member(MemberRefIr),
    InvokeInterface {
        method: MemberRefIr,
        count: u8,
    },
    InvokeDynamic {
        descriptor: Option<String>,
        constant_pool_index: u16,
    },
    Branch {
        target: i32,
    },
    Constant(ConstantKind),
    Increment {
        local: u16,
        amount: i16,
    },
    TableSwitch {
        default_target: i32,
        targets: Vec<i32>,
    },
    LookupSwitch {
        default_target: i32,
        targets: Vec<(i32, i32)>,
    },
    MultiArray {
        type_name: Option<String>,
        dimensions: u8,
        constant_pool_index: u16,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConstantKind {
    Integer,
    Float,
    Long,
    Double,
    String,
    Type,
    MethodHandle,
    MethodType,
    Unresolved,
}

#[derive(Debug, Clone)]
pub(crate) enum MemberRefIr {
    Resolved {
        owner: ClassName,
        name: String,
        descriptor: String,
    },
    Unresolved {
        constant_pool_index: u16,
    },
}

#[derive(Debug, Clone)]
pub(crate) struct ExceptionHandlerIr {
    pub(crate) start_offset: u16,
    pub(crate) end_offset: u16,
    pub(crate) handler_offset: u16,
    pub(crate) catch_type: Option<ClassName>,
}
