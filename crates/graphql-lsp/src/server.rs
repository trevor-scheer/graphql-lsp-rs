use dashmap::DashMap;
use graphql_config::{find_config, load_config};
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
    #[tracing::instrument(skip(self), fields(workspace_uri = %workspace_uri))]
    async fn load_workspace_config(&self, workspace_uri: &str, workspace_path: &PathBuf) {
        tracing::info!(path = ?workspace_path, "Loading GraphQL config");

        // Find graphql config
        match find_config(workspace_path) {
            Ok(Some(config_path)) => {
                tracing::info!(config_path = ?config_path, "Found GraphQL config");

                // Load the config
                match load_config(&config_path) {
                    Ok(config) => {
                        // Create projects from config
                        match GraphQLProject::from_config_with_base(&config, workspace_path) {
                            Ok(projects) => {
                                tracing::info!(count = projects.len(), "Loaded GraphQL projects");

                                // Load schemas and documents for all projects
                                for (name, project) in &projects {
                                    if let Err(e) = project.load_schema().await {
                                        tracing::error!(
                                            project = %name,
                                            error = %e,
                                            "Failed to load schema"
                                        );
                                        self.client
                                            .log_message(
                                                MessageType::ERROR,
                                                format!("Failed to load schema for project '{name}': {e}"),
                                            )
                                            .await;
                                    } else {
                                        tracing::info!(project = %name, "Loaded schema");
                                    }

                                    // Load documents to index all fragments
                                    if let Err(e) = project.load_documents() {
                                        tracing::error!(
                                            project = %name,
                                            error = %e,
                                            "Failed to load documents"
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
                                            project = %name,
                                            operations = doc_index.operations.len(),
                                            fragments = doc_index.fragments.len(),
                                            "Loaded documents"
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

    /// Re-validate all fragment definition files in the project
    /// This is called after document changes to update unused fragment warnings
    async fn revalidate_fragment_files(&self, changed_uri: &Uri) {
        let start = std::time::Instant::now();
        tracing::info!(
            "Starting re-validation of fragment files for: {:?}",
            changed_uri
        );

        // Find the workspace and project for the changed document
        let Some((workspace_uri, project_idx)) = self.find_workspace_and_project(changed_uri)
        else {
            tracing::debug!("No workspace found for URI: {:?}", changed_uri);
            return;
        };

        // Get all fragment files from the document index
        // We need to collect the file paths and then drop the borrow before validating
        let index_start = std::time::Instant::now();
        let fragment_files: std::collections::HashSet<String> = {
            let Some(projects) = self.projects.get(&workspace_uri) else {
                tracing::debug!("No projects loaded for workspace: {}", workspace_uri);
                return;
            };

            let Some((_, project)) = projects.get(project_idx) else {
                tracing::debug!(
                    "Project index {} not found in workspace {}",
                    project_idx,
                    workspace_uri
                );
                return;
            };

            let document_index = project.get_document_index();
            tracing::debug!("Got document index in {:?}", index_start.elapsed());

            document_index
                .fragments
                .values()
                .flatten() // Flatten Vec<FragmentInfo> to iterate over each FragmentInfo
                .map(|frag_info| frag_info.file_path.clone())
                .collect()
        }; // Drop the borrow here before we start validating

        tracing::info!(
            "Re-validating {} fragment files after document change",
            fragment_files.len()
        );

        // Re-validate each fragment file
        for file_path in fragment_files {
            let file_start = std::time::Instant::now();
            tracing::debug!("Re-validating fragment file: {}", file_path);

            // Convert file path to URI
            let Some(fragment_uri) = Uri::from_file_path(&file_path) else {
                tracing::warn!("Failed to convert file path to URI: {}", file_path);
                continue;
            };

            // Get content from cache or read from disk
            let content =
                if let Some(cached_content) = self.document_cache.get(&fragment_uri.to_string()) {
                    tracing::debug!("Using cached content for: {}", file_path);
                    cached_content.clone()
                } else {
                    // Fragment file not open in editor, read from disk
                    tracing::debug!("Reading fragment file from disk: {}", file_path);
                    match std::fs::read_to_string(&file_path) {
                        Ok(content) => content,
                        Err(e) => {
                            tracing::warn!("Failed to read fragment file {}: {}", file_path, e);
                            continue;
                        }
                    }
                };

            // Validate the fragment file
            self.validate_document(fragment_uri, &content).await;
            tracing::debug!(
                "Validated fragment file {} in {:?}",
                file_path,
                file_start.elapsed()
            );
        }

        tracing::info!(
            "Completed re-validation of fragment files in {:?}",
            start.elapsed()
        );
    }

    /// Validate a document and publish diagnostics
    #[allow(clippy::too_many_lines)]
    #[tracing::instrument(skip(self, content), fields(uri = ?uri))]
    async fn validate_document(&self, uri: Uri, content: &str) {
        let start = std::time::Instant::now();
        tracing::debug!("Validating document");

        let Some((workspace_uri, project_idx)) = self.find_workspace_and_project(&uri) else {
            tracing::warn!("No project found for document");
            return;
        };

        let file_path = uri.to_file_path();

        // Check if this is a schema file and update document index
        // We do this in a narrow scope to minimize lock duration
        {
            // Get the project from the workspace (need mutable to update document index)
            let Some(mut projects) = self.projects.get_mut(&workspace_uri) else {
                tracing::warn!("No projects loaded for workspace: {workspace_uri}");
                return;
            };

            let Some((_, project)) = projects.get_mut(project_idx) else {
                tracing::warn!(
                    "Project index {project_idx} not found in workspace {workspace_uri}"
                );
                return;
            };

            // Check if this is a schema file - schema files shouldn't be validated as executable documents
            if let Some(ref path) = file_path {
                if project.is_schema_file(path.as_ref()) {
                    tracing::debug!("Skipping validation for schema file");
                    // Clear any existing diagnostics
                    self.client.publish_diagnostics(uri, vec![], None).await;
                    return;
                }
            }

            // Update the document index for this specific file with in-memory content
            // This is more efficient than reloading all documents from disk and
            // ensures we use the latest editor content even before it's saved
            if let Some(path) = &file_path {
                let file_path_str = path.display().to_string();
                if let Err(e) = project.update_document_index(&file_path_str, content) {
                    tracing::warn!(
                        "Failed to update document index for {}: {}",
                        file_path_str,
                        e
                    );
                }
            }
        } // Drop the mutable lock here before validation

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
        // Now we get a read-only reference for validation, which won't block other operations
        let mut diagnostics = {
            let Some(projects) = self.projects.get(&workspace_uri) else {
                tracing::warn!("No projects loaded for workspace: {workspace_uri}");
                return;
            };

            let Some((_, project)) = projects.get(project_idx) else {
                tracing::warn!(
                    "Project index {project_idx} not found in workspace {workspace_uri}"
                );
                return;
            };

            if is_ts_js {
                self.validate_typescript_document(&uri, content, project)
            } else {
                self.validate_graphql_document(content, project)
            }
        }; // Drop the read lock here

        // Add project-wide duplicate name diagnostics for this file
        if let Some(path) = uri.to_file_path() {
            let file_path_str = path.display().to_string();

            let project_wide_diags = {
                let Some(projects) = self.projects.get(&workspace_uri) else {
                    tracing::warn!("No projects loaded for workspace: {workspace_uri}");
                    return;
                };

                let Some((_, project)) = projects.get(project_idx) else {
                    tracing::warn!(
                        "Project index {project_idx} not found in workspace {workspace_uri}"
                    );
                    return;
                };

                self.get_project_wide_diagnostics(&file_path_str, project)
            }; // Drop the read lock here

            diagnostics.extend(project_wide_diags);
        }

        // Filter out diagnostics with invalid ranges (defensive fix for stale diagnostics)
        // Count total lines in the content to validate ranges
        let line_count = content.lines().count();
        diagnostics.retain(|diag| {
            let start_line = diag.range.start.line as usize;
            let end_line = diag.range.end.line as usize;

            // Keep diagnostic only if both start and end are within document bounds
            if start_line >= line_count || end_line >= line_count {
                tracing::warn!(
                    "Filtered out diagnostic with invalid range: {:?} (document has {} lines)",
                    diag.range,
                    line_count
                );
                false
            } else {
                true
            }
        });

        self.client
            .publish_diagnostics(uri.clone(), diagnostics.clone(), None)
            .await;

        tracing::debug!(
            elapsed_ms = start.elapsed().as_millis(),
            diagnostic_count = diagnostics.len(),
            "Validated document"
        );

        // Refresh diagnostics for any other files affected by duplicate name changes
        self.refresh_affected_files_diagnostics(&workspace_uri, project_idx, &uri)
            .await;
    }

    /// Get project-wide duplicate name diagnostics for a specific file
    fn get_project_wide_diagnostics(
        &self,
        file_path: &str,
        project: &GraphQLProject,
    ) -> Vec<Diagnostic> {
        use graphql_project::LintSeverity;

        // Check if unique_names lint is enabled and get its severity
        let lint_config = project.get_lint_config();
        let severity = match lint_config.get_severity("unique_names") {
            Some(LintSeverity::Error) => graphql_project::Severity::Error,
            Some(LintSeverity::Warn) => graphql_project::Severity::Warning,
            Some(LintSeverity::Off) | None => return Vec::new(),
        };

        // Get the document index
        let document_index = project.get_document_index();

        // Check for duplicate names across the project with the configured severity
        let duplicate_diagnostics = document_index.check_duplicate_names(severity);

        // Filter to only diagnostics for this file and convert to LSP diagnostics
        duplicate_diagnostics
            .into_iter()
            .filter(|(path, _)| path == file_path)
            .map(|(_, diag)| self.convert_project_diagnostic(diag))
            .collect()
    }

    /// Refresh diagnostics for all files affected by duplicate name changes
    ///
    /// When a file is edited and introduces or removes duplicate names, other files
    /// that share those names need to have their diagnostics refreshed to show or
    /// clear duplicate name errors.
    async fn refresh_affected_files_diagnostics(
        &self,
        workspace_uri: &str,
        project_idx: usize,
        changed_file_uri: &Uri,
    ) {
        use graphql_project::LintSeverity;
        use std::collections::HashSet;

        // Get the project and check if unique_names lint is enabled
        let Some(projects) = self.projects.get(workspace_uri) else {
            return;
        };

        let Some((_, project)) = projects.get(project_idx) else {
            return;
        };

        let lint_config = project.get_lint_config();
        let severity = match lint_config.get_severity("unique_names") {
            Some(LintSeverity::Error) => graphql_project::Severity::Error,
            Some(LintSeverity::Warn) => graphql_project::Severity::Warning,
            Some(LintSeverity::Off) | None => return,
        };

        // Get all duplicate name diagnostics
        let document_index = project.get_document_index();
        let duplicate_diagnostics = document_index.check_duplicate_names(severity);

        // Extract unique file paths that have duplicate name diagnostics
        let affected_files: HashSet<String> = duplicate_diagnostics
            .iter()
            .map(|(path, _)| path.clone())
            .collect();

        let changed_file_path = changed_file_uri.to_file_path();

        // For each affected file (excluding the one we just validated), refresh diagnostics
        for file_path in affected_files {
            // Skip the file we just validated
            if let Some(ref changed_path) = changed_file_path {
                if file_path == changed_path.display().to_string() {
                    continue;
                }
            }

            // Try to convert the file path to a URI
            let Some(file_uri) = Uri::from_file_path(&file_path) else {
                tracing::warn!("Failed to convert file path to URI: {}", file_path);
                continue;
            };

            // Get the document content from cache, or read from disk
            let content = if let Some(cached) = self.document_cache.get(file_uri.as_str()) {
                cached.clone()
            } else {
                // File not in cache, try to read from disk
                match std::fs::read_to_string(&file_path) {
                    Ok(content) => content,
                    Err(e) => {
                        tracing::warn!("Failed to read file {}: {}", file_path, e);
                        continue;
                    }
                }
            };

            // Check if this is a TypeScript/JavaScript file
            let is_ts_js = std::path::Path::new(&file_path)
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| matches!(ext, "ts" | "tsx" | "js" | "jsx"));

            // Get document-specific diagnostics (type errors, etc.)
            let mut diagnostics = if is_ts_js {
                self.validate_typescript_document(&file_uri, &content, project)
            } else {
                self.validate_graphql_document(&content, project)
            };

            // Add project-wide duplicate name diagnostics for this file
            let project_wide_diags = self.get_project_wide_diagnostics(&file_path, project);
            diagnostics.extend(project_wide_diags);

            // Filter out diagnostics with invalid ranges
            let line_count = content.lines().count();
            diagnostics.retain(|diag| {
                let start_line = diag.range.start.line as usize;
                let end_line = diag.range.end.line as usize;

                if start_line >= line_count || end_line >= line_count {
                    tracing::warn!(
                        "Filtered out diagnostic with invalid range: {:?} (document has {} lines)",
                        diag.range,
                        line_count
                    );
                    false
                } else {
                    true
                }
            });

            // Publish diagnostics for the affected file
            tracing::debug!(
                "Refreshing diagnostics for affected file: {} ({} diagnostics)",
                file_path,
                diagnostics.len()
            );
            self.client
                .publish_diagnostics(file_uri, diagnostics, None)
                .await;
        }
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
        let extract_config = project.get_extract_config();
        let extracted = match graphql_extract::extract_from_file(temp_file.path(), &extract_config)
        {
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

        // Use the centralized validation logic from graphql-project (Apollo compiler)
        let file_path = uri.to_string();
        let mut all_diagnostics = project.validate_extracted_documents(&extracted, &file_path);

        // Run custom lints (if configured)
        let lint_config = project.get_lint_config();
        let linter = graphql_project::Linter::new(lint_config);
        let schema_index = project.get_schema_index();

        for block in &extracted {
            let lint_diagnostics = linter.lint_document(&block.source, &schema_index, &file_path);

            // Adjust positions for extracted blocks
            for mut diag in lint_diagnostics {
                diag.range.start.line += block.location.range.start.line;
                diag.range.end.line += block.location.range.start.line;

                // Adjust column only for first line
                if diag.range.start.line == block.location.range.start.line {
                    diag.range.start.character += block.location.range.start.column;
                }
                if diag.range.end.line == block.location.range.start.line {
                    diag.range.end.character += block.location.range.start.column;
                }

                all_diagnostics.push(diag);
            }
        }

        // Convert graphql-project diagnostics to LSP diagnostics
        all_diagnostics
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
    #[tracing::instrument(skip(self, params))]
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        tracing::info!("Initializing GraphQL Language Server");

        // Store workspace folders for later config loading
        if let Some(ref folders) = params.workspace_folders {
            tracing::info!(count = folders.len(), "Workspace folders");
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

    #[tracing::instrument(skip(self, params), fields(uri = ?params.text_document.uri))]
    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let uri = params.text_document.uri;
        let content = params.text_document.text;
        tracing::info!("Document opened");

        // Cache the document content
        self.document_cache.insert(uri.to_string(), content.clone());

        self.validate_document(uri, &content).await;
    }

    #[tracing::instrument(skip(self, params), fields(uri = ?params.text_document.uri))]
    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri;
        let start = std::time::Instant::now();
        tracing::info!("Document changed");

        // Get the latest content from changes (full sync mode)
        for change in params.content_changes {
            // Update the document cache
            self.document_cache
                .insert(uri.to_string(), change.text.clone());

            let validate_start = std::time::Instant::now();
            self.validate_document(uri.clone(), &change.text).await;
            tracing::debug!("Main validation took {:?}", validate_start.elapsed());

            // Re-validate all fragment definition files to update unused fragment warnings
            // This ensures that when fragment usage changes in one file, warnings in
            // fragment files are immediately updated
            let revalidate_start = std::time::Instant::now();
            self.revalidate_fragment_files(&uri).await;
            tracing::debug!(
                "Fragment revalidation took {:?}",
                revalidate_start.elapsed()
            );
        }

        tracing::info!(
            elapsed_ms = start.elapsed().as_millis(),
            "Completed did_change"
        );
    }

    #[tracing::instrument(skip(self, params), fields(uri = ?params.text_document.uri))]
    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        tracing::info!("Document saved");
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

        // Convert LSP position to graphql-project Position
        let position = graphql_project::Position {
            line: lsp_position.line as usize,
            character: lsp_position.character as usize,
        };

        // Get hover info from the project (handles TypeScript extraction internally)
        // Convert URI to file path for cache lookup consistency
        let file_path = uri
            .to_file_path()
            .map_or_else(|| uri.to_string(), |path| path.display().to_string());

        let Some(hover_info) = project.hover_info_at_position(&file_path, position, &content)
        else {
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
        let start = std::time::Instant::now();
        let uri = params.text_document_position_params.text_document.uri;
        let lsp_position = params.text_document_position_params.position;

        tracing::info!(
            "Go to definition requested: {:?} at line={} char={}",
            uri,
            lsp_position.line,
            lsp_position.character
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
            // Try to use cached extracted blocks first (Phase 3 optimization)
            let cached_blocks = project.get_extracted_blocks(&uri.to_string());

            // Find which extracted GraphQL block contains the cursor position
            let cursor_line = lsp_position.line as usize;

            if let Some(blocks) = cached_blocks {
                // Use cached blocks - no extraction or parsing needed!
                for block in blocks {
                    if cursor_line >= block.start_line && cursor_line <= block.end_line {
                        // Adjust position relative to the extracted GraphQL
                        #[allow(clippy::cast_possible_truncation)]
                        let relative_position = graphql_project::Position {
                            line: cursor_line - block.start_line,
                            character: if cursor_line == block.start_line {
                                lsp_position
                                    .character
                                    .saturating_sub(block.start_column as u32)
                                    as usize
                            } else {
                                lsp_position.character as usize
                            },
                        };

                        tracing::debug!(
                            "Using cached extracted block at position {:?}",
                            relative_position
                        );

                        // Get definition locations from the project using the cached GraphQL
                        let Some(locations) = project.goto_definition(
                            &block.content,
                            relative_position,
                            &uri.to_string(),
                        ) else {
                            tracing::debug!(
                                "No definition found at position {:?}",
                                relative_position
                            );
                            continue;
                        };

                        tracing::debug!("Found {} definition location(s)", locations.len());

                        // Convert to LSP Locations (adjust positions back to original file coordinates)
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
                                    let file_path = if std::path::Path::new(&loc.file_path)
                                        .is_absolute()
                                    {
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
                                let (start_line, start_char, end_line, end_char) = if file_uri
                                    == uri
                                {
                                    // Adjust positions back from extracted GraphQL to original file
                                    let adjusted_start_line =
                                        loc.range.start.line + block.start_line;
                                    let adjusted_start_char = if loc.range.start.line == 0 {
                                        loc.range.start.character + block.start_column
                                    } else {
                                        loc.range.start.character
                                    };
                                    let adjusted_end_line = loc.range.end.line + block.start_line;
                                    let adjusted_end_char = if loc.range.end.line == 0 {
                                        loc.range.end.character + block.start_column
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
            } else {
                // Fallback: Extract GraphQL from TypeScript file (cache miss)
                tracing::debug!("Cache miss - extracting GraphQL from TypeScript file");
                let extract_config = project.get_extract_config();
                let extracted =
                    match graphql_extract::extract_from_source(&content, language, &extract_config)
                    {
                        Ok(extracted) => extracted,
                        Err(e) => {
                            tracing::debug!(
                                "Failed to extract GraphQL from TypeScript file: {}",
                                e
                            );
                            return Ok(None);
                        }
                    };

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
                        let Some(locations) = project.goto_definition(
                            &item.source,
                            relative_position,
                            &uri.to_string(),
                        ) else {
                            tracing::debug!(
                                "No definition found at position {:?}",
                                relative_position
                            );
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
                                    let file_path = if std::path::Path::new(&loc.file_path)
                                        .is_absolute()
                                    {
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
                                let (start_line, start_char, end_line, end_char) = if file_uri
                                    == uri
                                {
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

        tracing::info!(
            "Goto definition completed in {:?}, returning {} location(s)",
            start.elapsed(),
            lsp_locations.len()
        );

        if lsp_locations.is_empty() {
            Ok(None)
        } else {
            Ok(Some(GotoDefinitionResponse::Array(lsp_locations)))
        }
    }

    async fn references(&self, params: ReferenceParams) -> Result<Option<Vec<Location>>> {
        let start = std::time::Instant::now();
        let uri = params.text_document_position.text_document.uri;
        let lsp_position = params.text_document_position.position;
        let include_declaration = params.context.include_declaration;

        tracing::info!(
            "Find references requested: {:?} at line={} char={} (include_declaration: {})",
            uri,
            lsp_position.line,
            lsp_position.character,
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
        let collect_start = std::time::Instant::now();
        let all_documents: Vec<(String, String)> = self
            .document_cache
            .iter()
            .map(|entry| {
                let uri_string = entry.key().clone();
                let content = entry.value().clone();
                (uri_string, content)
            })
            .collect();

        tracing::info!(
            "Collected {} documents for reference search in {:?}",
            all_documents.len(),
            collect_start.elapsed()
        );

        // For find_references optimization, we would parse all documents once here
        // However, since the documents are already cached in document_index via did_open/did_change,
        // the actual optimization happens by reusing those cached ASTs.
        // We pass None here as the ASTs will be retrieved from document_index internally.
        let document_asts: Option<&std::collections::HashMap<String, graphql_project::SyntaxTree>> =
            None;

        // For pure GraphQL files (TypeScript extraction not implemented yet for find references)
        let position = graphql_project::Position {
            line: lsp_position.line as usize,
            character: lsp_position.character as usize,
        };

        // Find references with pre-parsed ASTs
        let find_start = std::time::Instant::now();
        let Some(references) = project.find_references_with_asts(
            &content,
            position,
            &all_documents,
            include_declaration,
            Some(&uri.to_string()),
            document_asts,
        ) else {
            tracing::info!("No references found at position {:?}", position);
            return Ok(None);
        };

        tracing::info!(
            "Found {} reference(s) in {:?}",
            references.len(),
            find_start.elapsed()
        );

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

        tracing::info!(
            "Find references completed in {:?}, returning {} location(s)",
            start.elapsed(),
            lsp_locations.len()
        );

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
