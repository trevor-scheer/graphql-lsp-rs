# GraphQL Tooling in Rust

A comprehensive GraphQL tooling ecosystem in Rust, providing LSP (Language Server Protocol) for editor integration and CLI for CI/CD enforcement.

## Project Structure

```
graphql-lsp/
├── crates/
│   ├── graphql-config/       # .graphqlrc parser and loader
│   ├── graphql-extract/      # Extract GraphQL from source files
│   ├── graphql-project/      # Core: validation, indexing, diagnostics
│   ├── graphql-lsp/          # LSP server implementation
│   └── graphql-cli/          # CLI tool for CI/CD
└── .claude/
    └── project-plan.md       # Comprehensive project plan
```

## Crates

### graphql-config
Parses and loads `.graphqlrc` configuration files with parity to the npm `graphql-config` package.

**Features:**
- YAML and JSON config formats
- Single and multi-project configurations
- Schema and document patterns
- Configuration discovery (walks up directory tree)

### graphql-extract
Extracts GraphQL queries, mutations, and fragments from source files.

**Supported:**
- Raw GraphQL files (`.graphql`, `.gql`, `.gqls`)
- TypeScript/JavaScript (via SWC) - Coming soon
- Template literals with `gql` tags
- Magic comments (`/* GraphQL */`)

### graphql-project
Core library providing validation, indexing, and diagnostics.

**Features:**
- Schema loading from files and URLs
- Document loading and extraction
- Validation engine
- Schema and document indexing
- Diagnostic system

### graphql-lsp
Language Server Protocol implementation for GraphQL.

**Implemented Features:**
- Real-time validation with project-wide diagnostics
- Comprehensive go-to-definition support:
  - Fragment spreads, operations, types, fields
  - Variables, arguments, enum values
  - Directives and directive arguments
- Find references for fragments and type definitions
- Hover information for types and fields
- Works with embedded GraphQL in TypeScript/JavaScript

**Planned Features:**
- Additional find references support (fields, variables, directives, enum values)
- Autocomplete
- Document symbols
- Code actions

### graphql-cli
Command-line tool for validation and CI/CD integration.

**Commands:**
- `graphql validate` - Validate schema and documents
- `graphql check` - Check for breaking changes (coming soon)

## Installation

### CLI Tool

#### Install from Binary (Recommended)

**macOS and Linux:**
```bash
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/trevor-scheer/graphql-lsp/releases/latest/download/graphql-cli-installer.sh | sh
```

**Windows (PowerShell):**
```powershell
irm https://github.com/trevor-scheer/graphql-lsp/releases/latest/download/graphql-cli-installer.ps1 | iex
```

#### Install from Source

```bash
cargo install --git https://github.com/trevor-scheer/graphql-lsp graphql-cli
```

#### Download Binary Directly

Download the appropriate binary for your platform from the [releases page](https://github.com/trevor-scheer/graphql-lsp/releases):
- macOS (Intel): `graphql-cli-x86_64-apple-darwin.tar.xz`
- macOS (Apple Silicon): `graphql-cli-aarch64-apple-darwin.tar.xz`
- Linux (x86_64): `graphql-cli-x86_64-unknown-linux-gnu.tar.xz`
- Linux (ARM64): `graphql-cli-aarch64-unknown-linux-gnu.tar.xz`
- Windows: `graphql-cli-x86_64-pc-windows-msvc.zip`

### LSP Server

The VSCode extension will automatically download and install the LSP server binary on first use. However, you can also install it manually:

#### Automatic Installation (Recommended)

Simply install the VSCode extension - it will download the appropriate binary for your platform automatically.

#### Manual Installation

**Via cargo:**
```bash
cargo install graphql-lsp
```

**From releases:**
Download the appropriate binary from the [releases page](https://github.com/trevor-scheer/graphql-lsp/releases):
- macOS (Intel): `graphql-lsp-x86_64-apple-darwin.tar.xz`
- macOS (Apple Silicon): `graphql-lsp-aarch64-apple-darwin.tar.xz`
- Linux (x86_64): `graphql-lsp-x86_64-unknown-linux-gnu.tar.xz`
- Linux (ARM64): `graphql-lsp-aarch64-unknown-linux-gnu.tar.xz`
- Windows: `graphql-lsp-x86_64-pc-windows-msvc.zip`

**Custom binary path:**
Set the `graphql-lsp.serverPath` setting in VSCode to point to a custom binary location.

**For development:**
The extension will automatically use `target/debug/graphql-lsp` when running from the repository, or you can set the `GRAPHQL_LSP_PATH` environment variable.

## Getting Started

### Using the CLI

```bash
# Validate your GraphQL project
graphql validate

# Validate with a specific config file
graphql --config .graphqlrc.yml validate

# Output as JSON for CI/CD
graphql validate --format json

# Watch mode for development
graphql validate --watch
```

### Development

#### Build

```bash
cargo build --workspace
```

#### Run Tests

```bash
cargo test --workspace
```

#### Run CLI from Source

```bash
cargo run -p graphql-cli -- validate --help
```

#### Run LSP Server

```bash
cargo run -p graphql-lsp
```

## Development Status

✅ **Completed:**
- Cargo workspace structure
- graphql-config implementation (parsing, loading, validation)
- Core validation engine with project-wide diagnostics
- Document loading and indexing
- TypeScript/JavaScript extraction
- LSP features: validation, go-to-definition, find references, hover
- Schema and document indexing

🚧 **In Progress:**
- VS Code extension improvements
- Additional LSP features (completions, document symbols)

📋 **Planned:**
- Breaking change detection
- Code actions and refactoring
- Remote schema introspection
- Additional find references support (fields, variables, directives, enum values)

## Configuration Example

`.graphqlrc.yml`:
```yaml
schema: "schema.graphql"
documents: "src/**/*.{graphql,ts,tsx}"
```

Multi-project:
```yaml
projects:
  frontend:
    schema: "https://api.example.com/graphql"
    documents: "frontend/**/*.ts"
  backend:
    schema: "backend/schema.graphql"
    documents: "backend/**/*.graphql"
```

## License

MIT OR Apache-2.0
