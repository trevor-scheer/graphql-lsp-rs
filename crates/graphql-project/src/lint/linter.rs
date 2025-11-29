use crate::{Diagnostic, SchemaIndex, Severity};

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
            query GetUser { user(id: "2") { name } }
        "#;

        let diagnostics = linter.lint_document(document, &schema, "test.graphql");

        // Should have 2 errors for duplicate operation names
        let error_count = diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .count();
        assert_eq!(error_count, 2, "Should have 2 errors for duplicate names");

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
        let yaml = "\nunique_names: warn\ndeprecated_field: error\n";
        let config: LintConfig = serde_yaml::from_str(yaml).unwrap();
        let linter = Linter::new(config);
        let schema = create_test_schema();

        let document = r#"
            query GetUser { user(id: "1") { id email } }
            query GetUser { user(id: "2") { name } }
        "#;

        let diagnostics = linter.lint_document(document, &schema, "test.graphql");

        // Duplicate names should be warnings (custom config)
        let duplicate_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.message.contains("not unique"))
            .collect();
        assert!(duplicate_diags
            .iter()
            .all(|d| d.severity == Severity::Warning));

        // Deprecated field should be error (custom config)
        let deprecated_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.message.contains("deprecated"))
            .collect();
        assert!(deprecated_diags
            .iter()
            .all(|d| d.severity == Severity::Error));
    }

    #[test]
    fn test_linter_can_disable_specific_rules() {
        let yaml = "\nunique_names: error\ndeprecated_field: off\n";
        let config: LintConfig = serde_yaml::from_str(yaml).unwrap();
        let linter = Linter::new(config);
        let schema = create_test_schema();

        let document = r#"
            query GetUser { user(id: "1") { id email } }
            query GetUser { user(id: "2") { name } }
        "#;

        let diagnostics = linter.lint_document(document, &schema, "test.graphql");

        // Should only have errors for duplicate names
        assert!(diagnostics.iter().all(|d| d.message.contains("not unique")));
        assert!(!diagnostics.iter().any(|d| d.message.contains("deprecated")));
    }
}
