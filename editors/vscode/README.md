# GraphQL LSP VS Code Extension

A VS Code extension that provides GraphQL language support with validation, completion, and navigation features.

## Features

- **Validation**: Real-time GraphQL query and schema validation
- **Error Diagnostics**: Inline error messages with squiggly underlines
- **Future**: Autocompletion, hover info, go-to-definition (coming soon)

## Development Setup

1. Build the LSP server:
   ```bash
   cd ../..
   cargo build --package graphql-lsp
   ```

2. Install extension dependencies:
   ```bash
   cd editors/vscode
   npm install
   npm run compile
   ```

3. Set the path to the LSP server binary (optional):
   ```bash
   export GRAPHQL_LSP_PATH=/path/to/graphql-lsp/target/debug/graphql-lsp
   ```

4. Open this directory in VS Code and press F5 to launch the extension in a new window

## Testing

1. In the Extension Development Host window, create a new file with `.graphql` extension
2. Write a GraphQL query - you should see validation errors for invalid fields
3. Example:
   ```graphql
   query GetUser($id: ID!) {
     user(id: $id) {
       id
       name
       invalidField  # This should show an error
     }
   }
   ```

## Configuration

The extension can be configured in VS Code settings:

- `graphql-lsp.trace.server`: Control the verbosity of logging (off, messages, verbose)
