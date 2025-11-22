mod server;

use server::GraphQLLanguageServer;
use tower_lsp_server::{LspService, Server};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(|client| GraphQLLanguageServer::new(client));

    Server::new(stdin, stdout, socket).serve(service).await;
}
