use dashmap::DashMap;
use graphql_project::GraphQLProject;
use lsp_types::{
    CompletionOptions, CompletionParams, CompletionResponse, DidChangeTextDocumentParams,
    DidCloseTextDocumentParams, DidOpenTextDocumentParams, DidSaveTextDocumentParams,
    DocumentSymbolParams, DocumentSymbolResponse, GotoDefinitionParams, GotoDefinitionResponse,
    Hover, HoverParams, HoverProviderCapability, InitializeParams, InitializeResult,
    InitializedParams, Location, MessageType, OneOf, ReferenceParams, ServerCapabilities,
    ServerInfo, SymbolInformation, TextDocumentSyncCapability, TextDocumentSyncKind, Uri,
    WorkspaceSymbol, WorkspaceSymbolParams,
};
use std::sync::Arc;
use tower_lsp_server::jsonrpc::Result;
use tower_lsp_server::{Client, LanguageServer};

pub struct GraphQLLanguageServer {
    client: Client,
    #[allow(dead_code)] // Will be used when LSP features are implemented
    projects: Arc<DashMap<Uri, GraphQLProject>>,
}

impl GraphQLLanguageServer {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            projects: Arc::new(DashMap::new()),
        }
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
                    TextDocumentSyncKind::INCREMENTAL,
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
        tracing::info!("Document opened: {:?}", params.text_document.uri);
        // TODO: Load project, validate document, publish diagnostics
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        tracing::debug!("Document changed: {:?}", params.text_document.uri);
        // TODO: Update document, re-validate, publish diagnostics
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
