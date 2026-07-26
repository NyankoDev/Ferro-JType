use crate::ir::ClassIr;
use crate::{ClassInference, Error, InferenceConfig};

mod batch;
mod resolution;
mod summaries;

use summaries::analyze_class_with_method_summaries;

pub(crate) use batch::analyze_classes;

pub(crate) fn analyze_class(
    class: &ClassIr,
    config: &InferenceConfig,
) -> Result<ClassInference, Error> {
    analyze_class_with_method_summaries(class, config, config.method_summaries())
}
