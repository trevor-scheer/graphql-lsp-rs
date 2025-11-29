#![allow(clippy::too_many_lines)]

use crate::{DocumentIndex, Position, Range, SchemaIndex};
use apollo_parser::{
    cst::{self, CstNode},
    Parser, SyntaxTree,
};
use std::collections::HashMap;

/// Location information for find references
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReferenceLocation {
    /// File path where the reference is located
    pub file_path: String,
    /// Range of the reference
    pub range: Range,
}

impl ReferenceLocation {
    #[must_use]
    pub const fn new(file_path: String, range: Range) -> Self {
        Self { file_path, range }
    }
}

/// Find references provider
pub struct FindReferencesProvider;

impl FindReferencesProvider {
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Find all references to the element at a position in a GraphQL document
    ///
    /// Returns all locations where the element at the given position is referenced.
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn find_references(
        &self,
        source: &str,
        position: Position,
        document_index: &DocumentIndex,
        schema_index: &SchemaIndex,
        all_documents: &[(String, String)],
        include_declaration: bool,
    ) -> Option<Vec<ReferenceLocation>> {
        self.find_references_with_asts(
            source,
            position,
            document_index,
            schema_index,
            all_documents,
            include_declaration,
            None,
            None,
        )
    }

    /// Find all references with optional pre-parsed ASTs for optimization
    ///
    /// This method accepts optional cached ASTs to avoid re-parsing:
    /// - `source_ast`: Pre-parsed AST of the source document
    /// - `document_asts`: Pre-parsed ASTs of all workspace documents (`file_path` -> AST)
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    #[allow(clippy::option_if_let_else)]
    pub fn find_references_with_asts(
        &self,
        source: &str,
        position: Position,
        document_index: &DocumentIndex,
        schema_index: &SchemaIndex,
        all_documents: &[(String, String)],
        include_declaration: bool,
        source_ast: Option<&SyntaxTree>,
        document_asts: Option<&HashMap<String, SyntaxTree>>,
    ) -> Option<Vec<ReferenceLocation>> {
        tracing::info!(
            line = position.line,
            character = position.character,
            include_declaration,
            "find_references called"
        );

        let tree_holder;
        let tree = if let Some(ast) = source_ast {
            ast
        } else {
            let parser = Parser::new(source);
            tree_holder = parser.parse();
            &tree_holder
        };

        let error_count = tree.errors().count();
        if error_count > 0 {
            tracing::debug!(error_count, "Parser errors, returning None");
            return None;
        }

        let doc = tree.document();
        let byte_offset = Self::position_to_offset(source, position)?;
        let element_type = Self::find_element_at_position(&doc, byte_offset, source, schema_index)?;

        tracing::debug!(element_type = ?element_type, "Finding references for element");

        let references = Self::find_all_references_with_asts(
            &element_type,
            document_index,
            schema_index,
            all_documents,
            include_declaration,
            document_asts,
        )?;

        tracing::info!(count = references.len(), "Found references");
        Some(references)
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

    fn offset_to_position(source: &str, offset: usize) -> (usize, usize) {
        let mut line = 0;
        let mut column = 0;
        let mut current_offset = 0;

        for ch in source.chars() {
            if current_offset >= offset {
                break;
            }

            if ch == '\n' {
                line += 1;
                column = 0;
            } else {
                column += 1;
            }

            current_offset += ch.len_utf8();
        }

        (line, column)
    }

    fn find_element_at_position(
        doc: &cst::Document,
        byte_offset: usize,
        _source: &str,
        _schema_index: &SchemaIndex,
    ) -> Option<ElementType> {
        for definition in doc.definitions() {
            match definition {
                cst::Definition::FragmentDefinition(frag) => {
                    // Check if cursor is on the fragment definition name
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

                    // Check selection set for fragment spreads
                    if let Some(selection_set) = frag.selection_set() {
                        if let Some(element) =
                            Self::check_selection_set_for_spreads(&selection_set, byte_offset)
                        {
                            return Some(element);
                        }
                    }
                }
                cst::Definition::OperationDefinition(op) => {
                    // Check selection set for fragment spreads
                    if let Some(selection_set) = op.selection_set() {
                        if let Some(element) =
                            Self::check_selection_set_for_spreads(&selection_set, byte_offset)
                        {
                            return Some(element);
                        }
                    }
                }
                cst::Definition::ObjectTypeDefinition(obj) => {
                    if let Some(name) = obj.name() {
                        let range = name.syntax().text_range();
                        let start: usize = range.start().into();
                        let end: usize = range.end().into();

                        if byte_offset >= start && byte_offset < end {
                            return Some(ElementType::TypeDefinition {
                                type_name: name.text().to_string(),
                            });
                        }
                    }
                }
                cst::Definition::InterfaceTypeDefinition(iface) => {
                    if let Some(name) = iface.name() {
                        let range = name.syntax().text_range();
                        let start: usize = range.start().into();
                        let end: usize = range.end().into();

                        if byte_offset >= start && byte_offset < end {
                            return Some(ElementType::TypeDefinition {
                                type_name: name.text().to_string(),
                            });
                        }
                    }
                }
                cst::Definition::UnionTypeDefinition(union) => {
                    if let Some(name) = union.name() {
                        let range = name.syntax().text_range();
                        let start: usize = range.start().into();
                        let end: usize = range.end().into();

                        if byte_offset >= start && byte_offset < end {
                            return Some(ElementType::TypeDefinition {
                                type_name: name.text().to_string(),
                            });
                        }
                    }
                }
                cst::Definition::ScalarTypeDefinition(scalar) => {
                    if let Some(name) = scalar.name() {
                        let range = name.syntax().text_range();
                        let start: usize = range.start().into();
                        let end: usize = range.end().into();

                        if byte_offset >= start && byte_offset < end {
                            return Some(ElementType::TypeDefinition {
                                type_name: name.text().to_string(),
                            });
                        }
                    }
                }
                cst::Definition::EnumTypeDefinition(enum_def) => {
                    if let Some(name) = enum_def.name() {
                        let range = name.syntax().text_range();
                        let start: usize = range.start().into();
                        let end: usize = range.end().into();

                        if byte_offset >= start && byte_offset < end {
                            return Some(ElementType::TypeDefinition {
                                type_name: name.text().to_string(),
                            });
                        }
                    }
                }
                cst::Definition::InputObjectTypeDefinition(input) => {
                    if let Some(name) = input.name() {
                        let range = name.syntax().text_range();
                        let start: usize = range.start().into();
                        let end: usize = range.end().into();

                        if byte_offset >= start && byte_offset < end {
                            return Some(ElementType::TypeDefinition {
                                type_name: name.text().to_string(),
                            });
                        }
                    }
                }
                _ => {}
            }
        }

        None
    }

    fn check_selection_set_for_spreads(
        selection_set: &cst::SelectionSet,
        byte_offset: usize,
    ) -> Option<ElementType> {
        for selection in selection_set.selections() {
            match selection {
                cst::Selection::FragmentSpread(spread) => {
                    if let Some(frag_name) = spread.fragment_name().and_then(|f| f.name()) {
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
                cst::Selection::Field(field) => {
                    if let Some(nested_selection_set) = field.selection_set() {
                        if let Some(element) = Self::check_selection_set_for_spreads(
                            &nested_selection_set,
                            byte_offset,
                        ) {
                            return Some(element);
                        }
                    }
                }
                cst::Selection::InlineFragment(inline_frag) => {
                    if let Some(selection_set) = inline_frag.selection_set() {
                        if let Some(element) =
                            Self::check_selection_set_for_spreads(&selection_set, byte_offset)
                        {
                            return Some(element);
                        }
                    }
                }
            }
        }

        None
    }

    fn find_all_references_with_asts(
        element_type: &ElementType,
        document_index: &DocumentIndex,
        _schema_index: &SchemaIndex,
        all_documents: &[(String, String)],
        include_declaration: bool,
        document_asts: Option<&HashMap<String, SyntaxTree>>,
    ) -> Option<Vec<ReferenceLocation>> {
        match element_type {
            ElementType::FragmentDefinition { fragment_name } => {
                // Find all fragment spreads that use this fragment
                let mut references = Self::find_fragment_spread_references_with_asts(
                    fragment_name,
                    all_documents,
                    document_asts,
                )?;

                // Add fragment definitions if requested
                if include_declaration {
                    if let Some(fragments) = document_index.get_fragments_by_name(fragment_name) {
                        for frag in fragments {
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
                            references.push(ReferenceLocation::new(frag.file_path.clone(), range));
                        }
                    }
                }

                Some(references)
            }
            ElementType::FragmentSpread { fragment_name } => {
                // When on a spread, find all spreads (same as definition)
                Self::find_fragment_spread_references_with_asts(
                    fragment_name,
                    all_documents,
                    document_asts,
                )
            }
            ElementType::TypeDefinition { type_name } => {
                // Find all type references
                Self::find_type_references_with_asts(
                    type_name,
                    all_documents,
                    include_declaration,
                    document_asts,
                )
            }
        }
    }

    #[allow(clippy::option_if_let_else)]
    fn find_fragment_spread_references_with_asts(
        fragment_name: &str,
        all_documents: &[(String, String)],
        document_asts: Option<&HashMap<String, SyntaxTree>>,
    ) -> Option<Vec<ReferenceLocation>> {
        let mut references = Vec::new();

        for (file_path, source) in all_documents {
            // Try to use cached AST first, otherwise parse
            let tree_holder;
            let tree = if let Some(asts) = document_asts {
                if let Some(cached) = asts.get(file_path) {
                    cached
                } else {
                    // Parse if not in cache
                    let parser = Parser::new(source);
                    tree_holder = parser.parse();
                    &tree_holder
                }
            } else {
                // No AST cache provided, parse on demand
                let parser = Parser::new(source);
                tree_holder = parser.parse();
                &tree_holder
            };

            if tree.errors().count() > 0 {
                continue;
            }

            let doc = tree.document();
            Self::collect_fragment_spreads(&doc, fragment_name, file_path, source, &mut references);
        }

        if references.is_empty() {
            None
        } else {
            Some(references)
        }
    }

    fn collect_fragment_spreads(
        doc: &cst::Document,
        target_fragment: &str,
        file_path: &str,
        source: &str,
        references: &mut Vec<ReferenceLocation>,
    ) {
        for definition in doc.definitions() {
            match definition {
                cst::Definition::OperationDefinition(op) => {
                    if let Some(selection_set) = op.selection_set() {
                        Self::collect_fragment_spreads_from_selection_set(
                            &selection_set,
                            target_fragment,
                            file_path,
                            source,
                            references,
                        );
                    }
                }
                cst::Definition::FragmentDefinition(frag) => {
                    if let Some(selection_set) = frag.selection_set() {
                        Self::collect_fragment_spreads_from_selection_set(
                            &selection_set,
                            target_fragment,
                            file_path,
                            source,
                            references,
                        );
                    }
                }
                _ => {}
            }
        }
    }

    fn collect_fragment_spreads_from_selection_set(
        selection_set: &cst::SelectionSet,
        target_fragment: &str,
        file_path: &str,
        source: &str,
        references: &mut Vec<ReferenceLocation>,
    ) {
        for selection in selection_set.selections() {
            match selection {
                cst::Selection::Field(field) => {
                    if let Some(nested_selection_set) = field.selection_set() {
                        Self::collect_fragment_spreads_from_selection_set(
                            &nested_selection_set,
                            target_fragment,
                            file_path,
                            source,
                            references,
                        );
                    }
                }
                cst::Selection::FragmentSpread(spread) => {
                    if let Some(frag_name) = spread.fragment_name().and_then(|f| f.name()) {
                        if frag_name.text() == target_fragment {
                            let range = frag_name.syntax().text_range();
                            let (line, column) =
                                Self::offset_to_position(source, range.start().into());
                            let range = Range {
                                start: Position {
                                    line,
                                    character: column,
                                },
                                end: Position {
                                    line,
                                    character: column + target_fragment.len(),
                                },
                            };
                            references.push(ReferenceLocation::new(file_path.to_string(), range));
                        }
                    }
                }
                cst::Selection::InlineFragment(inline_frag) => {
                    if let Some(selection_set) = inline_frag.selection_set() {
                        Self::collect_fragment_spreads_from_selection_set(
                            &selection_set,
                            target_fragment,
                            file_path,
                            source,
                            references,
                        );
                    }
                }
            }
        }
    }

    /// Find all type references in all documents with optional pre-parsed ASTs
    #[allow(clippy::option_if_let_else)]
    fn find_type_references_with_asts(
        type_name: &str,
        all_documents: &[(String, String)],
        include_declaration: bool,
        document_asts: Option<&HashMap<String, SyntaxTree>>,
    ) -> Option<Vec<ReferenceLocation>> {
        let mut references = Vec::new();

        for (file_path, source) in all_documents {
            // Try to use cached AST first
            let tree_holder;
            let tree = if let Some(asts) = document_asts {
                if let Some(cached) = asts.get(file_path) {
                    cached
                } else {
                    let parser = Parser::new(source);
                    tree_holder = parser.parse();
                    &tree_holder
                }
            } else {
                let parser = Parser::new(source);
                tree_holder = parser.parse();
                &tree_holder
            };

            if tree.errors().count() > 0 {
                continue;
            }

            let doc = tree.document();
            Self::collect_type_references(
                &doc,
                type_name,
                file_path,
                source,
                &mut references,
                include_declaration,
            );
        }

        if references.is_empty() {
            None
        } else {
            Some(references)
        }
    }

    /// Collect all type references in a document
    fn collect_type_references(
        doc: &cst::Document,
        target_type: &str,
        file_path: &str,
        source: &str,
        references: &mut Vec<ReferenceLocation>,
        include_declaration: bool,
    ) {
        for definition in doc.definitions() {
            match definition {
                cst::Definition::ObjectTypeDefinition(obj) => {
                    // Check if this is the type definition itself
                    if let Some(name) = obj.name() {
                        if name.text() == target_type && include_declaration {
                            let range = name.syntax().text_range();
                            let (line, column) =
                                Self::offset_to_position(source, range.start().into());
                            let range = Range {
                                start: Position {
                                    line,
                                    character: column,
                                },
                                end: Position {
                                    line,
                                    character: column + target_type.len(),
                                },
                            };
                            references.push(ReferenceLocation::new(file_path.to_string(), range));
                        }
                    }
                    Self::collect_type_references_from_object_type(
                        &obj,
                        target_type,
                        file_path,
                        source,
                        references,
                    );
                }
                cst::Definition::InterfaceTypeDefinition(iface) => {
                    if let Some(name) = iface.name() {
                        if name.text() == target_type && include_declaration {
                            let range = name.syntax().text_range();
                            let (line, column) =
                                Self::offset_to_position(source, range.start().into());
                            let range = Range {
                                start: Position {
                                    line,
                                    character: column,
                                },
                                end: Position {
                                    line,
                                    character: column + target_type.len(),
                                },
                            };
                            references.push(ReferenceLocation::new(file_path.to_string(), range));
                        }
                    }
                    Self::collect_type_references_from_interface_type(
                        &iface,
                        target_type,
                        file_path,
                        source,
                        references,
                    );
                }
                cst::Definition::UnionTypeDefinition(union) => {
                    if let Some(name) = union.name() {
                        if name.text() == target_type && include_declaration {
                            let range = name.syntax().text_range();
                            let (line, column) =
                                Self::offset_to_position(source, range.start().into());
                            let range = Range {
                                start: Position {
                                    line,
                                    character: column,
                                },
                                end: Position {
                                    line,
                                    character: column + target_type.len(),
                                },
                            };
                            references.push(ReferenceLocation::new(file_path.to_string(), range));
                        }
                    }
                    Self::collect_type_references_from_union_type(
                        &union,
                        target_type,
                        file_path,
                        source,
                        references,
                    );
                }
                cst::Definition::InputObjectTypeDefinition(input) => {
                    if let Some(name) = input.name() {
                        if name.text() == target_type && include_declaration {
                            let range = name.syntax().text_range();
                            let (line, column) =
                                Self::offset_to_position(source, range.start().into());
                            let range = Range {
                                start: Position {
                                    line,
                                    character: column,
                                },
                                end: Position {
                                    line,
                                    character: column + target_type.len(),
                                },
                            };
                            references.push(ReferenceLocation::new(file_path.to_string(), range));
                        }
                    }
                    Self::collect_type_references_from_input_object_type(
                        &input,
                        target_type,
                        file_path,
                        source,
                        references,
                    );
                }
                cst::Definition::ScalarTypeDefinition(scalar) => {
                    if let Some(name) = scalar.name() {
                        if name.text() == target_type && include_declaration {
                            let range = name.syntax().text_range();
                            let (line, column) =
                                Self::offset_to_position(source, range.start().into());
                            let range = Range {
                                start: Position {
                                    line,
                                    character: column,
                                },
                                end: Position {
                                    line,
                                    character: column + target_type.len(),
                                },
                            };
                            references.push(ReferenceLocation::new(file_path.to_string(), range));
                        }
                    }
                    // Scalars don't have fields, so no need to check for references within
                }
                cst::Definition::EnumTypeDefinition(enum_def) => {
                    if let Some(name) = enum_def.name() {
                        if name.text() == target_type && include_declaration {
                            let range = name.syntax().text_range();
                            let (line, column) =
                                Self::offset_to_position(source, range.start().into());
                            let range = Range {
                                start: Position {
                                    line,
                                    character: column,
                                },
                                end: Position {
                                    line,
                                    character: column + target_type.len(),
                                },
                            };
                            references.push(ReferenceLocation::new(file_path.to_string(), range));
                        }
                    }
                    // Enums don't have field types, so no need to check for references within
                }
                _ => {}
            }
        }
    }

    /// Collect type references from an object type definition
    fn collect_type_references_from_object_type(
        obj: &cst::ObjectTypeDefinition,
        target_type: &str,
        file_path: &str,
        source: &str,
        references: &mut Vec<ReferenceLocation>,
    ) {
        // Check field types
        if let Some(fields) = obj.fields_definition() {
            for field in fields.field_definitions() {
                if let Some(ty) = field.ty() {
                    Self::collect_type_references_from_type(
                        &ty,
                        target_type,
                        file_path,
                        source,
                        references,
                    );
                }
                // Check argument types
                if let Some(args) = field.arguments_definition() {
                    for arg in args.input_value_definitions() {
                        if let Some(ty) = arg.ty() {
                            Self::collect_type_references_from_type(
                                &ty,
                                target_type,
                                file_path,
                                source,
                                references,
                            );
                        }
                    }
                }
            }
        }

        // Check implements interfaces
        if let Some(implements) = obj.implements_interfaces() {
            for named_type in implements.named_types() {
                if let Some(name) = named_type.name() {
                    if name.text() == target_type {
                        let range = name.syntax().text_range();
                        let (line, column) = Self::offset_to_position(source, range.start().into());
                        let range = Range {
                            start: Position {
                                line,
                                character: column,
                            },
                            end: Position {
                                line,
                                character: column + target_type.len(),
                            },
                        };
                        references.push(ReferenceLocation::new(file_path.to_string(), range));
                    }
                }
            }
        }
    }

    /// Collect type references from an interface type definition
    fn collect_type_references_from_interface_type(
        iface: &cst::InterfaceTypeDefinition,
        target_type: &str,
        file_path: &str,
        source: &str,
        references: &mut Vec<ReferenceLocation>,
    ) {
        // Check field types
        if let Some(fields) = iface.fields_definition() {
            for field in fields.field_definitions() {
                if let Some(ty) = field.ty() {
                    Self::collect_type_references_from_type(
                        &ty,
                        target_type,
                        file_path,
                        source,
                        references,
                    );
                }
                // Check argument types
                if let Some(args) = field.arguments_definition() {
                    for arg in args.input_value_definitions() {
                        if let Some(ty) = arg.ty() {
                            Self::collect_type_references_from_type(
                                &ty,
                                target_type,
                                file_path,
                                source,
                                references,
                            );
                        }
                    }
                }
            }
        }

        // Check implements interfaces
        if let Some(implements) = iface.implements_interfaces() {
            for named_type in implements.named_types() {
                if let Some(name) = named_type.name() {
                    if name.text() == target_type {
                        let range = name.syntax().text_range();
                        let (line, column) = Self::offset_to_position(source, range.start().into());
                        let range = Range {
                            start: Position {
                                line,
                                character: column,
                            },
                            end: Position {
                                line,
                                character: column + target_type.len(),
                            },
                        };
                        references.push(ReferenceLocation::new(file_path.to_string(), range));
                    }
                }
            }
        }
    }

    /// Collect type references from a union type definition
    fn collect_type_references_from_union_type(
        union: &cst::UnionTypeDefinition,
        target_type: &str,
        file_path: &str,
        source: &str,
        references: &mut Vec<ReferenceLocation>,
    ) {
        // Check union members
        if let Some(members) = union.union_member_types() {
            for named_type in members.named_types() {
                if let Some(name) = named_type.name() {
                    if name.text() == target_type {
                        let range = name.syntax().text_range();
                        let (line, column) = Self::offset_to_position(source, range.start().into());
                        let range = Range {
                            start: Position {
                                line,
                                character: column,
                            },
                            end: Position {
                                line,
                                character: column + target_type.len(),
                            },
                        };
                        references.push(ReferenceLocation::new(file_path.to_string(), range));
                    }
                }
            }
        }
    }

    /// Collect type references from an input object type definition
    fn collect_type_references_from_input_object_type(
        input: &cst::InputObjectTypeDefinition,
        target_type: &str,
        file_path: &str,
        source: &str,
        references: &mut Vec<ReferenceLocation>,
    ) {
        // Check input field types
        if let Some(fields) = input.input_fields_definition() {
            for field in fields.input_value_definitions() {
                if let Some(ty) = field.ty() {
                    Self::collect_type_references_from_type(
                        &ty,
                        target_type,
                        file_path,
                        source,
                        references,
                    );
                }
            }
        }
    }

    /// Collect type references from a Type node (handles List and `NonNull` wrappers)
    fn collect_type_references_from_type(
        ty: &cst::Type,
        target_type: &str,
        file_path: &str,
        source: &str,
        references: &mut Vec<ReferenceLocation>,
    ) {
        match ty {
            cst::Type::NamedType(named) => {
                if let Some(name) = named.name() {
                    if name.text() == target_type {
                        let range = name.syntax().text_range();
                        let (line, column) = Self::offset_to_position(source, range.start().into());
                        let range = Range {
                            start: Position {
                                line,
                                character: column,
                            },
                            end: Position {
                                line,
                                character: column + target_type.len(),
                            },
                        };
                        references.push(ReferenceLocation::new(file_path.to_string(), range));
                    }
                }
            }
            cst::Type::ListType(list) => {
                if let Some(inner_ty) = list.ty() {
                    Self::collect_type_references_from_type(
                        &inner_ty,
                        target_type,
                        file_path,
                        source,
                        references,
                    );
                }
            }
            cst::Type::NonNullType(non_null) => {
                // NonNullType can wrap either a NamedType or a ListType
                if let Some(named) = non_null.named_type() {
                    if let Some(name) = named.name() {
                        if name.text() == target_type {
                            let range = name.syntax().text_range();
                            let (line, column) =
                                Self::offset_to_position(source, range.start().into());
                            let range = Range {
                                start: Position {
                                    line,
                                    character: column,
                                },
                                end: Position {
                                    line,
                                    character: column + target_type.len(),
                                },
                            };
                            references.push(ReferenceLocation::new(file_path.to_string(), range));
                        }
                    }
                } else if let Some(list) = non_null.list_type() {
                    if let Some(inner_ty) = list.ty() {
                        Self::collect_type_references_from_type(
                            &inner_ty,
                            target_type,
                            file_path,
                            source,
                            references,
                        );
                    }
                }
            }
        }
    }
}

impl Default for FindReferencesProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, PartialEq)]
enum ElementType {
    FragmentSpread { fragment_name: String },
    FragmentDefinition { fragment_name: String },
    TypeDefinition { type_name: String },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FragmentInfo;

    #[test]
    fn test_find_fragment_spread_references() {
        let mut doc_index = DocumentIndex::new();
        doc_index.add_fragment(
            "UserFields".to_string(),
            FragmentInfo {
                name: "UserFields".to_string(),
                type_condition: "User".to_string(),
                file_path: "/path/to/fragments.graphql".to_string(),
                line: 0,
                column: 9,
            },
        );

        let schema = SchemaIndex::new();
        let provider = FindReferencesProvider::new();

        let fragment_doc = r"
fragment UserFields on User {
    id
    name
}
";

        let query_doc = r"
query GetUser {
    user {
        ...UserFields
    }
}
";

        let all_documents = vec![
            (
                "/path/to/fragments.graphql".to_string(),
                fragment_doc.to_string(),
            ),
            (
                "/path/to/queries.graphql".to_string(),
                query_doc.to_string(),
            ),
        ];

        // Position on fragment definition name
        let position = Position {
            line: 1,
            character: 12,
        };

        let references = provider
            .find_references(
                fragment_doc,
                position,
                &doc_index,
                &schema,
                &all_documents,
                false, // exclude declaration
            )
            .expect("Should find references");

        // Should find the fragment spread in query_doc
        assert_eq!(references.len(), 1);
        assert_eq!(references[0].file_path, "/path/to/queries.graphql");
    }

    #[test]
    fn test_find_references_include_declaration() {
        let mut doc_index = DocumentIndex::new();
        doc_index.add_fragment(
            "UserFields".to_string(),
            FragmentInfo {
                name: "UserFields".to_string(),
                type_condition: "User".to_string(),
                file_path: "/path/to/fragments.graphql".to_string(),
                line: 1,
                column: 9,
            },
        );

        let schema = SchemaIndex::new();
        let provider = FindReferencesProvider::new();

        let fragment_doc = r"
fragment UserFields on User {
    id
    name
}
";

        let query_doc = r"
query GetUser {
    user {
        ...UserFields
    }
}
";

        let all_documents = vec![
            (
                "/path/to/fragments.graphql".to_string(),
                fragment_doc.to_string(),
            ),
            (
                "/path/to/queries.graphql".to_string(),
                query_doc.to_string(),
            ),
        ];

        let position = Position {
            line: 1,
            character: 12,
        };

        let references = provider
            .find_references(
                fragment_doc,
                position,
                &doc_index,
                &schema,
                &all_documents,
                true, // include declaration
            )
            .expect("Should find references");

        // Should find the spread + the definition
        assert_eq!(references.len(), 2);
    }
}
