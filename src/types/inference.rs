use crate::{ClassName, TypeDescriptor};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ReferenceType {
    Exact(ClassName),
    Array(TypeDescriptor),
    Null,
    Unknown,
}

impl ReferenceType {
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

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum InferredType {
    Bottom,
    Int,
    Float,
    Long,
    Double,
    Reference(ReferenceType),
    Uninitialized {
        class_name: ClassName,
        allocation_offset: u16,
    },
    ReturnAddress,
    Conflict,
}

impl InferredType {
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
