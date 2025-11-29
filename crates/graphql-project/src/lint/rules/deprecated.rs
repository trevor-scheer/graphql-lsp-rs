use crate::{Diagnostic, Position, Range, SchemaIndex};
use apollo_parser::cst::{self, CstNode};
use apollo_parser::Parser;

use super::LintRule;

/// Lint rule that checks for usage of deprecated fields
pub struct DeprecatedFieldRule;

impl LintRule for DeprecatedFieldRule {
    fn name(&self) -> &'static str {
        "deprecated_field"
    }

    fn description(&self) -> &'static str {
        "Warns when using fields marked with @deprecated directive"
    }

    fn check(
        &self,
        document: &str,
        schema_index: &SchemaIndex,
        _file_name: &str,
    ) -> Vec<Diagnostic> {
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
                        check_selection_set_cst(
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
}

/// Recursively check a selection set (CST) for deprecated fields
fn check_selection_set_cst(
    selection_set: &cst::SelectionSet,
    parent_type_name: &str,
    schema_index: &SchemaIndex,
    warnings: &mut Vec<Diagnostic>,
    document: &str,
) {
    for selection in selection_set.selections() {
        match selection {
            cst::Selection::Field(field) => {
                if let Some(field_name) = field.name() {
                    let field_name_str = field_name.text();

                    // Check if this field is deprecated
                    if let Some(fields) = schema_index.get_fields(parent_type_name) {
                        if let Some(field_info) = fields.iter().find(|f| f.name == field_name_str) {
                            if let Some(ref reason) = field_info.deprecated {
                                // Get the source location of the field
                                let syntax_node = field_name.syntax();
                                let offset: usize = syntax_node.text_range().start().into();
                                let line_col = offset_to_line_col(document, offset);

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
                                        .with_code("deprecated_field")
                                        .with_source("graphql-linter"),
                                );
                            }

                            // Recursively check nested selections
                            if let Some(nested_selection_set) = field.selection_set() {
                                // Extract the base type name from the field type
                                let nested_type = field_info
                                    .type_name
                                    .trim_matches(|c| c == '[' || c == ']' || c == '!');

                                check_selection_set_cst(
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
                    let type_name_owned =
                        inline_fragment.type_condition().and_then(|type_condition| {
                            type_condition.named_type().and_then(|named_type| {
                                named_type.name().map(|name| name.text().to_string())
                            })
                        });

                    let type_name_ref = type_name_owned.as_deref().unwrap_or(parent_type_name);

                    check_selection_set_cst(
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deprecated_field_warning() {
        let schema = SchemaIndex::from_schema(
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
        );

        let rule = DeprecatedFieldRule;

        let document = r"
            query GetUser($id: ID!) {
                user(id: $id) {
                    id
                    name
                    email
                }
            }
        ";

        let warnings = rule.check(document, &schema, "test.graphql");

        assert_eq!(warnings.len(), 1, "Should have exactly one warning");
        assert!(warnings[0].message.contains("email"));
        assert!(warnings[0].message.contains("Use 'emailAddress' instead"));
        assert_eq!(warnings[0].severity, crate::Severity::Warning);
    }

    #[test]
    fn test_multiple_deprecated_fields() {
        let schema = SchemaIndex::from_schema(
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

        let rule = DeprecatedFieldRule;

        let document = r"
            query GetUser($id: ID!) {
                user(id: $id) {
                    id
                    oldName
                    oldEmail
                }
            }
        ";

        let warnings = rule.check(document, &schema, "test.graphql");

        assert_eq!(warnings.len(), 2, "Should have two warnings");
        assert!(warnings.iter().any(|w| w.message.contains("oldName")));
        assert!(warnings.iter().any(|w| w.message.contains("oldEmail")));
    }

    #[test]
    fn test_deprecated_field_in_nested_selection() {
        let schema = SchemaIndex::from_schema(
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

        let rule = DeprecatedFieldRule;

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

        let warnings = rule.check(document, &schema, "test.graphql");

        assert_eq!(warnings.len(), 1, "Should have one warning");
        assert!(warnings[0].message.contains("oldAvatar"));
        assert!(warnings[0].message.contains("Use 'avatarUrl' instead"));
    }

    #[test]
    fn test_no_warnings_for_non_deprecated_fields() {
        let schema = SchemaIndex::from_schema(
            r"
            type Query {
                user(id: ID!): User
            }

            type User {
                id: ID!
                name: String!
                email: String
            }
            ",
        );

        let rule = DeprecatedFieldRule;

        let document = r"
            query GetUser($id: ID!) {
                user(id: $id) {
                    id
                    name
                    email
                }
            }
        ";

        let warnings = rule.check(document, &schema, "test.graphql");

        assert_eq!(warnings.len(), 0, "Should have no warnings");
    }
}
