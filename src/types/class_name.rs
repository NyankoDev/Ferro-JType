use crate::NameError;

/// A JVM internal class name.
///
/// Internal names use `/` as the package separator, for example
/// `java/lang/String`, rather than Java source names such as
/// `java.lang.String`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ClassName(String);

impl ClassName {
    pub(crate) fn java_lang_string() -> Self {
        Self("java/lang/String".to_owned())
    }

    pub(crate) fn java_lang_object() -> Self {
        Self("java/lang/Object".to_owned())
    }

    pub(crate) fn java_lang_throwable() -> Self {
        Self("java/lang/Throwable".to_owned())
    }

    /// Parses and validates a JVM internal class name.
    ///
    /// Empty names and descriptor-only punctuation are rejected. The value is
    /// otherwise preserved exactly as supplied.
    pub fn parse(value: impl Into<String>) -> Result<Self, NameError> {
        let value = value.into();

        if value.is_empty() {
            return Err(NameError::Empty);
        }

        if value
            .chars()
            .any(|character| matches!(character, '.' | ';' | '[' | '(' | ')'))
        {
            return Err(NameError::Invalid { value });
        }

        Ok(Self(value))
    }

    /// Returns the internal name as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consumes this name and returns its owned string.
    #[must_use]
    pub fn into_string(self) -> String {
        self.0
    }
}

impl AsRef<str> for ClassName {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl std::fmt::Display for ClassName {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}
