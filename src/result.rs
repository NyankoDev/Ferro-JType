use crate::{ClassName, Diagnostic, InferredType, MethodDescriptor, ReturnType};

#[derive(Debug, Clone)]
pub struct ClassInference {
    class_name: ClassName,
    methods: Vec<MethodInference>,
    diagnostics: Vec<Diagnostic>,
}

impl ClassInference {
    pub(crate) fn new(
        class_name: ClassName,
        methods: Vec<MethodInference>,
        diagnostics: Vec<Diagnostic>,
    ) -> Self {
        Self {
            class_name,
            methods,
            diagnostics,
        }
    }

    #[must_use]
    pub const fn class_name(&self) -> &ClassName {
        &self.class_name
    }

    #[must_use]
    pub fn methods(&self) -> &[MethodInference] {
        &self.methods
    }

    #[must_use]
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }
}

#[derive(Debug, Clone)]
pub struct MethodInference {
    name: String,
    descriptor: MethodDescriptor,
    parameter_types: Vec<InferredType>,
    return_type: ReturnType,
    local_types: Vec<InferredType>,
    instructions: Vec<InstructionInference>,
}

impl MethodInference {
    pub(crate) fn new(
        name: String,
        descriptor: MethodDescriptor,
        parameter_types: Vec<InferredType>,
        return_type: ReturnType,
        local_types: Vec<InferredType>,
        instructions: Vec<InstructionInference>,
    ) -> Self {
        Self {
            name,
            descriptor,
            parameter_types,
            return_type,
            local_types,
            instructions,
        }
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub const fn descriptor(&self) -> &MethodDescriptor {
        &self.descriptor
    }

    #[must_use]
    pub fn parameter_types(&self) -> &[InferredType] {
        &self.parameter_types
    }

    #[must_use]
    pub const fn return_type(&self) -> &ReturnType {
        &self.return_type
    }

    #[must_use]
    pub fn local_types(&self) -> &[InferredType] {
        &self.local_types
    }

    #[must_use]
    pub fn instructions(&self) -> &[InstructionInference] {
        &self.instructions
    }
}

#[derive(Debug, Clone)]
pub struct InstructionInference {
    bytecode_offset: u16,
    local_types: Vec<InferredType>,
    stack_before: Vec<InferredType>,
    stack_after: Vec<InferredType>,
}

impl InstructionInference {
    pub(crate) fn new(
        bytecode_offset: u16,
        local_types: Vec<InferredType>,
        stack_before: Vec<InferredType>,
        stack_after: Vec<InferredType>,
    ) -> Self {
        Self {
            bytecode_offset,
            local_types,
            stack_before,
            stack_after,
        }
    }

    #[must_use]
    pub const fn bytecode_offset(&self) -> u16 {
        self.bytecode_offset
    }

    #[must_use]
    pub fn local_types(&self) -> &[InferredType] {
        &self.local_types
    }

    #[must_use]
    pub fn stack_before(&self) -> &[InferredType] {
        &self.stack_before
    }

    #[must_use]
    pub fn stack_after(&self) -> &[InferredType] {
        &self.stack_after
    }
}
