use crate::{Diagnostic, DocumentIndex, Position, Range, SchemaIndex};
use apollo_compiler::schema::ExtendedType;
use apollo_parser::cst;
use std::collections::{HashMap, HashSet};

use super::ProjectLintRule;

/// Lint rule that checks for schema fields that are never used in any operation or fragment
pub struct UnusedFieldsRule;

impl ProjectLintRule for UnusedFieldsRule {
    fn name(&self) -> &'static str {
        "unused_fields"
    }

    fn description(&self) -> &'static str {
        "Reports schema fields that are never used in any operation or fragment"
    }

    fn check_project(
        &self,
        document_index: &DocumentIndex,
        schema_index: &SchemaIndex,
    ) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();

        // Collect all fields used across all documents in the project
        let used_fields = collect_all_used_fields(document_index, schema_index);

        // Get all fields defined in the schema
        let schema_fields = collect_schema_fields(schema_index);

        // Find unused fields
        for (type_name, fields) in schema_fields {
            // Skip built-in introspection types
            if is_introspection_type(&type_name) {
                continue;
            }

            let used_in_type = used_fields.get(&type_name);

            for field_name in fields {
                // Skip introspection fields
                if is_introspection_field(&field_name) {
                    continue;
                }

                // Skip root operation type fields (Query/Mutation/Subscription fields are entry points)
                if is_root_type_in_schema(&type_name, schema_index) {
                    continue;
                }

                let is_used = used_in_type.is_some_and(|set| set.contains(&field_name));

                if !is_used {
                    // Create a diagnostic for this unused field
                    // Since we don't have the schema source location here, we'll create
                    // a diagnostic without a specific file location
                    let message = format!(
                        "Field '{type_name}.{field_name}' is defined in the schema but never used in any operation or fragment"
                    );

                    diagnostics.push(
                        Diagnostic::warning(
                            Range {
                                start: Position {
                                    line: 0,
                                    character: 0,
                                },
                                end: Position {
                                    line: 0,
                                    character: 0,
                                },
                            },
                            message,
                        )
                        .with_code("unused_field")
                        .with_source("graphql-linter"),
                    );
                }
            }
        }

        diagnostics
    }
}

/// Collect all fields used across all documents in the project
fn collect_all_used_fields(
    document_index: &DocumentIndex,
    schema_index: &SchemaIndex,
) -> HashMap<String, HashSet<String>> {
    let mut used_fields: HashMap<String, HashSet<String>> = HashMap::new();

    // Process all parsed ASTs
    for tree in document_index.parsed_asts.values() {
        if tree.errors().next().is_none() {
            let doc_cst = tree.document();
            for definition in doc_cst.definitions() {
                match definition {
                    cst::Definition::OperationDefinition(op_def) => {
                        // Get the root type name for this operation
                        let root_type_name = match op_def.operation_type() {
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
                            if let Some(selection_set) = op_def.selection_set() {
                                collect_fields_from_selection_set(
                                    &selection_set,
                                    root_type_name.as_str(),
                                    schema_index,
                                    &mut used_fields,
                                );
                            }
                        }
                    }
                    cst::Definition::FragmentDefinition(frag_def) => {
                        if let Some(type_condition) = frag_def.type_condition() {
                            if let Some(type_name) = type_condition
                                .named_type()
                                .and_then(|nt| nt.name())
                                .map(|n| n.text().to_string())
                            {
                                if let Some(selection_set) = frag_def.selection_set() {
                                    collect_fields_from_selection_set(
                                        &selection_set,
                                        &type_name,
                                        schema_index,
                                        &mut used_fields,
                                    );
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    used_fields
}

/// Recursively collect fields from a selection set
fn collect_fields_from_selection_set(
    selection_set: &cst::SelectionSet,
    parent_type_name: &str,
    schema_index: &SchemaIndex,
    used_fields: &mut HashMap<String, HashSet<String>>,
) {
    for selection in selection_set.selections() {
        match selection {
            cst::Selection::Field(field) => {
                if let Some(field_name) = field.name() {
                    let field_name_str = field_name.text().to_string();

                    // Record this field as used
                    used_fields
                        .entry(parent_type_name.to_string())
                        .or_default()
                        .insert(field_name_str.clone());

                    // Recursively process nested selections
                    if let Some(nested_selection_set) = field.selection_set() {
                        // Get the field's return type from schema
                        if let Some(fields) = schema_index.get_fields(parent_type_name) {
                            if let Some(field_info) =
                                fields.iter().find(|f| f.name == field_name_str)
                            {
                                // Extract the base type name (remove List/NonNull wrappers)
                                let nested_type = field_info
                                    .type_name
                                    .trim_matches(|c| c == '[' || c == ']' || c == '!');

                                collect_fields_from_selection_set(
                                    &nested_selection_set,
                                    nested_type,
                                    schema_index,
                                    used_fields,
                                );
                            }
                        }
                    }
                }
            }
            cst::Selection::FragmentSpread(_) => {
                // Fragment spreads are already processed separately via document_index.fragments
            }
            cst::Selection::InlineFragment(inline_fragment) => {
                if let Some(selection_set) = inline_fragment.selection_set() {
                    // Use the type condition if present, otherwise use parent type
                    let type_name_owned =
                        inline_fragment.type_condition().and_then(|type_condition| {
                            type_condition.named_type().and_then(|named_type| {
                                named_type.name().map(|name| name.text().to_string())
                            })
                        });

                    let type_name_ref = type_name_owned.as_deref().unwrap_or(parent_type_name);

                    collect_fields_from_selection_set(
                        &selection_set,
                        type_name_ref,
                        schema_index,
                        used_fields,
                    );
                }
            }
        }
    }
}

/// Collect all fields defined in the schema
fn collect_schema_fields(schema_index: &SchemaIndex) -> HashMap<String, HashSet<String>> {
    let mut schema_fields: HashMap<String, HashSet<String>> = HashMap::new();

    for (type_name, extended_type) in &schema_index.schema().types {
        match extended_type {
            ExtendedType::Object(object_type) => {
                let fields: HashSet<String> = object_type
                    .fields
                    .keys()
                    .map(std::string::ToString::to_string)
                    .collect();
                schema_fields.insert(type_name.to_string(), fields);
            }
            ExtendedType::Interface(interface_type) => {
                let fields: HashSet<String> = interface_type
                    .fields
                    .keys()
                    .map(std::string::ToString::to_string)
                    .collect();
                schema_fields.insert(type_name.to_string(), fields);
            }
            _ => {
                // We only track object and interface fields
            }
        }
    }

    schema_fields
}

/// Check if a type is a built-in introspection type
fn is_introspection_type(type_name: &str) -> bool {
    matches!(
        type_name,
        "__Schema"
            | "__Type"
            | "__Field"
            | "__InputValue"
            | "__EnumValue"
            | "__TypeKind"
            | "__Directive"
            | "__DirectiveLocation"
    )
}

/// Check if a field name is an introspection field
fn is_introspection_field(field_name: &str) -> bool {
    matches!(field_name, "__typename" | "__schema" | "__type")
}

/// Check if a type is a root operation type (Query/Mutation/Subscription)
fn is_root_type_in_schema(type_name: &str, schema_index: &SchemaIndex) -> bool {
    let schema_def = &schema_index.schema().schema_definition;
    schema_def.query.as_ref().is_some_and(|q| q == type_name)
        || schema_def.mutation.as_ref().is_some_and(|m| m == type_name)
        || schema_def
            .subscription
            .as_ref()
            .is_some_and(|s| s == type_name)
}

#[cfg(test)]
#[allow(clippy::needless_raw_string_hashes)]
mod tests {
    use super::*;
    use apollo_parser::Parser;

    fn create_test_schema() -> SchemaIndex {
        SchemaIndex::from_schema(
            r"
            type Query {
                user(id: ID!): User
                posts: [Post!]!
                unusedQuery: String
            }

            type User {
                id: ID!
                name: String!
                email: String!
                age: Int
                unusedField: String
            }

            type Post {
                id: ID!
                title: String!
                content: String!
                author: User!
                unusedPostField: Int
            }
        ",
        )
    }

    fn create_test_document_index(operations: &[(&str, &str)]) -> DocumentIndex {
        let mut index = DocumentIndex::new();

        for (idx, (_name, text)) in operations.iter().enumerate() {
            let parser = Parser::new(text);
            let tree = parser.parse();
            let tree_arc = std::sync::Arc::new(tree);

            // Cache the parsed AST
            let file_path = format!("test_{idx}.graphql");
            index
                .parsed_asts
                .insert(file_path.clone(), tree_arc.clone());

            // Also populate the operations map for completeness
            if let Some(op_def) = tree_arc.document().definitions().find_map(|def| {
                if let cst::Definition::OperationDefinition(op) = def {
                    Some(op)
                } else {
                    None
                }
            }) {
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

                let operation_name = op_def.name().map(|n| n.text().to_string());

                let operation_info = crate::OperationInfo {
                    name: operation_name.clone(),
                    operation_type: op_type,
                    file_path: file_path.clone(),
                    line: 0,
                    column: 0,
                };

                index
                    .operations
                    .entry(operation_name.clone().unwrap_or_default())
                    .or_default()
                    .push(operation_info);
            }
        }

        index
    }

    #[test]
    fn test_detects_unused_fields() {
        let rule = UnusedFieldsRule;
        let schema = create_test_schema();

        let document_index = create_test_document_index(&[
            (
                "GetUser",
                r#"query GetUser($id: ID!) {
                    user(id: $id) {
                        id
                        name
                        email
                    }
                }"#,
            ),
            (
                "GetPosts",
                r#"query GetPosts {
                    posts {
                        id
                        title
                        author {
                            name
                        }
                    }
                }"#,
            ),
        ]);

        let diagnostics = rule.check_project(&document_index, &schema);

        // Should detect: User.age, User.unusedField, Post.content, Post.unusedPostField
        assert!(
            diagnostics.len() >= 4,
            "Should detect at least 4 unused fields, got {}",
            diagnostics.len()
        );

        let messages: Vec<_> = diagnostics.iter().map(|d| &d.message).collect();

        // Check for some specific unused fields
        assert!(
            messages.iter().any(|m| m.contains("User.unusedField")),
            "Should detect User.unusedField"
        );
        assert!(
            messages.iter().any(|m| m.contains("User.age")),
            "Should detect User.age"
        );
        assert!(
            messages.iter().any(|m| m.contains("Post.unusedPostField")),
            "Should detect Post.unusedPostField"
        );
    }

    #[test]
    fn test_no_diagnostics_when_all_fields_used() {
        let rule = UnusedFieldsRule;
        let schema = SchemaIndex::from_schema(
            r"
            type Query {
                user(id: ID!): User
            }

            type User {
                id: ID!
                name: String!
            }
        ",
        );

        let document_index = create_test_document_index(&[(
            "GetUser",
            r#"query GetUser($id: ID!) {
                user(id: $id) {
                    id
                    name
                }
            }"#,
        )]);

        let diagnostics = rule.check_project(&document_index, &schema);

        assert_eq!(
            diagnostics.len(),
            0,
            "Should have no diagnostics when all fields are used"
        );
    }

    #[test]
    fn test_skips_root_operation_fields() {
        let rule = UnusedFieldsRule;
        let schema = SchemaIndex::from_schema(
            r"
            type Query {
                user(id: ID!): User
                unusedRootField: String
            }

            type User {
                id: ID!
            }
        ",
        );

        let document_index = create_test_document_index(&[(
            "GetUser",
            r#"query GetUser($id: ID!) {
                user(id: $id) {
                    id
                }
            }"#,
        )]);

        let diagnostics = rule.check_project(&document_index, &schema);

        // Root operation fields (Query fields) should not be reported as unused
        assert_eq!(
            diagnostics.len(),
            0,
            "Root operation fields should not be reported as unused"
        );
    }

    #[test]
    fn test_handles_nested_field_usage() {
        let rule = UnusedFieldsRule;
        let schema = create_test_schema();

        let document_index = create_test_document_index(&[(
            "GetPosts",
            r#"query GetPosts {
                posts {
                    id
                    author {
                        name
                        email
                    }
                }
            }"#,
        )]);

        let diagnostics = rule.check_project(&document_index, &schema);

        let messages: Vec<_> = diagnostics.iter().map(|d| &d.message).collect();

        // name and email are used, but id and age are not
        assert!(
            !messages.iter().any(|m| m.contains("User.name")),
            "User.name should not be reported (it's used)"
        );
        assert!(
            !messages.iter().any(|m| m.contains("User.email")),
            "User.email should not be reported (it's used)"
        );
        assert!(
            messages.iter().any(|m| m.contains("User.id")),
            "User.id should be reported (it's not used in this query)"
        );
    }
}
