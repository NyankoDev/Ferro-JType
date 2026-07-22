use std::collections::HashMap;

use crate::{ClassName, InferredType, MethodDescriptor};

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
