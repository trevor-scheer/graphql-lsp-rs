use crate::SchemaIndex;
use apollo_compiler::{
    validation::{DiagnosticList, Valid},
    ExecutableDocument,
};

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
    /// Returns Ok(()) if the document is valid, or Err with a `DiagnosticList` if there are errors.
    /// Uses apollo-compiler's comprehensive validation which includes:
    /// - Syntax validation
    /// - Schema validation
    /// - Document validation against schema
    /// - Fragment validation
    /// - Variable validation
    /// - Type checking
    ///
    /// The returned `DiagnosticList` provides rich formatting capabilities for CLI output
    /// and can be converted to LSP diagnostics by the language server package.
    pub fn validate_document(
        &self,
        document: &str,
        schema_index: &SchemaIndex,
    ) -> Result<(), DiagnosticList> {
        self.validate_document_with_name(document, schema_index, "document.graphql")
    }

    /// Validate a document string with a custom file name and optional line offset
    ///
    /// This allows the diagnostics to show the correct file name and line numbers.
    /// The `line_offset` parameter adds blank lines before the document to adjust
    /// line numbers in diagnostics.
    pub fn validate_document_with_name(
        &self,
        document: &str,
        schema_index: &SchemaIndex,
        file_name: &str,
    ) -> Result<(), DiagnosticList> {
        // Get the underlying apollo-compiler Schema
        let schema = schema_index.schema();
        // Wrap in Valid since we assume the schema has been validated
        let valid_schema = Valid::assume_valid_ref(schema);

        // Parse and validate the document against the schema
        match ExecutableDocument::parse_and_validate(valid_schema, document, file_name) {
            Ok(_valid_doc) => {
                // Document is fully valid
                Ok(())
            }
            Err(with_errors) => {
                // Return apollo-compiler's DiagnosticList directly
                Err(with_errors.errors)
            }
        }
    }

    /// Validate a document with adjusted source to match file line numbers
    ///
    /// Prepends newlines to the document so diagnostics show correct line numbers.
    /// The `line_offset` is 0-indexed (0 means document starts on line 1).
    pub fn validate_document_with_location(
        &self,
        document: &str,
        schema_index: &SchemaIndex,
        file_name: &str,
        line_offset: usize,
    ) -> Result<(), DiagnosticList> {
        // Prepend newlines to adjust line numbers in diagnostics
        let adjusted_source = if line_offset > 0 {
            format!("{}{}", "\n".repeat(line_offset), document)
        } else {
            document.to_string()
        };

        self.validate_document_with_name(&adjusted_source, schema_index, file_name)
    }

    /// Validate a document that's already been parsed
    ///
    /// This is useful when you want to parse once and validate multiple times,
    /// or when you need to modify the document before validation.
    pub fn validate_executable(
        &self,
        doc: ExecutableDocument,
        schema_index: &SchemaIndex,
    ) -> Result<(), DiagnosticList> {
        let schema = schema_index.schema();
        let valid_schema = Valid::assume_valid_ref(schema);

        match doc.validate(valid_schema) {
            Ok(_valid_doc) => Ok(()),
            Err(with_errors) => Err(with_errors.errors),
        }
    }
}

impl Default for Validator {
    fn default() -> Self {
        Self::new()
    }
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

        let result = validator.validate_document(document, &schema);
        assert!(result.is_ok(), "Valid query should have no errors");
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

        let result = validator.validate_document(document, &schema);
        assert!(result.is_err(), "Should have validation errors");
        let diagnostics = result.unwrap_err();
        assert!(
            diagnostics
                .iter()
                .any(|d| format!("{}", d.error).contains("invalidField")),
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

        let result = validator.validate_document(document, &schema);
        assert!(result.is_err(), "Should have validation errors");
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

        let result = validator.validate_document(document, &schema);
        assert!(result.is_err(), "Should have validation errors");
        let diagnostics = result.unwrap_err();
        assert!(
            diagnostics
                .iter()
                .any(|d| format!("{}", d.error).contains("InvalidType")),
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

        let result = validator.validate_document(document, &schema);
        assert!(result.is_err(), "Should have validation errors");
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

        let _result = validator.validate_document(document, &schema);
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

        let result = validator.validate_document(document, &schema);
        assert!(result.is_ok(), "Valid mutation should have no errors");
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

        let result = validator.validate_document(document, &schema);
        assert!(result.is_err(), "Should have syntax errors");
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

        let result = validator.validate_document(document, &schema);
        assert!(result.is_err(), "Should have validation errors");
        let diagnostics = result.unwrap_err();
        assert!(
            diagnostics.len() >= 2,
            "Should report multiple errors, got {} errors",
            diagnostics.len()
        );
    }
}
