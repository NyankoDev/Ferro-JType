/// A generic JVM signature preserved from a `Signature` attribute.
///
/// Generic signatures complement erased descriptors. They are metadata rather
/// than verifier input, so inference remains valid when they are absent or
/// malformed. The value uses JVM generic-signature syntax verbatim.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GenericSignature(String);

impl GenericSignature {
    pub(crate) fn new(value: String) -> Self {
        Self(value)
    }

    /// Returns the generic signature in JVM signature syntax.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consumes the signature and returns its JVM signature text.
    #[must_use]
    pub fn into_string(self) -> String {
        self.0
    }
}
