use thiserror::Error;

/// Error returned when parsing a JVM internal class name.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum NameError {
    /// The provided name was empty.
    #[error("an internal class name cannot be empty")]
    Empty,
    /// The provided name contains a character forbidden in an internal name.
    #[error("invalid internal class name `{value}`")]
    Invalid {
        /// The invalid name supplied by the caller.
        value: String,
    },
}

/// Error returned when parsing a JVM type or method descriptor.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DescriptorError {
    /// The descriptor ended before a complete value could be parsed.
    #[error("descriptor ended unexpectedly at byte {offset}")]
    UnexpectedEnd {
        /// Zero-based byte offset where input ended.
        offset: usize,
    },
    /// A required descriptor character was not found at the given byte offset.
    #[error("expected `{expected}` at byte {offset}")]
    Expected {
        /// Character required by the descriptor grammar.
        expected: char,
        /// Zero-based byte offset where the character was required.
        offset: usize,
    },
    /// An unrecognized descriptor tag was found at the given byte offset.
    #[error("invalid descriptor tag `{tag}` at byte {offset}")]
    InvalidTag {
        /// Unrecognized descriptor tag.
        tag: char,
        /// Zero-based byte offset of the tag.
        offset: usize,
    },
    /// An array descriptor exceeded the JVM's 255-dimension limit.
    #[error("array descriptor exceeds the JVM limit of 255 dimensions")]
    TooManyArrayDimensions,
    /// Extra data followed an otherwise complete descriptor.
    #[error("descriptor contains trailing input at byte {offset}")]
    TrailingInput {
        /// Zero-based byte offset of the first trailing byte.
        offset: usize,
    },
    /// The descriptor contained an invalid internal class name.
    #[error(transparent)]
    InvalidClassName(#[from] NameError),
}

/// Error returned when a class file cannot be analyzed.
#[derive(Debug, Error)]
pub enum Error {
    /// The supplied bytes could not be decoded as a Java class file.
    #[error("failed to decode class file: {0}")]
    Decode(#[from] ferro_babe::FerroBabeError),
    /// Recovery produced an incomplete class representation.
    #[error("class file recovery did not produce a complete class")]
    IncompleteClass,
    /// The class-file structure could not be traversed safely before inference.
    #[error("invalid class-file structure: {message}")]
    InvalidClassFile {
        /// Explanation of the malformed structure.
        message: String,
    },
    /// A descriptor embedded in the class file was invalid.
    #[error(transparent)]
    Descriptor(#[from] DescriptorError),
    /// An [`crate::InferenceConfig`] contained an invalid limit.
    #[error("invalid inference configuration: {message}")]
    InvalidConfiguration {
        /// Explanation of the rejected configuration value.
        message: String,
    },
    /// Strict mode rejected a warning or error diagnostic.
    #[error("strict analysis rejected a diagnostic: {message}")]
    StrictAnalysis {
        /// Message of the diagnostic rejected by strict mode.
        message: String,
    },
}
