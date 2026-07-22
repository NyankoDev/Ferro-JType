use crate::{
    ClassName, Diagnostic, DynamicCallKind, GenericSignature, InferredType, MethodDescriptor,
    ReturnType,
};

/// Type-inference results for one Java class file.
///
/// Diagnostics describe recoverable conditions observed while processing the
/// class. Inspect them when consuming inference from malformed, unsupported,
/// or intentionally obfuscated bytecode.
#[derive(Debug, Clone)]
pub struct ClassInference {
    class_name: ClassName,
    generic_signature: Option<GenericSignature>,
    methods: Vec<MethodInference>,
    diagnostics: Vec<Diagnostic>,
}

impl ClassInference {
    pub(crate) fn new(
        class_name: ClassName,
        generic_signature: Option<GenericSignature>,
        methods: Vec<MethodInference>,
        diagnostics: Vec<Diagnostic>,
    ) -> Self {
        Self {
            class_name,
            generic_signature,
            methods,
            diagnostics,
        }
    }

    /// Returns the JVM internal name of the analyzed class.
    #[must_use]
    pub const fn class_name(&self) -> &ClassName {
        &self.class_name
    }

    /// Returns the class's generic `Signature` attribute, when present.
    #[must_use]
    pub const fn generic_signature(&self) -> Option<&GenericSignature> {
        self.generic_signature.as_ref()
    }

    /// Returns one inference result for each method in the class file.
    #[must_use]
    pub fn methods(&self) -> &[MethodInference] {
        &self.methods
    }

    /// Returns diagnostics emitted while analyzing the class.
    #[must_use]
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    /// Returns whether every method reached a fixed point within configured limits.
    ///
    /// This does not hide instruction-level diagnostics; inspect
    /// [`Self::diagnostics`] for unsupported or malformed bytecode details.
    #[must_use]
    pub fn analysis_complete(&self) -> bool {
        self.methods.iter().all(MethodInference::analysis_complete)
    }
}

/// Type-inference results for one method.
///
/// Parameter and return types come from the method descriptor. Local-variable
/// and instruction states are inferred from the method's bytecode.
#[derive(Debug, Clone)]
pub struct MethodInference {
    name: String,
    descriptor: MethodDescriptor,
    generic_signature: Option<GenericSignature>,
    analysis_complete: bool,
    parameter_types: Vec<InferredType>,
    return_type: ReturnType,
    inferred_return_type: Option<InferredType>,
    local_types: Vec<InferredType>,
    instructions: Vec<InstructionInference>,
}

pub(crate) struct MethodHeader {
    pub(crate) descriptor: MethodDescriptor,
    pub(crate) generic_signature: Option<GenericSignature>,
    pub(crate) analysis_complete: bool,
    pub(crate) parameter_types: Vec<InferredType>,
    pub(crate) return_type: ReturnType,
    pub(crate) inferred_return_type: Option<InferredType>,
}

impl MethodInference {
    pub(crate) fn new(
        name: String,
        header: MethodHeader,
        local_types: Vec<InferredType>,
        instructions: Vec<InstructionInference>,
    ) -> Self {
        Self {
            name,
            descriptor: header.descriptor,
            generic_signature: header.generic_signature,
            analysis_complete: header.analysis_complete,
            parameter_types: header.parameter_types,
            return_type: header.return_type,
            inferred_return_type: header.inferred_return_type,
            local_types,
            instructions,
        }
    }

    /// Returns the method name as stored in the class file.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the parsed JVM method descriptor.
    #[must_use]
    pub const fn descriptor(&self) -> &MethodDescriptor {
        &self.descriptor
    }

    /// Returns the method's generic `Signature` attribute, when present.
    #[must_use]
    pub const fn generic_signature(&self) -> Option<&GenericSignature> {
        self.generic_signature.as_ref()
    }

    /// Returns whether analysis reached a fixed point within configured limits.
    #[must_use]
    pub const fn analysis_complete(&self) -> bool {
        self.analysis_complete
    }

    /// Returns the descriptor-derived parameter types in declaration order.
    #[must_use]
    pub fn parameter_types(&self) -> &[InferredType] {
        &self.parameter_types
    }

    /// Returns the descriptor-derived method return type.
    #[must_use]
    pub const fn return_type(&self) -> &ReturnType {
        &self.return_type
    }

    /// Returns the inferred value type observed at reachable value-return instructions.
    ///
    /// This is more precise than [`Self::return_type`] when a method declares a
    /// broad reference type but consistently returns a narrower one. `None`
    /// means the method is void, did not reach a value return, or did not reach
    /// a complete, descriptor-compatible fixed point.
    #[must_use]
    pub const fn inferred_return_type(&self) -> Option<&InferredType> {
        self.inferred_return_type.as_ref()
    }

    /// Returns inferred local-variable types indexed by JVM local slot.
    #[must_use]
    pub fn local_types(&self) -> &[InferredType] {
        &self.local_types
    }

    /// Returns type states for modeled bytecode instructions.
    #[must_use]
    pub fn instructions(&self) -> &[InstructionInference] {
        &self.instructions
    }
}

/// Inferred abstract state at one bytecode instruction.
///
/// The local-variable and operand-stack slices describe the state immediately
/// before and immediately after the instruction at [`Self::bytecode_offset`].
#[derive(Debug, Clone)]
pub struct InstructionInference {
    bytecode_offset: u16,
    dynamic_call_kind: Option<DynamicCallKind>,
    local_types: Vec<InferredType>,
    stack_before: Vec<InferredType>,
    stack_after: Vec<InferredType>,
}

impl InstructionInference {
    pub(crate) fn new(
        bytecode_offset: u16,
        dynamic_call_kind: Option<DynamicCallKind>,
        local_types: Vec<InferredType>,
        stack_before: Vec<InferredType>,
        stack_after: Vec<InferredType>,
    ) -> Self {
        Self {
            bytecode_offset,
            dynamic_call_kind,
            local_types,
            stack_before,
            stack_after,
        }
    }

    /// Returns this instruction's offset within its method's `Code` attribute.
    #[must_use]
    pub const fn bytecode_offset(&self) -> u16 {
        self.bytecode_offset
    }

    /// Returns recognized bootstrap metadata for an `invokedynamic` instruction.
    #[must_use]
    pub const fn dynamic_call_kind(&self) -> Option<DynamicCallKind> {
        self.dynamic_call_kind
    }

    /// Returns local-variable types immediately before this instruction.
    #[must_use]
    pub fn local_types(&self) -> &[InferredType] {
        &self.local_types
    }

    /// Returns operand-stack types immediately before this instruction.
    #[must_use]
    pub fn stack_before(&self) -> &[InferredType] {
        &self.stack_before
    }

    /// Returns operand-stack types immediately after this instruction.
    #[must_use]
    pub fn stack_after(&self) -> &[InferredType] {
        &self.stack_after
    }
}
