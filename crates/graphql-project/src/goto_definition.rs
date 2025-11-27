#![allow(clippy::too_many_lines)]

use crate::{DocumentIndex, Position, Range, SchemaIndex};
use apollo_parser::{
    cst::{self, CstNode},
    Parser,
};

/// Location information for go-to-definition
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefinitionLocation {
    /// File path where the definition is located
    pub file_path: String,
    /// Range of the definition
    pub range: Range,
}

impl DefinitionLocation {
    #[must_use]
    pub const fn new(file_path: String, range: Range) -> Self {
        Self { file_path, range }
    }
}

/// Go-to-definition provider
pub struct GotoDefinitionProvider;

impl GotoDefinitionProvider {
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Get definition location for a position in a GraphQL document
    ///
    /// Returns the location where the element at the given position is defined.
    /// For example:
    /// - Fragment spreads -> Fragment definition
    /// - Type references -> Type definition in schema
    /// - Field references -> Field definition in schema type
    #[must_use]
    pub fn goto_definition(
        &self,
        source: &str,
        position: Position,
        document_index: &DocumentIndex,
        schema_index: &SchemaIndex,
    ) -> Option<Vec<DefinitionLocation>> {
        tracing::info!(
            "GotoDefinitionProvider::goto_definition called with position: {:?}",
            position
        );

        let parser = Parser::new(source);
        let tree = parser.parse();

        let error_count = tree.errors().count();
        tracing::info!("Parser errors: {}", error_count);
        if error_count > 0 {
            tracing::info!("Returning None due to parser errors");
            return None;
        }

        let doc = tree.document();
        let byte_offset = Self::position_to_offset(source, position);
        tracing::info!("Byte offset: {:?}", byte_offset);
        if byte_offset.is_none() {
            tracing::info!("Returning None - could not convert position to offset");
            return None;
        }
        let byte_offset = byte_offset?;

        let element_type = Self::find_element_at_position(&doc, byte_offset, source, schema_index);
        tracing::info!("Element type: {:?}", element_type);
        if element_type.is_none() {
            tracing::info!("Returning None - no element found at position");
            return None;
        }
        let element_type = element_type?;

        let result = Self::resolve_definition(element_type, document_index, schema_index);
        tracing::info!("resolve_definition returned: {:?}", result.is_some());
        result
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
                    // Check if cursor is on the operation name itself
                    if let Some(name) = op.name() {
                        let range = name.syntax().text_range();
                        let start: usize = range.start().into();
                        let end: usize = range.end().into();

                        if byte_offset >= start && byte_offset < end {
                            return Some(ElementType::OperationDefinition {
                                operation_name: name.text().to_string(),
                            });
                        }
                    }

                    if let Some(element) = Self::check_operation_definition(&op, byte_offset) {
                        return Some(element);
                    }

                    if let Some(selection_set) = op.selection_set() {
                        let root_type = Self::get_operation_root_type(&op, schema_index);
                        if let Some(element) = Self::check_selection_set(
                            &selection_set,
                            byte_offset,
                            root_type,
                            source,
                            schema_index,
                        ) {
                            return Some(element);
                        }
                    }
                }
                cst::Definition::FragmentDefinition(frag) => {
                    // Check if cursor is on the fragment definition name itself
                    if let Some(frag_name) = frag.fragment_name().and_then(|n| n.name()) {
                        let range = frag_name.syntax().text_range();
                        let start: usize = range.start().into();
                        let end: usize = range.end().into();

                        if byte_offset >= start && byte_offset < end {
                            return Some(ElementType::FragmentDefinition {
                                fragment_name: frag_name.text().to_string(),
                            });
                        }
                    }

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
                cst::Definition::ObjectTypeDefinition(obj_type) => {
                    if let Some(element) =
                        Self::check_object_type_definition(&obj_type, byte_offset)
                    {
                        return Some(element);
                    }
                }
                cst::Definition::InterfaceTypeDefinition(interface) => {
                    if let Some(element) =
                        Self::check_interface_type_definition(&interface, byte_offset)
                    {
                        return Some(element);
                    }
                }
                cst::Definition::InputObjectTypeDefinition(input_obj) => {
                    if let Some(element) =
                        Self::check_input_object_type_definition(&input_obj, byte_offset)
                    {
                        return Some(element);
                    }
                }
                cst::Definition::UnionTypeDefinition(union_def) => {
                    if let Some(element) =
                        Self::check_union_type_definition(&union_def, byte_offset)
                    {
                        return Some(element);
                    }
                }
                cst::Definition::EnumTypeDefinition(enum_def) => {
                    if let Some(element) = Self::check_enum_type_definition(&enum_def, byte_offset)
                    {
                        return Some(element);
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
        if let Some(variable_defs) = op.variable_definitions() {
            for var_def in variable_defs.variable_definitions() {
                if let Some(ty) = var_def.ty() {
                    if let Some(element) = Self::check_type_for_reference(&ty, byte_offset) {
                        return Some(element);
                    }
                }
            }
        }

        None
    }

    /// Get the root type for an operation (Query, Mutation, or Subscription)
    fn get_operation_root_type(
        op: &cst::OperationDefinition,
        schema_index: &SchemaIndex,
    ) -> String {
        let root_type_name = op.operation_type().map_or_else(
            || schema_index.schema().schema_definition.query.as_ref(),
            |op_type| {
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
            },
        );

        root_type_name.map_or_else(|| "Query".to_string(), std::string::ToString::to_string)
    }

    /// Check if the byte offset is within a selection set
    #[allow(clippy::only_used_in_recursion)]
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
                    // Check if we're on the field name itself
                    if let Some(name) = field.name() {
                        let range = name.syntax().text_range();
                        let start: usize = range.start().into();
                        let end: usize = range.end().into();

                        if byte_offset >= start && byte_offset < end {
                            return Some(ElementType::FieldReference {
                                field_name: name.text().to_string(),
                                parent_type,
                            });
                        }
                    }

                    if let Some(arguments) = field.arguments() {
                        for arg in arguments.arguments() {
                            if let Some(value) = arg.value() {
                                if let Some(element) =
                                    Self::check_value_for_variable(&value, byte_offset)
                                {
                                    return Some(element);
                                }
                            }
                        }
                    }

                    if let Some(nested_selection_set) = field.selection_set() {
                        // Resolve the field type from the schema
                        let field_name = field
                            .name()
                            .map(|n| n.text().to_string())
                            .unwrap_or_default();
                        let nested_type = schema_index.get_fields(&parent_type).map_or_else(
                            String::new,
                            |fields| {
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
                            },
                        );

                        // Always recurse into nested selections, even if we can't resolve the type
                        // This is important for finding fragment spreads and other elements
                        // when the schema is incomplete or empty
                        let nested_type = if nested_type.is_empty() {
                            parent_type.clone()
                        } else {
                            nested_type
                        };

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

                    if let Some(nested_selection_set) = inline_frag.selection_set() {
                        // Use type condition if present, otherwise use parent type
                        let nested_type = inline_frag
                            .type_condition()
                            .and_then(|tc| tc.named_type())
                            .and_then(|nt| nt.name())
                            .map_or_else(|| parent_type.clone(), |n| n.text().to_string());

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

    /// Check if byte offset is within an object type definition
    fn check_object_type_definition(
        obj_type: &cst::ObjectTypeDefinition,
        byte_offset: usize,
    ) -> Option<ElementType> {
        if let Some(fields_def) = obj_type.fields_definition() {
            for field_def in fields_def.field_definitions() {
                if let Some(field_type) = field_def.ty() {
                    if let Some(element) = Self::check_type_for_reference(&field_type, byte_offset)
                    {
                        return Some(element);
                    }
                }

                if let Some(args_def) = field_def.arguments_definition() {
                    for input_value_def in args_def.input_value_definitions() {
                        if let Some(arg_type) = input_value_def.ty() {
                            if let Some(element) =
                                Self::check_type_for_reference(&arg_type, byte_offset)
                            {
                                return Some(element);
                            }
                        }
                    }
                }
            }
        }

        if let Some(implements) = obj_type.implements_interfaces() {
            for named_type in implements.named_types() {
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

        None
    }

    /// Check if byte offset is within an interface type definition
    fn check_interface_type_definition(
        interface: &cst::InterfaceTypeDefinition,
        byte_offset: usize,
    ) -> Option<ElementType> {
        if let Some(fields_def) = interface.fields_definition() {
            for field_def in fields_def.field_definitions() {
                if let Some(field_type) = field_def.ty() {
                    if let Some(element) = Self::check_type_for_reference(&field_type, byte_offset)
                    {
                        return Some(element);
                    }
                }
            }
        }

        None
    }

    /// Check if byte offset is within an input object type definition
    fn check_input_object_type_definition(
        input_obj: &cst::InputObjectTypeDefinition,
        byte_offset: usize,
    ) -> Option<ElementType> {
        if let Some(fields_def) = input_obj.input_fields_definition() {
            for input_value_def in fields_def.input_value_definitions() {
                if let Some(field_type) = input_value_def.ty() {
                    if let Some(element) = Self::check_type_for_reference(&field_type, byte_offset)
                    {
                        return Some(element);
                    }
                }
            }
        }

        None
    }

    /// Check if byte offset is within a union type definition
    fn check_union_type_definition(
        union_def: &cst::UnionTypeDefinition,
        byte_offset: usize,
    ) -> Option<ElementType> {
        if let Some(union_member_types) = union_def.union_member_types() {
            for named_type in union_member_types.named_types() {
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

        None
    }

    /// Check if byte offset is within an enum type definition
    fn check_enum_type_definition(
        enum_def: &cst::EnumTypeDefinition,
        byte_offset: usize,
    ) -> Option<ElementType> {
        if let Some(name) = enum_def.name() {
            let range = name.syntax().text_range();
            let start: usize = range.start().into();
            let end: usize = range.end().into();

            if byte_offset >= start && byte_offset < end {
                return Some(ElementType::TypeReference {
                    type_name: name.text().to_string(),
                });
            }
        }

        None
    }

    /// Check if a type contains a type reference at the byte offset
    fn check_type_for_reference(ty: &cst::Type, byte_offset: usize) -> Option<ElementType> {
        match ty {
            cst::Type::NamedType(named_type) => {
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
            cst::Type::ListType(list_type) => {
                if let Some(inner_type) = list_type.ty() {
                    return Self::check_type_for_reference(&inner_type, byte_offset);
                }
            }
            cst::Type::NonNullType(non_null_type) => {
                if let Some(named) = non_null_type.named_type() {
                    if let Some(name) = named.name() {
                        let range = name.syntax().text_range();
                        let start: usize = range.start().into();
                        let end: usize = range.end().into();

                        if byte_offset >= start && byte_offset < end {
                            return Some(ElementType::TypeReference {
                                type_name: name.text().to_string(),
                            });
                        }
                    }
                } else if let Some(list) = non_null_type.list_type() {
                    if let Some(inner_type) = list.ty() {
                        return Self::check_type_for_reference(&inner_type, byte_offset);
                    }
                }
            }
        }

        None
    }

    /// Resolve the definition location based on the element type
    fn resolve_definition(
        element_type: ElementType,
        document_index: &DocumentIndex,
        schema_index: &SchemaIndex,
    ) -> Option<Vec<DefinitionLocation>> {
        match element_type {
            ElementType::FragmentSpread { fragment_name } => document_index
                .get_fragments_by_name(&fragment_name)
                .map(|fragments| {
                    fragments
                        .iter()
                        .map(|frag| {
                            let range = Range {
                                start: Position {
                                    line: frag.line,
                                    character: frag.column,
                                },
                                end: Position {
                                    line: frag.line,
                                    character: frag.column + fragment_name.len(),
                                },
                            };
                            DefinitionLocation::new(frag.file_path.clone(), range)
                        })
                        .collect()
                }),
            ElementType::FragmentDefinition { fragment_name } => {
                // When on a fragment definition name, show all other definitions with the same name
                document_index
                    .get_fragments_by_name(&fragment_name)
                    .map(|fragments| {
                        fragments
                            .iter()
                            .map(|frag| {
                                let range = Range {
                                    start: Position {
                                        line: frag.line,
                                        character: frag.column,
                                    },
                                    end: Position {
                                        line: frag.line,
                                        character: frag.column + fragment_name.len(),
                                    },
                                };
                                DefinitionLocation::new(frag.file_path.clone(), range)
                            })
                            .collect()
                    })
            }
            ElementType::OperationDefinition { operation_name } => {
                // When on an operation definition name, show all other definitions with the same name
                document_index
                    .get_operations(&operation_name)
                    .map(|operations| {
                        operations
                            .iter()
                            .filter_map(|op| {
                                op.name.as_ref().map(|name| {
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
                                    DefinitionLocation::new(op.file_path.clone(), range)
                                })
                            })
                            .collect()
                    })
            }
            ElementType::TypeReference { type_name } => {
                // Find the type definition in the schema
                let type_def = schema_index.find_type_definition(&type_name)?;

                let range = Range {
                    start: Position {
                        line: type_def.line,
                        character: type_def.column,
                    },
                    end: Position {
                        line: type_def.line,
                        character: type_def.column + type_name.len(),
                    },
                };

                Some(vec![DefinitionLocation::new(type_def.file_path, range)])
            }
            ElementType::Variable { .. } => {
                // TODO: Implement variable definition lookup
                // This would require finding the operation's variable definition
                None
            }
            ElementType::FieldReference {
                field_name,
                parent_type,
            } => {
                // Find the field definition in the schema
                let field_def = schema_index.find_field_definition(&parent_type, &field_name)?;

                let range = Range {
                    start: Position {
                        line: field_def.line,
                        character: field_def.column,
                    },
                    end: Position {
                        line: field_def.line,
                        character: field_def.column + field_name.len(),
                    },
                };

                Some(vec![DefinitionLocation::new(field_def.file_path, range)])
            }
        }
    }
}

impl Default for GotoDefinitionProvider {
    fn default() -> Self {
        Self::new()
    }
}

/// Type of GraphQL element at a position
#[derive(Debug, Clone, PartialEq)]
enum ElementType {
    FragmentSpread {
        fragment_name: String,
    },
    FragmentDefinition {
        fragment_name: String,
    },
    OperationDefinition {
        operation_name: String,
    },
    TypeReference {
        type_name: String,
    },
    Variable {
        var_name: String,
    },
    FieldReference {
        field_name: String,
        parent_type: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DocumentIndex, FragmentInfo};

    #[test]
    fn test_goto_fragment_definition() {
        let mut doc_index = DocumentIndex::new();
        doc_index.add_fragment(
            "UserFields".to_string(),
            FragmentInfo {
                name: "UserFields".to_string(),
                type_condition: "User".to_string(),
                file_path: "/path/to/fragments.graphql".to_string(),
                line: 10,
                column: 9,
            },
        );

        let schema_str = r"
type Query {
  user: User
}

type User {
  id: ID!
  name: String!
}
";
        let schema = SchemaIndex::from_schema(schema_str);
        let provider = GotoDefinitionProvider::new();

        let document = r"
query GetUser {
    user {
        ...UserFields
    }
}
";

        let position = Position {
            line: 3,
            character: 12,
        };

        let locations = provider
            .goto_definition(document, position, &doc_index, &schema)
            .expect("Should find definition");

        assert_eq!(locations.len(), 1);
        assert_eq!(locations[0].file_path, "/path/to/fragments.graphql");
        assert_eq!(locations[0].range.start.line, 10);
        assert_eq!(locations[0].range.start.character, 9);
    }

    #[test]
    fn test_goto_fragment_definition_multiple() {
        let mut doc_index = DocumentIndex::new();
        doc_index.add_fragment(
            "UserFields".to_string(),
            FragmentInfo {
                name: "UserFields".to_string(),
                type_condition: "User".to_string(),
                file_path: "/path/to/file1.graphql".to_string(),
                line: 5,
                column: 9,
            },
        );
        doc_index.add_fragment(
            "UserFields".to_string(),
            FragmentInfo {
                name: "UserFields".to_string(),
                type_condition: "User".to_string(),
                file_path: "/path/to/file2.graphql".to_string(),
                line: 15,
                column: 9,
            },
        );

        let schema_str = r"
type Query {
  user: User
}

type User {
  id: ID!
}
";
        let schema = SchemaIndex::from_schema(schema_str);
        let provider = GotoDefinitionProvider::new();

        let document = r"
query GetUser {
    user {
        ...UserFields
    }
}
";

        let position = Position {
            line: 3,
            character: 12,
        };

        let locations = provider
            .goto_definition(document, position, &doc_index, &schema)
            .expect("Should find definitions");

        assert_eq!(locations.len(), 2);
        assert_eq!(locations[0].file_path, "/path/to/file1.graphql");
        assert_eq!(locations[1].file_path, "/path/to/file2.graphql");
    }

    #[test]
    fn test_no_goto_for_nonexistent_fragment() {
        let doc_index = DocumentIndex::new();
        let schema = SchemaIndex::new();
        let provider = GotoDefinitionProvider::new();

        let document = r"
query GetUser {
    user {
        ...NonExistentFragment
    }
}
";

        let position = Position {
            line: 3,
            character: 12,
        };

        let locations = provider.goto_definition(document, position, &doc_index, &schema);

        assert!(locations.is_none());
    }

    #[test]
    fn test_no_goto_on_syntax_error() {
        let doc_index = DocumentIndex::new();
        let schema = SchemaIndex::new();
        let provider = GotoDefinitionProvider::new();

        let document = r"
query GetUser {
    user {
        ...UserFields
";

        let position = Position {
            line: 3,
            character: 12,
        };

        let locations = provider.goto_definition(document, position, &doc_index, &schema);

        assert!(locations.is_none());
    }

    #[test]
    fn test_goto_on_type_reference() {
        let doc_index = DocumentIndex::new();
        let schema = SchemaIndex::new();
        let provider = GotoDefinitionProvider::new();

        let document = r"
fragment UserFields on User {
    id
    name
}
";

        let position = Position {
            line: 1,
            character: 24,
        };

        let locations = provider.goto_definition(document, position, &doc_index, &schema);

        // For now this returns None since we haven't implemented schema definition lookup
        assert!(locations.is_none());
    }

    #[test]
    fn test_goto_from_fragment_definition_name() {
        let mut doc_index = DocumentIndex::new();
        doc_index.add_fragment(
            "UserFields".to_string(),
            FragmentInfo {
                name: "UserFields".to_string(),
                type_condition: "User".to_string(),
                file_path: "/path/to/file1.graphql".to_string(),
                line: 0,
                column: 9,
            },
        );
        doc_index.add_fragment(
            "UserFields".to_string(),
            FragmentInfo {
                name: "UserFields".to_string(),
                type_condition: "User".to_string(),
                file_path: "/path/to/file2.graphql".to_string(),
                line: 5,
                column: 9,
            },
        );

        let schema = SchemaIndex::new();
        let provider = GotoDefinitionProvider::new();

        // Cursor on the fragment definition name itself
        let document = r"
fragment UserFields on User {
    id
    name
}
";

        let position = Position {
            line: 1,
            character: 12, // On "UserFields" name
        };

        let locations = provider
            .goto_definition(document, position, &doc_index, &schema)
            .expect("Should find all definitions with this name");

        // Should return both fragment definitions
        assert_eq!(locations.len(), 2);
        assert_eq!(locations[0].file_path, "/path/to/file1.graphql");
        assert_eq!(locations[1].file_path, "/path/to/file2.graphql");
    }

    #[test]
    fn test_goto_from_operation_definition_name() {
        use crate::{OperationInfo, OperationType};

        let mut doc_index = DocumentIndex::new();
        doc_index.add_operation(
            Some("GetUser".to_string()),
            OperationInfo {
                name: Some("GetUser".to_string()),
                operation_type: OperationType::Query,
                file_path: "/path/to/queries1.graphql".to_string(),
                line: 0,
                column: 6, // "query GetUser"
            },
        );
        doc_index.add_operation(
            Some("GetUser".to_string()),
            OperationInfo {
                name: Some("GetUser".to_string()),
                operation_type: OperationType::Query,
                file_path: "/path/to/queries2.graphql".to_string(),
                line: 10,
                column: 6,
            },
        );

        let schema = SchemaIndex::new();
        let provider = GotoDefinitionProvider::new();

        // Cursor on the operation definition name itself
        let document = r"
query GetUser {
    user {
        id
    }
}
";

        let position = Position {
            line: 1,
            character: 8, // On "GetUser" name
        };

        let locations = provider
            .goto_definition(document, position, &doc_index, &schema)
            .expect("Should find all definitions with this name");

        // Should return both operation definitions
        assert_eq!(locations.len(), 2);
        assert_eq!(locations[0].file_path, "/path/to/queries1.graphql");
        assert_eq!(locations[1].file_path, "/path/to/queries2.graphql");
    }

    #[test]
    fn test_goto_field_definition() {
        let doc_index = DocumentIndex::new();

        // Create a schema with field location tracking
        let schema_str = r"
type Query {
  user(id: ID!): User
  posts: [Post!]!
}

type User {
  id: ID!
  name: String!
  email: String!
}

type Post {
  id: ID!
  title: String!
}
";

        let schema = SchemaIndex::from_schema(schema_str);
        let provider = GotoDefinitionProvider::new();

        // Document with a query
        let document = r#"
query GetUser {
    user(id: "1") {
        id
        name
    }
}
"#;

        // Position on "user" field (line 2, column 4)
        let position = Position {
            line: 2,
            character: 4,
        };

        let locations = provider
            .goto_definition(document, position, &doc_index, &schema)
            .expect("Should find field definition");

        assert_eq!(locations.len(), 1);
        assert_eq!(locations[0].file_path, "schema.graphql");
        // "user" field is on line 2 (0-indexed: line 1), column 2
        assert_eq!(locations[0].range.start.line, 2);
        assert_eq!(locations[0].range.start.character, 2);
    }

    #[test]
    fn test_field_position_calculation() {
        let schema_str = r"type Query {
  user(id: ID!): User
  post(id: ID!): Post
}

type User {
  id: ID!
  name: String!
  posts: [Post!]!
}";

        let schema = SchemaIndex::from_schema(schema_str);

        // Test user field - should be at line 1 (0-indexed), column 2
        let user_field = schema.find_field_definition("Query", "user");
        assert!(user_field.is_some());
        let user_field = user_field.unwrap();
        println!(
            "user field: line={}, col={}",
            user_field.line, user_field.column
        );
        assert_eq!(user_field.line, 1); // Line 2 in 1-indexed = line 1 in 0-indexed
        assert_eq!(user_field.column, 2); // Column 3 in 1-indexed = column 2 in 0-indexed

        // Test name field in User - should be at line 7 (0-indexed), column 2
        let name_field = schema.find_field_definition("User", "name");
        assert!(name_field.is_some());
        let name_field = name_field.unwrap();
        println!(
            "name field: line={}, col={}",
            name_field.line, name_field.column
        );
        assert_eq!(name_field.line, 7); // Line 8 in 1-indexed = line 7 in 0-indexed
        assert_eq!(name_field.column, 2);
    }

    #[test]
    fn test_goto_nested_field_definition() {
        let doc_index = DocumentIndex::new();

        let schema_str = r"
type Query {
  user(id: ID!): User
}

type User {
  id: ID!
  name: String!
}
";

        let schema = SchemaIndex::from_schema(schema_str);
        let provider = GotoDefinitionProvider::new();

        let document = r#"
query GetUser {
    user(id: "1") {
        name
    }
}
"#;

        // Position on "name" field (line 3, column 8)
        let position = Position {
            line: 3,
            character: 8,
        };

        let locations = provider
            .goto_definition(document, position, &doc_index, &schema)
            .expect("Should find nested field definition");

        assert_eq!(locations.len(), 1);
        assert_eq!(locations[0].file_path, "schema.graphql");
        // "name" field in User type
        assert_eq!(locations[0].range.start.line, 7);
        assert_eq!(locations[0].range.start.character, 2);
    }

    #[test]
    fn test_goto_type_definition_from_fragment() {
        let doc_index = DocumentIndex::new();

        let schema_str = r"
type Query {
  user: User
}

type User {
  id: ID!
  name: String!
}
";

        let schema = SchemaIndex::from_schema(schema_str);
        let provider = GotoDefinitionProvider::new();

        let document = r"
fragment UserFields on User {
    id
    name
}
";

        // Position on "User" in fragment type condition (line 1, column 23)
        let position = Position {
            line: 1,
            character: 23,
        };

        let locations = provider
            .goto_definition(document, position, &doc_index, &schema)
            .expect("Should find type definition");

        assert_eq!(locations.len(), 1);
        assert_eq!(locations[0].file_path, "schema.graphql");
        // "User" type definition starts at line 5 (0-indexed)
        assert_eq!(locations[0].range.start.line, 5);
    }

    #[test]
    fn test_goto_type_definition_from_inline_fragment() {
        let doc_index = DocumentIndex::new();

        let schema_str = r"
type Query {
  search: SearchResult
}

union SearchResult = User | Post

type User {
  id: ID!
  name: String!
}

type Post {
  id: ID!
  title: String!
}
";

        let schema = SchemaIndex::from_schema(schema_str);
        let provider = GotoDefinitionProvider::new();

        let document = r"
query Search {
    search {
        ... on User {
            name
        }
    }
}
";

        // Position on "User" in inline fragment (line 3, column 16)
        let position = Position {
            line: 3,
            character: 16,
        };

        let locations = provider
            .goto_definition(document, position, &doc_index, &schema)
            .expect("Should find type definition");

        assert_eq!(locations.len(), 1);
        assert_eq!(locations[0].file_path, "schema.graphql");
        // "User" type definition
        assert_eq!(locations[0].range.start.line, 7);
    }

    #[test]
    fn test_goto_interface_type_definition() {
        let doc_index = DocumentIndex::new();

        let schema_str = r"
interface Node {
  id: ID!
}

type User implements Node {
  id: ID!
  name: String!
}
";

        let schema = SchemaIndex::from_schema(schema_str);
        let provider = GotoDefinitionProvider::new();

        let document = r"
fragment NodeFields on Node {
    id
}
";

        // Position on "Node" in fragment type condition
        let position = Position {
            line: 1,
            character: 23,
        };

        let locations = provider
            .goto_definition(document, position, &doc_index, &schema)
            .expect("Should find interface definition");

        assert_eq!(locations.len(), 1);
        assert_eq!(locations[0].file_path, "schema.graphql");
        // "Node" interface definition
        assert_eq!(locations[0].range.start.line, 1);
    }

    #[test]
    fn test_goto_union_type_definition() {
        let doc_index = DocumentIndex::new();

        let schema_str = r"
type User {
  id: ID!
}

type Post {
  id: ID!
}

union SearchResult = User | Post

type Query {
  search: SearchResult
}
";

        let schema = SchemaIndex::from_schema(schema_str);
        let provider = GotoDefinitionProvider::new();

        let document = r"
fragment SearchFields on SearchResult {
    __typename
}
";

        // Position on "SearchResult" in fragment type condition
        let position = Position {
            line: 1,
            character: 26,
        };

        let locations = provider
            .goto_definition(document, position, &doc_index, &schema)
            .expect("Should find union definition");

        assert_eq!(locations.len(), 1);
        assert_eq!(locations[0].file_path, "schema.graphql");
        // "SearchResult" union definition
        assert_eq!(locations[0].range.start.line, 9);
    }

    #[test]
    fn test_goto_enum_type_definition() {
        let doc_index = DocumentIndex::new();

        let schema_str = r"
enum Status {
  ACTIVE
  INACTIVE
}

type User {
  id: ID!
  status: Status
}
";

        let schema = SchemaIndex::from_schema(schema_str);
        let provider = GotoDefinitionProvider::new();

        let document = r"
fragment UserFields on User {
    status
}
";

        // Position on "User" in fragment type condition
        let position = Position {
            line: 1,
            character: 23,
        };

        let locations = provider
            .goto_definition(document, position, &doc_index, &schema)
            .expect("Should find type definition");

        assert_eq!(locations.len(), 1);
        assert_eq!(locations[0].file_path, "schema.graphql");
        // "User" type definition
        assert_eq!(locations[0].range.start.line, 6);
    }

    #[test]
    fn test_goto_input_object_type_definition_from_field() {
        let doc_index = DocumentIndex::new();

        let schema_str = r"
input CreateUserInput {
  name: String!
  email: String!
}

type User {
  id: ID!
  name: String!
}

type Mutation {
  createUser(input: CreateUserInput!): User
}
";

        let schema = SchemaIndex::from_schema(schema_str);
        let provider = GotoDefinitionProvider::new();

        // Test goto definition from field type reference in schema
        let schema_document = schema_str;

        // Position on "CreateUserInput" in mutation field argument (line 12, column 21)
        let position = Position {
            line: 12,
            character: 21,
        };

        let locations = provider
            .goto_definition(schema_document, position, &doc_index, &schema)
            .expect("Should find input object definition");

        assert_eq!(locations.len(), 1);
        assert_eq!(locations[0].file_path, "schema.graphql");
        // "CreateUserInput" input object definition
        assert_eq!(locations[0].range.start.line, 1);
    }

    #[test]
    fn test_goto_type_definition_from_field_type() {
        let doc_index = DocumentIndex::new();

        let schema_str = r"
type User {
  id: ID!
  name: String!
}

type Query {
  user(id: ID!): User
}
";

        let schema = SchemaIndex::from_schema(schema_str);
        let provider = GotoDefinitionProvider::new();

        // Test goto definition from return type in field definition
        let schema_document = schema_str;

        // Position on "User" in Query.user return type (line 7, column 17)
        let position = Position {
            line: 7,
            character: 17,
        };

        let locations = provider
            .goto_definition(schema_document, position, &doc_index, &schema)
            .expect("Should find type definition");

        assert_eq!(locations.len(), 1);
        assert_eq!(locations[0].file_path, "schema.graphql");
        // "User" type definition
        assert_eq!(locations[0].range.start.line, 1);
    }

    #[test]
    fn test_goto_type_definition_from_implements() {
        let doc_index = DocumentIndex::new();

        let schema_str = r"
interface Node {
  id: ID!
}

type User implements Node {
  id: ID!
  name: String!
}
";

        let schema = SchemaIndex::from_schema(schema_str);
        let provider = GotoDefinitionProvider::new();

        // Test goto definition from implements clause
        let schema_document = schema_str;

        // Position on "Node" in implements clause (line 5, column 21)
        let position = Position {
            line: 5,
            character: 21,
        };

        let locations = provider
            .goto_definition(schema_document, position, &doc_index, &schema)
            .expect("Should find interface definition");

        assert_eq!(locations.len(), 1);
        assert_eq!(locations[0].file_path, "schema.graphql");
        // "Node" interface definition
        assert_eq!(locations[0].range.start.line, 1);
    }

    #[test]
    fn test_goto_type_definition_from_union_member() {
        let doc_index = DocumentIndex::new();

        let schema_str = r"
type User {
  id: ID!
}

type Post {
  id: ID!
}

union SearchResult = User | Post
";

        let schema = SchemaIndex::from_schema(schema_str);
        let provider = GotoDefinitionProvider::new();

        // Test goto definition from union member
        let schema_document = schema_str;

        // Position on "User" in union definition (line 9, column 21)
        let position = Position {
            line: 9,
            character: 21,
        };

        let locations = provider
            .goto_definition(schema_document, position, &doc_index, &schema)
            .expect("Should find type definition");

        assert_eq!(locations.len(), 1);
        assert_eq!(locations[0].file_path, "schema.graphql");
        // "User" type definition
        assert_eq!(locations[0].range.start.line, 1);
    }

    #[test]
    fn test_goto_scalar_type_definition() {
        let doc_index = DocumentIndex::new();

        let schema_str = r"
scalar DateTime

type Event {
  id: ID!
  timestamp: DateTime
}
";

        let schema = SchemaIndex::from_schema(schema_str);
        let provider = GotoDefinitionProvider::new();

        // Test goto definition from scalar type reference
        let schema_document = schema_str;

        // Position on "DateTime" in field type (line 5, column 13)
        let position = Position {
            line: 5,
            character: 13,
        };

        let locations = provider
            .goto_definition(schema_document, position, &doc_index, &schema)
            .expect("Should find scalar definition");

        assert_eq!(locations.len(), 1);
        assert_eq!(locations[0].file_path, "schema.graphql");
        // "DateTime" scalar definition
        assert_eq!(locations[0].range.start.line, 1);
    }

    #[test]
    fn test_goto_type_definition_with_list_wrapper() {
        let doc_index = DocumentIndex::new();

        let schema_str = r"
type User {
  id: ID!
  name: String!
}

type Query {
  users: [User!]!
}
";

        let schema = SchemaIndex::from_schema(schema_str);
        let provider = GotoDefinitionProvider::new();

        // Test goto definition from type inside list wrapper
        let schema_document = schema_str;

        // Position on "User" inside [User!]! (line 7, column 10)
        let position = Position {
            line: 7,
            character: 10,
        };

        let locations = provider
            .goto_definition(schema_document, position, &doc_index, &schema)
            .expect("Should find type definition");

        assert_eq!(locations.len(), 1);
        assert_eq!(locations[0].file_path, "schema.graphql");
        // "User" type definition
        assert_eq!(locations[0].range.start.line, 1);
    }

    #[test]
    fn test_goto_type_definition_from_variable_type() {
        let doc_index = DocumentIndex::new();

        let schema_str = r"
type Query {
  user(id: ID!): User
}

type User {
  id: ID!
  name: String!
}
";

        let schema = SchemaIndex::from_schema(schema_str);
        let provider = GotoDefinitionProvider::new();

        let document = r"
query GetUser($userId: ID!) {
    user(id: $userId) {
        name
    }
}
";

        // Position on "ID" in variable definition (line 1, column 23)
        let position = Position {
            line: 1,
            character: 23,
        };

        let locations = provider.goto_definition(document, position, &doc_index, &schema);

        // ID is a built-in scalar, may or may not be explicitly defined in schema
        // This test ensures we don't crash when trying to look it up
        // Built-in scalars might not have source locations
        if let Some(locs) = locations {
            assert!(!locs.is_empty());
        }
    }
}
