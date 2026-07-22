use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum NameError {
    #[error("an internal class name cannot be empty")]
    Empty,
    #[error("invalid internal class name `{value}`")]
    Invalid { value: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DescriptorError {
    #[error("descriptor ended unexpectedly at byte {offset}")]
    UnexpectedEnd { offset: usize },
    #[error("expected `{expected}` at byte {offset}")]
    Expected { expected: char, offset: usize },
    #[error("invalid descriptor tag `{tag}` at byte {offset}")]
    InvalidTag { tag: char, offset: usize },
    #[error("array descriptor exceeds the JVM limit of 255 dimensions")]
    TooManyArrayDimensions,
    #[error("descriptor contains trailing input at byte {offset}")]
    TrailingInput { offset: usize },
    #[error(transparent)]
    InvalidClassName(#[from] NameError),
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("failed to decode class file: {0}")]
    Decode(#[from] ferro_babe::FerroBabeError),
    #[error("class file recovery did not produce a complete class")]
    IncompleteClass,
    #[error(transparent)]
    Descriptor(#[from] DescriptorError),
    #[error("invalid inference configuration: {message}")]
    InvalidConfiguration { message: String },
    #[error("strict analysis rejected a diagnostic: {message}")]
    StrictAnalysis { message: String },
}
