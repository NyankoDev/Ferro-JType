use crate::ir::parse_and_lower;
use crate::solver::analyze_class;
use crate::{ClassInference, Error};

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
    #[must_use]
    pub const fn strict(&self) -> bool {
        self.strict
    }

    #[must_use]
    pub const fn max_block_iterations(&self) -> usize {
        self.max_block_iterations
    }

    #[must_use]
    pub const fn max_work_items(&self) -> usize {
        self.max_work_items
    }

    #[must_use]
    pub const fn with_strict(mut self, strict: bool) -> Self {
        self.strict = strict;
        self
    }

    #[must_use]
    pub const fn with_max_block_iterations(mut self, max_block_iterations: usize) -> Self {
        self.max_block_iterations = max_block_iterations;
        self
    }

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
    pub fn new(config: InferenceConfig) -> Result<Self, Error> {
        config.validate()?;
        Ok(Self { config })
    }

    #[must_use]
    pub const fn config(&self) -> &InferenceConfig {
        &self.config
    }

    pub fn infer_class(&self, bytes: &[u8]) -> Result<ClassInference, Error> {
        let class = parse_and_lower(bytes)?;
        analyze_class(&class, &self.config)
    }
}

pub fn infer_class(bytes: &[u8]) -> Result<ClassInference, Error> {
    Inferer::default().infer_class(bytes)
}
