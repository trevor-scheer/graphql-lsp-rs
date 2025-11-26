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
        let parser = Parser::new(source);
        let tree = parser.parse();

        if tree.errors().count() > 0 {
            return None;
        }

        let doc = tree.document();
        let byte_offset = Self::position_to_offset(source, position)?;

        let element_type = Self::find_element_at_position(&doc, byte_offset)?;

        Self::resolve_definition(element_type, document_index, schema_index)
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
    fn find_element_at_position(doc: &cst::Document, byte_offset: usize) -> Option<ElementType> {
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
                        if let Some(element) =
                            Self::check_selection_set(&selection_set, byte_offset)
                        {
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
                        if let Some(element) =
                            Self::check_selection_set(&selection_set, byte_offset)
                        {
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

    /// Check if the byte offset is within a selection set
    fn check_selection_set(
        selection_set: &cst::SelectionSet,
        byte_offset: usize,
    ) -> Option<ElementType> {
        for selection in selection_set.selections() {
            match selection {
                cst::Selection::Field(field) => {
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
                        if let Some(element) =
                            Self::check_selection_set(&nested_selection_set, byte_offset)
                        {
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
                        if let Some(element) =
                            Self::check_selection_set(&nested_selection_set, byte_offset)
                        {
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
        _schema_index: &SchemaIndex,
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
            ElementType::TypeReference { .. } => {
                // TODO: Implement type definition lookup in schema
                // This would require tracking source locations in the schema
                None
            }
            ElementType::Variable { .. } => {
                // TODO: Implement variable definition lookup
                // This would require finding the operation's variable definition
                None
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
    FragmentSpread { fragment_name: String },
    FragmentDefinition { fragment_name: String },
    OperationDefinition { operation_name: String },
    TypeReference { type_name: String },
    Variable { var_name: String },
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

        let schema = SchemaIndex::new();
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

        let schema = SchemaIndex::new();
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
}
