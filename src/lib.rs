#![forbid(unsafe_code)]
#![warn(missing_docs)]

//! Type inference for Java class-file bytecode.
//!
//! Use [`infer_class`] for the default analysis configuration, or [`Inferer`]
//! when analysis limits or strict diagnostic handling need to be customized.
//! The returned [`ClassInference`] exposes inferred method, local-variable, and
//! operand-stack types without requiring a JDK, Java runtime, or external class
//! hierarchy.
//!
//! Class names use the JVM internal form, such as `java/lang/String`.

mod api;
mod cfg;
mod diagnostic;
mod error;
mod hierarchy;
mod ir;
mod result;
mod solver;
mod summary;
mod types;

pub use api::{InferenceConfig, Inferer, infer_class};
pub use diagnostic::{Diagnostic, DiagnosticKind, DiagnosticLocation, DiagnosticSeverity};
pub use error::{DescriptorError, Error, NameError};
pub use hierarchy::{ClassHierarchy, TypeHierarchy};
pub use result::{ClassInference, InstructionInference, MethodInference};
pub use summary::{FieldSummaries, FieldSummaryResolver, MethodSummaries, MethodSummaryResolver};
pub use types::{
    ClassName, DynamicCallKind, GenericSignature, InferredType, MethodDescriptor,
    MethodInvocationKind, PrimitiveType, ReferenceType, ReturnType, TypeDescriptor,
};
