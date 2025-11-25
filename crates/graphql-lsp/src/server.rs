use apollo_compiler::validation::DiagnosticList;
use dashmap::DashMap;
use graphql_config::{find_config, load_config};
use graphql_extract::ExtractConfig;
use graphql_project::GraphQLProject;
use lsp_types::{
    CompletionOptions, CompletionParams, CompletionResponse, Diagnostic, DiagnosticSeverity,
    DidChangeTextDocumentParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams,
    DidSaveTextDocumentParams, DocumentSymbolParams, DocumentSymbolResponse, GotoDefinitionParams,
    GotoDefinitionResponse, Hover, HoverParams, HoverProviderCapability, InitializeParams,
    InitializeResult, InitializedParams, Location, MessageType, OneOf, Position, Range,
    ReferenceParams, ServerCapabilities, ServerInfo, SymbolInformation, TextDocumentSyncCapability,
    TextDocumentSyncKind, Uri, WorkspaceSymbol, WorkspaceSymbolParams,
};
use std::path::PathBuf;
use std::sync::Arc;
use tower_lsp_server::jsonrpc::Result;
use tower_lsp_server::{Client, LanguageServer, UriExt};

pub struct GraphQLLanguageServer {
    client: Client,
    /// Workspace folders from initialization (stored temporarily until we load configs)
    init_workspace_folders: Arc<DashMap<String, PathBuf>>,
    /// Workspace roots indexed by workspace folder URI string
    workspace_roots: Arc<DashMap<String, PathBuf>>,
    /// GraphQL projects by workspace URI -> Vec<(`project_name`, project)>
    projects: Arc<DashMap<String, Vec<(String, GraphQLProject)>>>,
}

impl GraphQLLanguageServer {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            init_workspace_folders: Arc::new(DashMap::new()),
            workspace_roots: Arc::new(DashMap::new()),
            projects: Arc::new(DashMap::new()),
        }
    }

    /// Load GraphQL config from a workspace folder
    async fn load_workspace_config(&self, workspace_uri: &str, workspace_path: &PathBuf) {
        tracing::info!("Loading GraphQL config from {:?}", workspace_path);

        // Find graphql config
        match find_config(workspace_path) {
            Ok(Some(config_path)) => {
                tracing::info!("Found GraphQL config at {:?}", config_path);

                // Load the config
                match load_config(&config_path) {
                    Ok(config) => {
                        // Create projects from config
                        match GraphQLProject::from_config_with_base(&config, workspace_path) {
                            Ok(projects) => {
                                tracing::info!("Loaded {} GraphQL project(s)", projects.len());

                                // Load schemas and documents for all projects
                                for (name, project) in &projects {
                                    if let Err(e) = project.load_schema().await {
                                        tracing::error!(
                                            "Failed to load schema for project '{name}': {e}"
                                        );
                                        self.client
                                            .log_message(
                                                MessageType::ERROR,
                                                format!("Failed to load schema for project '{name}': {e}"),
                                            )
                                            .await;
                                    } else {
                                        tracing::info!("Loaded schema for project '{}'", name);
                                    }

                                    // Load documents to index all fragments
                                    if let Err(e) = project.load_documents() {
                                        tracing::error!(
                                            "Failed to load documents for project '{name}': {e}"
                                        );
                                        self.client
                                            .log_message(
                                                MessageType::WARNING,
                                                format!("Failed to load documents for project '{name}': {e}"),
                                            )
                                            .await;
                                    } else {
                                        let doc_index = project.get_document_index();
                                        tracing::info!(
                                            "Loaded documents for project '{}': {} operations, {} fragments",
                                            name,
                                            doc_index.operations.len(),
                                            doc_index.fragments.len()
                                        );
                                    }
                                }

                                // Store workspace and projects
                                self.workspace_roots
                                    .insert(workspace_uri.to_string(), workspace_path.clone());
                                self.projects.insert(workspace_uri.to_string(), projects);

                                self.client
                                    .log_message(
                                        MessageType::INFO,
                                        "GraphQL config loaded successfully",
                                    )
                                    .await;
                            }
                            Err(e) => {
                                tracing::error!("Failed to create projects from config: {e}");
                                self.client
                                    .log_message(
                                        MessageType::ERROR,
                                        format!("Failed to load GraphQL projects: {e}"),
                                    )
                                    .await;
                            }
                        }
                    }
                    Err(e) => {
                        tracing::error!("Failed to load config: {e}");
                        self.client
                            .log_message(
                                MessageType::ERROR,
                                format!("Failed to parse GraphQL config: {e}"),
                            )
                            .await;
                    }
                }
            }
            Ok(None) => {
                tracing::warn!("No GraphQL config found in workspace");
                self.client
                    .log_message(
                        MessageType::WARNING,
                        "No graphql.config found. Place a graphql.config.yaml in your workspace root.",
                    )
                    .await;
            }
            Err(e) => {
                tracing::error!("Error searching for config: {}", e);
            }
        }
    }

    /// Find the workspace and project for a given document URI
    fn find_workspace_and_project(&self, document_uri: &Uri) -> Option<(String, usize)> {
        let doc_path = document_uri.to_file_path()?;

        // Try to find which workspace this document belongs to
        for workspace_entry in self.workspace_roots.iter() {
            let workspace_uri = workspace_entry.key();
            let workspace_path = workspace_entry.value();

            if doc_path.as_ref().starts_with(workspace_path.as_path()) {
                // Found the workspace, return the workspace URI and project index (0 for now)
                // TODO: Match document to correct project based on includes/excludes
                return Some((workspace_uri.clone(), 0));
            }
        }

        None
    }

    /// Validate a document and publish diagnostics
    async fn validate_document(&self, uri: Uri, content: &str) {
        let Some((workspace_uri, project_idx)) = self.find_workspace_and_project(&uri) else {
            tracing::warn!("No project found for document: {:?}", uri);
            return;
        };

        // Get the project from the workspace
        let Some(projects) = self.projects.get(&workspace_uri) else {
            tracing::warn!("No projects loaded for workspace: {workspace_uri}");
            return;
        };

        let Some((_, project)) = projects.get(project_idx) else {
            tracing::warn!("Project index {project_idx} not found in workspace {workspace_uri}");
            return;
        };

        let file_path = uri.to_file_path();

        // Check if this is a schema file - schema files shouldn't be validated as executable documents
        if let Some(ref path) = file_path {
            if project.is_schema_file(path.as_ref()) {
                tracing::debug!("Skipping validation for schema file: {:?}", uri);
                // Clear any existing diagnostics
                self.client.publish_diagnostics(uri, vec![], None).await;
                return;
            }
        }

        // Check if this is a TypeScript/JavaScript file
        let is_ts_js = file_path
            .as_ref()
            .and_then(|path| {
                path.extension()
                    .and_then(|ext| ext.to_str())
                    .map(|ext| matches!(ext, "ts" | "tsx" | "js" | "jsx"))
            })
            .unwrap_or(false);

        let diagnostics = if is_ts_js {
            self.validate_typescript_document(&uri, content, project)
        } else {
            self.validate_graphql_document(content, project)
        };

        self.client
            .publish_diagnostics(uri, diagnostics, None)
            .await;
    }

    /// Validate a pure GraphQL document
    #[must_use]
    #[allow(clippy::cast_possible_truncation)]
    fn validate_graphql_document(
        &self,
        content: &str,
        project: &GraphQLProject,
    ) -> Vec<Diagnostic> {
        use apollo_compiler::{ExecutableDocument, parser::Parser};
        use apollo_compiler::validation::Valid;

        // Fragment-only documents should still be validated for schema correctness
        // (e.g., invalid fields, wrong types), just not for "unused fragment" warnings

        // Get the schema
        let schema_index = project.get_schema_index();
        let schema = schema_index.schema();
        let valid_schema = Valid::assume_valid_ref(schema);

        // Build an executable document
        let mut builder = ExecutableDocument::builder(Some(valid_schema));
        let is_fragment_only = self.is_fragment_only(content);

        // Add the current document
        Parser::new().parse_into_executable_builder(
            Some(valid_schema),
            content,
            "document.graphql",
            &mut builder,
        );

        // Only add project fragments if this document contains operations
        // Standalone fragments should be validated on their own, not with other fragments
        if !is_fragment_only {
            let document_index = project.get_document_index();
            for frag_info in document_index.fragments.values() {
                if let Ok(frag_extracted) = graphql_extract::extract_from_file(
                    std::path::Path::new(&frag_info.file_path),
                    &ExtractConfig::default(),
                ) {
                    for frag_item in frag_extracted {
                        if frag_item.source.trim_start().starts_with("fragment") {
                            Parser::new().parse_into_executable_builder(
                                Some(valid_schema),
                                &frag_item.source,
                                &frag_info.file_path,
                                &mut builder,
                            );
                        }
                    }
                }
            }
        }

        // Build and validate
        let mut diagnostics = match builder.build() {
            Ok(doc) => {
                // Successfully built, now validate
                match doc.validate(valid_schema) {
                    Ok(_) => vec![],
                    Err(with_errors) => {
                        let mut diags = self.convert_diagnostics(&with_errors.errors);
                        // Filter out "unused fragment" warnings for fragment-only documents
                        if is_fragment_only {
                            diags.retain(|d| {
                                !d.message.contains("unused") && !d.message.contains("never used")
                            });
                        }
                        diags
                    }
                }
            }
            Err(with_errors) => {
                // Build errors (e.g., undefined fragments, type errors)
                let mut diags = self.convert_diagnostics(&with_errors.errors);
                // Filter out "unused fragment" warnings for fragment-only documents
                if is_fragment_only {
                    diags.retain(|d| {
                        !d.message.contains("unused") && !d.message.contains("never used")
                    });
                }
                diags
            }
        };

        // Check for deprecated field usage
        let validator = graphql_project::Validator::new();
        let deprecation_warnings =
            validator.check_deprecated_fields_custom(content, &schema_index, "document.graphql");

        // Convert deprecation warnings to LSP diagnostics
        for warning in deprecation_warnings {
            diagnostics.push(Diagnostic {
                range: Range {
                    start: Position {
                        line: warning.range.start.line as u32,
                        character: warning.range.start.character as u32,
                    },
                    end: Position {
                        line: warning.range.end.line as u32,
                        character: warning.range.end.character as u32,
                    },
                },
                severity: Some(DiagnosticSeverity::WARNING),
                code: warning.code.map(lsp_types::NumberOrString::String),
                source: Some(warning.source),
                message: warning.message,
                ..Default::default()
            });
        }

        diagnostics
    }

    /// Check if a document contains only fragments (no operations)
    fn is_fragment_only(&self, content: &str) -> bool {
        let trimmed = content.trim();
        trimmed.starts_with("fragment")
            && !trimmed.contains("query")
            && !trimmed.contains("mutation")
            && !trimmed.contains("subscription")
    }

    /// Validate GraphQL embedded in TypeScript/JavaScript
    #[must_use]
    #[allow(clippy::cast_possible_truncation)]
    fn validate_typescript_document(
        &self,
        uri: &Uri,
        content: &str,
        project: &GraphQLProject,
    ) -> Vec<Diagnostic> {
        use apollo_compiler::{ExecutableDocument, parser::Parser};
        use apollo_compiler::validation::Valid;
        use std::io::Write;

        // Get the file extension from the original URI to preserve it in the temp file
        let extension = uri
            .to_file_path()
            .and_then(|path| {
                path.extension()
                    .and_then(|ext| ext.to_str())
                    .map(String::from)
            })
            .unwrap_or_else(|| "tsx".to_string());

        let temp_file = match tempfile::Builder::new()
            .suffix(&format!(".{extension}"))
            .tempfile()
        {
            Ok(mut file) => {
                if file.write_all(content.as_bytes()).is_err() {
                    return vec![];
                }
                file
            }
            Err(_) => return vec![],
        };

        // Extract GraphQL from TypeScript/JavaScript
        let extracted =
            match graphql_extract::extract_from_file(temp_file.path(), &ExtractConfig::default()) {
                Ok(extracted) => extracted,
                Err(e) => {
                    tracing::error!("Failed to extract GraphQL from {:?}: {}", uri, e);
                    return vec![];
                }
            };

        if extracted.is_empty() {
            return vec![];
        }

        tracing::info!(
            "Extracted {} GraphQL document(s) from {:?}",
            extracted.len(),
            uri
        );

        // Get the schema
        let schema_index = project.get_schema_index();
        let schema = schema_index.schema();
        let valid_schema = Valid::assume_valid_ref(schema);

        let mut all_diagnostics = Vec::new();

        // Collect all fragments from this file for use by operations
        let fragments_in_current_file: Vec<_> = extracted
            .iter()
            .filter(|item| self.is_fragment_only(&item.source))
            .collect();

        // Validate each extracted document
        for item in &extracted {
            let line_offset = item.location.range.start.line;
            let source = &item.source;

            // Fragment-only documents should still be validated for schema correctness
            // (e.g., invalid fields, wrong types), just not for "unused fragment" warnings

            // Build an executable document
            let mut builder = ExecutableDocument::builder(Some(valid_schema));
            let is_fragment_only = self.is_fragment_only(source);

            // Add the current document with line offset padding for correct diagnostics
            let padded_source = if line_offset > 0 {
                format!("{}{}", "\n".repeat(line_offset), source)
            } else {
                source.to_string()
            };

            Parser::new().parse_into_executable_builder(
                Some(valid_schema),
                &padded_source,
                &uri.to_string(),
                &mut builder,
            );

            // Only add fragments if this document contains operations
            // Standalone fragments should be validated on their own, not with other fragments
            if !is_fragment_only {
                // First, add fragments from the current file (without padding)
                for frag_item in &fragments_in_current_file {
                    Parser::new().parse_into_executable_builder(
                        Some(valid_schema),
                        &frag_item.source,
                        &uri.to_string(),
                        &mut builder,
                    );
                }

                // Then add fragments from other files in the project
                let document_index = project.get_document_index();
                let current_file_path = uri.to_file_path();

                for frag_info in document_index.fragments.values() {
                    // Skip fragments from the current file - they're already added above
                    if let Some(ref current_path) = current_file_path {
                        if std::path::Path::new(&frag_info.file_path) == current_path.as_ref() {
                            continue;
                        }
                    }

                    if let Ok(frag_extracted) = graphql_extract::extract_from_file(
                        std::path::Path::new(&frag_info.file_path),
                        &ExtractConfig::default(),
                    ) {
                        for frag_item in frag_extracted {
                            if frag_item.source.trim_start().starts_with("fragment") {
                                Parser::new().parse_into_executable_builder(
                                    Some(valid_schema),
                                    &frag_item.source,
                                    &frag_info.file_path,
                                    &mut builder,
                                );
                            }
                        }
                    }
                }
            }

            // Build and validate
            match builder.build() {
                Ok(doc) => {
                    // Successfully built, now validate
                    match doc.validate(valid_schema) {
                        Ok(_) => {}
                        Err(with_errors) => {
                            let mut diags = self.convert_diagnostics(&with_errors.errors);

                            // Filter out "unused fragment" warnings for fragment-only documents
                            if is_fragment_only {
                                diags.retain(|d| {
                                    !d.message.contains("unused") && !d.message.contains("never used")
                                });
                            } else {
                                // For operations: filter out errors from fragments (they're validated separately)
                                // Keep only errors within the current operation's line range
                                let source_line_count = source.lines().count();
                                diags.retain(|d| {
                                    let start_line = d.range.start.line as usize;
                                    start_line >= line_offset && start_line < line_offset + source_line_count
                                });
                            }

                            all_diagnostics.extend(diags);
                        }
                    }
                }
                Err(with_errors) => {
                    // Build errors (e.g., undefined fragments, type errors)
                    let mut diags = self.convert_diagnostics(&with_errors.errors);

                    // Filter out "unused fragment" warnings for fragment-only documents
                    if is_fragment_only {
                        diags.retain(|d| {
                            !d.message.contains("unused") && !d.message.contains("never used")
                        });
                    } else {
                        // For operations: filter out errors from fragments (they're validated separately)
                        // Keep only errors within the current operation's line range
                        let source_line_count = source.lines().count();
                        diags.retain(|d| {
                            let start_line = d.range.start.line as usize;
                            start_line >= line_offset && start_line < line_offset + source_line_count
                        });
                    }

                    all_diagnostics.extend(diags);
                }
            }

            // Check for deprecated field usage
            let validator = graphql_project::Validator::new();
            let deprecation_warnings = validator.check_deprecated_fields_custom(
                source,
                &schema_index,
                &uri.to_string(),
            );

            // Convert deprecation warnings to LSP diagnostics
            for warning in deprecation_warnings {
                all_diagnostics.push(Diagnostic {
                    range: Range {
                        start: Position {
                            line: (line_offset + warning.range.start.line) as u32,
                            character: warning.range.start.character as u32,
                        },
                        end: Position {
                            line: (line_offset + warning.range.end.line) as u32,
                            character: warning.range.end.character as u32,
                        },
                    },
                    severity: Some(DiagnosticSeverity::WARNING),
                    code: warning.code.map(lsp_types::NumberOrString::String),
                    source: Some(warning.source),
                    message: warning.message,
                    ..Default::default()
                });
            }
        }

        all_diagnostics
    }

    /// Convert apollo-compiler diagnostics to LSP diagnostics
    #[must_use]
    #[allow(clippy::cast_possible_truncation)]
    #[allow(clippy::unused_self)]
    fn convert_diagnostics(&self, diagnostic_list: &DiagnosticList) -> Vec<Diagnostic> {
        diagnostic_list
            .iter()
            .map(|diag| {
                let range = diag.line_column_range().map_or(
                    Range {
                        start: Position {
                            line: 0,
                            character: 0,
                        },
                        end: Position {
                            line: 0,
                            character: 1,
                        },
                    },
                    |loc_range| {
                        // apollo-compiler uses 1-based, LSP uses 0-based
                        // We allow cast_possible_truncation because line/column numbers
                        // in source files are unlikely to exceed u32::MAX
                        Range {
                            start: Position {
                                line: loc_range.start.line.saturating_sub(1) as u32,
                                character: loc_range.start.column.saturating_sub(1) as u32,
                            },
                            end: Position {
                                line: loc_range.end.line.saturating_sub(1) as u32,
                                character: loc_range.end.column.saturating_sub(1) as u32,
                            },
                        }
                    },
                );

                Diagnostic {
                    range,
                    severity: Some(DiagnosticSeverity::ERROR),
                    code: None,
                    source: Some("graphql".to_string()),
                    message: diag.error.to_string(),
                    ..Default::default()
                }
            })
            .collect()
    }
}

impl LanguageServer for GraphQLLanguageServer {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        tracing::info!("Initializing GraphQL Language Server");

        // Store workspace folders for later config loading
        if let Some(ref folders) = params.workspace_folders {
            tracing::info!("Workspace folders: {} folders", folders.len());
            for folder in folders {
                if let Some(path) = folder.uri.to_file_path() {
                    self.init_workspace_folders
                        .insert(folder.uri.to_string(), path.into_owned());
                }
            }
        }

        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                completion_provider: Some(CompletionOptions {
                    trigger_characters: Some(vec![
                        ".".to_string(),
                        "{".to_string(),
                        "@".to_string(),
                    ]),
                    ..Default::default()
                }),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                definition_provider: Some(OneOf::Left(true)),
                references_provider: Some(OneOf::Left(true)),
                document_symbol_provider: Some(OneOf::Left(true)),
                workspace_symbol_provider: Some(OneOf::Left(true)),
                ..Default::default()
            },
            server_info: Some(ServerInfo {
                name: "GraphQL Language Server".to_string(),
                version: Some(env!("CARGO_PKG_VERSION").to_string()),
            }),
        })
    }

    async fn initialized(&self, _params: InitializedParams) {
        tracing::info!("GraphQL Language Server initialized");
        self.client
            .log_message(MessageType::INFO, "GraphQL LSP initialized")
            .await;

        // Load GraphQL config from workspace folders we stored during initialize
        let folders: Vec<_> = self
            .init_workspace_folders
            .iter()
            .map(|entry| (entry.key().clone(), entry.value().clone()))
            .collect();

        for (uri, path) in folders {
            self.load_workspace_config(&uri, &path).await;
        }
    }

    async fn shutdown(&self) -> Result<()> {
        tracing::info!("Shutting down GraphQL Language Server");
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let uri = params.text_document.uri;
        let content = params.text_document.text;
        tracing::info!("Document opened: {:?}", uri);

        self.validate_document(uri, &content).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri;
        tracing::debug!("Document changed: {:?}", uri);

        // Get the latest content from changes (full sync mode)
        for change in params.content_changes {
            self.validate_document(uri.clone(), &change.text).await;
        }
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        tracing::info!("Document saved: {:?}", params.text_document.uri);
        // Re-validation happens automatically through did_change
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        tracing::info!("Document closed: {:?}", params.text_document.uri);
        // Clear diagnostics
        self.client
            .publish_diagnostics(params.text_document.uri, vec![], None)
            .await;
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        tracing::debug!(
            "Completion requested: {:?}",
            params.text_document_position.text_document.uri
        );
        // TODO: Implement autocompletion
        Ok(None)
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        tracing::debug!(
            "Hover requested: {:?}",
            params.text_document_position_params.text_document.uri
        );
        // TODO: Implement hover information
        Ok(None)
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        tracing::debug!(
            "Go to definition requested: {:?}",
            params.text_document_position_params.text_document.uri
        );
        // TODO: Implement go-to-definition
        Ok(None)
    }

    async fn references(&self, params: ReferenceParams) -> Result<Option<Vec<Location>>> {
        tracing::debug!(
            "References requested: {:?}",
            params.text_document_position.text_document.uri
        );
        // TODO: Implement find references
        Ok(None)
    }

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        tracing::debug!("Document symbols requested: {:?}", params.text_document.uri);
        // TODO: Implement document symbols
        Ok(None)
    }

    async fn symbol(
        &self,
        params: WorkspaceSymbolParams,
    ) -> Result<Option<OneOf<Vec<SymbolInformation>, Vec<WorkspaceSymbol>>>> {
        tracing::debug!("Workspace symbols requested: {}", params.query);
        // TODO: Implement workspace symbols
        Ok(None)
    }
}
