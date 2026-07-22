use crate::ir::parse_and_lower;
use crate::solver::analyze_class;
use crate::{ClassInference, Error};

/// Configuration for a bounded class-file type-inference run.
///
/// The default permits diagnostics and bounds the work performed for every
/// analyzed method. Use the builder-style methods to tighten or relax these
/// limits for a particular input corpus.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InferenceConfig {
    strict: bool,
    max_block_iterations: usize,
    max_work_items: usize,
}

impl Default for InferenceConfig {
    fn default() -> Self {
        Self {
            strict: false,
            max_block_iterations: 128,
            max_work_items: 50_000,
        }
    }
}

impl InferenceConfig {
    /// Returns whether diagnostics other than notes cause analysis to fail.
    #[must_use]
    pub const fn strict(&self) -> bool {
        self.strict
    }

    /// Returns the maximum number of times a basic block may be processed.
    #[must_use]
    pub const fn max_block_iterations(&self) -> usize {
        self.max_block_iterations
    }

    /// Returns the maximum number of work-queue entries processed per method.
    #[must_use]
    pub const fn max_work_items(&self) -> usize {
        self.max_work_items
    }

    /// Makes diagnostics other than notes fail with [`Error::StrictAnalysis`].
    #[must_use]
    pub const fn with_strict(mut self, strict: bool) -> Self {
        self.strict = strict;
        self
    }

    /// Sets the per-basic-block processing limit.
    ///
    /// A value of zero is rejected by [`Inferer::new`].
    #[must_use]
    pub const fn with_max_block_iterations(mut self, max_block_iterations: usize) -> Self {
        self.max_block_iterations = max_block_iterations;
        self
    }

    /// Sets the per-method work-queue processing limit.
    ///
    /// A value of zero is rejected by [`Inferer::new`].
    #[must_use]
    pub const fn with_max_work_items(mut self, max_work_items: usize) -> Self {
        self.max_work_items = max_work_items;
        self
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
}

/// Infers types from one complete Java class file using [`InferenceConfig::default`].
///
/// Use [`Inferer`] when custom limits or strict diagnostic handling are needed.
pub fn infer_class(bytes: &[u8]) -> Result<ClassInference, Error> {
    Inferer::default().infer_class(bytes)
}
