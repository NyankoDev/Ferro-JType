use crate::NameError;

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ClassName(String);

impl ClassName {
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

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

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
