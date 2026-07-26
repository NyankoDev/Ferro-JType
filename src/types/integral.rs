use crate::PrimitiveType;

/// The source-level primitive types still compatible with an integral JVM value.
///
/// The JVM verifier uses one integral category for `boolean`, `byte`, `char`,
/// `short`, and `int`. This set preserves the narrower information supplied by
/// descriptors and instructions without inventing a single source type when
/// the bytecode cannot prove one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct IntegralTypeSet(u8);

impl IntegralTypeSet {
    const BOOLEAN_BIT: u8 = 1 << 0;
    const BYTE_BIT: u8 = 1 << 1;
    const CHAR_BIT: u8 = 1 << 2;
    const SHORT_BIT: u8 = 1 << 3;
    const INT_BIT: u8 = 1 << 4;

    /// The exact `boolean` type.
    pub const BOOLEAN: Self = Self(Self::BOOLEAN_BIT);
    /// The exact `byte` type.
    pub const BYTE: Self = Self(Self::BYTE_BIT);
    /// The exact `char` type.
    pub const CHAR: Self = Self(Self::CHAR_BIT);
    /// The exact `short` type.
    pub const SHORT: Self = Self(Self::SHORT_BIT);
    /// The exact `int` type.
    pub const INT: Self = Self(Self::INT_BIT);
    /// Every source-level type represented by the JVM integral verification category.
    pub const ALL: Self =
        Self(Self::BOOLEAN_BIT | Self::BYTE_BIT | Self::CHAR_BIT | Self::SHORT_BIT | Self::INT_BIT);

    /// Returns whether `primitive` remains a candidate.
    #[must_use]
    pub const fn contains(self, primitive: PrimitiveType) -> bool {
        let bit = match primitive {
            PrimitiveType::Boolean => Self::BOOLEAN_BIT,
            PrimitiveType::Byte => Self::BYTE_BIT,
            PrimitiveType::Char => Self::CHAR_BIT,
            PrimitiveType::Short => Self::SHORT_BIT,
            PrimitiveType::Int => Self::INT_BIT,
            PrimitiveType::Float | PrimitiveType::Long | PrimitiveType::Double => return false,
        };
        self.0 & bit != 0
    }

    /// Returns the exact primitive type when this set has one candidate.
    #[must_use]
    pub const fn exact_type(self) -> Option<PrimitiveType> {
        match self.0 {
            Self::BOOLEAN_BIT => Some(PrimitiveType::Boolean),
            Self::BYTE_BIT => Some(PrimitiveType::Byte),
            Self::CHAR_BIT => Some(PrimitiveType::Char),
            Self::SHORT_BIT => Some(PrimitiveType::Short),
            Self::INT_BIT => Some(PrimitiveType::Int),
            _ => None,
        }
    }

    /// Returns whether this set has exactly one candidate.
    #[must_use]
    pub const fn is_exact(self) -> bool {
        self.exact_type().is_some()
    }

    pub(crate) const fn from_primitive(primitive: PrimitiveType) -> Option<Self> {
        match primitive {
            PrimitiveType::Boolean => Some(Self::BOOLEAN),
            PrimitiveType::Byte => Some(Self::BYTE),
            PrimitiveType::Char => Some(Self::CHAR),
            PrimitiveType::Short => Some(Self::SHORT),
            PrimitiveType::Int => Some(Self::INT),
            PrimitiveType::Float | PrimitiveType::Long | PrimitiveType::Double => None,
        }
    }

    pub(crate) const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}
