use std::collections::{BTreeMap, VecDeque};

use ferro_babe::{Disassembler, RecoveryMode};

use crate::ir::class_header_bytes;
use crate::{ClassName, DescriptorError, Error};

mod jar;

/// Supplies direct supertypes for optional reference-type refinement.
///
/// The inferer never loads classes by itself. Implement this trait when the
/// caller already owns class metadata and wants more precise reference merges.
pub trait TypeHierarchy: Send + Sync {
    /// Returns the direct superclass and interfaces of `class_name`.
    ///
    /// Return `None` when the class is unavailable. The inferer then preserves
    /// distinct reference candidates rather than inventing a common supertype.
    fn direct_supertypes(&self, class_name: &ClassName) -> Option<Vec<ClassName>>;
}

/// An in-memory [`TypeHierarchy`] built solely from caller-supplied class files.
///
/// Add class bytes, reference JAR bytes, or explicitly selected JAR files.
/// It never discovers a JDK or loads or executes Java classes automatically.
#[derive(Debug, Clone, Default)]
pub struct ClassHierarchy {
    parents: BTreeMap<ClassName, Vec<ClassName>>,
}

impl ClassHierarchy {
    /// Creates an empty hierarchy.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Decodes and adds one class header from caller-supplied bytes.
    ///
    /// Returns the class's internal name. Only the constant pool and hierarchy
    /// header are decoded; method bodies and attributes do not affect indexing.
    pub fn insert_class(&mut self, bytes: &[u8]) -> Result<ClassName, Error> {
        let bytes = class_header_bytes(bytes)?;
        let disassembly = Disassembler::builder()
            .recovery(RecoveryMode::Strict)
            .build()
            .parse(&bytes)?;
        let class = disassembly.class().ok_or(Error::IncompleteClass)?;
        let class_name = parse_class_name(class.name())?;
        let mut parents = class
            .super_name()
            .map(parse_class_name)
            .transpose()?
            .into_iter()
            .collect::<Vec<_>>();
        parents.extend(
            class
                .interfaces()
                .map(parse_class_name)
                .collect::<Result<Vec<_>, _>>()?,
        );
        parents.sort();
        parents.dedup();
        self.parents.insert(class_name.clone(), parents);
        Ok(class_name)
    }

    /// Returns the number of class headers held by this hierarchy.
    #[must_use]
    pub fn len(&self) -> usize {
        self.parents.len()
    }

    /// Returns whether this hierarchy has no class headers.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.parents.is_empty()
    }
}

fn parse_class_name(value: &str) -> Result<ClassName, Error> {
    ClassName::parse(value)
        .map_err(|error| Error::Descriptor(DescriptorError::InvalidClassName(error)))
}

impl TypeHierarchy for ClassHierarchy {
    fn direct_supertypes(&self, class_name: &ClassName) -> Option<Vec<ClassName>> {
        self.parents.get(class_name).cloned()
    }
}

pub(crate) fn common_supertype(
    hierarchy: Option<&dyn TypeHierarchy>,
    left: &ClassName,
    right: &ClassName,
) -> Option<ClassName> {
    if left == right {
        return Some(left.clone());
    }
    if left.as_str() == "java/lang/Object" || right.as_str() == "java/lang/Object" {
        return Some(ClassName::java_lang_object());
    }

    let hierarchy = hierarchy?;
    let left_distances = ancestor_distances(hierarchy, left);
    let right_distances = ancestor_distances(hierarchy, right);
    left_distances
        .iter()
        .filter_map(|(candidate, left_distance)| {
            let right_distance = right_distances.get(candidate)?;
            Some((
                candidate,
                (*left_distance).max(*right_distance),
                *left_distance + *right_distance,
            ))
        })
        .min_by_key(|(candidate, furthest_distance, total_distance)| {
            (*furthest_distance, *total_distance, (*candidate).clone())
        })
        .map(|(candidate, _, _)| candidate.clone())
}

fn ancestor_distances(
    hierarchy: &dyn TypeHierarchy,
    root: &ClassName,
) -> BTreeMap<ClassName, usize> {
    let object = ClassName::java_lang_object();
    let mut distances = BTreeMap::from([(root.clone(), 0_usize)]);
    let mut queue = VecDeque::from([root.clone()]);

    while let Some(current) = queue.pop_front() {
        let distance = distances[&current];
        let parents = if current == object {
            Vec::new()
        } else {
            hierarchy.direct_supertypes(&current).unwrap_or_default()
        };
        for parent in parents {
            if let std::collections::btree_map::Entry::Vacant(entry) =
                distances.entry(parent.clone())
            {
                entry.insert(distance + 1);
                queue.push_back(parent);
            }
        }
    }

    distances
}
