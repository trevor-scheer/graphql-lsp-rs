#![allow(clippy::format_push_string)]
#![allow(clippy::unnecessary_wraps)]
#![allow(clippy::too_many_lines)]
#![allow(clippy::option_if_let_else)]
#![allow(clippy::single_match_else)]
#![allow(clippy::only_used_in_recursion)]

use crate::{Position, Range, SchemaIndex};
use apollo_parser::{
    cst::{self, CstNode},
    Parser,
};

/// Information to display when hovering over a GraphQL element
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HoverInfo {
    /// Markdown formatted content to display
    pub contents: String,
    /// Optional range to highlight
    pub range: Option<Range>,
}

impl HoverInfo {
    #[must_use]
    pub const fn new(contents: String, range: Option<Range>) -> Self {
        Self { contents, range }
    }
}

/// Type of GraphQL element at a position
#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)]
enum ElementType {
    Field {
        field_name: String,
        parent_type: String,
    },
    TypeReference {
        type_name: String,
    },
    Argument {
        arg_name: String,
        field_name: String,
        parent_type: String,
    },
    Variable {
        var_name: String,
    },
    FragmentSpread {
        fragment_name: String,
    },
    FragmentDefinition {
        fragment_name: String,
        type_condition: String,
    },
    Directive {
        directive_name: String,
    },
    EnumValue {
        value_name: String,
        enum_type: Option<String>,
    },
    Operation {
        operation_type: String,
        operation_name: Option<String>,
    },
}

/// Hover information provider
pub struct HoverProvider;

impl HoverProvider {
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Get hover information for a position in a GraphQL document
    #[must_use]
    pub fn hover(
        &self,
        source: &str,
        position: Position,
        schema_index: &SchemaIndex,
    ) -> Option<HoverInfo> {
        let parser = Parser::new(source);
        let tree = parser.parse();

        // If there are syntax errors, we may not be able to provide accurate hover info
        if tree.errors().count() > 0 {
            return None;
        }

        let doc = tree.document();
        let byte_offset = Self::position_to_offset(source, position)?;

        // Find the element at this position
        let element_type = Self::find_element_at_position(&doc, byte_offset, source, schema_index)?;

        // Generate hover content based on element type
        Self::generate_hover_content(element_type, schema_index)
    }

    /// Convert a line/column position to a byte offset
    fn position_to_offset(source: &str, position: Position) -> Option<usize> {
        let mut current_line = 0;
        let mut current_col = 0;
        let mut offset = 0;

        for ch in source.chars() {
            if current_line == position.line && current_col == position.character {
                return Some(offset);
            }

            if ch == '\n' {
                current_line += 1;
                current_col = 0;
            } else {
                current_col += 1;
            }

            offset += ch.len_utf8();
        }

        // Handle position at end of file
        if current_line == position.line && current_col == position.character {
            Some(offset)
        } else {
            None
        }
    }

    /// Find the GraphQL element at the given byte offset
    fn find_element_at_position(
        doc: &cst::Document,
        byte_offset: usize,
        source: &str,
        schema_index: &SchemaIndex,
    ) -> Option<ElementType> {
        for definition in doc.definitions() {
            match definition {
                cst::Definition::OperationDefinition(op) => {
                    // Check if we're on the operation keyword or name
                    if let Some(element) = Self::check_operation_definition(&op, byte_offset) {
                        return Some(element);
                    }

                    // Check the selection set
                    if let Some(selection_set) = op.selection_set() {
                        if let Some(element) = Self::check_selection_set(
                            &selection_set,
                            byte_offset,
                            Self::get_operation_root_type(&op, source, schema_index),
                            source,
                            schema_index,
                        ) {
                            return Some(element);
                        }
                    }
                }
                cst::Definition::FragmentDefinition(frag) => {
                    // Check if we're on the fragment name
                    if let Some(frag_name) = frag.fragment_name().and_then(|n| n.name()) {
                        let range = frag_name.syntax().text_range();
                        let start: usize = range.start().into();
                        let end: usize = range.end().into();

                        if byte_offset >= start && byte_offset < end {
                            let type_condition = frag
                                .type_condition()
                                .and_then(|tc| tc.named_type())
                                .and_then(|nt| nt.name())
                                .map(|n| n.text().to_string())
                                .unwrap_or_default();

                            return Some(ElementType::FragmentDefinition {
                                fragment_name: frag_name.text().to_string(),
                                type_condition,
                            });
                        }
                    }

                    // Check if we're on the type condition
                    if let Some(type_condition) = frag.type_condition() {
                        if let Some(named_type) = type_condition.named_type() {
                            if let Some(name) = named_type.name() {
                                let range = name.syntax().text_range();
                                let start: usize = range.start().into();
                                let end: usize = range.end().into();

                                if byte_offset >= start && byte_offset < end {
                                    return Some(ElementType::TypeReference {
                                        type_name: name.text().to_string(),
                                    });
                                }
                            }
                        }
                    }

                    // Check the selection set
                    if let Some(selection_set) = frag.selection_set() {
                        let type_condition = frag
                            .type_condition()
                            .and_then(|tc| tc.named_type())
                            .and_then(|nt| nt.name())
                            .map(|n| n.text().to_string())
                            .unwrap_or_default();

                        if let Some(element) = Self::check_selection_set(
                            &selection_set,
                            byte_offset,
                            type_condition,
                            source,
                            schema_index,
                        ) {
                            return Some(element);
                        }
                    }
                }
                _ => {}
            }
        }

        None
    }

    /// Check if the byte offset is within an operation definition
    fn check_operation_definition(
        op: &cst::OperationDefinition,
        byte_offset: usize,
    ) -> Option<ElementType> {
        // Check if we're on the operation name
        if let Some(name) = op.name() {
            let range = name.syntax().text_range();
            let start: usize = range.start().into();
            let end: usize = range.end().into();

            if byte_offset >= start && byte_offset < end {
                let op_type = if let Some(op_type) = op.operation_type() {
                    if op_type.query_token().is_some() {
                        "query"
                    } else if op_type.mutation_token().is_some() {
                        "mutation"
                    } else if op_type.subscription_token().is_some() {
                        "subscription"
                    } else {
                        "query"
                    }
                } else {
                    "query"
                };

                return Some(ElementType::Operation {
                    operation_type: op_type.to_string(),
                    operation_name: Some(name.text().to_string()),
                });
            }
        }

        // Check if we're on a variable definition
        if let Some(variable_defs) = op.variable_definitions() {
            for var_def in variable_defs.variable_definitions() {
                if let Some(variable) = var_def.variable() {
                    if let Some(name) = variable.name() {
                        let range = name.syntax().text_range();
                        let start: usize = range.start().into();
                        let end: usize = range.end().into();

                        if byte_offset >= start && byte_offset < end {
                            return Some(ElementType::Variable {
                                var_name: name.text().to_string(),
                            });
                        }
                    }
                }
            }
        }

        None
    }

    /// Get the root type for an operation
    fn get_operation_root_type(
        op: &cst::OperationDefinition,
        _source: &str,
        schema_index: &SchemaIndex,
    ) -> String {
        let root_type_name = if let Some(op_type) = op.operation_type() {
            if op_type.query_token().is_some() {
                schema_index.schema().schema_definition.query.as_ref()
            } else if op_type.mutation_token().is_some() {
                schema_index.schema().schema_definition.mutation.as_ref()
            } else if op_type.subscription_token().is_some() {
                schema_index
                    .schema()
                    .schema_definition
                    .subscription
                    .as_ref()
            } else {
                schema_index.schema().schema_definition.query.as_ref()
            }
        } else {
            schema_index.schema().schema_definition.query.as_ref()
        };

        root_type_name.map_or_else(|| "Query".to_string(), std::string::ToString::to_string)
    }

    /// Check if the byte offset is within a selection set
    fn check_selection_set(
        selection_set: &cst::SelectionSet,
        byte_offset: usize,
        parent_type: String,
        source: &str,
        schema_index: &SchemaIndex,
    ) -> Option<ElementType> {
        for selection in selection_set.selections() {
            match selection {
                cst::Selection::Field(field) => {
                    // Check if we're on the field name
                    if let Some(name) = field.name() {
                        let range = name.syntax().text_range();
                        let start: usize = range.start().into();
                        let end: usize = range.end().into();

                        if byte_offset >= start && byte_offset < end {
                            return Some(ElementType::Field {
                                field_name: name.text().to_string(),
                                parent_type,
                            });
                        }
                    }

                    // Check if we're on an argument
                    if let Some(arguments) = field.arguments() {
                        for arg in arguments.arguments() {
                            if let Some(name) = arg.name() {
                                let range = name.syntax().text_range();
                                let start: usize = range.start().into();
                                let end: usize = range.end().into();

                                if byte_offset >= start && byte_offset < end {
                                    let field_name = field
                                        .name()
                                        .map(|n| n.text().to_string())
                                        .unwrap_or_default();

                                    return Some(ElementType::Argument {
                                        arg_name: name.text().to_string(),
                                        field_name,
                                        parent_type,
                                    });
                                }
                            }

                            // Check if we're on a variable in the argument value
                            if let Some(value) = arg.value() {
                                if let Some(element) =
                                    Self::check_value_for_variable(&value, byte_offset)
                                {
                                    return Some(element);
                                }
                            }
                        }
                    }

                    // Check nested selection set
                    if let Some(nested_selection_set) = field.selection_set() {
                        // Resolve the field type from the schema
                        let field_name = field
                            .name()
                            .map(|n| n.text().to_string())
                            .unwrap_or_default();
                        let nested_type =
                            if let Some(fields) = schema_index.get_fields(&parent_type) {
                                fields
                                    .iter()
                                    .find(|f| f.name == field_name)
                                    .map(|f| {
                                        // Extract base type name (strip [], !)
                                        f.type_name
                                            .trim_matches(|c| c == '[' || c == ']' || c == '!')
                                            .to_string()
                                    })
                                    .unwrap_or_default()
                            } else {
                                String::new()
                            };

                        if !nested_type.is_empty() {
                            if let Some(element) = Self::check_selection_set(
                                &nested_selection_set,
                                byte_offset,
                                nested_type,
                                source,
                                schema_index,
                            ) {
                                return Some(element);
                            }
                        }
                    }
                }
                cst::Selection::FragmentSpread(spread) => {
                    if let Some(frag_name) = spread.fragment_name().and_then(|n| n.name()) {
                        let range = frag_name.syntax().text_range();
                        let start: usize = range.start().into();
                        let end: usize = range.end().into();

                        if byte_offset >= start && byte_offset < end {
                            return Some(ElementType::FragmentSpread {
                                fragment_name: frag_name.text().to_string(),
                            });
                        }
                    }
                }
                cst::Selection::InlineFragment(inline_frag) => {
                    // Check if we're on the type condition
                    if let Some(type_condition) = inline_frag.type_condition() {
                        if let Some(named_type) = type_condition.named_type() {
                            if let Some(name) = named_type.name() {
                                let range = name.syntax().text_range();
                                let start: usize = range.start().into();
                                let end: usize = range.end().into();

                                if byte_offset >= start && byte_offset < end {
                                    return Some(ElementType::TypeReference {
                                        type_name: name.text().to_string(),
                                    });
                                }
                            }
                        }
                    }

                    // Check nested selection set
                    if let Some(nested_selection_set) = inline_frag.selection_set() {
                        let type_name = inline_frag
                            .type_condition()
                            .and_then(|tc| tc.named_type())
                            .and_then(|nt| nt.name())
                            .map_or_else(|| parent_type.clone(), |n| n.text().to_string());

                        if let Some(element) = Self::check_selection_set(
                            &nested_selection_set,
                            byte_offset,
                            type_name,
                            source,
                            schema_index,
                        ) {
                            return Some(element);
                        }
                    }
                }
            }
        }

        None
    }

    /// Check if a value contains a variable at the byte offset
    fn check_value_for_variable(value: &cst::Value, byte_offset: usize) -> Option<ElementType> {
        if let cst::Value::Variable(var) = value {
            if let Some(name) = var.name() {
                let range = name.syntax().text_range();
                let start: usize = range.start().into();
                let end: usize = range.end().into();

                if byte_offset >= start && byte_offset < end {
                    return Some(ElementType::Variable {
                        var_name: name.text().to_string(),
                    });
                }
            }
        }

        None
    }

    /// Generate hover content based on the element type
    fn generate_hover_content(
        element_type: ElementType,
        schema_index: &SchemaIndex,
    ) -> Option<HoverInfo> {
        match element_type {
            ElementType::Field {
                field_name,
                parent_type,
            } => Self::generate_field_hover(&field_name, &parent_type, schema_index),

            ElementType::TypeReference { type_name } => {
                Self::generate_type_hover(&type_name, schema_index)
            }

            ElementType::Argument {
                arg_name,
                field_name,
                parent_type,
            } => Self::generate_argument_hover(&arg_name, &field_name, &parent_type, schema_index),

            ElementType::Variable { var_name } => Self::generate_variable_hover(&var_name),

            ElementType::FragmentSpread { fragment_name } => {
                Self::generate_fragment_spread_hover(&fragment_name)
            }

            ElementType::FragmentDefinition {
                fragment_name,
                type_condition,
            } => Self::generate_fragment_definition_hover(&fragment_name, &type_condition),

            ElementType::Directive { directive_name } => {
                Self::generate_directive_hover(&directive_name, schema_index)
            }

            ElementType::EnumValue {
                value_name,
                enum_type,
            } => Self::generate_enum_value_hover(&value_name, enum_type.as_deref(), schema_index),

            ElementType::Operation {
                operation_type,
                operation_name,
            } => Self::generate_operation_hover(&operation_type, operation_name.as_deref()),
        }
    }

    /// Generate hover content for a field
    fn generate_field_hover(
        field_name: &str,
        parent_type: &str,
        schema_index: &SchemaIndex,
    ) -> Option<HoverInfo> {
        let fields = schema_index.get_fields(parent_type)?;
        let field_info = fields.iter().find(|f| f.name == field_name)?;

        let mut content = format!("### Field: `{field_name}`\n");
        content.push_str(&format!("**Type:** `{}`\n\n", field_info.type_name));

        if let Some(ref description) = field_info.description {
            content.push_str(description);
            content.push_str("\n\n");
        }

        if let Some(ref deprecated) = field_info.deprecated {
            content.push_str(&format!("⚠️ **Deprecated:** {deprecated}\n\n"));
        }

        if !field_info.arguments.is_empty() {
            content.push_str("**Arguments:**\n");
            for arg in &field_info.arguments {
                content.push_str(&format!("- `{}`: `{}`", arg.name, arg.type_name));
                if let Some(ref default) = arg.default_value {
                    content.push_str(&format!(" = `{default}`"));
                }
                if let Some(ref desc) = arg.description {
                    content.push_str(&format!(" - {desc}"));
                }
                content.push('\n');
            }
            content.push('\n');
        }

        content.push_str(&format!("**Defined in:** `{parent_type}` type"));

        Some(HoverInfo::new(content, None))
    }

    /// Generate hover content for a type
    fn generate_type_hover(type_name: &str, schema_index: &SchemaIndex) -> Option<HoverInfo> {
        // Strip wrapping characters ([], !)
        let base_type = type_name.trim_matches(|c| c == '[' || c == ']' || c == '!');

        let type_info = schema_index.get_type(base_type)?;

        let mut content = format!(
            "### Type: `{}`\n**Kind:** {}\n\n",
            type_info.name,
            Self::format_type_kind(&type_info.kind)
        );

        if let Some(ref description) = type_info.description {
            content.push_str(description);
            content.push_str("\n\n");
        }

        // Add fields for object/interface types
        if matches!(
            type_info.kind,
            crate::index::TypeKind::Object | crate::index::TypeKind::Interface
        ) {
            if let Some(fields) = schema_index.get_fields(&type_info.name) {
                if !fields.is_empty() {
                    content.push_str("**Fields:**\n");
                    for field in fields.iter().take(10) {
                        content.push_str(&format!("- `{}`: `{}`", field.name, field.type_name));
                        if field.deprecated.is_some() {
                            content.push_str(" ⚠️");
                        }
                        content.push('\n');
                    }
                    if fields.len() > 10 {
                        content.push_str(&format!("- ... and {} more\n", fields.len() - 10));
                    }
                }
            }
        }

        Some(HoverInfo::new(content, None))
    }

    /// Generate hover content for an argument
    fn generate_argument_hover(
        arg_name: &str,
        field_name: &str,
        parent_type: &str,
        schema_index: &SchemaIndex,
    ) -> Option<HoverInfo> {
        let fields = schema_index.get_fields(parent_type)?;
        let field_info = fields.iter().find(|f| f.name == field_name)?;
        let arg_info = field_info.arguments.iter().find(|a| a.name == arg_name)?;

        let mut content = format!("### Argument: `{arg_name}`\n");
        content.push_str(&format!("**Type:** `{}`\n\n", arg_info.type_name));

        if let Some(ref description) = arg_info.description {
            content.push_str(description);
            content.push_str("\n\n");
        }

        if let Some(ref default) = arg_info.default_value {
            content.push_str(&format!("**Default value:** `{default}`\n\n"));
        }

        let required = arg_info.type_name.ends_with('!');
        content.push_str(&format!(
            "**Required:** {}\n\n",
            if required { "Yes" } else { "No" }
        ));

        content.push_str(&format!(
            "**Defined in:** `{parent_type}.{field_name}` field"
        ));

        Some(HoverInfo::new(content, None))
    }

    /// Generate hover content for a variable
    fn generate_variable_hover(var_name: &str) -> Option<HoverInfo> {
        let content = format!("### Variable: `${var_name}`\n\nVariable usage in this operation");
        Some(HoverInfo::new(content, None))
    }

    /// Generate hover content for a fragment spread
    fn generate_fragment_spread_hover(fragment_name: &str) -> Option<HoverInfo> {
        let content =
            format!("### Fragment Spread: `{fragment_name}`\n\nReferences the fragment definition");
        Some(HoverInfo::new(content, None))
    }

    /// Generate hover content for a fragment definition
    fn generate_fragment_definition_hover(
        fragment_name: &str,
        type_condition: &str,
    ) -> Option<HoverInfo> {
        let content = format!(
            "### Fragment: `{fragment_name}`\n**Type condition:** `{type_condition}`\n\nFragment definition"
        );
        Some(HoverInfo::new(content, None))
    }

    /// Generate hover content for a directive
    fn generate_directive_hover(
        directive_name: &str,
        schema_index: &SchemaIndex,
    ) -> Option<HoverInfo> {
        let directive_info = schema_index.get_directive(directive_name)?;

        let mut content = format!("### Directive: `@{directive_name}`\n\n");

        if let Some(ref description) = directive_info.description {
            content.push_str(description);
            content.push_str("\n\n");
        }

        if !directive_info.locations.is_empty() {
            content.push_str("**Valid locations:**\n");
            for location in &directive_info.locations {
                content.push_str(&format!("- {location}\n"));
            }
        }

        Some(HoverInfo::new(content, None))
    }

    /// Generate hover content for an enum value
    const fn generate_enum_value_hover(
        _value_name: &str,
        _enum_type: Option<&str>,
        _schema_index: &SchemaIndex,
    ) -> Option<HoverInfo> {
        // TODO: Implement enum value hover
        None
    }

    /// Generate hover content for an operation
    fn generate_operation_hover(
        operation_type: &str,
        operation_name: Option<&str>,
    ) -> Option<HoverInfo> {
        let name_part = operation_name
            .map(|n| format!(" `{n}`"))
            .unwrap_or_default();

        let content = format!(
            "### {} Operation{}\n\nGraphQL {} operation",
            operation_type.to_uppercase(),
            name_part,
            operation_type
        );

        Some(HoverInfo::new(content, None))
    }

    /// Format a type kind for display
    const fn format_type_kind(kind: &crate::index::TypeKind) -> &'static str {
        match kind {
            crate::index::TypeKind::Object => "Object",
            crate::index::TypeKind::Interface => "Interface",
            crate::index::TypeKind::Union => "Union",
            crate::index::TypeKind::Enum => "Enum",
            crate::index::TypeKind::InputObject => "Input Object",
            crate::index::TypeKind::Scalar => "Scalar",
        }
    }
}

impl Default for HoverProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_schema() -> SchemaIndex {
        SchemaIndex::from_schema(
            r#"
            """
            A user in the system
            """
            type User {
                """
                Unique identifier
                """
                id: ID!

                """
                User's display name
                """
                name: String!

                """
                User's email address
                """
                email: String @deprecated(reason: "Use emailAddress instead")

                emailAddress: String

                """
                User's posts
                """
                posts(
                    """
                    Number of posts to fetch
                    """
                    first: Int = 10

                    """
                    Fetch posts after this cursor
                    """
                    after: String
                ): [Post!]!
            }

            type Post {
                id: ID!
                title: String!
                content: String!
                author: User!
            }

            type Query {
                """
                Get a user by ID
                """
                user(
                    """
                    The user ID
                    """
                    id: ID!
                ): User

                """
                Get all users
                """
                users: [User!]!
            }

            type Mutation {
                createUser(name: String!): User
            }

            enum Status {
                ACTIVE
                INACTIVE
                PENDING
            }

            directive @auth(requires: String!) on FIELD_DEFINITION

            "#,
        )
    }

    #[test]
    fn test_hover_on_field() {
        let schema = create_test_schema();
        let provider = HoverProvider::new();

        let document = r"
query GetUser($id: ID!) {
    user(id: $id) {
        name
        email
    }
}
";

        // Hover on "name" field
        let position = Position {
            line: 3,
            character: 8,
        };

        let hover_info = provider.hover(document, position, &schema);
        assert!(hover_info.is_some());

        let info = hover_info.unwrap();
        assert!(info.contents.contains("Field: `name`"));
        assert!(info.contents.contains("String!"));
        assert!(info.contents.contains("User's display name"));
    }

    #[test]
    fn test_hover_on_deprecated_field() {
        let schema = create_test_schema();
        let provider = HoverProvider::new();

        let document = r"
query GetUser($id: ID!) {
    user(id: $id) {
        email
    }
}
";

        // Hover on "email" field
        let position = Position {
            line: 3,
            character: 8,
        };

        let hover_info = provider.hover(document, position, &schema);
        assert!(hover_info.is_some());

        let info = hover_info.unwrap();
        assert!(info.contents.contains("Field: `email`"));
        assert!(info.contents.contains("Deprecated"));
        assert!(info.contents.contains("Use emailAddress instead"));
    }

    #[test]
    fn test_hover_on_field_with_arguments() {
        let schema = create_test_schema();
        let provider = HoverProvider::new();

        let document = r"
query GetUser($id: ID!) {
    user(id: $id) {
        posts(first: 5) {
            title
        }
    }
}
";

        // Hover on "posts" field
        let position = Position {
            line: 3,
            character: 8,
        };

        let hover_info = provider.hover(document, position, &schema);
        assert!(hover_info.is_some());

        let info = hover_info.unwrap();
        assert!(info.contents.contains("Field: `posts`"));
        assert!(info.contents.contains("[Post!]!"));
        assert!(info.contents.contains("Arguments:"));
        assert!(info.contents.contains("`first`"));
        assert!(info.contents.contains("`after`"));
    }

    #[test]
    fn test_hover_on_argument() {
        let schema = create_test_schema();
        let provider = HoverProvider::new();

        let document = r"
query GetUser($id: ID!) {
    user(id: $id) {
        id
    }
}
";

        // Hover on "id" argument
        let position = Position {
            line: 2,
            character: 10,
        };

        let hover_info = provider.hover(document, position, &schema);
        assert!(hover_info.is_some());

        let info = hover_info.unwrap();
        assert!(info.contents.contains("Argument: `id`"));
        assert!(info.contents.contains("ID!"));
        assert!(info.contents.contains("The user ID"));
        assert!(info.contents.contains("Required"));
    }

    #[test]
    fn test_hover_on_type() {
        let schema = create_test_schema();
        let provider = HoverProvider::new();

        let document = r"
fragment UserFields on User {
    id
    name
}
";

        // Hover on "User" type condition
        let position = Position {
            line: 1,
            character: 24,
        };

        let hover_info = provider.hover(document, position, &schema);
        assert!(hover_info.is_some());

        let info = hover_info.unwrap();
        assert!(info.contents.contains("Type: `User`"));
        assert!(info.contents.contains("Object"));
        assert!(info.contents.contains("A user in the system"));
        assert!(info.contents.contains("Fields"));
    }

    #[test]
    fn test_hover_on_variable() {
        let schema = create_test_schema();
        let provider = HoverProvider::new();

        let document = r"
query GetUser($userId: ID!) {
    user(id: $userId) {
        id
    }
}
";

        // Hover on "$userId" in variable definition
        let position = Position {
            line: 1,
            character: 15,
        };

        let hover_info = provider.hover(document, position, &schema);
        assert!(hover_info.is_some());

        let info = hover_info.unwrap();
        assert!(info.contents.contains("Variable: `$userId`"));
    }

    #[test]
    fn test_hover_on_fragment_definition() {
        let schema = create_test_schema();
        let provider = HoverProvider::new();

        let document = r"
fragment UserFields on User {
    id
    name
}
";

        // Hover on "UserFields" fragment name
        let position = Position {
            line: 1,
            character: 10,
        };

        let hover_info = provider.hover(document, position, &schema);
        assert!(hover_info.is_some());

        let info = hover_info.unwrap();
        assert!(info.contents.contains("Fragment: `UserFields`"));
        assert!(info.contents.contains("User"));
    }

    #[test]
    fn test_hover_on_operation() {
        let schema = create_test_schema();
        let provider = HoverProvider::new();

        let document = r"
query GetUser($id: ID!) {
    user(id: $id) {
        id
    }
}
";

        // Hover on "GetUser" operation name
        let position = Position {
            line: 1,
            character: 7,
        };

        let hover_info = provider.hover(document, position, &schema);
        assert!(hover_info.is_some());

        let info = hover_info.unwrap();
        assert!(info.contents.contains("QUERY Operation"));
        assert!(info.contents.contains("`GetUser`"));
    }

    #[test]
    fn test_no_hover_on_syntax_error() {
        let schema = create_test_schema();
        let provider = HoverProvider::new();

        let document = r"
query GetUser($id: ID!) {
    user(id: $id) {
        name
"; // Incomplete document

        let position = Position {
            line: 3,
            character: 8,
        };

        let hover_info = provider.hover(document, position, &schema);
        assert!(hover_info.is_none());
    }
}
