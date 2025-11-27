# graphql-lsp

The main Language Server Protocol (LSP) implementation for GraphQL language support.

## Purpose

This crate implements a GraphQL language server that provides IDE features like diagnostics, validation, goto definition, and hover information for GraphQL files. It communicates with editors via the LSP protocol using JSON-RPC over stdin/stdout.

## How it Fits

This is the entry point of the LSP server. It:
- Implements the LSP server using `tower-lsp`
- Manages the lifecycle of GraphQL projects
- Handles LSP requests from editors (textDocument/didOpen, textDocument/hover, etc.)
- Coordinates between the editor and the underlying GraphQL project infrastructure
- Delegates GraphQL-specific functionality to `graphql-project`

## Architecture

```
Editor <-> LSP Protocol <-> graphql-lsp <-> graphql-project <-> GraphQL files
                                        \-> graphql-config
                                        \-> graphql-extract
```

## Key Components

### server.rs

The main LSP server implementation:
- `GraphQLLanguageServer`: Implements the tower-lsp `LanguageServer` trait
- Handles LSP lifecycle methods (initialize, initialized, shutdown)
- Implements text document synchronization (didOpen, didChange, didClose)
- Provides language features (hover, goto definition, diagnostics)

### main.rs

Entry point that:
- Sets up tracing/logging to stderr (LSP uses stdin/stdout for protocol)
- Creates the LSP service and starts the server

## Usage

### Running the LSP Server

The LSP server is typically launched by an editor/IDE:

```bash
cargo build --package graphql-lsp
./target/debug/graphql-lsp
```

The server reads LSP requests from stdin and writes responses to stdout. All logging goes to stderr.

### Integration with Editors

For VSCode, see the [editors/vscode](../../editors/vscode/) extension.

### Configuration

The LSP server discovers GraphQL configuration files (`.graphqlrc`, `graphql.config.js`) in the workspace and uses them to:
- Locate GraphQL schema files
- Find GraphQL documents
- Configure validation rules

## Development

Key files to understand:
- [src/server.rs](src/server.rs) - Main LSP server implementation
- [src/main.rs](src/main.rs) - Entry point

When adding new LSP features:
1. Add the handler method to `GraphQLLanguageServer` in server.rs
2. Implement the feature using `graphql-project` APIs
3. Update the VSCode extension if needed
