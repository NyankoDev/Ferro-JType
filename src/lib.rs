#![forbid(unsafe_code)]

mod api;
mod cfg;
mod diagnostic;
mod error;
mod ir;
mod result;
mod solver;
mod types;

pub use api::{InferenceConfig, Inferer, infer_class};
pub use diagnostic::{Diagnostic, DiagnosticKind, DiagnosticLocation, DiagnosticSeverity};
pub use error::{DescriptorError, Error, NameError};
pub use result::{ClassInference, InstructionInference, MethodInference};
pub use types::{
    ClassName, InferredType, MethodDescriptor, PrimitiveType, ReferenceType, ReturnType,
    TypeDescriptor,
};
