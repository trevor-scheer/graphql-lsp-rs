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
    /// Document content cache indexed by URI string
    document_cache: Arc<DashMap<String, String>>,
}

impl GraphQLLanguageServer {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            init_workspace_folders: Arc::new(DashMap::new()),
            workspace_roots: Arc::new(DashMap::new()),
            projects: Arc::new(DashMap::new()),
            document_cache: Arc::new(DashMap::new()),
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

        // Get the project from the workspace (need mutable to reload documents)
        let Some(mut projects) = self.projects.get_mut(&workspace_uri) else {
            tracing::warn!("No projects loaded for workspace: {workspace_uri}");
            return;
        };

        let Some((_, project)) = projects.get_mut(project_idx) else {
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

        // Update the document index for this specific file with in-memory content
        // This is more efficient than reloading all documents from disk and
        // ensures we use the latest editor content even before it's saved
        if let Some(path) = uri.to_file_path() {
            let file_path_str = path.display().to_string();
            if let Err(e) = project.update_document_index(&file_path_str, content) {
                tracing::warn!(
                    "Failed to update document index for {}: {}",
                    file_path_str,
                    e
                );
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

        // Get document-specific diagnostics (type errors, etc.)
        let mut diagnostics = if is_ts_js {
            self.validate_typescript_document(&uri, content, project)
        } else {
            self.validate_graphql_document(content, project)
        };

        // Add project-wide duplicate name diagnostics for this file
        if let Some(path) = uri.to_file_path() {
            let file_path_str = path.display().to_string();
            let project_wide_diags = self.get_project_wide_diagnostics(&file_path_str, project);
            diagnostics.extend(project_wide_diags);
        }

        self.client
            .publish_diagnostics(uri.clone(), diagnostics, None)
            .await;
    }

    /// Get project-wide duplicate name diagnostics for a specific file
    fn get_project_wide_diagnostics(
        &self,
        file_path: &str,
        project: &GraphQLProject,
    ) -> Vec<Diagnostic> {
        // Get the document index
        let document_index = project.get_document_index();

        // Check for duplicate names across the project
        let duplicate_diagnostics = document_index.check_duplicate_names();

        // Filter to only diagnostics for this file and convert to LSP diagnostics
        duplicate_diagnostics
            .into_iter()
            .filter(|(path, _)| path == file_path)
            .map(|(_, diag)| self.convert_project_diagnostic(diag))
            .collect()
    }

    /// Validate a pure GraphQL document
    #[must_use]
    #[allow(clippy::cast_possible_truncation)]
    fn validate_graphql_document(
        &self,
        content: &str,
        project: &GraphQLProject,
    ) -> Vec<Diagnostic> {
        // Use the centralized validation logic from graphql-project
        let project_diagnostics = project.validate_document_source(content, "document.graphql");

        // Convert graphql-project diagnostics to LSP diagnostics
        project_diagnostics
            .into_iter()
            .map(|d| self.convert_project_diagnostic(d))
            .collect()
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

        // Use the centralized validation logic from graphql-project
        let file_path = uri.to_string();
        let project_diagnostics = project.validate_extracted_documents(&extracted, &file_path);

        // Convert graphql-project diagnostics to LSP diagnostics
        project_diagnostics
            .into_iter()
            .map(|d| self.convert_project_diagnostic(d))
            .collect()
    }

    /// Convert graphql-project diagnostic to LSP diagnostic
    #[must_use]
    #[allow(clippy::cast_possible_truncation)]
    #[allow(clippy::unused_self)]
    fn convert_project_diagnostic(&self, diag: graphql_project::Diagnostic) -> Diagnostic {
        use graphql_project::Severity;

        let severity = match diag.severity {
            Severity::Error => DiagnosticSeverity::ERROR,
            Severity::Warning => DiagnosticSeverity::WARNING,
            Severity::Information => DiagnosticSeverity::INFORMATION,
            Severity::Hint => DiagnosticSeverity::HINT,
        };

        Diagnostic {
            range: Range {
                start: Position {
                    line: diag.range.start.line as u32,
                    character: diag.range.start.character as u32,
                },
                end: Position {
                    line: diag.range.end.line as u32,
                    character: diag.range.end.character as u32,
                },
            },
            severity: Some(severity),
            code: diag.code.map(lsp_types::NumberOrString::String),
            source: Some(diag.source),
            message: diag.message,
            ..Default::default()
        }
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
                    trigger_characters: Some(vec!["{".to_string(), "@".to_string()]),
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

        // Cache the document content
        self.document_cache.insert(uri.to_string(), content.clone());

        self.validate_document(uri, &content).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri;
        tracing::debug!("Document changed: {:?}", uri);

        // Get the latest content from changes (full sync mode)
        for change in params.content_changes {
            // Update the document cache
            self.document_cache
                .insert(uri.to_string(), change.text.clone());

            self.validate_document(uri.clone(), &change.text).await;
        }
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        tracing::info!("Document saved: {:?}", params.text_document.uri);
        // Re-validation happens automatically through did_change
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        tracing::info!("Document closed: {:?}", params.text_document.uri);

        // Remove from document cache
        self.document_cache
            .remove(&params.text_document.uri.to_string());

        // Clear diagnostics
        self.client
            .publish_diagnostics(params.text_document.uri, vec![], None)
            .await;
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let uri = params.text_document_position.text_document.uri;
        let lsp_position = params.text_document_position.position;

        tracing::debug!("Completion requested: {:?} at {:?}", uri, lsp_position);

        let Some(content) = self.document_cache.get(&uri.to_string()) else {
            tracing::warn!("No cached content for document: {:?}", uri);
            return Ok(None);
        };

        let Some((workspace_uri, project_idx)) = self.find_workspace_and_project(&uri) else {
            tracing::warn!("No project found for document: {:?}", uri);
            return Ok(None);
        };

        let Some(projects) = self.projects.get(&workspace_uri) else {
            tracing::warn!("No projects loaded for workspace: {workspace_uri}");
            return Ok(None);
        };

        let Some((_, project)) = projects.get(project_idx) else {
            tracing::warn!("Project index {project_idx} not found in workspace {workspace_uri}");
            return Ok(None);
        };

        let position = graphql_project::Position {
            line: lsp_position.line as usize,
            character: lsp_position.character as usize,
        };

        let file_path = uri.to_string();
        let Some(items) = project.complete(&content, position, &file_path) else {
            return Ok(None);
        };

        let lsp_items: Vec<lsp_types::CompletionItem> = items
            .into_iter()
            .map(|item| {
                let kind = match item.kind {
                    graphql_project::CompletionItemKind::Field => {
                        Some(lsp_types::CompletionItemKind::FIELD)
                    }
                    graphql_project::CompletionItemKind::Type => {
                        Some(lsp_types::CompletionItemKind::CLASS)
                    }
                    graphql_project::CompletionItemKind::Fragment => {
                        Some(lsp_types::CompletionItemKind::SNIPPET)
                    }
                    graphql_project::CompletionItemKind::Operation => {
                        Some(lsp_types::CompletionItemKind::FUNCTION)
                    }
                    graphql_project::CompletionItemKind::Directive => {
                        Some(lsp_types::CompletionItemKind::KEYWORD)
                    }
                    graphql_project::CompletionItemKind::EnumValue => {
                        Some(lsp_types::CompletionItemKind::ENUM_MEMBER)
                    }
                    graphql_project::CompletionItemKind::Argument => {
                        Some(lsp_types::CompletionItemKind::PROPERTY)
                    }
                    graphql_project::CompletionItemKind::Variable => {
                        Some(lsp_types::CompletionItemKind::VARIABLE)
                    }
                };

                let documentation = item.documentation.map(|doc| {
                    lsp_types::Documentation::MarkupContent(lsp_types::MarkupContent {
                        kind: lsp_types::MarkupKind::Markdown,
                        value: doc,
                    })
                });

                lsp_types::CompletionItem {
                    label: item.label,
                    kind,
                    detail: item.detail,
                    documentation,
                    deprecated: Some(item.deprecated),
                    insert_text: item.insert_text,
                    ..Default::default()
                }
            })
            .collect();

        Ok(Some(CompletionResponse::Array(lsp_items)))
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let uri = params.text_document_position_params.text_document.uri;
        let lsp_position = params.text_document_position_params.position;

        tracing::debug!("Hover requested: {:?} at {:?}", uri, lsp_position);

        // Get the cached document content
        let Some(content) = self.document_cache.get(&uri.to_string()) else {
            tracing::warn!("No cached content for document: {:?}", uri);
            return Ok(None);
        };

        // Find the workspace and project for this document
        let Some((workspace_uri, project_idx)) = self.find_workspace_and_project(&uri) else {
            tracing::warn!("No project found for document: {:?}", uri);
            return Ok(None);
        };

        // Get the project
        let Some(projects) = self.projects.get(&workspace_uri) else {
            tracing::warn!("No projects loaded for workspace: {workspace_uri}");
            return Ok(None);
        };

        let Some((_, project)) = projects.get(project_idx) else {
            tracing::warn!("Project index {project_idx} not found in workspace {workspace_uri}");
            return Ok(None);
        };

        // Convert LSP position to graphql-project Position (0-indexed)
        let position = graphql_project::Position {
            line: lsp_position.line as usize,
            character: lsp_position.character as usize,
        };

        // Get hover info from the project
        let file_path = uri.to_string();
        let Some(hover_info) = project.hover_info(&content, position, &file_path) else {
            return Ok(None);
        };

        // Convert to LSP Hover
        #[allow(clippy::cast_possible_truncation)]
        let hover = Hover {
            contents: lsp_types::HoverContents::Markup(lsp_types::MarkupContent {
                kind: lsp_types::MarkupKind::Markdown,
                value: hover_info.contents,
            }),
            range: hover_info.range.map(|r| Range {
                start: Position {
                    line: r.start.line as u32,
                    character: r.start.character as u32,
                },
                end: Position {
                    line: r.end.line as u32,
                    character: r.end.character as u32,
                },
            }),
        };

        Ok(Some(hover))
    }

    #[allow(clippy::too_many_lines)]
    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let uri = params.text_document_position_params.text_document.uri;
        let lsp_position = params.text_document_position_params.position;

        tracing::info!(
            "Go to definition requested: {:?} at {:?}",
            uri,
            lsp_position
        );

        // Get the cached document content
        let Some(content) = self.document_cache.get(&uri.to_string()) else {
            tracing::warn!("No cached content for document: {:?}", uri);
            return Ok(None);
        };

        // Find the workspace and project for this document
        let Some((workspace_uri, project_idx)) = self.find_workspace_and_project(&uri) else {
            tracing::warn!("No project found for document: {:?}", uri);
            return Ok(None);
        };

        // Get the project
        let Some(projects) = self.projects.get(&workspace_uri) else {
            tracing::warn!("No projects loaded for workspace: {workspace_uri}");
            return Ok(None);
        };

        let Some((_, project)) = projects.get(project_idx) else {
            tracing::warn!("Project index {project_idx} not found in workspace {workspace_uri}");
            return Ok(None);
        };

        // Check if this is a TypeScript/JavaScript file that needs GraphQL extraction
        let file_path = uri.to_file_path();
        let (is_ts_file, language) =
            file_path
                .as_ref()
                .map_or((false, graphql_extract::Language::GraphQL), |path| {
                    path.extension().and_then(|e| e.to_str()).map_or(
                        (false, graphql_extract::Language::GraphQL),
                        |ext| match ext {
                            "ts" | "tsx" => (true, graphql_extract::Language::TypeScript),
                            "js" | "jsx" => (true, graphql_extract::Language::JavaScript),
                            _ => (false, graphql_extract::Language::GraphQL),
                        },
                    )
                });

        if is_ts_file {
            // Extract GraphQL from TypeScript file
            let extracted = match graphql_extract::extract_from_source(
                &content,
                language,
                &ExtractConfig::default(),
            ) {
                Ok(extracted) => extracted,
                Err(e) => {
                    tracing::debug!("Failed to extract GraphQL from TypeScript file: {}", e);
                    return Ok(None);
                }
            };

            // Find which extracted GraphQL block contains the cursor position
            let cursor_line = lsp_position.line as usize;
            for item in extracted {
                let start_line = item.location.range.start.line;
                let end_line = item.location.range.end.line;

                if cursor_line >= start_line && cursor_line <= end_line {
                    // Adjust position relative to the extracted GraphQL
                    #[allow(clippy::cast_possible_truncation)]
                    let relative_position = graphql_project::Position {
                        line: cursor_line - start_line,
                        character: if cursor_line == start_line {
                            lsp_position
                                .character
                                .saturating_sub(item.location.range.start.column as u32)
                                as usize
                        } else {
                            lsp_position.character as usize
                        },
                    };

                    tracing::debug!(
                        "Adjusted position from {:?} to {:?} for extracted GraphQL",
                        lsp_position,
                        relative_position
                    );

                    // Get definition locations from the project using the extracted GraphQL
                    let Some(locations) =
                        project.goto_definition(&item.source, relative_position, &uri.to_string())
                    else {
                        tracing::debug!("No definition found at position {:?}", relative_position);
                        continue;
                    };

                    tracing::debug!("Found {} definition location(s)", locations.len());

                    // Convert to LSP Locations
                    #[allow(clippy::cast_possible_truncation)]
                    let lsp_locations: Vec<Location> = locations
                        .iter()
                        .filter_map(|loc| {
                            // Check if the file_path is already a URI
                            let file_uri = if loc.file_path.starts_with("file://") {
                                // Already a URI, parse it directly
                                loc.file_path.parse::<Uri>().ok()?
                            } else {
                                // Resolve the file path relative to the workspace if it's not absolute
                                let file_path =
                                    if std::path::Path::new(&loc.file_path).is_absolute() {
                                        std::path::PathBuf::from(&loc.file_path)
                                    } else {
                                        // Resolve relative to workspace root
                                        let workspace_path: Uri = workspace_uri.parse().ok()?;
                                        let workspace_file_path = workspace_path.to_file_path()?;
                                        workspace_file_path.join(&loc.file_path)
                                    };
                                Uri::from_file_path(file_path)?
                            };

                            // If the location is in the same file, adjust positions back to original file coordinates
                            let (start_line, start_char, end_line, end_char) = if file_uri == uri {
                                // Adjust positions back from extracted GraphQL to original file
                                let adjusted_start_line = loc.range.start.line + start_line;
                                let adjusted_start_char = if loc.range.start.line == 0 {
                                    loc.range.start.character + item.location.range.start.column
                                } else {
                                    loc.range.start.character
                                };
                                let adjusted_end_line = loc.range.end.line + start_line;
                                let adjusted_end_char = if loc.range.end.line == 0 {
                                    loc.range.end.character + item.location.range.start.column
                                } else {
                                    loc.range.end.character
                                };
                                (
                                    adjusted_start_line as u32,
                                    adjusted_start_char as u32,
                                    adjusted_end_line as u32,
                                    adjusted_end_char as u32,
                                )
                            } else {
                                (
                                    loc.range.start.line as u32,
                                    loc.range.start.character as u32,
                                    loc.range.end.line as u32,
                                    loc.range.end.character as u32,
                                )
                            };

                            Some(Location {
                                uri: file_uri,
                                range: Range {
                                    start: Position {
                                        line: start_line,
                                        character: start_char,
                                    },
                                    end: Position {
                                        line: end_line,
                                        character: end_char,
                                    },
                                },
                            })
                        })
                        .collect();

                    if !lsp_locations.is_empty() {
                        return Ok(Some(GotoDefinitionResponse::Array(lsp_locations)));
                    }
                }
            }

            return Ok(None);
        }

        // For pure GraphQL files, use the content as-is
        let position = graphql_project::Position {
            line: lsp_position.line as usize,
            character: lsp_position.character as usize,
        };

        tracing::info!(
            "Calling project.goto_definition with position: {:?}",
            position
        );
        tracing::info!("Content length: {} bytes", content.len());

        // Get definition locations from the project
        let Some(locations) = project.goto_definition(&content, position, &uri.to_string()) else {
            tracing::info!(
                "project.goto_definition returned None at position {:?}",
                position
            );
            return Ok(None);
        };

        tracing::info!("Found {} definition location(s)", locations.len());
        for (idx, loc) in locations.iter().enumerate() {
            tracing::info!(
                "Location {}: file={}, line={}, col={}",
                idx,
                loc.file_path,
                loc.range.start.line,
                loc.range.start.character
            );
        }

        // Convert to LSP Locations
        #[allow(clippy::cast_possible_truncation)]
        let lsp_locations: Vec<Location> = locations
            .iter()
            .filter_map(|loc| {
                // Check if the file_path is already a URI
                let file_uri = if loc.file_path.starts_with("file://") {
                    // Already a URI, parse it directly
                    loc.file_path.parse::<Uri>().ok()?
                } else {
                    // Resolve the file path relative to the workspace if it's not absolute
                    let file_path = if std::path::Path::new(&loc.file_path).is_absolute() {
                        std::path::PathBuf::from(&loc.file_path)
                    } else {
                        // Resolve relative to workspace root
                        let workspace_path: Uri = workspace_uri.parse().ok()?;
                        let workspace_file_path = workspace_path.to_file_path()?;
                        workspace_file_path.join(&loc.file_path)
                    };

                    tracing::info!("Resolved file path: {:?}", file_path);
                    Uri::from_file_path(&file_path)?
                };

                tracing::info!("Created URI: {:?}", file_uri);

                let lsp_loc = Location {
                    uri: file_uri,
                    range: Range {
                        start: Position {
                            line: loc.range.start.line as u32,
                            character: loc.range.start.character as u32,
                        },
                        end: Position {
                            line: loc.range.end.line as u32,
                            character: loc.range.end.character as u32,
                        },
                    },
                };
                tracing::info!(
                    "LSP Location: uri={:?}, range={:?}",
                    lsp_loc.uri,
                    lsp_loc.range
                );
                Some(lsp_loc)
            })
            .collect();

        if lsp_locations.is_empty() {
            Ok(None)
        } else {
            Ok(Some(GotoDefinitionResponse::Array(lsp_locations)))
        }
    }

    async fn references(&self, params: ReferenceParams) -> Result<Option<Vec<Location>>> {
        let uri = params.text_document_position.text_document.uri;
        let lsp_position = params.text_document_position.position;
        let include_declaration = params.context.include_declaration;

        tracing::info!(
            "Find references requested: {:?} at {:?} (include_declaration: {})",
            uri,
            lsp_position,
            include_declaration
        );

        // Get the cached document content
        let Some(content) = self.document_cache.get(&uri.to_string()) else {
            tracing::warn!("No cached content for document: {:?}", uri);
            return Ok(None);
        };

        // Find the workspace and project for this document
        let Some((workspace_uri, project_idx)) = self.find_workspace_and_project(&uri) else {
            tracing::warn!("No project found for document: {:?}", uri);
            return Ok(None);
        };

        // Get the project
        let Some(projects) = self.projects.get(&workspace_uri) else {
            tracing::warn!("No projects loaded for workspace: {workspace_uri}");
            return Ok(None);
        };

        let Some((_, project)) = projects.get(project_idx) else {
            tracing::warn!("Project index {project_idx} not found in workspace {workspace_uri}");
            return Ok(None);
        };

        // Collect all documents from the cache
        let all_documents: Vec<(String, String)> = self
            .document_cache
            .iter()
            .map(|entry| {
                let uri_string = entry.key().clone();
                let content = entry.value().clone();
                (uri_string, content)
            })
            .collect();

        tracing::debug!(
            "Collected {} documents for reference search",
            all_documents.len()
        );

        // For pure GraphQL files (TypeScript extraction not implemented yet for find references)
        let position = graphql_project::Position {
            line: lsp_position.line as usize,
            character: lsp_position.character as usize,
        };

        // Find references
        let Some(references) =
            project.find_references(&content, position, &all_documents, include_declaration)
        else {
            tracing::info!("No references found at position {:?}", position);
            return Ok(None);
        };

        tracing::info!("Found {} reference(s)", references.len());

        // Convert to LSP Locations
        #[allow(clippy::cast_possible_truncation)]
        let lsp_locations: Vec<Location> = references
            .iter()
            .filter_map(|reference_loc| {
                // The file_path in reference_loc is the URI string from the document cache
                // Parse it as a URI
                let file_uri: Uri = reference_loc.file_path.parse().ok()?;

                Some(Location {
                    uri: file_uri,
                    range: Range {
                        start: Position {
                            line: reference_loc.range.start.line as u32,
                            character: reference_loc.range.start.character as u32,
                        },
                        end: Position {
                            line: reference_loc.range.end.line as u32,
                            character: reference_loc.range.end.character as u32,
                        },
                    },
                })
            })
            .collect();

        if lsp_locations.is_empty() {
            Ok(None)
        } else {
            Ok(Some(lsp_locations))
        }
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
