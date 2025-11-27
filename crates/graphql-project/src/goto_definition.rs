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
        file_path: &str,
    ) -> Option<Vec<DefinitionLocation>> {
        self.goto_definition_with_ast(
            source,
            position,
            document_index,
            schema_index,
            file_path,
            None,
        )
    }

    /// Get definition location with an optional cached AST
    #[must_use]
    #[allow(clippy::option_if_let_else)]
    pub fn goto_definition_with_ast(
        &self,
        source: &str,
        position: Position,
        document_index: &DocumentIndex,
        schema_index: &SchemaIndex,
        file_path: &str,
        cached_ast: Option<&apollo_parser::SyntaxTree>,
    ) -> Option<Vec<DefinitionLocation>> {
        tracing::info!(
            "GotoDefinitionProvider::goto_definition called with position: {:?}",
            position
        );

        let tree_holder;
        let tree = if let Some(ast) = cached_ast {
            ast
        } else {
            let parser = Parser::new(source);
            tree_holder = parser.parse();
            &tree_holder
        };

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

        let result = Self::resolve_definition(
            element_type,
            document_index,
            schema_index,
            source,
            file_path,
        );
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
                    // Get the operation start offset for variable lookups
                    let operation_start: usize = op.syntax().text_range().start().into();

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

                    // Check directives on the operation
                    if let Some(element) =
                        Self::check_directives(op.directives(), byte_offset, schema_index)
                    {
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
                            operation_start,
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

                    // Check directives on the fragment
                    if let Some(element) =
                        Self::check_directives(frag.directives(), byte_offset, schema_index)
                    {
                        return Some(element);
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
                            0,
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
        operation_start: usize,
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

                    let field_name = field
                        .name()
                        .map(|n| n.text().to_string())
                        .unwrap_or_default();

                    if let Some(arguments) = field.arguments() {
                        for arg in arguments.arguments() {
                            // Check if cursor is on the argument name
                            if let Some(arg_name) = arg.name() {
                                let range = arg_name.syntax().text_range();
                                let start: usize = range.start().into();
                                let end: usize = range.end().into();

                                if byte_offset >= start && byte_offset < end {
                                    return Some(ElementType::ArgumentReference {
                                        argument_name: arg_name.text().to_string(),
                                        field_name,
                                        parent_type,
                                    });
                                }
                            }

                            // Get the argument name for enum type resolution
                            let arg_name =
                                arg.name().map(|n| n.text().to_string()).unwrap_or_default();

                            if let Some(value) = arg.value() {
                                if let Some(element) = Self::check_value_for_element(
                                    &value,
                                    byte_offset,
                                    operation_start,
                                    &parent_type,
                                    &field_name,
                                    &arg_name,
                                    schema_index,
                                ) {
                                    return Some(element);
                                }
                            }
                        }
                    }

                    // Check directives on the field
                    if let Some(element) =
                        Self::check_directives(field.directives(), byte_offset, schema_index)
                    {
                        return Some(element);
                    }

                    if let Some(nested_selection_set) = field.selection_set() {
                        // Resolve the field type from the schema
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
                            operation_start,
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

                    // Check directives on the fragment spread
                    if let Some(element) =
                        Self::check_directives(spread.directives(), byte_offset, schema_index)
                    {
                        return Some(element);
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

                    // Check directives on the inline fragment
                    if let Some(element) =
                        Self::check_directives(inline_frag.directives(), byte_offset, schema_index)
                    {
                        return Some(element);
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
                            operation_start,
                        ) {
                            return Some(element);
                        }
                    }
                }
            }
        }

        None
    }

    /// Check if a value contains a variable or enum value at the byte offset
    fn check_value_for_element(
        value: &cst::Value,
        byte_offset: usize,
        operation_start: usize,
        parent_type: &str,
        field_name: &str,
        arg_name: &str,
        schema_index: &SchemaIndex,
    ) -> Option<ElementType> {
        match value {
            cst::Value::Variable(var) => {
                if let Some(name) = var.name() {
                    let range = name.syntax().text_range();
                    let start: usize = range.start().into();
                    let end: usize = range.end().into();

                    if byte_offset >= start && byte_offset < end {
                        return Some(ElementType::Variable {
                            var_name: name.text().to_string(),
                            operation_offset: operation_start,
                        });
                    }
                }
            }
            cst::Value::EnumValue(enum_val) => {
                if let Some(name) = enum_val.name() {
                    let range = name.syntax().text_range();
                    let start: usize = range.start().into();
                    let end: usize = range.end().into();

                    if byte_offset >= start && byte_offset < end {
                        let enum_type = Self::get_enum_type_for_argument(
                            parent_type,
                            field_name,
                            arg_name,
                            schema_index,
                        );
                        return Some(ElementType::EnumValue {
                            enum_value: name.text().to_string(),
                            enum_type,
                        });
                    }
                }
            }
            cst::Value::ListValue(list) => {
                for item in list.values() {
                    if let Some(element) = Self::check_value_for_element(
                        &item,
                        byte_offset,
                        operation_start,
                        parent_type,
                        field_name,
                        arg_name,
                        schema_index,
                    ) {
                        return Some(element);
                    }
                }
            }
            cst::Value::ObjectValue(obj) => {
                for field in obj.object_fields() {
                    if let Some(val) = field.value() {
                        if let Some(element) = Self::check_value_for_element(
                            &val,
                            byte_offset,
                            operation_start,
                            parent_type,
                            field_name,
                            arg_name,
                            schema_index,
                        ) {
                            return Some(element);
                        }
                    }
                }
            }
            _ => {}
        }

        None
    }

    /// Get the enum type for a field argument
    fn get_enum_type_for_argument(
        parent_type: &str,
        field_name: &str,
        arg_name: &str,
        schema_index: &SchemaIndex,
    ) -> String {
        schema_index
            .get_fields(parent_type)
            .and_then(|fields| {
                fields.iter().find(|f| f.name == field_name).and_then(|f| {
                    // Look for the argument in the field's arguments
                    f.arguments.iter().find(|a| a.name == arg_name).map(|a| {
                        // Extract base type name (strip [], !)
                        a.type_name
                            .trim_matches(|c| c == '[' || c == ']' || c == '!')
                            .to_string()
                    })
                })
            })
            .unwrap_or_default()
    }

    /// Check directives for goto definition
    fn check_directives(
        directives: Option<cst::Directives>,
        byte_offset: usize,
        schema_index: &SchemaIndex,
    ) -> Option<ElementType> {
        let directives = directives?;

        for directive in directives.directives() {
            // Check directive name
            if let Some(name) = directive.name() {
                let range = name.syntax().text_range();
                let start: usize = range.start().into();
                let end: usize = range.end().into();

                if byte_offset >= start && byte_offset < end {
                    return Some(ElementType::Directive {
                        directive_name: name.text().to_string(),
                    });
                }
            }

            // Check directive arguments
            if let Some(arguments) = directive.arguments() {
                let directive_name = directive
                    .name()
                    .map(|n| n.text().to_string())
                    .unwrap_or_default();

                for arg in arguments.arguments() {
                    // Check if cursor is on the argument name
                    if let Some(arg_name) = arg.name() {
                        let range = arg_name.syntax().text_range();
                        let start: usize = range.start().into();
                        let end: usize = range.end().into();

                        if byte_offset >= start && byte_offset < end {
                            return Some(ElementType::DirectiveArgument {
                                argument_name: arg_name.text().to_string(),
                                directive_name,
                            });
                        }
                    }

                    // Check values in directive arguments (variables, enums, etc.)
                    if let Some(value) = arg.value() {
                        if let Some(element) = Self::check_value_for_element(
                            &value,
                            byte_offset,
                            0,
                            "",
                            "",
                            "",
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

    /// Find variable definition in an operation
    fn find_variable_definition(
        source: &str,
        var_name: &str,
        operation_offset: usize,
        file_path: &str,
    ) -> Option<Vec<DefinitionLocation>> {
        // Parse from the operation start to find variable definitions
        let parser = Parser::new(&source[operation_offset..]);
        let tree = parser.parse();

        if tree.errors().count() > 0 {
            return None;
        }

        let doc = tree.document();

        for definition in doc.definitions() {
            if let cst::Definition::OperationDefinition(op) = definition {
                if let Some(variable_defs) = op.variable_definitions() {
                    for var_def in variable_defs.variable_definitions() {
                        if let Some(variable) = var_def.variable() {
                            if let Some(name) = variable.name() {
                                if name.text() == var_name {
                                    let range = name.syntax().text_range();
                                    let start: usize = range.start().into();
                                    let end: usize = range.end().into();

                                    // Convert back to absolute offsets
                                    let abs_start = operation_offset + start;
                                    let abs_end = operation_offset + end;

                                    // Convert to line/column positions
                                    let start_pos = Self::offset_to_position(source, abs_start)?;
                                    let end_pos = Self::offset_to_position(source, abs_end)?;

                                    return Some(vec![DefinitionLocation::new(
                                        file_path.to_string(),
                                        Range {
                                            start: start_pos,
                                            end: end_pos,
                                        },
                                    )]);
                                }
                            }
                        }
                    }
                }

                break;
            }
        }

        None
    }

    /// Convert a byte offset to a line/column position
    fn offset_to_position(source: &str, offset: usize) -> Option<Position> {
        let mut current_line = 0;
        let mut current_col = 0;
        let mut current_offset = 0;

        for ch in source.chars() {
            if current_offset == offset {
                return Some(Position {
                    line: current_line,
                    character: current_col,
                });
            }

            if ch == '\n' {
                current_line += 1;
                current_col = 0;
            } else {
                current_col += 1;
            }

            current_offset += ch.len_utf8();
        }

        if current_offset == offset {
            Some(Position {
                line: current_line,
                character: current_col,
            })
        } else {
            None
        }
    }

    /// Resolve the definition location based on the element type
    fn resolve_definition(
        element_type: ElementType,
        document_index: &DocumentIndex,
        schema_index: &SchemaIndex,
        source: &str,
        file_path: &str,
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
            ElementType::Variable {
                var_name,
                operation_offset,
            } => {
                // Find the variable definition in the operation
                Self::find_variable_definition(source, &var_name, operation_offset, file_path)
            }
            ElementType::ArgumentReference {
                argument_name,
                field_name,
                parent_type,
            } => {
                let arg_def = schema_index.find_argument_definition(
                    &parent_type,
                    &field_name,
                    &argument_name,
                )?;

                let range = Range {
                    start: Position {
                        line: arg_def.line,
                        character: arg_def.column,
                    },
                    end: Position {
                        line: arg_def.line,
                        character: arg_def.column + argument_name.len(),
                    },
                };

                Some(vec![DefinitionLocation::new(arg_def.file_path, range)])
            }
            ElementType::EnumValue {
                enum_value,
                enum_type,
            } => {
                if enum_type.is_empty() {
                    return None;
                }

                let enum_val_def =
                    schema_index.find_enum_value_definition(&enum_type, &enum_value)?;

                let range = Range {
                    start: Position {
                        line: enum_val_def.line,
                        character: enum_val_def.column,
                    },
                    end: Position {
                        line: enum_val_def.line,
                        character: enum_val_def.column + enum_value.len(),
                    },
                };

                Some(vec![DefinitionLocation::new(enum_val_def.file_path, range)])
            }
            ElementType::Directive { directive_name } => {
                let directive_def = schema_index.find_directive_definition(&directive_name)?;

                let range = Range {
                    start: Position {
                        line: directive_def.line,
                        character: directive_def.column,
                    },
                    end: Position {
                        line: directive_def.line,
                        character: directive_def.column + directive_name.len(),
                    },
                };

                Some(vec![DefinitionLocation::new(
                    directive_def.file_path,
                    range,
                )])
            }
            ElementType::DirectiveArgument {
                argument_name,
                directive_name,
            } => {
                let arg_def = schema_index
                    .find_directive_argument_definition(&directive_name, &argument_name)?;

                let range = Range {
                    start: Position {
                        line: arg_def.line,
                        character: arg_def.column,
                    },
                    end: Position {
                        line: arg_def.line,
                        character: arg_def.column + argument_name.len(),
                    },
                };

                Some(vec![DefinitionLocation::new(arg_def.file_path, range)])
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
        operation_offset: usize,
    },
    FieldReference {
        field_name: String,
        parent_type: String,
    },
    ArgumentReference {
        argument_name: String,
        field_name: String,
        parent_type: String,
    },
    EnumValue {
        enum_value: String,
        enum_type: String,
    },
    Directive {
        directive_name: String,
    },
    DirectiveArgument {
        argument_name: String,
        directive_name: String,
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
            .goto_definition(
                document,
                position,
                &doc_index,
                &schema,
                "file:///test.graphql",
            )
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
            .goto_definition(
                document,
                position,
                &doc_index,
                &schema,
                "file:///test.graphql",
            )
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

        let locations = provider.goto_definition(
            document,
            position,
            &doc_index,
            &schema,
            "file:///test.graphql",
        );

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

        let locations = provider.goto_definition(
            document,
            position,
            &doc_index,
            &schema,
            "file:///test.graphql",
        );

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

        let locations = provider.goto_definition(
            document,
            position,
            &doc_index,
            &schema,
            "file:///test.graphql",
        );

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
            .goto_definition(
                document,
                position,
                &doc_index,
                &schema,
                "file:///test.graphql",
            )
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
            .goto_definition(
                document,
                position,
                &doc_index,
                &schema,
                "file:///test.graphql",
            )
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
            .goto_definition(
                document,
                position,
                &doc_index,
                &schema,
                "file:///test.graphql",
            )
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
            .goto_definition(
                document,
                position,
                &doc_index,
                &schema,
                "file:///test.graphql",
            )
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
            .goto_definition(
                document,
                position,
                &doc_index,
                &schema,
                "file:///test.graphql",
            )
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
            .goto_definition(
                document,
                position,
                &doc_index,
                &schema,
                "file:///test.graphql",
            )
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
            .goto_definition(
                document,
                position,
                &doc_index,
                &schema,
                "file:///test.graphql",
            )
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
            .goto_definition(
                document,
                position,
                &doc_index,
                &schema,
                "file:///test.graphql",
            )
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
            .goto_definition(
                document,
                position,
                &doc_index,
                &schema,
                "file:///test.graphql",
            )
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
            .goto_definition(
                schema_document,
                position,
                &doc_index,
                &schema,
                "file:///schema.graphql",
            )
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
            .goto_definition(
                schema_document,
                position,
                &doc_index,
                &schema,
                "file:///schema.graphql",
            )
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
            .goto_definition(
                schema_document,
                position,
                &doc_index,
                &schema,
                "file:///schema.graphql",
            )
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
            .goto_definition(
                schema_document,
                position,
                &doc_index,
                &schema,
                "file:///schema.graphql",
            )
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
            .goto_definition(
                schema_document,
                position,
                &doc_index,
                &schema,
                "file:///schema.graphql",
            )
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
            .goto_definition(
                schema_document,
                position,
                &doc_index,
                &schema,
                "file:///schema.graphql",
            )
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

        let locations = provider.goto_definition(
            document,
            position,
            &doc_index,
            &schema,
            "file:///test.graphql",
        );

        // ID is a built-in scalar, may or may not be explicitly defined in schema
        // This test ensures we don't crash when trying to look it up
        // Built-in scalars might not have source locations
        if let Some(locs) = locations {
            assert!(!locs.is_empty());
        }
    }

    #[test]
    fn test_goto_variable_definition() {
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

        // Position on "$userId" in the field argument (line 2, column 14)
        let position = Position {
            line: 2,
            character: 14,
        };

        let locations = provider
            .goto_definition(
                document,
                position,
                &doc_index,
                &schema,
                "file:///test.graphql",
            )
            .expect("Should find variable definition");

        assert_eq!(locations.len(), 1);
        // Should point to the variable definition in line 1
        assert_eq!(locations[0].range.start.line, 1);
        assert_eq!(locations[0].range.start.character, 15); // Position of "$userId"
    }

    #[test]
    fn test_goto_argument_definition() {
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
    user(id: "123") {
        name
    }
}
"#;

        // Position on "id" argument name (line 2, column 9)
        let position = Position {
            line: 2,
            character: 9,
        };

        let locations = provider
            .goto_definition(
                document,
                position,
                &doc_index,
                &schema,
                "file:///test.graphql",
            )
            .expect("Should find argument definition");

        assert_eq!(locations.len(), 1);
        assert_eq!(locations[0].file_path, "schema.graphql");
        // "id" argument definition in Query.user
        assert_eq!(locations[0].range.start.line, 2);
    }

    #[test]
    fn test_goto_enum_value_definition() {
        let doc_index = DocumentIndex::new();
        let schema_str = r"
enum UserStatus {
  ACTIVE
  INACTIVE
  SUSPENDED
}

type User {
  id: ID!
  status: UserStatus
}

type Query {
  user(status: UserStatus): User
}
";
        let schema = SchemaIndex::from_schema(schema_str);
        let provider = GotoDefinitionProvider::new();

        let document = r"
query GetUser {
    user(status: ACTIVE) {
        id
    }
}
";

        // Position on "ACTIVE" enum value (line 2, column 17)
        let position = Position {
            line: 2,
            character: 17,
        };

        let locations = provider
            .goto_definition(
                document,
                position,
                &doc_index,
                &schema,
                "file:///test.graphql",
            )
            .expect("Should find enum value definition");

        assert_eq!(locations.len(), 1);
        assert_eq!(locations[0].file_path, "schema.graphql");
        // "ACTIVE" enum value
        assert_eq!(locations[0].range.start.line, 2);
    }

    #[test]
    fn test_goto_directive_definition() {
        let doc_index = DocumentIndex::new();
        let schema_str = r"
directive @auth(requires: String!) on QUERY | FIELD

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

        let document = r#"
query GetUser @auth(requires: "USER") {
    user {
        name
    }
}
"#;

        // Position on "@auth" directive name (line 1, column 15)
        let position = Position {
            line: 1,
            character: 15,
        };

        let locations = provider
            .goto_definition(
                document,
                position,
                &doc_index,
                &schema,
                "file:///test.graphql",
            )
            .expect("Should find directive definition");

        assert_eq!(locations.len(), 1);
        assert_eq!(locations[0].file_path, "schema.graphql");
        // "@auth" directive definition
        assert_eq!(locations[0].range.start.line, 1);
    }

    #[test]
    fn test_goto_directive_argument_definition() {
        let doc_index = DocumentIndex::new();
        let schema_str = r"
directive @auth(requires: String!) on QUERY | FIELD

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

        let document = r#"
query GetUser @auth(requires: "USER") {
    user {
        name
    }
}
"#;

        // Position on "requires" argument name (line 1, column 21)
        let position = Position {
            line: 1,
            character: 21,
        };

        let locations = provider
            .goto_definition(
                document,
                position,
                &doc_index,
                &schema,
                "file:///test.graphql",
            )
            .expect("Should find directive argument definition");

        assert_eq!(locations.len(), 1);
        assert_eq!(locations[0].file_path, "schema.graphql");
        // "requires" argument in @auth directive
        assert_eq!(locations[0].range.start.line, 1);
    }

    #[test]
    fn test_goto_directive_on_field() {
        let doc_index = DocumentIndex::new();
        let schema_str = r"
directive @deprecated(reason: String) on FIELD_DEFINITION | ENUM_VALUE

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

        let document = r#"
query GetUser {
    user {
        name @deprecated(reason: "Use fullName")
    }
}
"#;

        // Position on "@deprecated" directive (line 3, column 14)
        let position = Position {
            line: 3,
            character: 14,
        };

        let locations = provider
            .goto_definition(
                document,
                position,
                &doc_index,
                &schema,
                "file:///test.graphql",
            )
            .expect("Should find directive definition");

        assert_eq!(locations.len(), 1);
        assert_eq!(locations[0].file_path, "schema.graphql");
        assert_eq!(locations[0].range.start.line, 1);
    }

    #[test]
    fn test_goto_variable_in_nested_field() {
        let doc_index = DocumentIndex::new();
        let schema_str = r"
type Query {
  user(id: ID!): User
}

type User {
  id: ID!
  posts(limit: Int): [Post!]!
}

type Post {
  id: ID!
  title: String!
}
";
        let schema = SchemaIndex::from_schema(schema_str);
        let provider = GotoDefinitionProvider::new();

        let document = r"
query GetUser($userId: ID!, $limit: Int) {
    user(id: $userId) {
        posts(limit: $limit) {
            title
        }
    }
}
";

        // Position on "$limit" in the nested field argument (line 3, column 22)
        let position = Position {
            line: 3,
            character: 22,
        };

        let locations = provider
            .goto_definition(
                document,
                position,
                &doc_index,
                &schema,
                "file:///test.graphql",
            )
            .expect("Should find variable definition");

        assert_eq!(locations.len(), 1);
        // Should point to the variable definition in line 1
        assert_eq!(locations[0].range.start.line, 1);
        assert_eq!(locations[0].range.start.character, 29); // Position of "$limit"
    }

    #[test]
    fn test_goto_argument_in_nested_field() {
        let doc_index = DocumentIndex::new();
        let schema_str = r"
type Query {
  user(id: ID!): User
}

type User {
  id: ID!
  posts(limit: Int, offset: Int): [Post!]!
}

type Post {
  id: ID!
  title: String!
}
";
        let schema = SchemaIndex::from_schema(schema_str);
        let provider = GotoDefinitionProvider::new();

        let document = r#"
query GetUser {
    user(id: "123") {
        posts(limit: 10, offset: 0) {
            title
        }
    }
}
"#;

        // Position on "offset" argument name (line 3, column 26)
        let position = Position {
            line: 3,
            character: 26,
        };

        let locations = provider
            .goto_definition(
                document,
                position,
                &doc_index,
                &schema,
                "file:///test.graphql",
            )
            .expect("Should find argument definition");

        assert_eq!(locations.len(), 1);
        assert_eq!(locations[0].file_path, "schema.graphql");
        // "offset" argument in User.posts
        assert_eq!(locations[0].range.start.line, 7);
    }

    #[test]
    fn test_goto_enum_value_in_list() {
        let doc_index = DocumentIndex::new();
        let schema_str = r"
enum Role {
  ADMIN
  USER
  GUEST
}

type Query {
  users(roles: [Role!]): [User!]!
}

type User {
  id: ID!
  name: String!
}
";
        let schema = SchemaIndex::from_schema(schema_str);
        let provider = GotoDefinitionProvider::new();

        let document = r"
query GetUsers {
    users(roles: [ADMIN, USER]) {
        name
    }
}
";

        // Position on "USER" enum value in list (line 2, column 25)
        let position = Position {
            line: 2,
            character: 25,
        };

        let locations = provider
            .goto_definition(
                document,
                position,
                &doc_index,
                &schema,
                "file:///test.graphql",
            )
            .expect("Should find enum value definition");

        assert_eq!(locations.len(), 1);
        assert_eq!(locations[0].file_path, "schema.graphql");
        // "USER" enum value
        assert_eq!(locations[0].range.start.line, 3);
    }
}
