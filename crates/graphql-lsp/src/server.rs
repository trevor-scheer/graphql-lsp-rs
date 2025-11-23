use dashmap::DashMap;
use graphql_project::{GraphQLProject, SchemaIndex, Validator};
use lsp_types::{
    CompletionOptions, CompletionParams, CompletionResponse, Diagnostic, DiagnosticSeverity,
    DidChangeTextDocumentParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams,
    DidSaveTextDocumentParams, DocumentSymbolParams, DocumentSymbolResponse,
    GotoDefinitionParams, GotoDefinitionResponse, Hover, HoverParams, HoverProviderCapability,
    InitializeParams, InitializeResult, InitializedParams, Location, MessageType, OneOf,
    Position, Range, ReferenceParams, ServerCapabilities, ServerInfo, SymbolInformation,
    TextDocumentSyncCapability, TextDocumentSyncKind, Uri, WorkspaceSymbol,
    WorkspaceSymbolParams,
};
use std::sync::Arc;
use tower_lsp_server::jsonrpc::Result;
use tower_lsp_server::{Client, LanguageServer};

/// Stores document content and associated schema
struct DocumentState {
    content: String,
    schema: Option<SchemaIndex>,
}

pub struct GraphQLLanguageServer {
    client: Client,
    #[allow(dead_code)] // Will be used when LSP features are implemented
    projects: Arc<DashMap<Uri, GraphQLProject>>,
    documents: Arc<DashMap<Uri, DocumentState>>,
    validator: Validator,
}

impl GraphQLLanguageServer {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            projects: Arc::new(DashMap::new()),
            documents: Arc::new(DashMap::new()),
            validator: Validator::new(),
        }
    }

    /// Validate a document and publish diagnostics
    async fn validate_and_publish(&self, uri: Uri, content: &str, schema: &SchemaIndex) {
        let diagnostics = match self.validator.validate_document(content, schema) {
            Ok(_) => {
                // No errors
                vec![]
            }
            Err(diagnostic_list) => {
                // Convert apollo-compiler diagnostics to LSP diagnostics
                diagnostic_list
                    .iter()
                    .filter_map(|diag| {
                        // Extract location information from the diagnostic
                        let range = if let Some(loc_range) = diag.line_column_range() {
                            // apollo-compiler uses 1-based line/column, LSP uses 0-based
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
                        } else {
                            // Fallback: if no location, place at start of document
                            Range {
                                start: Position {
                                    line: 0,
                                    character: 0,
                                },
                                end: Position {
                                    line: 0,
                                    character: 1,
                                },
                            }
                        };

                        // Extract just the error message without location prefix
                        let message = format!("{}", diag.error);

                        Some(Diagnostic {
                            range,
                            severity: Some(DiagnosticSeverity::ERROR),
                            code: None,
                            source: Some("graphql".to_string()),
                            message,
                            ..Default::default()
                        })
                    })
                    .collect()
            }
        };

        self.client.publish_diagnostics(uri, diagnostics, None).await;
    }
}

impl LanguageServer for GraphQLLanguageServer {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        tracing::info!("Initializing GraphQL Language Server");

        if let Some(ref folders) = params.workspace_folders {
            tracing::info!("Workspace folders: {} folders", folders.len());
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
    }

    async fn shutdown(&self) -> Result<()> {
        tracing::info!("Shutting down GraphQL Language Server");
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let uri = params.text_document.uri.clone();
        let content = params.text_document.text.clone();
        tracing::info!("Document opened: {:?}", uri);

        // For now, use a simple schema. Later this should load from graphql.config
        // You can create a test schema here or load from workspace
        let schema_content = r#"
            type Query {
                user(id: ID!): User
                post(id: ID!): Post
            }

            type User {
                id: ID!
                name: String!
                posts: [Post!]!
            }

            type Post {
                id: ID!
                title: String!
                content: String!
                author: User!
            }
        "#;

        let schema = SchemaIndex::from_schema(schema_content);

        // Store document state
        self.documents.insert(
            uri.clone(),
            DocumentState {
                content: content.clone(),
                schema: Some(schema.clone()),
            },
        );

        // Validate and publish diagnostics
        self.validate_and_publish(uri, &content, &schema).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri.clone();
        tracing::debug!("Document changed: {:?}", uri);

        // Get the latest content from the changes
        for change in params.content_changes {
            if let Some(mut doc_state) = self.documents.get_mut(&uri) {
                // For full sync, replace entire document
                doc_state.content = change.text.clone();

                // Validate if we have a schema
                if let Some(schema) = &doc_state.schema {
                    let content = doc_state.content.clone();
                    let schema = schema.clone();
                    drop(doc_state); // Release the lock before async call

                    self.validate_and_publish(uri.clone(), &content, &schema)
                        .await;
                }
            }
        }
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        tracing::info!("Document saved: {:?}", params.text_document.uri);
        // TODO: Re-validate document
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        tracing::info!("Document closed: {:?}", params.text_document.uri);
        // TODO: Clean up document state
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
