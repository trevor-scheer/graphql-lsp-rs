use crate::{Diagnostic, DocumentIndex, SchemaIndex, Severity};

use super::config::{LintConfig, LintSeverity};
use super::rules;

/// Linter that runs configured lint rules
pub struct Linter {
    config: LintConfig,
}

impl Linter {
    /// Create a new linter with the given configuration
    #[must_use]
    pub const fn new(config: LintConfig) -> Self {
        Self { config }
    }

    /// Run all enabled lints on a document
    #[must_use]
    pub fn lint_document(
        &self,
        document: &str,
        schema_index: &SchemaIndex,
        file_name: &str,
    ) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();

        // Get all available rules
        let all_rules = rules::all_rules();

        for rule in all_rules {
            let rule_name = rule.name();

            // Skip if rule is not enabled (opt-in behavior)
            if !self.config.is_enabled(rule_name) {
                continue;
            }

            // Run the rule
            let mut rule_diagnostics = rule.check(document, schema_index, file_name);

            // Apply configured severity
            if let Some(severity) = self.config.get_severity(rule_name) {
                for diag in &mut rule_diagnostics {
                    diag.severity = match severity {
                        LintSeverity::Error => Severity::Error,
                        LintSeverity::Warn => Severity::Warning,
                        LintSeverity::Off => unreachable!("Off rules are skipped"),
                    };
                }
            }

            diagnostics.extend(rule_diagnostics);
        }

        diagnostics
    }

    /// Run all enabled project-wide lints across all documents
    #[must_use]
    pub fn lint_project(
        &self,
        document_index: &DocumentIndex,
        schema_index: &SchemaIndex,
    ) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();

        // Get all available project-wide rules
        let all_project_rules = rules::all_project_rules();

        for rule in all_project_rules {
            let rule_name = rule.name();

            // Skip if rule is not enabled (opt-in behavior)
            if !self.config.is_enabled(rule_name) {
                continue;
            }

            // Run the rule
            let mut rule_diagnostics = rule.check_project(document_index, schema_index);

            // Apply configured severity
            if let Some(severity) = self.config.get_severity(rule_name) {
                for diag in &mut rule_diagnostics {
                    diag.severity = match severity {
                        LintSeverity::Error => Severity::Error,
                        LintSeverity::Warn => Severity::Warning,
                        LintSeverity::Off => unreachable!("Off rules are skipped"),
                    };
                }
            }

            diagnostics.extend(rule_diagnostics);
        }

        diagnostics
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_schema() -> SchemaIndex {
        SchemaIndex::from_schema(
            r#"
            type Query {
                user(id: ID!): User
            }

            type User {
                id: ID!
                name: String!
                email: String @deprecated(reason: "Use 'emailAddress' instead")
                emailAddress: String
            }
        "#,
        )
    }

    #[test]
    fn test_linter_with_no_config_runs_no_lints() {
        let config = LintConfig::default();
        let linter = Linter::new(config);
        let schema = create_test_schema();

        let document = r#"
            query GetUser { user(id: "1") { id } }
            query GetUser { user(id: "2") { name } }
        "#;

        let diagnostics = linter.lint_document(document, &schema, "test.graphql");
        assert_eq!(
            diagnostics.len(),
            0,
            "No diagnostics should be generated without config"
        );
    }

    #[test]
    fn test_linter_with_recommended_config() {
        let config = LintConfig::recommended();
        let linter = Linter::new(config);
        let schema = create_test_schema();

        let document = r#"
            query GetUser { user(id: "1") { id email } }
        "#;

        let diagnostics = linter.lint_document(document, &schema, "test.graphql");

        // Should have 1 warning for deprecated field
        let warning_count = diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Warning)
            .count();
        assert_eq!(
            warning_count, 1,
            "Should have 1 warning for deprecated field"
        );
    }

    #[test]
    fn test_linter_respects_custom_severity() {
        let yaml = "\ndeprecated_field: error\n";
        let config: LintConfig = serde_yaml::from_str(yaml).unwrap();
        let linter = Linter::new(config);
        let schema = create_test_schema();

        let document = r#"
            query GetUser { user(id: "1") { id email } }
        "#;

        let diagnostics = linter.lint_document(document, &schema, "test.graphql");

        // Deprecated field should be error (custom config)
        let deprecated_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.message.contains("deprecated"))
            .collect();
        assert_eq!(
            deprecated_diags.len(),
            1,
            "Should have one deprecated warning"
        );
        assert!(deprecated_diags
            .iter()
            .all(|d| d.severity == Severity::Error));
    }

    #[test]
    fn test_linter_can_disable_specific_rules() {
        let yaml = "\ndeprecated_field: off\n";
        let config: LintConfig = serde_yaml::from_str(yaml).unwrap();
        let linter = Linter::new(config);
        let schema = create_test_schema();

        let document = r#"
            query GetUser { user(id: "1") { id email } }
        "#;

        let diagnostics = linter.lint_document(document, &schema, "test.graphql");

        // Should have no diagnostics since deprecated_field is disabled
        assert_eq!(
            diagnostics.len(),
            0,
            "Should have no diagnostics when rule is disabled"
        );
    }
}
