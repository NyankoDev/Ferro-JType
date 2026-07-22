use std::cmp::Ordering;

use crate::hierarchy::{TypeHierarchy, common_supertype};
use crate::{ClassName, PrimitiveType, TypeDescriptor};

/// Inferred state for a JVM reference value.
///
/// Without an external class hierarchy, incompatible known references merge
/// to `java/lang/Object`. [`Self::Unknown`] is reserved for values whose class
/// bytes do not supply enough information to establish a reference type.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ReferenceType {
    /// A reference whose bytecode supplies this static class type.
    ///
    /// This is not a claim about the object's exact runtime subclass. For
    /// example, an `Object`-returning method still has this static type when
    /// its implementation happens to return a `String`.
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

    fn join(&self, other: &Self, hierarchy: Option<&dyn TypeHierarchy>) -> Self {
        match (self, other) {
            (Self::Null, reference) | (reference, Self::Null) => reference.clone(),
            (Self::Unknown, _) | (_, Self::Unknown) => Self::Unknown,
            (Self::Exact(left), Self::Exact(right)) if left == right => Self::Exact(left.clone()),
            (Self::Array(left), Self::Array(right)) if left == right => Self::Array(left.clone()),
            (Self::Exact(left), Self::Exact(right)) => {
                Self::Exact(common_supertype(hierarchy, left, right))
            }
            _ => Self::Exact(ClassName::java_lang_object()),
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
    /// The receiver of an instance constructor before its parent constructor
    /// has completed.
    UninitializedThis {
        /// Class whose constructor owns this receiver.
        class_name: ClassName,
    },
    /// A return address used by legacy `jsr` and `ret` bytecode.
    ReturnAddress,
    /// Distinct types observed for one reused local-variable slot.
    ///
    /// This is only used for local slots and method summaries. Operand-stack
    /// conflicts continue to use [`Self::Conflict`].
    Alternatives(Vec<InferredType>),
    /// Incompatible values reached the same control-flow position.
    Conflict,
}

/// Joins local-variable types while preserving independent slot lifetimes.
#[must_use]
pub(crate) fn join_local_types(
    left: &InferredType,
    right: &InferredType,
    hierarchy: Option<&dyn TypeHierarchy>,
) -> InferredType {
    let joined = left.join_with_hierarchy(right, hierarchy);
    if !matches!(joined, InferredType::Conflict) {
        return joined;
    }

    let mut alternatives = Vec::new();
    append_alternatives(&mut alternatives, left);
    append_alternatives(&mut alternatives, right);
    alternatives.sort_by(compare_inferred_types);
    InferredType::Alternatives(alternatives)
}

fn append_alternatives(destination: &mut Vec<InferredType>, value: &InferredType) {
    match value {
        InferredType::Alternatives(values) => {
            for value in values {
                append_alternatives(destination, value);
            }
        }
        value if !destination.contains(value) => destination.push(value.clone()),
        _ => {}
    }
}

fn compare_inferred_types(left: &InferredType, right: &InferredType) -> Ordering {
    let rank = |value: &InferredType| match value {
        InferredType::Bottom => 0,
        InferredType::Int => 1,
        InferredType::Float => 2,
        InferredType::Long => 3,
        InferredType::Double => 4,
        InferredType::Reference(_) => 5,
        InferredType::Uninitialized { .. } => 6,
        InferredType::UninitializedThis { .. } => 7,
        InferredType::ReturnAddress => 8,
        InferredType::Alternatives(_) => 9,
        InferredType::Conflict => 10,
    };
    rank(left)
        .cmp(&rank(right))
        .then_with(|| match (left, right) {
            (InferredType::Reference(left), InferredType::Reference(right)) => {
                compare_references(left, right)
            }
            (
                InferredType::Uninitialized {
                    class_name: left_name,
                    allocation_offset: left_offset,
                },
                InferredType::Uninitialized {
                    class_name: right_name,
                    allocation_offset: right_offset,
                },
            ) => left_name
                .cmp(right_name)
                .then(left_offset.cmp(right_offset)),
            (
                InferredType::UninitializedThis {
                    class_name: left_name,
                },
                InferredType::UninitializedThis {
                    class_name: right_name,
                },
            ) => left_name.cmp(right_name),
            (InferredType::Alternatives(left), InferredType::Alternatives(right)) => left
                .iter()
                .zip(right)
                .map(|(left, right)| compare_inferred_types(left, right))
                .find(|comparison| !comparison.is_eq())
                .unwrap_or_else(|| left.len().cmp(&right.len())),
            _ => Ordering::Equal,
        })
}

fn compare_references(left: &ReferenceType, right: &ReferenceType) -> Ordering {
    let rank = |value: &ReferenceType| match value {
        ReferenceType::Exact(_) => 0,
        ReferenceType::Array(_) => 1,
        ReferenceType::Null => 2,
        ReferenceType::Unknown => 3,
    };
    rank(left)
        .cmp(&rank(right))
        .then_with(|| match (left, right) {
            (ReferenceType::Exact(left), ReferenceType::Exact(right)) => left.cmp(right),
            (ReferenceType::Array(left), ReferenceType::Array(right)) => {
                compare_descriptors(left, right)
            }
            _ => Ordering::Equal,
        })
}

fn compare_descriptors(left: &TypeDescriptor, right: &TypeDescriptor) -> Ordering {
    let rank = |value: &TypeDescriptor| match value {
        TypeDescriptor::Primitive(_) => 0,
        TypeDescriptor::Reference(_) => 1,
        TypeDescriptor::Array { .. } => 2,
    };
    rank(left)
        .cmp(&rank(right))
        .then_with(|| match (left, right) {
            (TypeDescriptor::Primitive(left), TypeDescriptor::Primitive(right)) => {
                primitive_rank(*left).cmp(&primitive_rank(*right))
            }
            (TypeDescriptor::Reference(left), TypeDescriptor::Reference(right)) => left.cmp(right),
            (
                TypeDescriptor::Array {
                    dimensions: left_dimensions,
                    element: left_element,
                },
                TypeDescriptor::Array {
                    dimensions: right_dimensions,
                    element: right_element,
                },
            ) => left_dimensions
                .cmp(right_dimensions)
                .then_with(|| compare_descriptors(left_element, right_element)),
            _ => Ordering::Equal,
        })
}

const fn primitive_rank(value: PrimitiveType) -> u8 {
    match value {
        PrimitiveType::Boolean => 0,
        PrimitiveType::Byte => 1,
        PrimitiveType::Char => 2,
        PrimitiveType::Short => 3,
        PrimitiveType::Int => 4,
        PrimitiveType::Float => 5,
        PrimitiveType::Long => 6,
        PrimitiveType::Double => 7,
    }
}

impl InferredType {
    /// Conservatively joins two types at a control-flow merge point.
    ///
    /// Compatible values preserve their type. Incompatible primitive or
    /// uninitialized values become [`Self::Conflict`], while incompatible
    /// references become [`ReferenceType::Unknown`].
    #[must_use]
    pub fn join(&self, other: &Self) -> Self {
        self.join_with_hierarchy(other, None)
    }

    pub(crate) fn join_with_hierarchy(
        &self,
        other: &Self,
        hierarchy: Option<&dyn TypeHierarchy>,
    ) -> Self {
        match (self, other) {
            (Self::Bottom, value) | (value, Self::Bottom) => value.clone(),
            (Self::Int, Self::Int)
            | (Self::Float, Self::Float)
            | (Self::Long, Self::Long)
            | (Self::Double, Self::Double)
            | (Self::ReturnAddress, Self::ReturnAddress)
            | (Self::Conflict, Self::Conflict) => self.clone(),
            (Self::Reference(left), Self::Reference(right)) => {
                Self::Reference(left.join(right, hierarchy))
            }
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
            (
                Self::UninitializedThis {
                    class_name: left_name,
                },
                Self::UninitializedThis {
                    class_name: right_name,
                },
            ) if left_name == right_name => self.clone(),
            _ => Self::Conflict,
        }
    }
}
