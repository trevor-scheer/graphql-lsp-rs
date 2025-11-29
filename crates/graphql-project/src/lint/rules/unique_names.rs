use crate::{Diagnostic, Position, Range, SchemaIndex};
use apollo_parser::cst::CstNode;
use apollo_parser::{cst, Parser};
use std::collections::HashMap;

use super::LintRule;

/// Lint rule that checks for unique operation and fragment names
pub struct UniqueNamesRule;

impl LintRule for UniqueNamesRule {
    fn name(&self) -> &'static str {
        "unique_names"
    }

    fn description(&self) -> &'static str {
        "Ensures operation and fragment names are unique within a document"
    }

    fn check(
        &self,
        document: &str,
        _schema_index: &SchemaIndex,
        _file_name: &str,
    ) -> Vec<Diagnostic> {
        let mut errors = Vec::new();
        let parser = Parser::new(document);
        let tree = parser.parse();

        // If there are syntax errors, we can't reliably check for duplicates
        if tree.errors().len() > 0 {
            return errors;
        }

        let doc_cst = tree.document();

        // Track operation names and their locations
        let mut operation_names: HashMap<String, Vec<(usize, usize)>> = HashMap::new();
        // Track fragment names and their locations
        let mut fragment_names: HashMap<String, Vec<(usize, usize)>> = HashMap::new();

        // Walk through all definitions in the document
        for definition in doc_cst.definitions() {
            match definition {
                cst::Definition::OperationDefinition(operation) => {
                    if let Some(name) = operation.name() {
                        let name_str = name.text().to_string();
                        let syntax_node = name.syntax();
                        let offset: usize = syntax_node.text_range().start().into();
                        let line_col = offset_to_line_col(document, offset);

                        operation_names.entry(name_str).or_default().push(line_col);
                    }
                }
                cst::Definition::FragmentDefinition(fragment) => {
                    if let Some(name) = fragment.fragment_name().and_then(|n| n.name()) {
                        let name_str = name.text().to_string();
                        let syntax_node = name.syntax();
                        let offset: usize = syntax_node.text_range().start().into();
                        let line_col = offset_to_line_col(document, offset);

                        fragment_names.entry(name_str).or_default().push(line_col);
                    }
                }
                _ => {}
            }
        }

        // Check for duplicate operation names
        for (name, locations) in operation_names {
            if locations.len() > 1 {
                for (line, col) in locations {
                    let range = Range {
                        start: Position {
                            line,
                            character: col,
                        },
                        end: Position {
                            line,
                            character: col + name.len(),
                        },
                    };

                    let message = format!(
                        "Operation name '{name}' is not unique. Operation names must be unique within a document."
                    );

                    errors.push(
                        Diagnostic::error(range, message)
                            .with_code("unique_operation_names")
                            .with_source("graphql-linter"),
                    );
                }
            }
        }

        // Check for duplicate fragment names
        for (name, locations) in fragment_names {
            if locations.len() > 1 {
                for (line, col) in locations {
                    let range = Range {
                        start: Position {
                            line,
                            character: col,
                        },
                        end: Position {
                            line,
                            character: col + name.len(),
                        },
                    };

                    let message = format!(
                        "Fragment name '{name}' is not unique. Fragment names must be unique within a document."
                    );

                    errors.push(
                        Diagnostic::error(range, message)
                            .with_code("unique_fragment_names")
                            .with_source("graphql-linter"),
                    );
                }
            }
        }

        errors
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
#[allow(clippy::needless_raw_string_hashes)]
mod tests {
    use super::*;

    fn create_test_schema() -> SchemaIndex {
        SchemaIndex::from_schema(
            r"
            type Query {
                user(id: ID!): User
            }

            type User {
                id: ID!
                name: String!
            }
        ",
        )
    }

    #[test]
    fn test_duplicate_operation_names() {
        let rule = UniqueNamesRule;
        let schema = create_test_schema();

        let document = r#"
            query GetUser($id: ID!) {
                user(id: $id) {
                    id
                    name
                }
            }

            query GetUser($userId: ID!) {
                user(id: $userId) {
                    id
                }
            }
        "#;

        let diagnostics = rule.check(document, &schema, "test.graphql");

        assert_eq!(
            diagnostics.len(),
            2,
            "Should have two errors (one for each duplicate)"
        );
        assert!(diagnostics.iter().all(|e| e.message.contains("GetUser")));
        assert!(diagnostics.iter().all(|e| e.message.contains("not unique")));
        assert!(diagnostics
            .iter()
            .all(|e| e.severity == crate::Severity::Error));
    }

    #[test]
    fn test_duplicate_fragment_names() {
        let rule = UniqueNamesRule;
        let schema = create_test_schema();

        let document = r#"
            fragment UserFields on User {
                id
                name
            }

            query GetUser($id: ID!) {
                user(id: $id) {
                    ...UserFields
                }
            }

            fragment UserFields on User {
                id
            }
        "#;

        let diagnostics = rule.check(document, &schema, "test.graphql");

        assert_eq!(
            diagnostics.len(),
            2,
            "Should have two errors (one for each duplicate)"
        );
        assert!(diagnostics.iter().all(|e| e.message.contains("UserFields")));
        assert!(diagnostics.iter().all(|e| e.message.contains("not unique")));
    }

    #[test]
    fn test_unique_names_no_errors() {
        let rule = UniqueNamesRule;
        let schema = create_test_schema();

        let document = r#"
            fragment UserFields on User {
                id
                name
            }

            query GetUser($id: ID!) {
                user(id: $id) {
                    ...UserFields
                }
            }

            query GetUsers {
                user(id: "1") {
                    ...UserFields
                }
            }
        "#;

        let diagnostics = rule.check(document, &schema, "test.graphql");
        assert_eq!(
            diagnostics.len(),
            0,
            "Should have no errors for unique names"
        );
    }

    #[test]
    fn test_anonymous_operations_dont_conflict() {
        let rule = UniqueNamesRule;
        let schema = create_test_schema();

        let document = r#"
            {
                user(id: "1") {
                    id
                }
            }

            {
                user(id: "2") {
                    name
                }
            }
        "#;

        let diagnostics = rule.check(document, &schema, "test.graphql");
        assert_eq!(
            diagnostics.len(),
            0,
            "Anonymous operations should not conflict"
        );
    }
}
