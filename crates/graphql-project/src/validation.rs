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
    /// - Deprecated field usage warnings (custom)
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
        ExecutableDocument::parse_and_validate(valid_schema, document, file_name)
            .map(|_| ())
            .map_err(|with_errors| with_errors.errors)

        // Note: Deprecation warnings will be handled separately at the LSP/CLI level
        // using the check_deprecated_fields_custom method, as apollo-compiler's DiagnosticList
        // is not easily extensible
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

    /// Check for deprecated field usage in a GraphQL document
    ///
    /// This method parses the document and walks through all field selections to check
    /// if they are marked with the `@deprecated` directive in the schema. Returns
    /// a Vec of our custom Diagnostic type with warnings for any deprecated fields.
    ///
    /// This is separate from the main validation flow because apollo-compiler's
    /// `DiagnosticList` is not easily extensible with custom warnings.
    #[must_use]
    pub fn check_deprecated_fields_custom(
        &self,
        document: &str,
        schema_index: &SchemaIndex,
        _file_name: &str,
    ) -> Vec<crate::Diagnostic> {
        use apollo_parser::{cst, Parser};

        let mut warnings = Vec::new();
        let parser = Parser::new(document);
        let tree = parser.parse();

        // If there are syntax errors, we can't reliably check for deprecated fields
        if tree.errors().len() > 0 {
            return warnings;
        }

        let doc_cst = tree.document();

        // Walk through all definitions in the document
        for definition in doc_cst.definitions() {
            if let cst::Definition::OperationDefinition(operation) = definition {
                // Get the root type name for this operation
                let root_type_name = match operation.operation_type() {
                    Some(op_type) if op_type.query_token().is_some() => {
                        schema_index.schema().schema_definition.query.as_ref()
                    }
                    Some(op_type) if op_type.mutation_token().is_some() => {
                        schema_index.schema().schema_definition.mutation.as_ref()
                    }
                    Some(op_type) if op_type.subscription_token().is_some() => schema_index
                        .schema()
                        .schema_definition
                        .subscription
                        .as_ref(),
                    None => schema_index.schema().schema_definition.query.as_ref(),
                    _ => None,
                };

                if let Some(root_type_name) = root_type_name {
                    if let Some(selection_set) = operation.selection_set() {
                        Self::check_selection_set_cst(
                            &selection_set,
                            root_type_name.as_str(),
                            schema_index,
                            &mut warnings,
                            document,
                        );
                    }
                }
            }
        }

        warnings
    }

    /// Recursively check a selection set (CST) for deprecated fields
    fn check_selection_set_cst(
        selection_set: &apollo_parser::cst::SelectionSet,
        parent_type_name: &str,
        schema_index: &SchemaIndex,
        warnings: &mut Vec<crate::Diagnostic>,
        document: &str,
    ) {
        use crate::{Diagnostic, Position, Range};
        use apollo_parser::cst::{self, CstNode};

        for selection in selection_set.selections() {
            match selection {
                cst::Selection::Field(field) => {
                    if let Some(field_name) = field.name() {
                        let field_name_str = field_name.text();

                        // Check if this field is deprecated
                        if let Some(fields) = schema_index.get_fields(parent_type_name) {
                            if let Some(field_info) =
                                fields.iter().find(|f| f.name == field_name_str)
                            {
                                if let Some(ref reason) = field_info.deprecated {
                                    // Get the source location of the field
                                    let syntax_node = field_name.syntax();
                                    let offset: usize = syntax_node.text_range().start().into();
                                    let line_col = Self::offset_to_line_col(document, offset);

                                    let range = Range {
                                        start: Position {
                                            line: line_col.0,
                                            character: line_col.1,
                                        },
                                        end: Position {
                                            line: line_col.0,
                                            character: line_col.1 + field_name_str.len(),
                                        },
                                    };

                                    let message =
                                        format!("Field '{field_name_str}' is deprecated. {reason}");

                                    warnings.push(
                                        Diagnostic::warning(range, message)
                                            .with_code("deprecated-field")
                                            .with_source("graphql-validator"),
                                    );
                                }

                                // Recursively check nested selections
                                if let Some(nested_selection_set) = field.selection_set() {
                                    // Extract the base type name from the field type
                                    let nested_type = field_info
                                        .type_name
                                        .trim_matches(|c| c == '[' || c == ']' || c == '!');

                                    Self::check_selection_set_cst(
                                        &nested_selection_set,
                                        nested_type,
                                        schema_index,
                                        warnings,
                                        document,
                                    );
                                }
                            }
                        }
                    }
                }
                cst::Selection::FragmentSpread(_) => {
                    // TODO: Handle fragment spreads
                }
                cst::Selection::InlineFragment(inline_fragment) => {
                    if let Some(selection_set) = inline_fragment.selection_set() {
                        // For inline fragments, use the type condition if present
                        // We need to extract it as a String to avoid lifetime issues
                        let type_name_owned =
                            inline_fragment.type_condition().and_then(|type_condition| {
                                type_condition.named_type().and_then(|named_type| {
                                    named_type.name().map(|name| name.text().to_string())
                                })
                            });

                        let type_name_ref = type_name_owned.as_deref().unwrap_or(parent_type_name);

                        Self::check_selection_set_cst(
                            &selection_set,
                            type_name_ref,
                            schema_index,
                            warnings,
                            document,
                        );
                    }
                }
            }
        }
    }

    /// Convert a byte offset to a line and column (0-indexed)
    fn offset_to_line_col(document: &str, offset: usize) -> (usize, usize) {
        let mut line = 0;
        let mut col = 0;
        let mut current_offset = 0;

        for ch in document.chars() {
            if current_offset >= offset {
                break;
            }

            if ch == '\n' {
                line += 1;
                col = 0;
            } else {
                col += 1;
            }

            current_offset += ch.len_utf8();
        }

        (line, col)
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

    #[test]
    fn test_deprecated_field_warning() {
        let schema_with_deprecated = crate::SchemaIndex::from_schema(
            r#"
            type Query {
                user(id: ID!): User
                users: [User!]!
            }

            type User {
                id: ID!
                name: String!
                email: String @deprecated(reason: "Use 'emailAddress' instead")
                emailAddress: String
                oldField: String @deprecated
            }
            "#,
        );

        let validator = Validator::new();

        let document = r"
            query GetUser($id: ID!) {
                user(id: $id) {
                    id
                    name
                    email
                }
            }
        ";

        // Check for deprecated field warnings using the custom method
        let warnings = validator.check_deprecated_fields_custom(
            document,
            &schema_with_deprecated,
            "test.graphql",
        );

        assert_eq!(warnings.len(), 1, "Should have exactly one warning");
        assert!(warnings[0].message.contains("email"));
        assert!(warnings[0].message.contains("Use 'emailAddress' instead"));
        assert_eq!(warnings[0].severity, crate::Severity::Warning);
    }

    #[test]
    fn test_multiple_deprecated_fields() {
        let schema_with_deprecated = crate::SchemaIndex::from_schema(
            r#"
            type Query {
                user(id: ID!): User
            }

            type User {
                id: ID!
                oldName: String @deprecated(reason: "Use 'name' instead")
                name: String
                oldEmail: String @deprecated(reason: "Use 'email' instead")
                email: String
            }
            "#,
        );

        let validator = Validator::new();

        let document = r"
            query GetUser($id: ID!) {
                user(id: $id) {
                    id
                    oldName
                    oldEmail
                }
            }
        ";

        let warnings = validator.check_deprecated_fields_custom(
            document,
            &schema_with_deprecated,
            "test.graphql",
        );

        assert_eq!(warnings.len(), 2, "Should have two warnings");
        assert!(warnings.iter().any(|w| w.message.contains("oldName")));
        assert!(warnings.iter().any(|w| w.message.contains("oldEmail")));
    }

    #[test]
    fn test_deprecated_field_in_nested_selection() {
        let schema_with_deprecated = crate::SchemaIndex::from_schema(
            r#"
            type Query {
                user(id: ID!): User
            }

            type User {
                id: ID!
                name: String!
                profile: Profile
            }

            type Profile {
                bio: String
                oldAvatar: String @deprecated(reason: "Use 'avatarUrl' instead")
                avatarUrl: String
            }
            "#,
        );

        let validator = Validator::new();

        let document = r"
            query GetUser($id: ID!) {
                user(id: $id) {
                    id
                    name
                    profile {
                        bio
                        oldAvatar
                    }
                }
            }
        ";

        let warnings = validator.check_deprecated_fields_custom(
            document,
            &schema_with_deprecated,
            "test.graphql",
        );

        assert_eq!(warnings.len(), 1, "Should have one warning");
        assert!(warnings[0].message.contains("oldAvatar"));
        assert!(warnings[0].message.contains("Use 'avatarUrl' instead"));
    }

    #[test]
    fn test_no_warnings_for_non_deprecated_fields() {
        let schema_with_deprecated = crate::SchemaIndex::from_schema(
            r"
            type Query {
                user(id: ID!): User
            }

            type User {
                id: ID!
                name: String!
                email: String
                oldField: String @deprecated
            }
            ",
        );

        let validator = Validator::new();

        let document = r"
            query GetUser($id: ID!) {
                user(id: $id) {
                    id
                    name
                    email
                }
            }
        ";

        let warnings = validator.check_deprecated_fields_custom(
            document,
            &schema_with_deprecated,
            "test.graphql",
        );

        assert_eq!(warnings.len(), 0, "Should have no warnings");
    }
}
