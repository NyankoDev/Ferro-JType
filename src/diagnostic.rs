/// Severity assigned to an analysis diagnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum DiagnosticSeverity {
    /// Informational diagnostic that does not indicate lost precision.
    Note,
    /// Recoverable condition that can reduce inference precision.
    Warning,
    /// Condition that prevented complete analysis of part of the input.
    Error,
}

/// Category of a condition observed during analysis.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum DiagnosticKind {
    /// The class-file decoder recovered from malformed input.
    ParserRecovery,
    /// A branch or control-flow target could not be represented safely.
    InvalidControlFlow,
    /// An instruction required more operand-stack values than were available.
    StackUnderflow,
    /// Incoming control-flow paths disagree on operand-stack height.
    StackHeightMismatch,
    /// Incoming control-flow paths have incompatible inferred types.
    TypeConflict,
    /// An instruction is valid bytecode but has no transfer model yet.
    UnsupportedInstruction,
    /// Configured analysis work limits were reached.
    AnalysisLimitReached,
}

/// Location associated with an analysis diagnostic.
///
/// Class-level diagnostics have no method or offset. Method-level diagnostics
/// can optionally identify the bytecode instruction that triggered them.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct DiagnosticLocation {
    method_name: Option<String>,
    method_descriptor: Option<String>,
    bytecode_offset: Option<u16>,
}

impl DiagnosticLocation {
    /// Creates a location for a diagnostic that applies to the whole class.
    #[must_use]
    pub fn class_level() -> Self {
        Self::default()
    }

    /// Creates a location for a diagnostic that applies to one method.
    ///
    /// `method_descriptor` must use JVM descriptor syntax, such as `(I)V`.
    #[must_use]
    pub fn method(method_name: impl Into<String>, method_descriptor: impl Into<String>) -> Self {
        Self {
            method_name: Some(method_name.into()),
            method_descriptor: Some(method_descriptor.into()),
            bytecode_offset: None,
        }
    }

    /// Adds an offset in the method's `Code` attribute to this location.
    #[must_use]
    pub fn at_offset(mut self, bytecode_offset: u16) -> Self {
        self.bytecode_offset = Some(bytecode_offset);
        self
    }

    /// Returns the affected method name, if any.
    #[must_use]
    pub fn method_name(&self) -> Option<&str> {
        self.method_name.as_deref()
    }

    /// Returns the affected method descriptor, if any.
    #[must_use]
    pub fn method_descriptor(&self) -> Option<&str> {
        self.method_descriptor.as_deref()
    }

    /// Returns the affected bytecode offset, if any.
    #[must_use]
    pub const fn bytecode_offset(&self) -> Option<u16> {
        self.bytecode_offset
    }
}

/// A recoverable condition observed while inferring types.
///
/// Diagnostics are returned in [`crate::ClassInference::diagnostics`]. In
/// strict mode, the first warning or error is reported as
/// [`crate::Error::StrictAnalysis`] instead.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Diagnostic {
    severity: DiagnosticSeverity,
    kind: DiagnosticKind,
    location: DiagnosticLocation,
    message: String,
}

impl Diagnostic {
    /// Creates a diagnostic with a caller-provided message.
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

    /// Returns the diagnostic's severity.
    #[must_use]
    pub const fn severity(&self) -> DiagnosticSeverity {
        self.severity
    }

    /// Returns the category of condition that was observed.
    #[must_use]
    pub const fn kind(&self) -> &DiagnosticKind {
        &self.kind
    }

    /// Returns the class, method, and offset information for this diagnostic.
    #[must_use]
    pub const fn location(&self) -> &DiagnosticLocation {
        &self.location
    }

    /// Returns a human-readable explanation of the condition.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}
