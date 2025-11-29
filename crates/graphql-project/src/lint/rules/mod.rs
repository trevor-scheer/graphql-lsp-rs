mod deprecated;
mod unique_names;

pub use deprecated::DeprecatedFieldRule;
pub use unique_names::UniqueNamesRule;

use crate::{Diagnostic, SchemaIndex};

/// Trait for implementing lint rules
pub trait LintRule {
    /// Unique identifier for this rule (e.g., "unique-operation-names")
    fn name(&self) -> &'static str;

    /// Human-readable description
    #[allow(dead_code)]
    fn description(&self) -> &'static str;

    /// Run the lint check on a document
    fn check(&self, document: &str, schema_index: &SchemaIndex, file_name: &str)
        -> Vec<Diagnostic>;
}

/// Get all available lint rules
pub fn all_rules() -> Vec<Box<dyn LintRule>> {
    vec![Box::new(UniqueNamesRule), Box::new(DeprecatedFieldRule)]
}
