#![allow(clippy::too_many_lines)]

use crate::{DocumentIndex, Position, SchemaIndex};
use apollo_parser::{
    cst::{self, CstNode},
    Parser,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompletionItemKind {
    Field,
    Type,
    Fragment,
    Operation,
    Directive,
    EnumValue,
    Argument,
    Variable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionItem {
    pub label: String,
    pub kind: CompletionItemKind,
    pub detail: Option<String>,
    pub documentation: Option<String>,
    pub deprecated: bool,
    pub insert_text: Option<String>,
}

impl CompletionItem {
    #[must_use]
    pub const fn new(
        label: String,
        kind: CompletionItemKind,
        detail: Option<String>,
        documentation: Option<String>,
        deprecated: bool,
        insert_text: Option<String>,
    ) -> Self {
        Self {
            label,
            kind,
            detail,
            documentation,
            deprecated,
            insert_text,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)]
enum CompletionContext {
    FieldSelection {
        parent_type: String,
    },
    FragmentSpread,
    TypeCondition,
    Directive {
        location: DirectiveLocation,
    },
    Argument {
        parent_type: String,
        field_name: String,
    },
    EnumValue {
        enum_type: String,
    },
    VariableDefinition,
    FieldType,
}

#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)]
enum DirectiveLocation {
    Query,
    Field,
    FragmentDefinition,
    FragmentSpread,
    InlineFragment,
}

pub struct CompletionProvider;

impl Default for CompletionProvider {
    fn default() -> Self {
        Self
    }
}

impl CompletionProvider {
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    #[must_use]
    pub fn complete(
        &self,
        source: &str,
        position: Position,
        document_index: &DocumentIndex,
        schema_index: &SchemaIndex,
    ) -> Option<Vec<CompletionItem>> {
        let parser = Parser::new(source);
        let tree = parser.parse();

        let doc = tree.document();
        let byte_offset = Self::position_to_offset(source, position)?;

        let context = Self::determine_completion_context(&doc, byte_offset, source, schema_index)?;

        Some(Self::generate_completions(
            context,
            document_index,
            schema_index,
        ))
    }

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

    fn determine_completion_context(
        doc: &cst::Document,
        byte_offset: usize,
        source: &str,
        schema_index: &SchemaIndex,
    ) -> Option<CompletionContext> {
        for def in doc.definitions() {
            if let Some(context) =
                Self::check_definition_for_context(&def, byte_offset, source, schema_index)
            {
                return Some(context);
            }
        }
        None
    }

    fn check_definition_for_context(
        def: &cst::Definition,
        byte_offset: usize,
        source: &str,
        schema_index: &SchemaIndex,
    ) -> Option<CompletionContext> {
        match def {
            cst::Definition::OperationDefinition(op) => {
                Self::check_operation_for_context(op, byte_offset, source, schema_index)
            }
            cst::Definition::FragmentDefinition(frag) => {
                Self::check_fragment_for_context(frag, byte_offset, source, schema_index)
            }
            _ => None,
        }
    }

    fn check_operation_for_context(
        op: &cst::OperationDefinition,
        byte_offset: usize,
        source: &str,
        schema_index: &SchemaIndex,
    ) -> Option<CompletionContext> {
        let op_range = op.syntax().text_range();
        if !Self::range_contains(op_range.start().into(), op_range.end().into(), byte_offset) {
            return None;
        }

        if let Some(selection_set) = op.selection_set() {
            let op_type = op.operation_type().map_or("Query", |op_type_node| {
                if op_type_node.query_token().is_some() {
                    "Query"
                } else if op_type_node.mutation_token().is_some() {
                    "Mutation"
                } else if op_type_node.subscription_token().is_some() {
                    "Subscription"
                } else {
                    "Query"
                }
            });

            if let Some(context) = Self::check_selection_set_for_context(
                &selection_set,
                byte_offset,
                source,
                schema_index,
                op_type,
            ) {
                return Some(context);
            }
        }

        if let Some(directives) = op.directives() {
            if Self::is_in_directives(&directives, byte_offset) {
                return Some(CompletionContext::Directive {
                    location: DirectiveLocation::Query,
                });
            }
        }

        None
    }

    fn check_fragment_for_context(
        frag: &cst::FragmentDefinition,
        byte_offset: usize,
        source: &str,
        schema_index: &SchemaIndex,
    ) -> Option<CompletionContext> {
        let frag_range = frag.syntax().text_range();
        if !Self::range_contains(
            frag_range.start().into(),
            frag_range.end().into(),
            byte_offset,
        ) {
            return None;
        }

        if let Some(type_cond) = frag.type_condition() {
            if let Some(named_type) = type_cond.named_type() {
                let type_name = named_type.name()?.text();

                if let Some(selection_set) = frag.selection_set() {
                    if let Some(context) = Self::check_selection_set_for_context(
                        &selection_set,
                        byte_offset,
                        source,
                        schema_index,
                        &type_name,
                    ) {
                        return Some(context);
                    }
                }
            }
        }

        if let Some(directives) = frag.directives() {
            if Self::is_in_directives(&directives, byte_offset) {
                return Some(CompletionContext::Directive {
                    location: DirectiveLocation::FragmentDefinition,
                });
            }
        }

        None
    }

    fn check_selection_set_for_context(
        selection_set: &cst::SelectionSet,
        byte_offset: usize,
        source: &str,
        schema_index: &SchemaIndex,
        parent_type: &str,
    ) -> Option<CompletionContext> {
        let ss_range = selection_set.syntax().text_range();
        if !Self::range_contains(ss_range.start().into(), ss_range.end().into(), byte_offset) {
            return None;
        }

        for selection in selection_set.selections() {
            match selection {
                cst::Selection::Field(field) => {
                    if let Some(context) = Self::check_field_for_context(
                        &field,
                        byte_offset,
                        source,
                        schema_index,
                        parent_type,
                    ) {
                        return Some(context);
                    }
                }
                cst::Selection::FragmentSpread(spread) => {
                    let spread_range = spread.syntax().text_range();
                    if Self::range_contains(
                        spread_range.start().into(),
                        spread_range.end().into(),
                        byte_offset,
                    ) {
                        return Some(CompletionContext::FragmentSpread);
                    }
                }
                cst::Selection::InlineFragment(inline_frag) => {
                    if let Some(context) = Self::check_inline_fragment_for_context(
                        &inline_frag,
                        byte_offset,
                        source,
                        schema_index,
                        parent_type,
                    ) {
                        return Some(context);
                    }
                }
            }
        }

        Some(CompletionContext::FieldSelection {
            parent_type: parent_type.to_string(),
        })
    }

    fn check_field_for_context(
        field: &cst::Field,
        byte_offset: usize,
        source: &str,
        schema_index: &SchemaIndex,
        parent_type: &str,
    ) -> Option<CompletionContext> {
        let field_range = field.syntax().text_range();
        if !Self::range_contains(
            field_range.start().into(),
            field_range.end().into(),
            byte_offset,
        ) {
            return None;
        }

        let field_name = field.name()?.text();

        if let Some(arguments) = field.arguments() {
            if Self::is_in_arguments(&arguments, byte_offset) {
                return Some(CompletionContext::Argument {
                    parent_type: parent_type.to_string(),
                    field_name: field_name.to_string(),
                });
            }
        }

        if let Some(directives) = field.directives() {
            if Self::is_in_directives(&directives, byte_offset) {
                return Some(CompletionContext::Directive {
                    location: DirectiveLocation::Field,
                });
            }
        }

        if let Some(selection_set) = field.selection_set() {
            let field_type = schema_index
                .get_fields(parent_type)?
                .into_iter()
                .find(|f| f.name == field_name)?
                .type_name;

            let base_type = Self::extract_base_type(&field_type);

            if let Some(context) = Self::check_selection_set_for_context(
                &selection_set,
                byte_offset,
                source,
                schema_index,
                &base_type,
            ) {
                return Some(context);
            }
        }

        None
    }

    fn check_inline_fragment_for_context(
        inline_frag: &cst::InlineFragment,
        byte_offset: usize,
        source: &str,
        schema_index: &SchemaIndex,
        parent_type: &str,
    ) -> Option<CompletionContext> {
        let frag_range = inline_frag.syntax().text_range();
        if !Self::range_contains(
            frag_range.start().into(),
            frag_range.end().into(),
            byte_offset,
        ) {
            return None;
        }

        let type_name = if let Some(type_cond) = inline_frag.type_condition() {
            type_cond.named_type()?.name()?.text().to_string()
        } else {
            parent_type.to_string()
        };

        if let Some(directives) = inline_frag.directives() {
            if Self::is_in_directives(&directives, byte_offset) {
                return Some(CompletionContext::Directive {
                    location: DirectiveLocation::InlineFragment,
                });
            }
        }

        if let Some(selection_set) = inline_frag.selection_set() {
            if let Some(context) = Self::check_selection_set_for_context(
                &selection_set,
                byte_offset,
                source,
                schema_index,
                &type_name,
            ) {
                return Some(context);
            }
        }

        None
    }

    fn is_in_directives(directives: &cst::Directives, byte_offset: usize) -> bool {
        let dir_range = directives.syntax().text_range();
        Self::range_contains(
            dir_range.start().into(),
            dir_range.end().into(),
            byte_offset,
        )
    }

    fn is_in_arguments(arguments: &cst::Arguments, byte_offset: usize) -> bool {
        let args_range = arguments.syntax().text_range();
        Self::range_contains(
            args_range.start().into(),
            args_range.end().into(),
            byte_offset,
        )
    }

    const fn range_contains(start: usize, end: usize, offset: usize) -> bool {
        offset >= start && offset <= end
    }

    fn extract_base_type(type_str: &str) -> String {
        type_str
            .trim_end_matches('!')
            .trim_start_matches('[')
            .trim_end_matches(']')
            .trim_end_matches('!')
            .to_string()
    }

    fn generate_completions(
        context: CompletionContext,
        document_index: &DocumentIndex,
        schema_index: &SchemaIndex,
    ) -> Vec<CompletionItem> {
        match context {
            CompletionContext::FieldSelection { parent_type } => {
                Self::complete_fields(&parent_type, schema_index)
            }
            CompletionContext::FragmentSpread => Self::complete_fragments(document_index),
            CompletionContext::TypeCondition | CompletionContext::FieldType => {
                Self::complete_types(schema_index)
            }
            CompletionContext::Directive { .. } => Self::complete_directives(schema_index),
            CompletionContext::Argument {
                parent_type,
                field_name,
            } => Self::complete_arguments(&parent_type, &field_name, schema_index),
            CompletionContext::EnumValue { enum_type } => {
                Self::complete_enum_values(&enum_type, schema_index)
            }
            CompletionContext::VariableDefinition => Vec::new(),
        }
    }

    fn complete_fields(parent_type: &str, schema_index: &SchemaIndex) -> Vec<CompletionItem> {
        let mut items = Vec::new();

        let Some(fields) = schema_index.get_fields(parent_type) else {
            return items;
        };

        for field in fields {
            let detail = Some(field.type_name.clone());
            let documentation = field.description.clone();
            let deprecated = field.deprecated.is_some();

            items.push(CompletionItem::new(
                field.name.clone(),
                CompletionItemKind::Field,
                detail,
                documentation,
                deprecated,
                None,
            ));
        }

        items
    }

    fn complete_fragments(document_index: &DocumentIndex) -> Vec<CompletionItem> {
        let mut items = Vec::new();

        for (fragment_name, fragment_infos) in &document_index.fragments {
            if let Some(first) = fragment_infos.first() {
                let detail = Some(format!("on {}", first.type_condition));

                items.push(CompletionItem::new(
                    fragment_name.clone(),
                    CompletionItemKind::Fragment,
                    detail,
                    None,
                    false,
                    None,
                ));
            }
        }

        items
    }

    fn complete_types(schema_index: &SchemaIndex) -> Vec<CompletionItem> {
        let mut items = Vec::new();

        for type_info in schema_index.all_types() {
            let detail = Some(format!("{:?}", type_info.kind));
            let documentation = type_info.description.clone();

            items.push(CompletionItem::new(
                type_info.name.clone(),
                CompletionItemKind::Type,
                detail,
                documentation,
                false,
                None,
            ));
        }

        items
    }

    fn complete_directives(schema_index: &SchemaIndex) -> Vec<CompletionItem> {
        let mut items = Vec::new();

        for directive in schema_index.all_directives() {
            let documentation = directive.description.clone();

            items.push(CompletionItem::new(
                directive.name.clone(),
                CompletionItemKind::Directive,
                None,
                documentation,
                false,
                Some(format!("@{}", directive.name)),
            ));
        }

        items
    }

    fn complete_arguments(
        parent_type: &str,
        field_name: &str,
        schema_index: &SchemaIndex,
    ) -> Vec<CompletionItem> {
        let mut items = Vec::new();

        let Some(fields) = schema_index.get_fields(parent_type) else {
            return items;
        };

        for field in fields {
            if field.name == field_name {
                for arg in &field.arguments {
                    let detail = Some(arg.type_name.clone());
                    let documentation = arg.description.clone();

                    items.push(CompletionItem::new(
                        arg.name.clone(),
                        CompletionItemKind::Argument,
                        detail,
                        documentation,
                        false,
                        None,
                    ));
                }
                break;
            }
        }

        items
    }

    fn complete_enum_values(enum_type: &str, schema_index: &SchemaIndex) -> Vec<CompletionItem> {
        let mut items = Vec::new();

        for enum_value in schema_index.get_enum_values(enum_type) {
            let documentation = enum_value.description.clone();
            let deprecated = enum_value.deprecated.is_some();

            items.push(CompletionItem::new(
                enum_value.name.clone(),
                CompletionItemKind::EnumValue,
                None,
                documentation,
                deprecated,
                None,
            ));
        }

        items
    }
}
