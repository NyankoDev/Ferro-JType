mod local_variables;
mod reader;
mod sanitize;

pub(crate) use local_variables::{LocalVariableIntegralHint, local_variable_integral_hints};
pub(crate) use sanitize::strip_stack_map_tables;
