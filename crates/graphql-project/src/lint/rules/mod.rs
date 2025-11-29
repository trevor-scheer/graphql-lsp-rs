mod deprecated;
mod unique_names;
mod unused_fields;

pub use deprecated::DeprecatedFieldRule;
pub use unique_names::UniqueNamesRule;
pub use unused_fields::UnusedFieldsRule;

use crate::{Diagnostic, DocumentIndex, SchemaIndex};

/// Trait for implementing per-document lint rules
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

/// Trait for implementing project-wide lint rules that need access to all documents
pub trait ProjectLintRule {
    /// Unique identifier for this rule (e.g., "unused-fields")
    fn name(&self) -> &'static str;

    /// Human-readable description
    #[allow(dead_code)]
    fn description(&self) -> &'static str;

    /// Run the lint check across the entire project
    fn check_project(
        &self,
        document_index: &DocumentIndex,
        schema_index: &SchemaIndex,
    ) -> Vec<Diagnostic>;
}

/// Get all available per-document lint rules
pub fn all_rules() -> Vec<Box<dyn LintRule>> {
    vec![Box::new(DeprecatedFieldRule)]
}

/// Get all available project-wide lint rules
pub fn all_project_rules() -> Vec<Box<dyn ProjectLintRule>> {
    vec![Box::new(UniqueNamesRule), Box::new(UnusedFieldsRule)]
}
