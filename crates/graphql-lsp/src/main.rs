mod server;

use server::GraphQLLanguageServer;
use tower_lsp_server::{LspService, Server};

#[tokio::main]
async fn main() {
    // IMPORTANT: LSP uses stdin/stdout for JSON-RPC communication
    // All logs MUST go to stderr to avoid breaking the protocol
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_ansi(false) // Disable ANSI colors since LSP output doesn't support them
        .init();

    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(GraphQLLanguageServer::new);

    Server::new(stdin, stdout, socket).serve(service).await;
}
