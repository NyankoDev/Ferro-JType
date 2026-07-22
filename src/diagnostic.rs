#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum DiagnosticSeverity {
    Note,
    Warning,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum DiagnosticKind {
    ParserRecovery,
    InvalidControlFlow,
    StackUnderflow,
    StackHeightMismatch,
    TypeConflict,
    UnsupportedInstruction,
    AnalysisLimitReached,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct DiagnosticLocation {
    method_name: Option<String>,
    method_descriptor: Option<String>,
    bytecode_offset: Option<u16>,
}

impl DiagnosticLocation {
    #[must_use]
    pub fn class_level() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn method(method_name: impl Into<String>, method_descriptor: impl Into<String>) -> Self {
        Self {
            method_name: Some(method_name.into()),
            method_descriptor: Some(method_descriptor.into()),
            bytecode_offset: None,
        }
    }

    #[must_use]
    pub fn at_offset(mut self, bytecode_offset: u16) -> Self {
        self.bytecode_offset = Some(bytecode_offset);
        self
    }

    #[must_use]
    pub fn method_name(&self) -> Option<&str> {
        self.method_name.as_deref()
    }

    #[must_use]
    pub fn method_descriptor(&self) -> Option<&str> {
        self.method_descriptor.as_deref()
    }

    #[must_use]
    pub const fn bytecode_offset(&self) -> Option<u16> {
        self.bytecode_offset
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Diagnostic {
    severity: DiagnosticSeverity,
    kind: DiagnosticKind,
    location: DiagnosticLocation,
    message: String,
}

impl Diagnostic {
    #[must_use]
    pub fn new(
        severity: DiagnosticSeverity,
        kind: DiagnosticKind,
        location: DiagnosticLocation,
        message: impl Into<String>,
    ) -> Self {
        Self {
            severity,
            kind,
            location,
            message: message.into(),
        }
    }

    #[must_use]
    pub const fn severity(&self) -> DiagnosticSeverity {
        self.severity
    }

    #[must_use]
    pub const fn kind(&self) -> &DiagnosticKind {
        &self.kind
    }

    #[must_use]
    pub const fn location(&self) -> &DiagnosticLocation {
        &self.location
    }

    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}
