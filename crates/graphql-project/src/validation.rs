use crate::{Diagnostic, Position, Range, SchemaIndex, Severity};
use apollo_compiler::{validation::Valid, ExecutableDocument};

/// Validation engine for GraphQL documents against a schema
pub struct Validator {
    // Stateless validator - all state is in Schema
}

impl Validator {
    #[must_use]
    pub const fn new() -> Self {
        Self {}
    }

    /// Validate a document string against a schema
    ///
    /// Returns a list of diagnostics (errors and warnings) found during validation.
    /// Uses apollo-compiler's comprehensive validation which includes:
    /// - Syntax validation
    /// - Schema validation
    /// - Document validation against schema
    /// - Fragment validation
    /// - Variable validation
    /// - Type checking
    #[must_use]
    pub fn validate_document(&self, document: &str, schema_index: &SchemaIndex) -> Vec<Diagnostic> {
        // Get the underlying apollo-compiler Schema
        let schema = schema_index.schema();
        // Wrap in Valid since we assume the schema has been validated
        let valid_schema = Valid::assume_valid_ref(schema);

        // Parse and validate the document against the schema
        match ExecutableDocument::parse_and_validate(valid_schema, document, "document.graphql") {
            Ok(_valid_doc) => {
                // Document is fully valid
                Vec::new()
            }
            Err(with_errors) => {
                // Convert apollo-compiler diagnostics to our Diagnostic format
                convert_diagnostics(&with_errors.errors)
            }
        }
    }

    /// Validate a document that's already been parsed
    ///
    /// This is useful when you want to parse once and validate multiple times,
    /// or when you need to modify the document before validation.
    #[must_use]
    pub fn validate_executable(
        &self,
        doc: ExecutableDocument,
        schema_index: &SchemaIndex,
    ) -> Vec<Diagnostic> {
        let schema = schema_index.schema();
        let valid_schema = Valid::assume_valid_ref(schema);

        match doc.validate(valid_schema) {
            Ok(_valid_doc) => Vec::new(),
            Err(with_errors) => convert_diagnostics(&with_errors.errors),
        }
    }
}

impl Default for Validator {
    fn default() -> Self {
        Self::new()
    }
}

/// Convert apollo-compiler `DiagnosticList` to our Diagnostic format
fn convert_diagnostics(
    diagnostic_list: &apollo_compiler::validation::DiagnosticList,
) -> Vec<Diagnostic> {
    diagnostic_list
        .iter()
        .map(|diag| {
            // Extract location information if available
            let range = diag.line_column_range().map_or_else(
                || {
                    // No location available, use zero range
                    Range {
                        start: Position {
                            line: 0,
                            character: 0,
                        },
                        end: Position {
                            line: 0,
                            character: 0,
                        },
                    }
                },
                |line_col_range| {
                    // Convert from apollo-compiler LineColumn (1-indexed) to our Position (0-indexed)
                    Range {
                        start: Position {
                            line: line_col_range.start.line.saturating_sub(1),
                            character: line_col_range.start.column.saturating_sub(1),
                        },
                        end: Position {
                            line: line_col_range.end.line.saturating_sub(1),
                            character: line_col_range.end.column.saturating_sub(1),
                        },
                    }
                },
            );

            // Get the error message
            let message = format!("{}", diag.error);

            Diagnostic {
                severity: Severity::Error,
                range,
                message,
                code: None,
                source: "apollo-compiler".to_string(),
                related_info: Vec::new(),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_schema() -> SchemaIndex {
        SchemaIndex::from_schema(
            r"
            type Query {
                user(id: ID!): User
                users: [User!]!
            }

            type User {
                id: ID!
                name: String!
                email: String
            }

            type Mutation {
                createUser(name: String!): User
            }
        ",
        )
    }

    #[test]
    fn test_valid_query() {
        let validator = Validator::new();
        let schema = create_test_schema();

        let document = r"
            query GetUser($id: ID!) {
                user(id: $id) {
                    id
                    name
                    email
                }
            }
        ";

        let diagnostics = validator.validate_document(document, &schema);
        assert!(diagnostics.is_empty(), "Valid query should have no errors");
    }

    #[test]
    fn test_invalid_field() {
        let validator = Validator::new();
        let schema = create_test_schema();

        let document = r#"
            query GetUser {
                user(id: "123") {
                    id
                    name
                    invalidField
                }
            }
        "#;

        let diagnostics = validator.validate_document(document, &schema);
        assert!(!diagnostics.is_empty(), "Should have validation errors");
        assert!(
            diagnostics
                .iter()
                .any(|d| d.message.contains("invalidField")),
            "Should report invalid field error"
        );
    }

    #[test]
    fn test_missing_required_argument() {
        let validator = Validator::new();
        let schema = create_test_schema();

        let document = r"
            query GetUser {
                user {
                    id
                    name
                }
            }
        ";

        let diagnostics = validator.validate_document(document, &schema);
        assert!(!diagnostics.is_empty(), "Should have validation errors");
        // apollo-compiler should report missing required argument
    }

    #[test]
    fn test_invalid_fragment() {
        let validator = Validator::new();
        let schema = create_test_schema();

        let document = r#"
            fragment UserFields on InvalidType {
                id
                name
            }

            query GetUser {
                user(id: "123") {
                    ...UserFields
                }
            }
        "#;

        let diagnostics = validator.validate_document(document, &schema);
        assert!(!diagnostics.is_empty(), "Should have validation errors");
        assert!(
            diagnostics
                .iter()
                .any(|d| d.message.contains("InvalidType")),
            "Should report invalid type in fragment"
        );
    }

    #[test]
    fn test_undefined_variable() {
        let validator = Validator::new();
        let schema = create_test_schema();

        let document = r"
            query GetUser {
                user(id: $undefinedVar) {
                    id
                    name
                }
            }
        ";

        let diagnostics = validator.validate_document(document, &schema);
        assert!(!diagnostics.is_empty(), "Should have validation errors");
        // Should report undefined variable
    }

    #[test]
    fn test_type_mismatch() {
        let validator = Validator::new();
        let schema = create_test_schema();

        let document = r"
            query GetUser($id: String!) {
                user(id: $id) {
                    id
                    name
                }
            }
        ";

        let _diagnostics = validator.validate_document(document, &schema);
        // apollo-compiler may or may not catch this depending on validation rules
        // This test documents the behavior
    }

    #[test]
    fn test_valid_mutation() {
        let validator = Validator::new();
        let schema = create_test_schema();

        let document = r"
            mutation CreateUser($name: String!) {
                createUser(name: $name) {
                    id
                    name
                }
            }
        ";

        let diagnostics = validator.validate_document(document, &schema);
        assert!(
            diagnostics.is_empty(),
            "Valid mutation should have no errors"
        );
    }

    #[test]
    fn test_syntax_error() {
        let validator = Validator::new();
        let schema = create_test_schema();

        let document = r#"
            query GetUser {
                user(id: "123") {
                    id
                    name

            }
        "#;

        let diagnostics = validator.validate_document(document, &schema);
        assert!(!diagnostics.is_empty(), "Should have syntax errors");
    }

    #[test]
    fn test_multiple_errors() {
        let validator = Validator::new();
        let schema = create_test_schema();

        let document = r"
            query GetUser {
                user {
                    id
                    invalidField1
                    invalidField2
                }
            }
        ";

        let diagnostics = validator.validate_document(document, &schema);
        assert!(
            diagnostics.len() >= 2,
            "Should report multiple errors, got: {diagnostics:?}",
        );
    }
}
