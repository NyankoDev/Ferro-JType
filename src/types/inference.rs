use crate::{ClassName, TypeDescriptor};

/// Inferred state for a JVM reference value.
///
/// Without an external class hierarchy, references with incompatible exact
/// classes merge conservatively to [`Self::Unknown`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ReferenceType {
    /// A reference known to have exactly this runtime class.
    Exact(ClassName),
    /// A reference known to be an array of the described type.
    Array(TypeDescriptor),
    /// The JVM `null` value.
    Null,
    /// A reference whose precise type could not be preserved.
    Unknown,
}

impl ReferenceType {
    /// Creates an exact reference type for `class_name`.
    #[must_use]
    pub const fn exact(class_name: ClassName) -> Self {
        Self::Exact(class_name)
    }

    fn join(&self, other: &Self) -> Self {
        match (self, other) {
            (Self::Null, reference) | (reference, Self::Null) => reference.clone(),
            (Self::Unknown, _) | (_, Self::Unknown) => Self::Unknown,
            (Self::Exact(left), Self::Exact(right)) if left == right => Self::Exact(left.clone()),
            (Self::Array(left), Self::Array(right)) if left == right => Self::Array(left.clone()),
            _ => Self::Unknown,
        }
    }
}

/// Abstract JVM type inferred for a local variable or operand-stack value.
///
/// The integral JVM verification types `boolean`, `byte`, `char`, `short`, and
/// `int` are represented by [`Self::Int`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum InferredType {
    /// No control-flow path has supplied a value for this position yet.
    Bottom,
    /// A category-one JVM integral value.
    Int,
    /// A category-one JVM floating-point value.
    Float,
    /// A category-two JVM integral value.
    Long,
    /// A category-two JVM floating-point value.
    Double,
    /// A JVM reference value.
    Reference(ReferenceType),
    /// An object produced by `new` before its constructor has completed.
    Uninitialized {
        /// Class allocated by the `new` instruction.
        class_name: ClassName,
        /// Bytecode offset of the allocation instruction.
        allocation_offset: u16,
    },
    /// A return address used by legacy `jsr` and `ret` bytecode.
    ReturnAddress,
    /// Incompatible values reached the same control-flow position.
    Conflict,
}

impl InferredType {
    /// Conservatively joins two types at a control-flow merge point.
    ///
    /// Compatible values preserve their type. Incompatible primitive or
    /// uninitialized values become [`Self::Conflict`], while incompatible
    /// references become [`ReferenceType::Unknown`].
    #[must_use]
    pub fn join(&self, other: &Self) -> Self {
        match (self, other) {
            (Self::Bottom, value) | (value, Self::Bottom) => value.clone(),
            (Self::Int, Self::Int)
            | (Self::Float, Self::Float)
            | (Self::Long, Self::Long)
            | (Self::Double, Self::Double)
            | (Self::ReturnAddress, Self::ReturnAddress)
            | (Self::Conflict, Self::Conflict) => self.clone(),
            (Self::Reference(left), Self::Reference(right)) => Self::Reference(left.join(right)),
            (
                Self::Uninitialized {
                    class_name: left_name,
                    allocation_offset: left_offset,
                },
                Self::Uninitialized {
                    class_name: right_name,
                    allocation_offset: right_offset,
                },
            ) if left_name == right_name && left_offset == right_offset => self.clone(),
            _ => Self::Conflict,
        }
    }
}
