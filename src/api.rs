use std::{collections::BTreeSet, sync::Arc};

use crate::ir::parse_and_lower;
use crate::solver::{analyze_class, analyze_classes};
use crate::{
    ClassInference, ClassInferences, Error, FieldSummaryResolver, MethodSummaryResolver,
    TypeHierarchy,
};

/// Configuration for a class-file type-inference run.
///
/// The default permits diagnostics and runs every analyzed method to a fixed
/// point without an iteration budget. Use the builder-style methods to opt
/// into limits for an untrusted input corpus.
#[derive(Clone)]
pub struct InferenceConfig {
    strict: bool,
    max_block_iterations: usize,
    max_work_items: usize,
    unbounded_analysis: bool,
    hierarchy: Option<Arc<dyn TypeHierarchy>>,
    method_summaries: Option<Arc<dyn MethodSummaryResolver>>,
    field_summaries: Option<Arc<dyn FieldSummaryResolver>>,
}

impl std::fmt::Debug for InferenceConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("InferenceConfig")
            .field("strict", &self.strict)
            .field("max_block_iterations", &self.max_block_iterations)
            .field("max_work_items", &self.max_work_items)
            .field("unbounded_analysis", &self.unbounded_analysis)
            .field("has_type_hierarchy", &self.hierarchy.is_some())
            .field("has_method_summaries", &self.method_summaries.is_some())
            .field("has_field_summaries", &self.field_summaries.is_some())
            .finish()
    }
}

impl PartialEq for InferenceConfig {
    fn eq(&self, other: &Self) -> bool {
        self.strict == other.strict
            && self.max_block_iterations == other.max_block_iterations
            && self.max_work_items == other.max_work_items
            && self.unbounded_analysis == other.unbounded_analysis
            && match (&self.hierarchy, &other.hierarchy) {
                (Some(left), Some(right)) => Arc::ptr_eq(left, right),
                (None, None) => true,
                _ => false,
            }
            && match (&self.method_summaries, &other.method_summaries) {
                (Some(left), Some(right)) => Arc::ptr_eq(left, right),
                (None, None) => true,
                _ => false,
            }
            && match (&self.field_summaries, &other.field_summaries) {
                (Some(left), Some(right)) => Arc::ptr_eq(left, right),
                (None, None) => true,
                _ => false,
            }
    }
}

impl Eq for InferenceConfig {}

impl Default for InferenceConfig {
    fn default() -> Self {
        Self {
            strict: false,
            max_block_iterations: 128,
            max_work_items: 50_000,
            unbounded_analysis: true,
            hierarchy: None,
            method_summaries: None,
            field_summaries: None,
        }
    }
}

impl InferenceConfig {
    /// Returns whether diagnostics other than notes cause analysis to fail.
    #[must_use]
    pub const fn strict(&self) -> bool {
        self.strict
    }

    /// Returns the maximum number of times a basic block may be processed
    /// when bounded analysis is enabled.
    #[must_use]
    pub const fn max_block_iterations(&self) -> usize {
        self.max_block_iterations
    }

    /// Returns the maximum number of work-queue entries processed per method
    /// when bounded analysis is enabled.
    ///
    /// The same budget also bounds automatic class-local method-summary
    /// revisits after every method has received its initial analysis pass.
    #[must_use]
    pub const fn max_work_items(&self) -> usize {
        self.max_work_items
    }

    /// Returns whether analysis runs without configured work limits.
    #[must_use]
    pub const fn unbounded_analysis(&self) -> bool {
        self.unbounded_analysis
    }

    /// Returns whether optional class-hierarchy refinement is enabled.
    #[must_use]
    pub const fn has_type_hierarchy(&self) -> bool {
        self.hierarchy.is_some()
    }

    /// Returns whether caller-provided method-return summaries are enabled.
    #[must_use]
    pub const fn has_method_summaries(&self) -> bool {
        self.method_summaries.is_some()
    }

    /// Returns whether caller-provided static-field summaries are enabled.
    #[must_use]
    pub const fn has_field_summaries(&self) -> bool {
        self.field_summaries.is_some()
    }

    /// Makes diagnostics other than notes fail with [`Error::StrictAnalysis`].
    #[must_use]
    pub const fn with_strict(mut self, strict: bool) -> Self {
        self.strict = strict;
        self
    }

    /// Sets the per-basic-block processing limit and enables bounded analysis.
    ///
    /// A value of zero is rejected by [`Inferer::new`].
    #[must_use]
    pub const fn with_max_block_iterations(mut self, max_block_iterations: usize) -> Self {
        self.max_block_iterations = max_block_iterations;
        self.unbounded_analysis = false;
        self
    }

    /// Sets the per-method work-queue processing limit and enables bounded analysis.
    ///
    /// The same value bounds automatic class-local method-summary revisits. A
    /// value of zero is rejected by [`Inferer::new`].
    #[must_use]
    pub const fn with_max_work_items(mut self, max_work_items: usize) -> Self {
        self.max_work_items = max_work_items;
        self.unbounded_analysis = false;
        self
    }

    /// Disables work-item and per-block limits.
    ///
    /// This is the default behavior and is useful for deeply flattened control
    /// flow when completion is more important than a fixed resource budget,
    /// including class-local method-summary convergence. It never executes Java
    /// code.
    #[must_use]
    pub const fn with_unbounded_analysis(mut self) -> Self {
        self.unbounded_analysis = true;
        self
    }

    /// Enables hierarchy-aware reference merges with a caller-provided source.
    ///
    /// The supplied hierarchy is consulted only during type merges. It never
    /// causes the inferer to load classes or execute Java code.
    #[must_use]
    pub fn with_shared_type_hierarchy(mut self, hierarchy: Arc<dyn TypeHierarchy>) -> Self {
        self.hierarchy = Some(hierarchy);
        self
    }

    /// Enables caller-provided return summaries for resolved member calls.
    ///
    /// The resolver is queried using only information already present in the
    /// class file. It never causes the inferer to load classes or execute Java
    /// code.
    #[must_use]
    pub fn with_shared_method_summaries(
        mut self,
        method_summaries: Arc<dyn MethodSummaryResolver>,
    ) -> Self {
        self.method_summaries = Some(method_summaries);
        self
    }

    /// Enables caller-provided value summaries for resolved `getstatic` calls.
    ///
    /// The resolver is queried using only information already present in the
    /// class file. It never causes the inferer to load classes or execute Java
    /// code. Instance-field reads continue to use their descriptor type.
    #[must_use]
    pub fn with_shared_field_summaries(
        mut self,
        field_summaries: Arc<dyn FieldSummaryResolver>,
    ) -> Self {
        self.field_summaries = Some(field_summaries);
        self
    }

    pub(crate) fn type_hierarchy(&self) -> Option<&dyn TypeHierarchy> {
        self.hierarchy.as_deref()
    }

    pub(crate) fn method_summaries(&self) -> Option<&dyn MethodSummaryResolver> {
        self.method_summaries.as_deref()
    }

    pub(crate) fn field_summaries(&self) -> Option<&dyn FieldSummaryResolver> {
        self.field_summaries.as_deref()
    }

    pub(crate) fn validate(&self) -> Result<(), Error> {
        if self.max_block_iterations == 0 {
            return Err(Error::InvalidConfiguration {
                message: "max_block_iterations must be greater than zero".to_owned(),
            });
        }
        if self.max_work_items == 0 {
            return Err(Error::InvalidConfiguration {
                message: "max_work_items must be greater than zero".to_owned(),
            });
        }
        Ok(())
    }
}

/// A reusable class-file type inferer.
///
/// An inferer owns an [`InferenceConfig`] and can analyze more than one class
/// file with it.
#[derive(Debug, Clone)]
pub struct Inferer {
    config: InferenceConfig,
}

impl Default for Inferer {
    fn default() -> Self {
        Self::new(InferenceConfig::default()).expect("default inference configuration is valid")
    }
}

impl Inferer {
    /// Creates an inferer using `config`.
    ///
    /// Returns [`Error::InvalidConfiguration`] when either configured limit is
    /// zero.
    pub fn new(config: InferenceConfig) -> Result<Self, Error> {
        config.validate()?;
        Ok(Self { config })
    }

    /// Returns this inferer's analysis configuration.
    #[must_use]
    pub const fn config(&self) -> &InferenceConfig {
        &self.config
    }

    /// Infers types from one complete Java class file.
    ///
    /// The analysis works from the supplied class-file bytes and does not load
    /// JDK symbols or resolve an external class hierarchy. `StackMapTable` is
    /// ignored so missing or forged verification frames cannot affect results.
    pub fn infer_class(&self, bytes: &[u8]) -> Result<ClassInference, Error> {
        let class = parse_and_lower(bytes)?;
        analyze_class(&class, &self.config)
    }

    /// Infers types from a batch of complete Java class files.
    ///
    /// The batch keeps its input order and rejects duplicate JVM internal class
    /// names. This method never reads a JAR, directory, class loader, JDK, or
    /// Java runtime; it only analyzes the supplied class-file bytes. Static and
    /// special calls to another class in the batch automatically consume that
    /// class's converged return summary. Virtual calls do the same only for a
    /// `final` class, a `final` method, or a fresh allocation of the member
    /// owner. Calls without a uniquely provable target keep their
    /// descriptor-derived return type.
    pub fn infer_classes<I, B>(&self, class_files: I) -> Result<ClassInferences, Error>
    where
        I: IntoIterator<Item = B>,
        B: AsRef<[u8]>,
    {
        let mut names = BTreeSet::new();
        let mut classes = Vec::new();
        for bytes in class_files {
            let class = parse_and_lower(bytes.as_ref())?;
            if !names.insert(class.name.clone()) {
                return Err(Error::DuplicateClass {
                    class_name: class.name,
                });
            }
            classes.push(class);
        }

        analyze_classes(&classes, &self.config).map(ClassInferences::new)
    }
}

/// Infers types from one complete Java class file using [`InferenceConfig::default`].
///
/// Use [`Inferer`] when custom limits or strict diagnostic handling are needed.
pub fn infer_class(bytes: &[u8]) -> Result<ClassInference, Error> {
    Inferer::default().infer_class(bytes)
}

/// Infers types from caller-supplied Java class files using [`InferenceConfig::default`].
///
/// Use [`Inferer`] when custom limits, strict diagnostic handling, or shared
/// summaries are needed.
pub fn infer_classes<I, B>(class_files: I) -> Result<ClassInferences, Error>
where
    I: IntoIterator<Item = B>,
    B: AsRef<[u8]>,
{
    Inferer::default().infer_classes(class_files)
}
