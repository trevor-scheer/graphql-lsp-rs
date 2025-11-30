use crate::{
    CompletionItem, CompletionProvider, DefinitionLocation, Diagnostic, DocumentIndex,
    DocumentLoader, FindReferencesProvider, GotoDefinitionProvider, HoverInfo, HoverProvider,
    Position, ReferenceLocation, Result, SchemaIndex, SchemaLoader, Validator,
};
use apollo_compiler::validation::DiagnosticList;
use graphql_config::{GraphQLConfig, ProjectConfig};
use graphql_extract::ExtractConfig;
use std::sync::{Arc, RwLock};

/// Main project structure that manages schema, documents, and validation
pub struct GraphQLProject {
    config: ProjectConfig,
    base_dir: Option<std::path::PathBuf>,
    schema_index: Arc<RwLock<SchemaIndex>>,
    document_index: Arc<RwLock<DocumentIndex>>,
}

/// Extract `ExtractConfig` from `ProjectConfig` extensions
fn get_extract_config(config: &ProjectConfig) -> ExtractConfig {
    config
        .extensions
        .as_ref()
        .and_then(|ext| ext.get("extractConfig"))
        .and_then(|value| serde_json::from_value(value.clone()).ok())
        .unwrap_or_default()
}

/// Extract `LintConfig` from `ProjectConfig` extensions
fn get_lint_config(config: &ProjectConfig) -> crate::LintConfig {
    config
        .extensions
        .as_ref()
        .and_then(|ext| ext.get("project"))
        .and_then(|value| value.get("lint"))
        .and_then(|value| serde_json::from_value(value.clone()).ok())
        .unwrap_or_default()
}

impl GraphQLProject {
    /// Create a new project from configuration
    #[must_use]
    pub fn new(config: ProjectConfig) -> Self {
        Self {
            config,
            base_dir: None,
            schema_index: Arc::new(RwLock::new(SchemaIndex::new())),
            document_index: Arc::new(RwLock::new(DocumentIndex::new())),
        }
    }

    /// Create a new project with a base directory for resolving relative paths
    #[must_use]
    pub fn with_base_dir(mut self, base_dir: std::path::PathBuf) -> Self {
        self.base_dir = Some(base_dir);
        self
    }

    /// Create projects from GraphQL config (single or multi-project)
    pub fn from_config(config: &GraphQLConfig) -> Result<Vec<(String, Self)>> {
        let mut projects = Vec::new();

        for (name, project_config) in config.projects() {
            let project = Self::new(project_config.clone());
            projects.push((name.to_string(), project));
        }

        Ok(projects)
    }

    /// Create projects from GraphQL config with a base directory
    pub fn from_config_with_base(
        config: &GraphQLConfig,
        base_dir: &std::path::Path,
    ) -> Result<Vec<(String, Self)>> {
        let mut projects = Vec::new();

        for (name, project_config) in config.projects() {
            let project = Self::new(project_config.clone()).with_base_dir(base_dir.to_path_buf());
            projects.push((name.to_string(), project));
        }

        Ok(projects)
    }

    /// Load the schema from configured sources
    pub async fn load_schema(&self) -> Result<()> {
        let loader = SchemaLoader::new(self.config.schema.clone());
        let schema_files = loader.load_with_paths().await?;

        // Build index from schema files (preserves source locations per file)
        let index = SchemaIndex::from_schema_files(schema_files);

        // Update state
        {
            let mut schema_index = self.schema_index.write().unwrap();
            *schema_index = index;
        }

        Ok(())
    }

    /// Update schema index with in-memory content for a specific schema file
    ///
    /// This reloads the entire schema from disk, replacing the specified file's content
    /// with the provided in-memory content. This is used by the LSP to keep the schema
    /// up-to-date with editor changes without needing to save to disk.
    ///
    /// Note: Unlike document updates, we reload the entire schema because:
    /// 1. Schema files change less frequently than operation files
    /// 2. Schema relationships (extends, implements) require full rebuild
    /// 3. The performance impact is acceptable for typical schema sizes
    pub async fn update_schema_index(&self, file_path: &str, content: &str) -> Result<()> {
        let loader = SchemaLoader::new(self.config.schema.clone());

        // Set base path if we have one
        let loader = if let Some(ref base_dir) = self.base_dir {
            loader.with_base_path(base_dir)
        } else {
            loader
        };

        let mut schema_files = loader.load_with_paths().await?;

        // Replace the content of the specified file with in-memory content
        let mut found = false;
        for (path, file_content) in &mut schema_files {
            // Normalize paths for comparison (handle both absolute and canonical paths)
            let normalized_path = std::path::Path::new(path.as_str())
                .canonicalize()
                .unwrap_or_else(|_| std::path::PathBuf::from(path.as_str()));
            let normalized_file_path = std::path::Path::new(file_path)
                .canonicalize()
                .unwrap_or_else(|_| std::path::PathBuf::from(file_path));

            if normalized_path == normalized_file_path {
                *file_content = content.to_string();
                found = true;
                break;
            }
        }

        // If the file wasn't found in the schema files, add it
        // (This can happen if the file matches the schema pattern but wasn't loaded yet)
        if !found {
            schema_files.push((file_path.to_string(), content.to_string()));
        }

        // Rebuild the schema index with updated content
        let index = SchemaIndex::from_schema_files(schema_files);

        // Update state
        {
            let mut schema_index = self.schema_index.write().unwrap();
            *schema_index = index;
        }

        Ok(())
    }

    /// Load documents from configured sources
    pub fn load_documents(&self) -> Result<()> {
        // Return early if no documents configured
        let Some(ref documents_config) = self.config.documents else {
            return Ok(());
        };

        let mut loader = DocumentLoader::new(documents_config.clone());

        // Set base path if we have one
        if let Some(ref base_dir) = self.base_dir {
            loader = loader.with_base_path(base_dir);
        }

        // Set extract config from project extensions
        loader = loader.with_extract_config(get_extract_config(&self.config));

        let index = loader.load()?;

        // Update document index
        {
            let mut document_index = self.document_index.write().unwrap();
            *document_index = index;
        }

        Ok(())
    }

    /// Validate a single document string against the loaded schema
    ///
    /// Returns Ok(()) if valid, or Err with a `DiagnosticList` containing errors and warnings.
    /// This validates a single GraphQL document against the project's schema.
    ///
    /// The `DiagnosticList` can be used directly for CLI output or converted to LSP diagnostics
    /// by the language server package.
    pub fn validate_document(&self, document: &str) -> std::result::Result<(), DiagnosticList> {
        let schema_index = self.schema_index.read().unwrap();
        let validator = Validator::new();
        validator.validate_document(document, &schema_index)
    }

    /// Validate a document with file location information for accurate diagnostics
    ///
    /// This method adjusts the source to include line offsets, making apollo-compiler's
    /// diagnostics show the correct file name and line numbers.
    pub fn validate_document_with_location(
        &self,
        document: &str,
        file_name: &str,
        line_offset: usize,
    ) -> std::result::Result<(), DiagnosticList> {
        let schema_index = self.schema_index.read().unwrap();
        let validator = Validator::new();
        validator.validate_document_with_location(document, &schema_index, file_name, line_offset)
    }

    /// Get the schema index for advanced operations
    #[must_use]
    pub fn get_schema_index(&self) -> SchemaIndex {
        self.schema_index.read().unwrap().clone()
    }

    /// Get document index
    #[must_use]
    pub fn get_document_index(&self) -> DocumentIndex {
        let index = self.document_index.read().unwrap();
        // Clone the inner data
        DocumentIndex {
            operations: index.operations.clone(),
            fragments: index.fragments.clone(),
            parsed_asts: index.parsed_asts.clone(),
            extracted_blocks: index.extracted_blocks.clone(),
            line_indices: index.line_indices.clone(),
        }
    }

    /// Get cached extracted blocks for a file
    #[must_use]
    pub fn get_extracted_blocks(&self, file_path: &str) -> Option<Vec<crate::ExtractedBlock>> {
        let index = self.document_index.read().unwrap();
        index.get_extracted_blocks(file_path).cloned()
    }

    /// Get the extract configuration for this project
    #[must_use]
    pub fn get_extract_config(&self) -> ExtractConfig {
        get_extract_config(&self.config)
    }

    /// Get the lint configuration for this project
    #[must_use]
    pub fn get_lint_config(&self) -> crate::LintConfig {
        get_lint_config(&self.config)
    }

    /// Run project-wide lint rules on all documents
    ///
    /// This runs lint rules that require analyzing the entire project, such as
    /// detecting unused schema fields across all operations and fragments.
    #[must_use]
    pub fn lint_project(&self) -> Vec<Diagnostic> {
        let linter = crate::Linter::new(self.get_lint_config());
        let document_index = self.document_index.read().unwrap();
        let schema_index = self.schema_index.read().unwrap();

        linter.lint_project(&document_index, &schema_index)
    }

    /// Update document index for a single file with in-memory content
    ///
    /// This removes all operations and fragments from the specified file path,
    /// then re-indexes the provided content as if it came from that file.
    /// This is used by the LSP to keep the index up-to-date with editor changes
    /// without needing to reload all files from disk.
    #[allow(clippy::significant_drop_tightening)]
    pub fn update_document_index(&self, file_path: &str, content: &str) -> Result<()> {
        use apollo_parser::Parser;
        use graphql_extract::{extract_from_source, Language};
        use std::path::Path;

        // Determine language from file extension
        let path = Path::new(file_path);
        let language = path.extension().and_then(|ext| ext.to_str()).map_or(
            Language::GraphQL,
            |ext| match ext.to_lowercase().as_str() {
                "ts" | "tsx" => Language::TypeScript,
                "js" | "jsx" => Language::JavaScript,
                _ => Language::GraphQL,
            },
        );

        // Parse the full content once and cache it
        let parsed = Parser::new(content).parse();
        let parsed_arc = std::sync::Arc::new(parsed);

        // Extract GraphQL from the content
        let extracted =
            extract_from_source(content, language, &get_extract_config(&self.config))
                .map_err(|e| crate::ProjectError::DocumentLoad(format!("Extract error: {e}")))?;

        // Acquire write lock and update index
        {
            let mut document_index = self.document_index.write().unwrap();

            // Remove all existing entries for this file
            document_index.operations.retain(|_, ops| {
                ops.retain(|op| op.file_path != file_path);
                !ops.is_empty()
            });
            document_index.fragments.retain(|_, frags| {
                frags.retain(|frag| frag.file_path != file_path);
                !frags.is_empty()
            });

            // Cache the parsed AST
            document_index.cache_ast(file_path.to_string(), parsed_arc);

            // Build and cache line index for fast position<->offset conversion
            let line_index = crate::LineIndex::new(content);
            document_index.cache_line_index(file_path.to_string(), std::sync::Arc::new(line_index));

            // Cache extracted blocks with their parsed ASTs (Phase 3 optimization)
            let mut cached_blocks = Vec::new();
            for item in &extracted {
                // Parse each extracted block and cache it
                let block_parsed = Parser::new(&item.source).parse();
                let block = crate::ExtractedBlock {
                    content: item.source.clone(),
                    offset: item.location.offset,
                    length: item.location.length,
                    start_line: item.location.range.start.line,
                    start_column: item.location.range.start.column,
                    end_line: item.location.range.end.line,
                    end_column: item.location.range.end.column,
                    parsed: std::sync::Arc::new(block_parsed),
                };
                cached_blocks.push(block);
            }
            // Always update the cache, even if empty, to ensure stale data is cleared
            if cached_blocks.is_empty() {
                // Remove the cached blocks for this file if there are none
                document_index.remove_extracted_blocks(file_path);
            } else {
                document_index.cache_extracted_blocks(file_path.to_string(), cached_blocks);
            }

            // Parse and index each extracted GraphQL block
            for item in extracted {
                DocumentLoader::parse_and_index(&item, file_path, &mut document_index);
            }
        }

        Ok(())
    }

    /// Validate a GraphQL document source with global fragment resolution
    ///
    /// This method handles validation of GraphQL documents (pure .graphql files or extracted sources)
    /// with proper fragment resolution. It automatically includes all fragments from the project
    /// when validating operations, and validates fragments standalone for schema correctness.
    ///
    /// # Arguments
    /// * `source` - The GraphQL source code to validate
    /// * `file_name` - Name/path for error reporting
    ///
    /// # Returns
    /// A list of validation diagnostics (errors and warnings)
    #[must_use]
    #[allow(clippy::significant_drop_tightening)]
    pub fn validate_document_source(&self, source: &str, file_name: &str) -> Vec<Diagnostic> {
        use apollo_compiler::validation::Valid;
        use apollo_compiler::{parser::Parser, ExecutableDocument};

        let schema_index = self.schema_index.read().unwrap();
        let schema = schema_index.schema();
        let valid_schema = Valid::assume_valid_ref(schema);

        let mut errors =
            apollo_compiler::validation::DiagnosticList::new(std::sync::Arc::default());
        let mut builder = ExecutableDocument::builder(Some(valid_schema), &mut errors);
        let is_fragment_only = Self::is_fragment_only(source);

        // Add the current document
        Parser::new().parse_into_executable_builder(source, file_name, &mut builder);

        // Only add referenced fragments (and their dependencies) if this document uses fragment spreads
        if !is_fragment_only && source.contains("...") {
            // Find all fragment names referenced in this document (recursively)
            let referenced_fragments = Self::collect_referenced_fragments(source, self);

            // Add each referenced fragment individually
            for fragment_name in referenced_fragments {
                if let Some(frag_info) = self.get_fragment(&fragment_name) {
                    // Extract just this specific fragment from the file
                    if let Some(fragment_source) = self.extract_fragment_from_file(
                        std::path::Path::new(&frag_info.file_path),
                        &fragment_name,
                    ) {
                        // Add this specific fragment to the builder
                        Parser::new().parse_into_executable_builder(
                            &fragment_source,
                            &frag_info.file_path,
                            &mut builder,
                        );
                    }
                }
            }
        }

        // Build and validate
        let doc = builder.build();

        // Collect fragment names used across the entire project
        let used_fragments = self.collect_used_fragment_names();

        let mut diagnostics = if errors.is_empty() {
            match doc.validate(valid_schema) {
                Ok(_) => vec![],
                Err(with_errors) => Self::convert_compiler_diagnostics(
                    &with_errors.errors,
                    is_fragment_only,
                    &used_fragments,
                    file_name,
                ),
            }
        } else {
            Self::convert_compiler_diagnostics(
                &errors,
                is_fragment_only,
                &used_fragments,
                file_name,
            )
        };

        // Add deprecation warnings
        let validator = Validator::new();
        let deprecation_warnings =
            validator.check_deprecated_fields_custom(source, &schema_index, file_name);
        diagnostics.extend(deprecation_warnings);

        // Add unused fragment warnings for fragments defined in this file
        let unused_fragment_warnings =
            Self::check_unused_fragments_in_file(source, file_name, &used_fragments);
        diagnostics.extend(unused_fragment_warnings);

        // Note: Within-document unique name validation is handled by apollo-compiler
        // Project-wide unique name validation is handled separately via DocumentIndex

        diagnostics
    }

    /// Validate GraphQL documents extracted from TypeScript/JavaScript files
    ///
    /// This method validates multiple extracted GraphQL documents from a single source file,
    /// handling line offsets, fragment resolution, and filtering errors to their correct locations.
    ///
    /// # Arguments
    /// * `extracted` - List of extracted GraphQL documents with their locations
    /// * `file_path` - Path to the source file for error reporting
    ///
    /// # Returns
    /// A list of validation diagnostics with correct line/column positions
    #[must_use]
    #[allow(clippy::significant_drop_tightening)]
    #[allow(clippy::too_many_lines)]
    pub fn validate_extracted_documents(
        &self,
        extracted: &[graphql_extract::ExtractedGraphQL],
        file_path: &str,
    ) -> Vec<Diagnostic> {
        use apollo_compiler::validation::Valid;
        use apollo_compiler::{parser::Parser, ExecutableDocument};

        if extracted.is_empty() {
            return vec![];
        }

        let schema_index = self.schema_index.read().unwrap();
        let schema = schema_index.schema();
        let valid_schema = Valid::assume_valid_ref(schema);

        let mut all_diagnostics = Vec::new();

        // Validate each extracted document
        for item in extracted {
            let line_offset = item.location.range.start.line;
            let col_offset = item.location.range.start.column;
            let source = &item.source;

            let mut errors =
                apollo_compiler::validation::DiagnosticList::new(std::sync::Arc::default());
            let mut builder = ExecutableDocument::builder(Some(valid_schema), &mut errors);
            let is_fragment_only = Self::is_fragment_only(source);

            // Use source_offset for accurate error reporting (convert 0-indexed to 1-indexed)
            // graphql-extract gives us 0-indexed positions, apollo-compiler wants 1-indexed
            let offset = apollo_compiler::parser::SourceOffset {
                line: line_offset + 1,  // Convert to 1-indexed
                column: col_offset + 1, // Convert to 1-indexed
            };

            Parser::new()
                .source_offset(offset)
                .parse_into_executable_builder(source, file_path, &mut builder);

            // Only add referenced fragments (and their dependencies) if this document uses fragment spreads
            if !is_fragment_only && source.contains("...") {
                // Find all fragment names referenced in this document (recursively)
                let referenced_fragments = Self::collect_referenced_fragments(source, self);
                let current_path = std::path::Path::new(file_path);

                // Add only the referenced fragments from OTHER files
                // Fragments in the current file are already in the builder
                for fragment_name in referenced_fragments {
                    if let Some(frag_info) = self.get_fragment(&fragment_name) {
                        // Skip fragments from the current file
                        if std::path::Path::new(&frag_info.file_path) == current_path {
                            continue;
                        }

                        // Extract just this specific fragment from the file
                        if let Some(fragment_source) = self.extract_fragment_from_file(
                            std::path::Path::new(&frag_info.file_path),
                            &fragment_name,
                        ) {
                            // Add this specific fragment to the builder
                            Parser::new().parse_into_executable_builder(
                                &fragment_source,
                                &frag_info.file_path,
                                &mut builder,
                            );
                        }
                    }
                }
            }

            // Build and validate
            let doc = builder.build();

            // Collect fragment names used across the entire project
            let used_fragments = self.collect_used_fragment_names();

            let mut diagnostics = if errors.is_empty() {
                match doc.validate(valid_schema) {
                    Ok(_) => vec![],
                    Err(with_errors) => {
                        let mut diags = Self::convert_compiler_diagnostics(
                            &with_errors.errors,
                            is_fragment_only,
                            &used_fragments,
                            file_path,
                        );

                        // For operations: filter to only errors within this document's line range
                        if !is_fragment_only {
                            let source_line_count = source.lines().count();
                            let end_line = line_offset + source_line_count;
                            diags.retain(|d| {
                                let start_line = d.range.start.line;
                                start_line >= line_offset && start_line < end_line
                            });
                        }

                        diags
                    }
                }
            } else {
                let mut diags = Self::convert_compiler_diagnostics(
                    &errors,
                    is_fragment_only,
                    &used_fragments,
                    file_path,
                );

                // For operations: filter to only errors within this document's line range
                if !is_fragment_only {
                    let source_line_count = source.lines().count();
                    let end_line = line_offset + source_line_count;
                    diags.retain(|d| {
                        let start_line = d.range.start.line;
                        start_line >= line_offset && start_line < end_line
                    });
                }

                diags
            };

            // Add deprecation warnings
            // Note: We still need to manually adjust line offsets for deprecation warnings
            // since check_deprecated_fields_custom uses apollo-parser directly without offset support
            let validator = Validator::new();
            let deprecation_warnings =
                validator.check_deprecated_fields_custom(source, &schema_index, file_path);

            for mut warning in deprecation_warnings {
                warning.range.start.line += line_offset;
                warning.range.end.line += line_offset;
                diagnostics.push(warning);
            }

            // Add unused fragment warnings for fragments defined in this extracted block
            let unused_warnings =
                Self::check_unused_fragments_in_file(source, file_path, &used_fragments);
            for mut warning in unused_warnings {
                warning.range.start.line += line_offset;
                warning.range.end.line += line_offset;
                diagnostics.push(warning);
            }

            // Note: Within-document unique name validation is handled by apollo-compiler
            // Project-wide unique name validation is handled separately via DocumentIndex

            all_diagnostics.extend(diagnostics);
        }

        all_diagnostics
    }

    /// Check if a document contains only fragments (no operations)
    fn is_fragment_only(content: &str) -> bool {
        let trimmed = content.trim();
        trimmed.starts_with("fragment")
            && !trimmed.contains("query")
            && !trimmed.contains("mutation")
            && !trimmed.contains("subscription")
    }

    /// Collect all fragment names that are actually used (via fragment spreads) across the project
    ///
    /// This scans all documents in the project to find fragment spreads and returns a set
    /// of fragment names that are referenced anywhere in the codebase.
    ///
    /// Uses cached parsed ASTs from the document index for efficiency and to ensure we use
    /// up-to-date in-memory content rather than stale files from disk.
    fn collect_used_fragment_names(&self) -> std::collections::HashSet<String> {
        use apollo_parser::cst;
        use std::collections::HashSet;

        let mut used_fragments = HashSet::new();

        // Use the cached parsed ASTs from the document index
        // This ensures we use up-to-date in-memory content that was updated via update_document_index
        let document_index = self.document_index.read().unwrap();

        // Scan each cached AST for fragment spreads
        for ast in document_index.parsed_asts.values() {
            // Walk the document looking for fragment spreads
            for definition in ast.document().definitions() {
                if let cst::Definition::OperationDefinition(operation) = definition {
                    if let Some(selection_set) = operation.selection_set() {
                        Self::collect_fragment_spreads_from_selection_set(
                            &selection_set,
                            &mut used_fragments,
                        );
                    }
                }
            }
        }

        // Also check extracted blocks for TypeScript/JavaScript files
        for blocks in document_index.extracted_blocks.values() {
            for block in blocks {
                // Walk the pre-parsed AST looking for fragment spreads
                for definition in block.parsed.document().definitions() {
                    if let cst::Definition::OperationDefinition(operation) = definition {
                        if let Some(selection_set) = operation.selection_set() {
                            Self::collect_fragment_spreads_from_selection_set(
                                &selection_set,
                                &mut used_fragments,
                            );
                        }
                    }
                }
            }
        }

        drop(document_index);

        used_fragments
    }

    /// Recursively collect fragment spread names from a selection set
    fn collect_fragment_spreads_from_selection_set(
        selection_set: &apollo_parser::cst::SelectionSet,
        used_fragments: &mut std::collections::HashSet<String>,
    ) {
        use apollo_parser::cst;

        for selection in selection_set.selections() {
            match selection {
                cst::Selection::Field(field) => {
                    // Recursively check nested selections
                    if let Some(nested_selection_set) = field.selection_set() {
                        Self::collect_fragment_spreads_from_selection_set(
                            &nested_selection_set,
                            used_fragments,
                        );
                    }
                }
                cst::Selection::FragmentSpread(spread) => {
                    // Found a fragment spread - record the fragment name
                    if let Some(fragment_name) = spread.fragment_name() {
                        if let Some(name) = fragment_name.name() {
                            used_fragments.insert(name.text().to_string());
                        }
                    }
                }
                cst::Selection::InlineFragment(inline_fragment) => {
                    // Recursively check inline fragment selections
                    if let Some(nested_selection_set) = inline_fragment.selection_set() {
                        Self::collect_fragment_spreads_from_selection_set(
                            &nested_selection_set,
                            used_fragments,
                        );
                    }
                }
            }
        }
    }

    /// Collect all fragment names referenced in a document (recursively)
    ///
    /// This finds all fragment spreads in the document, then recursively finds
    /// fragments that those fragments depend on, building a complete set of all
    /// fragments needed to validate this document.
    fn collect_referenced_fragments(
        source: &str,
        project: &Self,
    ) -> std::collections::HashSet<String> {
        use apollo_parser::{cst, Parser};
        use std::collections::{HashSet, VecDeque};

        let mut referenced = HashSet::new();
        let mut to_process = VecDeque::new();

        // First, find all fragment spreads directly in this document
        let parser = Parser::new(source);
        let tree = parser.parse();

        for definition in tree.document().definitions() {
            if let cst::Definition::OperationDefinition(operation) = definition {
                if let Some(selection_set) = operation.selection_set() {
                    let mut direct_fragments = HashSet::new();
                    Self::collect_fragment_spreads_from_selection_set(
                        &selection_set,
                        &mut direct_fragments,
                    );
                    for frag_name in direct_fragments {
                        if !referenced.contains(&frag_name) {
                            referenced.insert(frag_name.clone());
                            to_process.push_back(frag_name);
                        }
                    }
                }
            }
        }

        // Now recursively process fragment dependencies
        while let Some(fragment_name) = to_process.pop_front() {
            // Get the fragment definition and scan it for more fragment spreads
            if let Some(frag_info) = project.get_fragment(&fragment_name) {
                if let Ok(frag_extracted) = graphql_extract::extract_from_file(
                    std::path::Path::new(&frag_info.file_path),
                    &get_extract_config(&project.config),
                ) {
                    for frag_item in frag_extracted {
                        let frag_parser = Parser::new(&frag_item.source);
                        let frag_tree = frag_parser.parse();

                        for definition in frag_tree.document().definitions() {
                            if let cst::Definition::FragmentDefinition(fragment) = definition {
                                if let Some(selection_set) = fragment.selection_set() {
                                    let mut nested_fragments = HashSet::new();
                                    Self::collect_fragment_spreads_from_selection_set(
                                        &selection_set,
                                        &mut nested_fragments,
                                    );
                                    for nested_frag_name in nested_fragments {
                                        if !referenced.contains(&nested_frag_name) {
                                            referenced.insert(nested_frag_name.clone());
                                            to_process.push_back(nested_frag_name);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        referenced
    }

    /// Get a fragment by name from the document index
    fn get_fragment(&self, name: &str) -> Option<crate::FragmentInfo> {
        let document_index = self.document_index.read().unwrap();
        document_index
            .fragments
            .get(name)
            .and_then(|infos| infos.first().cloned())
    }

    /// Extract a specific fragment definition from a file
    ///
    /// This parses the file and extracts only the named fragment, rather than
    /// including all fragments in the file. Returns None if the fragment isn't found.
    fn extract_fragment_from_file(
        &self,
        file_path: &std::path::Path,
        fragment_name: &str,
    ) -> Option<String> {
        use apollo_parser::{cst::CstNode, Parser};

        // Extract GraphQL from the file
        let extracted =
            graphql_extract::extract_from_file(file_path, &get_extract_config(&self.config))
                .ok()?;

        // Parse each extracted block looking for the fragment
        for item in extracted {
            let parser = Parser::new(&item.source);
            let tree = parser.parse();

            for definition in tree.document().definitions() {
                if let apollo_parser::cst::Definition::FragmentDefinition(fragment) = definition {
                    if let Some(frag_name_node) = fragment.fragment_name() {
                        if let Some(name_node) = frag_name_node.name() {
                            if name_node.text() == fragment_name {
                                // Found the fragment - extract its text from the source
                                let syntax_node = fragment.syntax();
                                let start: usize = syntax_node.text_range().start().into();
                                let end: usize = syntax_node.text_range().end().into();
                                return Some(item.source[start..end].to_string());
                            }
                        }
                    }
                }
            }
        }

        None
    }

    /// Convert apollo-compiler diagnostics to our diagnostic format
    ///
    /// # Arguments
    /// * `compiler_diags` - Diagnostics from apollo-compiler
    /// * `is_fragment_only` - Whether the document contains only fragments
    /// * `used_fragments` - Set of fragment names that are used anywhere in the project
    /// * `_file_name` - Name of the file being validated (reserved for future use)
    fn convert_compiler_diagnostics(
        compiler_diags: &apollo_compiler::validation::DiagnosticList,
        is_fragment_only: bool,
        _used_fragments: &std::collections::HashSet<String>,
        _file_name: &str,
    ) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();

        for diag in compiler_diags.iter() {
            let message = diag.error.to_string();
            let message_lower = message.to_lowercase();

            // Skip "unused fragment" and "must be used" errors for fragment-only documents
            if is_fragment_only
                && (message_lower.contains("unused")
                    || message_lower.contains("never used")
                    || message_lower.contains("must be used"))
            {
                continue;
            }

            // Skip ALL "unused fragment" errors from apollo-compiler
            // We handle unused fragment warnings separately in check_unused_fragments_in_file
            // which reports them at the correct location (the fragment definition file)
            // rather than at arbitrary operation locations.
            if message_lower.contains("fragment")
                && (message_lower.contains("unused")
                    || message_lower.contains("never used")
                    || message_lower.contains("must be used"))
            {
                continue;
            }

            if let Some(loc_range) = diag.line_column_range() {
                diagnostics.push(Diagnostic {
                    range: crate::Range {
                        start: crate::Position {
                            // apollo-compiler uses 1-based, we use 0-based
                            line: loc_range.start.line.saturating_sub(1),
                            character: loc_range.start.column.saturating_sub(1),
                        },
                        end: crate::Position {
                            line: loc_range.end.line.saturating_sub(1),
                            character: loc_range.end.column.saturating_sub(1),
                        },
                    },
                    severity: crate::Severity::Error,
                    code: None,
                    source: "graphql".to_string(),
                    message,
                    related_info: Vec::new(),
                });
            }
        }

        diagnostics
    }

    /// Check for unused fragments defined in this specific file/source
    ///
    /// Returns warnings for any fragment definitions in the source that are not
    /// used anywhere in the project (based on the `used_fragments` set).
    fn check_unused_fragments_in_file(
        source: &str,
        _file_name: &str,
        used_fragments: &std::collections::HashSet<String>,
    ) -> Vec<Diagnostic> {
        use crate::{Diagnostic, Position, Range};
        use apollo_parser::{cst::CstNode, Parser};

        let mut warnings = Vec::new();
        let parser = Parser::new(source);
        let tree = parser.parse();

        // If there are syntax errors, skip unused fragment checking
        if tree.errors().len() > 0 {
            return warnings;
        }

        // Walk through all fragment definitions in this source
        for definition in tree.document().definitions() {
            if let apollo_parser::cst::Definition::FragmentDefinition(fragment) = definition {
                if let Some(fragment_name_node) = fragment.fragment_name() {
                    if let Some(name_node) = fragment_name_node.name() {
                        let fragment_name = name_node.text().to_string();

                        // Check if this fragment is used anywhere in the project
                        if !used_fragments.contains(&fragment_name) {
                            // Fragment is truly unused - create a warning
                            let syntax_node = name_node.syntax();
                            let offset: usize = syntax_node.text_range().start().into();
                            let (line, col) = Self::offset_to_line_col(source, offset);

                            let range = Range {
                                start: Position {
                                    line,
                                    character: col,
                                },
                                end: Position {
                                    line,
                                    character: col + fragment_name.len(),
                                },
                            };

                            let message = format!(
                                "Fragment '{fragment_name}' is defined but never used in any operation"
                            );

                            warnings.push(
                                Diagnostic::warning(range, message)
                                    .with_code("unused-fragment")
                                    .with_source("graphql-validator"),
                            );
                        }
                    }
                }
            }
        }

        warnings
    }

    /// Convert a byte offset to a line and column (0-indexed)
    fn offset_to_line_col(source: &str, offset: usize) -> (usize, usize) {
        let mut line = 0;
        let mut col = 0;
        let mut current_offset = 0;

        for ch in source.chars() {
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

    /// Get hover information for a position in a GraphQL document
    ///
    /// Returns hover information (type info, descriptions, etc.) for the element
    /// at the given position in the source code.
    #[must_use]
    pub fn hover_info(
        &self,
        source: &str,
        position: Position,
        file_path: &str,
    ) -> Option<HoverInfo> {
        let schema_index = self.schema_index.read().unwrap();
        let cached_ast = self.document_index.read().unwrap().get_ast(file_path);
        let hover_provider = HoverProvider::new();

        hover_provider.hover_with_ast(source, position, &schema_index, cached_ast.as_deref())
    }

    /// Get hover information at a position, handling TypeScript/JavaScript extraction
    ///
    /// This method automatically detects if the file is TypeScript/JavaScript and
    /// extracts GraphQL blocks, adjusting positions accordingly. For pure GraphQL
    /// files, it delegates to `hover_info`.
    ///
    /// # Arguments
    /// * `file_path` - The file path or URI (used to determine file type and retrieve cached blocks)
    /// * `position` - The position in the original file
    /// * `full_content` - The full document content (used for GraphQL files)
    #[must_use]
    #[allow(clippy::case_sensitive_file_extension_comparisons)]
    pub fn hover_info_at_position(
        &self,
        file_path: &str,
        position: Position,
        full_content: &str,
    ) -> Option<HoverInfo> {
        // Check if this is a TypeScript/JavaScript file
        // file_path can be a URI (file:///...) or a regular path, so we use ends_with
        let is_ts_file = file_path.ends_with(".ts")
            || file_path.ends_with(".tsx")
            || file_path.ends_with(".js")
            || file_path.ends_with(".jsx");

        if is_ts_file {
            tracing::debug!(
                "Detected TypeScript/JavaScript file, looking for extracted blocks for: {}",
                file_path
            );

            // Try to use cached extracted blocks
            let cached_blocks = self.get_extracted_blocks(file_path)?;

            tracing::debug!("Found {} extracted blocks", cached_blocks.len());

            // Find which extracted GraphQL block contains the cursor position
            for block in cached_blocks {
                if position.line >= block.start_line && position.line <= block.end_line {
                    // Adjust position relative to the extracted GraphQL
                    let relative_position = Position {
                        line: position.line - block.start_line,
                        character: if position.line == block.start_line {
                            position.character.saturating_sub(block.start_column)
                        } else {
                            position.character
                        },
                    };

                    tracing::debug!(
                        "Adjusted position from {:?} to {:?} for extracted block",
                        position,
                        relative_position
                    );

                    // Get hover info using the extracted GraphQL content and its cached AST
                    // Use the block's parsed AST instead of looking up by file_path
                    // (which would return the TypeScript file's AST with syntax errors)
                    let hover_result = {
                        let schema_index = self.schema_index.read().unwrap();
                        let hover_provider = HoverProvider::new();
                        hover_provider.hover_with_ast(
                            &block.content,
                            relative_position,
                            &schema_index,
                            Some(&block.parsed),
                        )
                    };

                    if hover_result.is_none() {
                        tracing::debug!(
                            "hover_info returned None for extracted block at position {:?}. Block content:\n{}",
                            relative_position,
                            block.content
                        );
                    } else {
                        tracing::debug!("hover_info succeeded for extracted block");
                    }

                    return hover_result;
                }
            }

            // Cursor not in any GraphQL block
            None
        } else {
            // For .graphql files, use the original logic
            self.hover_info(full_content, position, file_path)
        }
    }

    /// Get completion items for a position in a GraphQL document
    #[must_use]
    pub fn complete(
        &self,
        source: &str,
        position: Position,
        file_path: &str,
    ) -> Option<Vec<CompletionItem>> {
        let cached_ast = self.document_index.read().unwrap().get_ast(file_path);
        let document_index = self.document_index.read().unwrap();
        let schema_index = self.schema_index.read().unwrap();
        let completion_provider = CompletionProvider::new();

        completion_provider.complete_with_ast(
            source,
            position,
            &document_index,
            &schema_index,
            cached_ast.as_deref(),
        )
    }

    /// Get definition locations for a position in a GraphQL document
    ///
    /// Returns the locations where the element at the given position is defined.
    /// For example, clicking on a fragment spread will return the location of the
    /// fragment definition.
    #[must_use]
    pub fn goto_definition(
        &self,
        source: &str,
        position: Position,
        file_path: &str,
    ) -> Option<Vec<DefinitionLocation>> {
        let cached_ast = self.document_index.read().unwrap().get_ast(file_path);
        let document_index = self.document_index.read().unwrap();
        let schema_index = self.schema_index.read().unwrap();
        let provider = GotoDefinitionProvider::new();

        provider.goto_definition_with_ast(
            source,
            position,
            &document_index,
            &schema_index,
            file_path,
            cached_ast.as_deref(),
        )
    }

    /// Find all references to the element at a position in a GraphQL document
    ///
    /// Returns all locations where the element at the given position is referenced.
    /// For example, finding references on a fragment definition will return all
    /// fragment spreads that use that fragment.
    ///
    /// # Arguments
    /// * `source` - The GraphQL source code of the current document
    /// * `position` - The position in the document to find references for
    /// * `all_documents` - All GraphQL documents in the project (for finding usages)
    /// * `include_declaration` - Whether to include the declaration/definition in results
    #[must_use]
    pub fn find_references(
        &self,
        source: &str,
        position: Position,
        all_documents: &[(String, String)],
        include_declaration: bool,
    ) -> Option<Vec<ReferenceLocation>> {
        self.find_references_with_asts(
            source,
            position,
            all_documents,
            include_declaration,
            None,
            None,
        )
    }

    /// Find all references with optional pre-parsed ASTs for optimization
    ///
    /// # Arguments
    /// * `source` - The GraphQL source code of the current document
    /// * `position` - The position in the document to find references for
    /// * `all_documents` - All GraphQL documents in the project (for finding usages)
    /// * `include_declaration` - Whether to include the declaration/definition in results
    /// * `source_file_path` - File path of the source document (for AST lookup)
    /// * `document_asts` - Pre-parsed ASTs map to avoid re-parsing all documents
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn find_references_with_asts(
        &self,
        source: &str,
        position: Position,
        all_documents: &[(String, String)],
        include_declaration: bool,
        source_file_path: Option<&str>,
        document_asts: Option<&std::collections::HashMap<String, apollo_parser::SyntaxTree>>,
    ) -> Option<Vec<ReferenceLocation>> {
        let document_index = self.document_index.read().unwrap();
        let schema_index = self.schema_index.read().unwrap();
        let provider = FindReferencesProvider::new();

        // Get source AST from cache if available
        let source_ast = source_file_path.and_then(|path| document_index.get_ast(path));

        provider.find_references_with_asts(
            source,
            position,
            &document_index,
            &schema_index,
            all_documents,
            include_declaration,
            source_ast.as_deref(),
            document_asts,
        )
    }

    /// Check if a file path matches the schema configuration
    ///
    /// This is used by the LSP to determine if a file should be validated
    /// as a schema file (type definitions) or as a document (executable operations).
    #[must_use]
    pub fn is_schema_file(&self, file_path: &std::path::Path) -> bool {
        use glob::Pattern;

        let schema_patterns = self.config.schema.paths();

        // Get the file path as a string for matching
        let Some(file_str) = file_path.to_str() else {
            return false;
        };

        // Check if file matches any schema pattern
        for pattern_str in schema_patterns {
            // Resolve the pattern to an absolute path if we have a base_dir
            if let Some(ref base) = self.base_dir {
                // Normalize the pattern by stripping leading ./ if present
                let normalized_pattern = pattern_str.strip_prefix("./").unwrap_or(pattern_str);

                // Join with base directory to get absolute path
                let full_path = base.join(normalized_pattern);

                // Canonicalize both paths if possible for comparison
                let canonical_full = full_path.canonicalize().ok();
                let canonical_file = file_path.canonicalize().ok();

                // Try exact match with canonicalized paths
                if let (Some(ref full), Some(ref file)) = (&canonical_full, &canonical_file) {
                    if full == file {
                        return true;
                    }
                }

                // Also try glob pattern matching
                if let (Some(full_str), Ok(pattern)) =
                    (full_path.to_str(), Pattern::new(normalized_pattern))
                {
                    if pattern.matches(file_str) {
                        return true;
                    }
                    // Try matching against the full resolved path
                    if let Ok(full_pattern) = Pattern::new(full_str) {
                        if full_pattern.matches(file_str) {
                            return true;
                        }
                    }
                }
            } else {
                // No base directory, try matching against the pattern directly
                if let Ok(pattern) = Pattern::new(pattern_str) {
                    if pattern.matches(file_str) {
                        return true;
                    }
                }
            }
        }

        false
    }

    /// Get all schema file paths for this project
    ///
    /// Returns a list of all schema files that match the project's schema patterns.
    /// This is used by the LSP to find schema files that need to be revalidated.
    #[must_use]
    pub fn get_schema_file_paths(&self) -> Vec<String> {
        let schema_patterns = self.config.schema.paths();
        let mut schema_files = Vec::new();

        for pattern_str in schema_patterns {
            // Skip remote schemas (http/https URLs)
            if pattern_str.starts_with("http://") || pattern_str.starts_with("https://") {
                continue;
            }

            // Resolve pattern to absolute path if we have a base_dir
            let pattern_to_glob = self.base_dir.as_ref().map_or_else(
                || pattern_str.to_string(),
                |base| {
                    let normalized_pattern = pattern_str.strip_prefix("./").unwrap_or(pattern_str);
                    base.join(normalized_pattern).display().to_string()
                },
            );

            // Use glob to find matching files
            if let Ok(paths) = glob::glob(&pattern_to_glob) {
                for entry in paths.flatten() {
                    schema_files.push(entry.display().to_string());
                }
            }
        }

        schema_files
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use graphql_config::{DocumentsConfig, SchemaConfig};

    #[test]
    fn test_create_project() {
        let config = ProjectConfig {
            schema: SchemaConfig::Path("schema.graphql".to_string()),
            documents: Some(DocumentsConfig::Pattern("**/*.graphql".to_string())),
            include: None,
            exclude: None,
            extensions: None,
        };

        let _project = GraphQLProject::new(config);
        // Project created successfully with empty schema index
    }

    #[test]
    fn test_from_single_config() {
        let config = GraphQLConfig::Single(ProjectConfig {
            schema: SchemaConfig::Path("schema.graphql".to_string()),
            documents: None,
            include: None,
            exclude: None,
            extensions: None,
        });

        let projects = GraphQLProject::from_config(&config).unwrap();
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].0, "default");
    }

    #[test]
    fn test_get_extract_config_default() {
        let config = ProjectConfig {
            schema: SchemaConfig::Path("schema.graphql".to_string()),
            documents: None,
            include: None,
            exclude: None,
            extensions: None,
        };
        let extract_config = get_extract_config(&config);
        assert_eq!(extract_config.magic_comment, "GraphQL");
        assert_eq!(extract_config.tag_identifiers, vec!["gql", "graphql"]);
        assert!(!extract_config.allow_global_identifiers);
    }

    #[test]
    fn test_get_extract_config_from_extensions() {
        let mut extensions = std::collections::HashMap::new();
        extensions.insert(
            "extractConfig".to_string(),
            serde_json::json!({
                "magicComment": "CustomGraphQL",
                "tagIdentifiers": ["gql", "customTag"],
                "modules": ["custom-module"],
                "allowGlobalIdentifiers": true
            }),
        );
        let config = ProjectConfig {
            schema: SchemaConfig::Path("schema.graphql".to_string()),
            documents: None,
            include: None,
            exclude: None,
            extensions: Some(extensions),
        };
        let extract_config = get_extract_config(&config);
        assert_eq!(extract_config.magic_comment, "CustomGraphQL");
        assert_eq!(extract_config.tag_identifiers, vec!["gql", "customTag"]);
        assert_eq!(extract_config.modules, vec!["custom-module"]);
        assert!(extract_config.allow_global_identifiers);
    }

    #[test]
    fn test_get_extract_config_partial() {
        let mut extensions = std::collections::HashMap::new();
        extensions.insert(
            "extractConfig".to_string(),
            serde_json::json!({
                "allowGlobalIdentifiers": true
            }),
        );
        let config = ProjectConfig {
            schema: SchemaConfig::Path("schema.graphql".to_string()),
            documents: None,
            include: None,
            exclude: None,
            extensions: Some(extensions),
        };
        let extract_config = get_extract_config(&config);
        assert_eq!(extract_config.magic_comment, "GraphQL");
        assert_eq!(extract_config.tag_identifiers, vec!["gql", "graphql"]);
        assert!(extract_config.allow_global_identifiers);
    }
}
