# GraphQL LSP Test Workspace

This workspace is for testing the GraphQL LSP extension.

## Testing the Extension

### Option 1: From VS Code (Recommended)

1. Open the `editors/vscode` directory in VS Code:
   ```bash
   cd /Users/trevor/Repositories/graphql-lsp/editors/vscode
   code .
   ```

2. Press `F5` to launch the Extension Development Host

3. In the new VS Code window that opens, open this test workspace:
   ```
   File > Open Folder > Select /Users/trevor/Repositories/graphql-lsp/test-workspace
   ```

4. Open [example.graphql](example.graphql) - you should see validation errors highlighted for `invalidField` and `anotherInvalidField`

### Option 2: Test the LSP Server Directly

You can test the LSP server directly from the command line:

```bash
# The LSP server communicates via stdin/stdout using JSON-RPC
echo '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}' | \
  /Users/trevor/Repositories/graphql-lsp/target/debug/graphql-lsp
```

## What to Expect

- **Valid Query** (lines 1-11): Should have no errors
- **Invalid Query** (lines 13-25): Should show validation errors for:
  - `invalidField` on line 17
  - `email` on line 18
  - `anotherInvalidField` on line 22

## Current Schema

The LSP server currently uses a hardcoded schema:

```graphql
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
```

## Future Enhancements

- Load schema from `graphql.config.yaml` or `.graphqlrc`
- Support for TypeScript/JavaScript files with embedded GraphQL
- Autocompletion
- Hover documentation
- Go-to-definition
- Find references
