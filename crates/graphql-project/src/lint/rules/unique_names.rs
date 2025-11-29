use crate::{Diagnostic, DocumentIndex, Position, Range, SchemaIndex};

use super::ProjectLintRule;

/// Lint rule that checks for unique operation and fragment names across the entire project
pub struct UniqueNamesRule;

impl ProjectLintRule for UniqueNamesRule {
    fn name(&self) -> &'static str {
        "unique_names"
    }

    fn description(&self) -> &'static str {
        "Ensures operation and fragment names are unique across the entire project"
    }

    #[allow(clippy::too_many_lines)]
    fn check_project(
        &self,
        document_index: &DocumentIndex,
        _schema_index: &SchemaIndex,
    ) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();

        // Check for duplicate operation names
        for (name, operations) in &document_index.operations {
            if operations.len() > 1 {
                for op in operations {
                    let message = format!(
                        "Operation name '{}' is not unique across the project. Found {} definitions.",
                        name,
                        operations.len()
                    );

                    let range = Range {
                        start: Position {
                            line: op.line,
                            character: op.column,
                        },
                        end: Position {
                            line: op.line,
                            character: op.column + name.len(),
                        },
                    };

                    let mut diag = Diagnostic::error(range, message)
                        .with_code("unique_operation_names")
                        .with_source("graphql-linter");

                    // Add related information for all other occurrences
                    for other_op in operations {
                        if other_op.file_path != op.file_path
                            || other_op.line != op.line
                            || other_op.column != op.column
                        {
                            use crate::diagnostics::{Location, RelatedInfo};
                            let other_range = Range {
                                start: Position {
                                    line: other_op.line,
                                    character: other_op.column,
                                },
                                end: Position {
                                    line: other_op.line,
                                    character: other_op.column + name.len(),
                                },
                            };
                            let related = RelatedInfo {
                                message: format!("Operation '{name}' also defined here"),
                                location: Location {
                                    uri: format!("file://{}", other_op.file_path),
                                    range: other_range,
                                },
                            };
                            diag = diag.with_related_info(related);
                        }
                    }

                    diagnostics.push(diag);
                }
            }
        }

        // Check for duplicate fragment names
        for (name, fragments) in &document_index.fragments {
            if fragments.len() > 1 {
                for frag in fragments {
                    let message = format!(
                        "Fragment name '{}' is not unique across the project. Found {} definitions.",
                        name,
                        fragments.len()
                    );

                    let range = Range {
                        start: Position {
                            line: frag.line,
                            character: frag.column,
                        },
                        end: Position {
                            line: frag.line,
                            character: frag.column + name.len(),
                        },
                    };

                    let mut diag = Diagnostic::error(range, message)
                        .with_code("unique_fragment_names")
                        .with_source("graphql-linter");

                    // Add related information for all other occurrences
                    for other_frag in fragments {
                        if other_frag.file_path != frag.file_path
                            || other_frag.line != frag.line
                            || other_frag.column != frag.column
                        {
                            use crate::diagnostics::{Location, RelatedInfo};
                            let other_range = Range {
                                start: Position {
                                    line: other_frag.line,
                                    character: other_frag.column,
                                },
                                end: Position {
                                    line: other_frag.line,
                                    character: other_frag.column + name.len(),
                                },
                            };
                            let related = RelatedInfo {
                                message: format!("Fragment '{name}' also defined here"),
                                location: Location {
                                    uri: format!("file://{}", other_frag.file_path),
                                    range: other_range,
                                },
                            };
                            diag = diag.with_related_info(related);
                        }
                    }

                    diagnostics.push(diag);
                }
            }
        }

        diagnostics
    }
}

#[cfg(test)]
#[allow(clippy::needless_raw_string_hashes)]
mod tests {
    use super::*;
    use apollo_parser::{cst, Parser};

    fn create_test_document_index(files: Vec<(&str, &str)>) -> DocumentIndex {
        let mut index = DocumentIndex::new();

        for (file_path, content) in files {
            let parser = Parser::new(content);
            let tree = parser.parse();
            let tree_arc = std::sync::Arc::new(tree);

            // Cache the parsed AST
            index
                .parsed_asts
                .insert(file_path.to_string(), tree_arc.clone());

            // Index operations and fragments
            let doc_cst = tree_arc.document();
            for definition in doc_cst.definitions() {
                match definition {
                    cst::Definition::OperationDefinition(op_def) => {
                        if let Some(name_node) = op_def.name() {
                            let name = name_node.text().to_string();
                            let op_type = if op_def
                                .operation_type()
                                .is_some_and(|t| t.mutation_token().is_some())
                            {
                                crate::OperationType::Mutation
                            } else if op_def
                                .operation_type()
                                .is_some_and(|t| t.subscription_token().is_some())
                            {
                                crate::OperationType::Subscription
                            } else {
                                crate::OperationType::Query
                            };

                            let operation_info = crate::OperationInfo {
                                name: Some(name.clone()),
                                operation_type: op_type,
                                file_path: file_path.to_string(),
                                line: 0,
                                column: 0,
                            };

                            index
                                .operations
                                .entry(name)
                                .or_default()
                                .push(operation_info);
                        }
                    }
                    cst::Definition::FragmentDefinition(frag_def) => {
                        if let Some(frag_name) = frag_def.fragment_name().and_then(|n| n.name()) {
                            let name = frag_name.text().to_string();
                            let type_condition = frag_def
                                .type_condition()
                                .and_then(|tc| tc.named_type())
                                .and_then(|nt| nt.name())
                                .map(|n| n.text().to_string())
                                .unwrap_or_default();

                            let fragment_info = crate::FragmentInfo {
                                name: name.clone(),
                                type_condition,
                                file_path: file_path.to_string(),
                                line: 0,
                                column: 0,
                            };

                            index.fragments.entry(name).or_default().push(fragment_info);
                        }
                    }
                    _ => {}
                }
            }
        }

        index
    }

    #[test]
    fn test_detects_duplicate_operations_across_files() {
        let rule = UniqueNamesRule;
        let schema = SchemaIndex::new();

        let document_index = create_test_document_index(vec![
            ("file1.graphql", r#"query GetUser { __typename }"#),
            ("file2.graphql", r#"query GetUser { __typename }"#),
        ]);

        let diagnostics = rule.check_project(&document_index, &schema);

        assert_eq!(
            diagnostics.len(),
            2,
            "Should have 2 errors for duplicate operation"
        );
        assert!(diagnostics.iter().all(|d| d.message.contains("GetUser")));
        assert!(diagnostics.iter().all(|d| d.message.contains("not unique")));
    }

    #[test]
    fn test_detects_duplicate_fragments_across_files() {
        let rule = UniqueNamesRule;
        let schema = SchemaIndex::new();

        let document_index = create_test_document_index(vec![
            ("file1.graphql", r#"fragment UserFields on User { id }"#),
            ("file2.graphql", r#"fragment UserFields on User { name }"#),
        ]);

        let diagnostics = rule.check_project(&document_index, &schema);

        assert_eq!(
            diagnostics.len(),
            2,
            "Should have 2 errors for duplicate fragment"
        );
        assert!(diagnostics.iter().all(|d| d.message.contains("UserFields")));
        assert!(diagnostics.iter().all(|d| d.message.contains("not unique")));
    }

    #[test]
    fn test_no_errors_for_unique_names() {
        let rule = UniqueNamesRule;
        let schema = SchemaIndex::new();

        let document_index = create_test_document_index(vec![
            (
                "file1.graphql",
                r#"query GetUser { __typename } fragment UserFields on User { id }"#,
            ),
            (
                "file2.graphql",
                r#"query GetPost { __typename } fragment PostFields on Post { id }"#,
            ),
        ]);

        let diagnostics = rule.check_project(&document_index, &schema);

        assert_eq!(
            diagnostics.len(),
            0,
            "Should have no errors for unique names"
        );
    }

    #[test]
    fn test_includes_related_info() {
        let rule = UniqueNamesRule;
        let schema = SchemaIndex::new();

        let document_index = create_test_document_index(vec![
            ("file1.graphql", r#"query GetUser { __typename }"#),
            ("file2.graphql", r#"query GetUser { __typename }"#),
        ]);

        let diagnostics = rule.check_project(&document_index, &schema);

        assert_eq!(diagnostics.len(), 2);
        // Each diagnostic should have related info pointing to the other occurrence
        assert!(diagnostics.iter().all(|d| !d.related_info.is_empty()));
    }
}
