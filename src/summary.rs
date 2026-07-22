use std::collections::HashMap;

use crate::{ClassName, InferredType, MethodDescriptor, MethodInvocationKind, TypeDescriptor};

/// Resolves a caller-supplied inferred return type for one member invocation.
///
/// Return `None` when no trustworthy summary is available. The inferer then
/// uses the method descriptor's declared return type. Resolvers must return a
/// value compatible with the descriptor's JVM return category.
pub trait MethodSummaryResolver: Send + Sync {
    /// Returns the inferred return type for one exact member reference.
    fn return_type(
        &self,
        owner: &ClassName,
        name: &str,
        descriptor: &MethodDescriptor,
    ) -> Option<InferredType>;

    /// Returns the inferred return type for one invocation with its dispatch kind.
    ///
    /// The default preserves compatibility with resolvers that only distinguish
    /// member references. Override this method when the summary depends on
    /// whether the call is statically or dynamically dispatched.
    fn return_type_for_invocation(
        &self,
        owner: &ClassName,
        name: &str,
        descriptor: &MethodDescriptor,
        invocation_kind: MethodInvocationKind,
    ) -> Option<InferredType> {
        let _ = invocation_kind;
        self.return_type(owner, name, descriptor)
    }
}

/// Resolves a caller-supplied inferred value type for one static field read.
///
/// Return `None` when no trustworthy summary is available. The inferer then
/// uses the field descriptor's declared type. Resolvers are consulted only for
/// `getstatic`; instance-field values can vary by receiver and are not global
/// summaries.
pub trait FieldSummaryResolver: Send + Sync {
    /// Returns the inferred value type for one exact static field reference.
    fn value_type(
        &self,
        owner: &ClassName,
        name: &str,
        descriptor: &TypeDescriptor,
    ) -> Option<InferredType>;
}

/// In-memory method-return summaries supplied by the caller.
///
/// Entries are keyed by the owner, method name, and complete JVM descriptor
/// from a member reference. This type stores no class loader or Java runtime
/// state. Values commonly come from [`crate::MethodInference::inferred_return_type`]
/// after analyzing a related class.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MethodSummaries {
    returns: HashMap<ClassName, HashMap<String, HashMap<MethodDescriptor, InferredType>>>,
}

/// In-memory static-field value summaries supplied by the caller.
///
/// Entries are keyed by the owner, field name, and complete JVM descriptor
/// from a `getstatic` member reference. This type stores no class loader or
/// Java runtime state.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FieldSummaries {
    values: HashMap<ClassName, HashMap<String, HashMap<TypeDescriptor, InferredType>>>,
}

impl FieldSummaries {
    /// Creates an empty collection of static-field value summaries.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Associates an inferred value type with one exact static field reference.
    ///
    /// Returns the previous summary, if the same field reference was already
    /// present. The inferer ignores a supplied value whose JVM category is
    /// incompatible with the referenced field descriptor.
    pub fn insert_value_type(
        &mut self,
        owner: ClassName,
        name: impl Into<String>,
        descriptor: TypeDescriptor,
        value_type: InferredType,
    ) -> Option<InferredType> {
        self.values
            .entry(owner)
            .or_default()
            .entry(name.into())
            .or_default()
            .insert(descriptor, value_type)
    }

    /// Returns the number of exact static-field summaries held by this map.
    #[must_use]
    pub fn len(&self) -> usize {
        self.values
            .values()
            .flat_map(|fields| fields.values())
            .map(HashMap::len)
            .sum()
    }

    /// Returns whether this map has no static-field value summaries.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
}

impl MethodSummaries {
    /// Creates an empty collection of method-return summaries.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Associates an inferred return type with one exact member reference.
    ///
    /// Returns the previous summary, if the same member reference was already
    /// present. The inferer ignores a supplied value whose JVM category is
    /// incompatible with the referenced method descriptor.
    pub fn insert_return_type(
        &mut self,
        owner: ClassName,
        name: impl Into<String>,
        descriptor: MethodDescriptor,
        return_type: InferredType,
    ) -> Option<InferredType> {
        self.returns
            .entry(owner)
            .or_default()
            .entry(name.into())
            .or_default()
            .insert(descriptor, return_type)
    }

    /// Returns the number of exact member-return summaries held by this map.
    #[must_use]
    pub fn len(&self) -> usize {
        self.returns
            .values()
            .flat_map(|methods| methods.values())
            .map(HashMap::len)
            .sum()
    }

    /// Returns whether this map has no method-return summaries.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.returns.is_empty()
    }

    pub(crate) fn remove_return_type(
        &mut self,
        owner: &ClassName,
        name: &str,
        descriptor: &MethodDescriptor,
    ) -> Option<InferredType> {
        let removed = self
            .returns
            .get_mut(owner)?
            .get_mut(name)?
            .remove(descriptor);

        if self
            .returns
            .get(owner)
            .and_then(|methods| methods.get(name))
            .is_some_and(HashMap::is_empty)
        {
            self.returns.get_mut(owner)?.remove(name);
        }
        if self.returns.get(owner).is_some_and(HashMap::is_empty) {
            self.returns.remove(owner);
        }

        removed
    }
}

impl MethodSummaryResolver for MethodSummaries {
    fn return_type(
        &self,
        owner: &ClassName,
        name: &str,
        descriptor: &MethodDescriptor,
    ) -> Option<InferredType> {
        self.returns.get(owner)?.get(name)?.get(descriptor).cloned()
    }
}

impl FieldSummaryResolver for FieldSummaries {
    fn value_type(
        &self,
        owner: &ClassName,
        name: &str,
        descriptor: &TypeDescriptor,
    ) -> Option<InferredType> {
        self.values.get(owner)?.get(name)?.get(descriptor).cloned()
    }
}

pub(crate) fn value_type_matches_descriptor(
    descriptor: &TypeDescriptor,
    value_type: &InferredType,
) -> bool {
    match descriptor {
        TypeDescriptor::Primitive(primitive) => matches!(
            (primitive, value_type),
            (
                crate::PrimitiveType::Boolean
                    | crate::PrimitiveType::Byte
                    | crate::PrimitiveType::Char
                    | crate::PrimitiveType::Short
                    | crate::PrimitiveType::Int,
                InferredType::Int
            ) | (crate::PrimitiveType::Float, InferredType::Float)
                | (crate::PrimitiveType::Long, InferredType::Long)
                | (crate::PrimitiveType::Double, InferredType::Double)
        ),
        TypeDescriptor::Reference(_) => matches!(
            value_type,
            InferredType::Reference(
                crate::ReferenceType::Exact(_)
                    | crate::ReferenceType::Array(_)
                    | crate::ReferenceType::Null
            )
        ),
        TypeDescriptor::Array { .. } => matches!(
            value_type,
            InferredType::Reference(crate::ReferenceType::Array(_) | crate::ReferenceType::Null)
        ),
    }
}
