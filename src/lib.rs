#![forbid(unsafe_code)]

mod diagnostic;
mod error;
mod types;

pub use diagnostic::{Diagnostic, DiagnosticKind, DiagnosticLocation, DiagnosticSeverity};
pub use error::{DescriptorError, Error, NameError};
pub use types::{
    ClassName, InferredType, MethodDescriptor, PrimitiveType, ReferenceType, ReturnType,
    TypeDescriptor,
};
