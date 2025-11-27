use crate::{
    CompletionItem, CompletionProvider, DefinitionLocation, Diagnostic, DocumentIndex,
    DocumentLoader, FindReferencesProvider, GotoDefinitionProvider, HoverInfo, HoverProvider,
    Position, ReferenceLocation, Result, SchemaIndex, SchemaLoader, Validator,
};
use apollo_compiler::validation::DiagnosticList;
use graphql_config::{GraphQLConfig, ProjectConfig};
use std::sync::{Arc, RwLock};

/// Main project structure that manages schema, documents, and validation
pub struct GraphQLProject {
    config: ProjectConfig,
    base_dir: Option<std::path::PathBuf>,
    schema_index: Arc<RwLock<SchemaIndex>>,
    document_index: Arc<RwLock<DocumentIndex>>,
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
        }
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
        use graphql_extract::{extract_from_source, ExtractConfig, Language};
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
        let extracted = extract_from_source(content, language, &ExtractConfig::default())
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

        // Only add project fragments if this document contains operations AND uses fragment spreads
        if !is_fragment_only && source.contains("...") {
            let document_index = self.document_index.read().unwrap();
            for frag_infos in document_index.fragments.values() {
                // Process all fragments with this name (in case of duplicates)
                for frag_info in frag_infos {
                    if let Ok(frag_extracted) = graphql_extract::extract_from_file(
                        std::path::Path::new(&frag_info.file_path),
                        &graphql_extract::ExtractConfig::default(),
                    ) {
                        for frag_item in frag_extracted {
                            if frag_item.source.trim_start().starts_with("fragment") {
                                Parser::new().parse_into_executable_builder(
                                    &frag_item.source,
                                    &frag_info.file_path,
                                    &mut builder,
                                );
                            }
                        }
                    }
                }
            }
        }

        // Build and validate
        let doc = builder.build();
        let mut diagnostics = if errors.is_empty() {
            match doc.validate(valid_schema) {
                Ok(_) => vec![],
                Err(with_errors) => {
                    Self::convert_compiler_diagnostics(&with_errors.errors, is_fragment_only)
                }
            }
        } else {
            Self::convert_compiler_diagnostics(&errors, is_fragment_only)
        };

        // Add deprecation warnings
        let validator = Validator::new();
        let deprecation_warnings =
            validator.check_deprecated_fields_custom(source, &schema_index, file_name);
        diagnostics.extend(deprecation_warnings);

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

            // Only add fragments if this document contains operations AND uses fragment spreads
            if !is_fragment_only && source.contains("...") {
                // Add fragments from OTHER files (not current file)
                // Fragments in the current file will be validated separately
                let document_index = self.document_index.read().unwrap();
                let current_path = std::path::Path::new(file_path);

                for frag_infos in document_index.fragments.values() {
                    // Process all fragments with this name (in case of duplicates)
                    for frag_info in frag_infos {
                        // Skip fragments from the current file
                        if std::path::Path::new(&frag_info.file_path) == current_path {
                            continue;
                        }

                        if let Ok(frag_extracted) = graphql_extract::extract_from_file(
                            std::path::Path::new(&frag_info.file_path),
                            &graphql_extract::ExtractConfig::default(),
                        ) {
                            for frag_item in frag_extracted {
                                if frag_item.source.trim_start().starts_with("fragment") {
                                    Parser::new().parse_into_executable_builder(
                                        &frag_item.source,
                                        &frag_info.file_path,
                                        &mut builder,
                                    );
                                }
                            }
                        }
                    }
                }
            }

            // Build and validate
            let doc = builder.build();
            let mut diagnostics = if errors.is_empty() {
                match doc.validate(valid_schema) {
                    Ok(_) => vec![],
                    Err(with_errors) => {
                        let mut diags = Self::convert_compiler_diagnostics(
                            &with_errors.errors,
                            is_fragment_only,
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
                let mut diags = Self::convert_compiler_diagnostics(&errors, is_fragment_only);

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

    /// Convert apollo-compiler diagnostics to our diagnostic format
    fn convert_compiler_diagnostics(
        compiler_diags: &apollo_compiler::validation::DiagnosticList,
        is_fragment_only: bool,
    ) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();

        for diag in compiler_diags.iter() {
            let message = diag.error.to_string();

            // Skip "unused fragment" and "must be used" errors for fragment-only documents
            if is_fragment_only {
                let message_lower = message.to_lowercase();
                if message_lower.contains("unused")
                    || message_lower.contains("never used")
                    || message_lower.contains("must be used")
                {
                    continue;
                }
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
        let document_index = self.document_index.read().unwrap();
        let schema_index = self.schema_index.read().unwrap();
        let provider = FindReferencesProvider::new();
        provider.find_references(
            source,
            position,
            &document_index,
            &schema_index,
            all_documents,
            include_declaration,
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
}
