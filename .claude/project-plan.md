# GraphQL Rust Tooling - Project Plan

## Vision

Build a comprehensive GraphQL tooling ecosystem in Rust that provides:
- **LSP (Language Server Protocol)** for editor integration and DX
- **CLI** for CI/CD enforcement and validation
- Parity with TypeScript ecosystem tools (graphql-config, graphql-tag-pluck, etc.)

## Current Status

**Overall Progress**: Phase 1-6 Foundation Complete ✅

We have built a working GraphQL LSP server with VS Code extension that provides real-time validation for both standalone `.graphql` files and embedded GraphQL in TypeScript/JavaScript template literals.

### What's Working

✅ **graphql-config** - Full implementation with 13 passing tests
- Parses `.graphqlrc.yml`, `.graphqlrc.json`, and other formats
- Single and multi-project configurations
- Configuration discovery (walks up directory tree)
- Schema loading from files, globs, and URLs

✅ **graphql-extract** - Extraction for .graphql and TypeScript/JavaScript
- Direct parsing of `.graphql` and `.gql` files
- TypeScript/JavaScript extraction via SWC
- Template literal extraction (`gql`, `graphql`, `gqltag` tags)
- Preserves source locations for accurate diagnostics
- 6 passing tests

✅ **graphql-project** - Core engine with indexing and validation
- Schema loading from local files and globs
- Document loading and indexing
- Full GraphQL validation using apollo-compiler
- Schema file detection to skip document validation
- Thread-safe caching with DashMap
- 4 passing tests

✅ **graphql-lsp** - LSP server with diagnostics
- Full tower-lsp integration
- Multi-workspace support
- GraphQL config integration
- Real-time document validation
- TypeScript/JavaScript extraction with temp files
- Schema file detection
- Accurate line/column positions (1-based indexing)
- Clean stderr logging (no ANSI codes)

✅ **graphql-cli** - CLI with document validation
- `graphql validate` command for schema validation
- `graphql validate document` for document validation
- Colored output support
- Progress indicators
- Exit codes for CI/CD

✅ **VS Code Extension** - Full editor integration
- LSP client with auto-start
- GraphQL language support (`.graphql`, `.gql` files)
- TypeScript/JavaScript language support (`.ts`, `.tsx`, `.js`, `.jsx`)
- TextMate grammar for syntax highlighting
- Injection grammar for template literal highlighting
- Auto-closing pairs and bracket matching
- Configuration-based activation
- Status indicators

### Test Results

```
✅ 84 tests passing (total across all crates)
✅ 0 clippy warnings
✅ All formatting checks pass
🚫 0 test failures
```

### Recent Fixes (Latest PR)

1. **Config Integration** - Full GraphQL config loading in LSP
2. **TypeScript Extraction** - Proper temp file extensions for graphql-extract
3. **Schema File Detection** - Path canonicalization to prevent spurious errors
4. **Clean Logging** - Disabled ANSI color codes for VS Code output
5. **Syntax Highlighting** - TextMate grammars for GraphQL files and template literals
6. **Accurate Diagnostics** - 1-based line/column positions matching apollo-compiler

---

## Architecture Overview

```
graphql-lsp/
├── crates/
│   ├── graphql-config/       # ✅ .graphqlrc parser and loader
│   ├── graphql-extract/      # ✅ Extract GraphQL from source files
│   ├── graphql-project/      # ✅ Core: validation, indexing, diagnostics
│   ├── graphql-lsp/          # ✅ LSP server implementation
│   └── graphql-cli/          # ✅ CLI tool for CI/CD
├── editors/
│   └── vscode/               # ✅ VS Code extension
└── docs/                     # Documentation
```

---

## Core Components

### 1. graphql-config (Foundation) ✅ COMPLETE

**Status**: Fully implemented with 13 passing tests

**Supported Formats**:
- ✅ `.graphqlrc` (YAML/JSON)
- ✅ `.graphqlrc.yml` / `.graphqlrc.yaml`
- ✅ `.graphqlrc.json`
- ✅ `graphql.config.yml` / `graphql.config.json`

**Key Features**:
- ✅ Glob pattern support for schema and documents
- ✅ Multi-project configuration
- ✅ Schema loading from local files, globs, and URLs
- ✅ Configuration discovery (walks up directory tree)
- ✅ Validation of configuration structure

**API**:
```rust
pub enum GraphQLConfig {
    Single(ProjectConfig),
    Multi(HashMap<String, ProjectConfig>),
}

pub struct ProjectConfig {
    pub schema: SchemaConfig,
    pub documents: Option<DocumentsConfig>,
    pub include: Option<Vec<String>>,
    pub exclude: Option<Vec<String>>,
}

pub enum SchemaConfig {
    Path(String),
    Paths(Vec<String>),
}

pub fn load_config(path: &Path) -> Result<GraphQLConfig, ConfigError>;
pub fn find_config(start_dir: &Path) -> Result<Option<PathBuf>, IoError>;
```

---

### 2. graphql-extract (GraphQL Extraction) ✅ COMPLETE

**Status**: Working for .graphql and TypeScript/JavaScript files

**Supported File Types**:
- ✅ `.graphql`, `.gql` (raw GraphQL files)
- ✅ `.ts`, `.tsx`, `.js`, `.jsx` (TypeScript/JavaScript via SWC)
- ⏳ `.vue`, `.svelte`, `.astro` (framework files) - Phase 7

**Extraction Methods**:
1. ✅ **Raw GraphQL Files**: Direct parsing
2. ✅ **Template Tag Literals**:
   ```typescript
   import gql from 'graphql-tag';
   const query = gql`query { ... }`;
   ```
3. ⏳ **Magic Comments** (future):
   ```typescript
   const query = /* GraphQL */ `query { ... }`;
   ```

**Supported Tags**:
- ✅ `gql`
- ✅ `graphql`
- ✅ `gqltag`

**Key Features**:
- ✅ Preserve source location mapping
- ✅ Handle TypeScript/JavaScript via SWC
- ✅ Support multiple tag identifiers

---

### 3. graphql-project (Core Engine) ✅ COMPLETE

**Status**: Core validation and indexing complete

**Implemented Features**:

#### A. Schema Management ✅
- ✅ Load schemas from files and globs
- ✅ Parse and validate schema syntax
- ✅ Build schema index (types, fields, directives)
- ✅ Detect schema files vs document files
- ⏳ Watch for schema changes (future)
- ⏳ Remote URL/introspection loading (future)

#### B. Document Management ✅
- ✅ Load documents using graphql-config
- ✅ Extract GraphQL using graphql-extract
- ✅ Parse and validate document syntax
- ✅ Build document index (operations, fragments)

#### C. Validation Engine ✅
- ✅ Validate documents against schema using apollo-compiler
- ✅ Full GraphQL spec validation
- ✅ Structured diagnostics with severity levels
- ✅ Accurate source locations
- ⏳ Custom validation rules (future)

#### D. Indexing & Caching ✅
- ✅ Fast type lookups (HashMap-based, O(1))
- ✅ Field definitions with arguments and types
- ✅ Interface and union type tracking
- ✅ Enum values indexing
- ✅ Directive definitions
- ✅ Thread-safe DashMap for concurrent access
- ⏳ Incremental updates on file changes (future)

#### E. Diagnostics System ✅
- ✅ Structured errors and warnings
- ✅ Severity levels (error, warning, info)
- ✅ Source location with accurate ranges
- ✅ 1-based line/column indexing (LSP standard)
- ⏳ Related information links (future)
- ⏳ Quick fixes / code actions (future)

**API**:
```rust
pub struct GraphQLProject {
    config: ProjectConfig,
    base_dir: Option<PathBuf>,
    schema: Arc<RwLock<Schema>>,
}

impl GraphQLProject {
    pub async fn load_schema(&self) -> Result<(), ProjectError>;
    pub fn validate_document(&self, source: &str) -> Vec<Diagnostic>;
    pub fn is_schema_file(&self, file_path: &Path) -> bool;
    // Future: completions, hover, definition, references
}

pub struct Diagnostic {
    pub severity: Severity,
    pub range: Range,
    pub message: String,
}

pub enum Severity {
    Error,
    Warning,
    Information,
}
```

---

### 4. graphql-cli (Command-Line Tool) ✅ COMPLETE (Phase 1)

**Status**: Basic validation commands working

**Implemented Commands**:
```bash
# Validate schema
✅ graphql validate [--config .graphqlrc.yml]

# Validate document against schema
✅ graphql validate document <file> [--config .graphqlrc.yml]
```

**Future Commands**:
```bash
# Check schema for breaking changes
⏳ graphql check --base main --head feature-branch

# Generate types
⏳ graphql codegen

# Format GraphQL files
⏳ graphql format

# Lint with custom rules
⏳ graphql lint
```

**Features**:
- ✅ Colored terminal output
- ✅ Exit codes for CI integration
- ⏳ JSON output mode for tooling (future)
- ⏳ Watch mode for development (future)
- ⏳ Parallel validation for multi-project configs (future)

---

### 5. graphql-lsp (Language Server) ✅ COMPLETE (Phase 1)

**Status**: Diagnostics and validation working

**Implemented LSP Features**:

#### Phase 1 - Diagnostics ✅
- ✅ Real-time validation
- ✅ Syntax errors
- ✅ Schema validation errors
- ✅ Push diagnostics to client
- ✅ Multi-workspace support
- ✅ GraphQL config integration
- ✅ TypeScript/JavaScript extraction
- ✅ Schema file detection
- ✅ Accurate line/column positions

#### Phase 2 - Navigation ⏳
- ⏳ Go to definition (types, fields, fragments)
- ⏳ Find references
- ⏳ Document symbols
- ⏳ Workspace symbols

#### Phase 3 - Editing ⏳
- ⏳ Autocomplete (fields, arguments, types)
- ⏳ Hover information (type info, descriptions)
- ⏳ Signature help (for arguments)

#### Phase 4 - Refactoring ⏳
- ⏳ Rename symbol
- ⏳ Code actions / quick fixes
- ⏳ Format document

**Key Features**:
- ✅ Support for embedded GraphQL (TS/JS template literals)
- ✅ Multi-project workspace support
- ✅ Temporary file handling for extraction
- ⏳ Configuration auto-reload (future)
- ⏳ Incremental document updates (future)
- ⏳ Debounced validation (future)

**API**:
```rust
pub struct GraphQLLanguageServer {
    client: Client,
    init_workspace_folders: Arc<DashMap<String, PathBuf>>,
    workspace_roots: Arc<DashMap<String, PathBuf>>,
    projects: Arc<DashMap<String, Vec<(String, GraphQLProject)>>>,
}

// Implemented:
// - initialize, initialized, shutdown
// - did_open, did_change, did_save, did_close

// Future:
// - completion, hover, goto_definition
// - references, document_symbol
```

---

### 6. VS Code Extension ✅ COMPLETE (Phase 1)

**Status**: Full basic integration working

**Implemented Features**:
- ✅ LSP client auto-start
- ✅ GraphQL language support (`.graphql`, `.gql`)
- ✅ TypeScript/JavaScript language support (`.ts`, `.tsx`, `.js`, `.jsx`)
- ✅ TextMate grammar for syntax highlighting
- ✅ Injection grammar for template literals (`gql`, `graphql`, `gqltag`)
- ✅ Auto-closing pairs and bracket matching
- ✅ Comment toggling (# for GraphQL)
- ✅ Configuration-based activation (detects `graphql.config.*` or `.graphqlrc*`)
- ✅ Trace server configuration

**Files**:
```
editors/vscode/
├── src/
│   └── extension.ts              # ✅ LSP client setup
├── syntaxes/
│   ├── graphql.tmLanguage.json   # ✅ GraphQL syntax highlighting
│   └── graphql.injection.tmLanguage.json  # ✅ Template literal injection
├── language-configuration.json   # ✅ Editor behavior config
├── package.json                  # ✅ Extension manifest
└── tsconfig.json
```

**Future Features**:
- ⏳ Commands for validation, formatting
- ⏳ Status bar indicators
- ⏳ Code actions UI

---

## Implementation Roadmap

### Phase 1: Foundation ✅ COMPLETE
- ✅ Set up Cargo workspace structure
- ✅ Implement graphql-config (basic schema + documents)
- ✅ Implement graphql-extract for `.graphql` files
- ✅ Choose and integrate GraphQL parser (apollo-parser)

### Phase 2: Core Engine ✅ COMPLETE
- ✅ Implement schema loading and parsing
- ✅ Implement document loading and parsing
- ✅ Build validation engine with apollo-compiler
- ✅ Implement diagnostics system
- ✅ Add indexing (types, fields, directives, enums)

### Phase 3: CLI ✅ COMPLETE (Basic)
- ✅ Build CLI structure with clap
- ✅ Implement `validate` command
- ✅ Add configuration discovery
- ⏳ Add multi-project support (future)
- ⏳ Add watch mode (future)

### Phase 4: TS/JS Extraction ✅ COMPLETE
- ✅ Integrate SWC for TypeScript/JavaScript parsing
- ✅ Implement template literal extraction
- ✅ Add source position mapping
- ✅ Test with real-world codebases (spotify-showcase)
- ⏳ Implement magic comment extraction (future)

### Phase 5: LSP Server ✅ COMPLETE (Diagnostics)
- ✅ Set up tower-lsp server
- ✅ Implement diagnostics publishing
- ✅ Add multi-project workspace support
- ✅ Add TypeScript/JavaScript support
- ⏳ Implement go-to-definition (future)
- ⏳ Implement find references (future)
- ⏳ Implement autocomplete (future)
- ⏳ Implement hover information (future)

### Phase 6: VS Code Extension ✅ COMPLETE (Basic)
- ✅ Create extension scaffolding
- ✅ Implement LSP client
- ✅ Add syntax highlighting (TextMate grammar)
- ✅ Add template literal highlighting (injection grammar)
- ✅ Add configuration settings
- ✅ Package and test

### Phase 7: Advanced Features ⏳ FUTURE
- ⏳ Autocompletion (fields, types, arguments, fragments)
- ⏳ Hover information with type details and documentation
- ⏳ Go-to-definition navigation
- ⏳ Find all references
- ⏳ Document symbols outline
- ⏳ Workspace-wide symbol search
- ⏳ Custom validation rules
- ⏳ Code actions / quick fixes
- ⏳ Rename refactoring
- ⏳ Format document
- ⏳ Breaking change detection
- ⏳ Additional language support (.vue, .svelte, etc.)
- ⏳ Schema registry integration (Apollo, Hive)
- ⏳ Magic comment support (`/* GraphQL */`)
- ⏳ Configuration hot-reloading
- ⏳ Incremental validation
- ⏳ Schema change watching

---

## Testing Strategy

### Unit Tests ✅
- ✅ Each crate has comprehensive unit tests
- ✅ Parser tests with fixtures
- ✅ Validation tests with apollo-compiler
- ✅ Position mapping tests
- ✅ 84 total tests passing

### Integration Tests ✅
- ✅ End-to-end CLI validation
- ✅ LSP server feature tests
- ✅ Real-world project testing (spotify-showcase)

### Fixtures
- ✅ Real-world schema examples (GitHub-like, Shopify-like)
- ✅ Complex document examples
- ✅ TypeScript/JavaScript codebases
- ✅ Various configuration formats

### CI/CD ✅
- ✅ GitHub Actions workflows
- ✅ Test on Linux, macOS, Windows
- ✅ Clippy strict linting
- ✅ Formatting checks
- ✅ Security audits

---

## Technical Decisions Summary

| Decision | Choice | Rationale |
|----------|--------|-----------|
| GraphQL Parser | apollo-parser | Official, spec-compliant, good error recovery |
| GraphQL Validator | apollo-compiler | Full spec validation, accurate diagnostics |
| TS/JS Parser | SWC | Fast, production-ready, native Rust |
| LSP Framework | tower-lsp | Standard choice for Rust LSP servers |
| CLI Framework | clap | Feature-rich, ergonomic API |
| HTTP Client | reqwest | Most popular, good async support |
| Async Runtime | tokio | Industry standard for Rust async |
| Config Format | YAML + JSON | Match graphql-config behavior |
| Concurrent Storage | DashMap | Lock-free HashMap for LSP state |

---

## Success Metrics

1. ✅ **Correctness**: Using apollo-compiler for full spec validation
2. ⏳ **Performance**: Validate 1000+ documents in < 1 second (not yet benchmarked)
3. ✅ **Compatibility**: Load 100% of valid graphql-config files
4. ✅ **Extraction**: Working with real-world TypeScript codebases
5. ⏳ **LSP**: Autocomplete response < 100ms (not yet implemented)
6. ⏳ **Adoption**: VS Code extension published (not yet published)

---

## Resources & References

### Specifications
- [GraphQL Specification](https://spec.graphql.org/)
- [LSP Specification](https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/)

### Existing Tools (Reference)
- [graphql-config](https://the-guild.dev/graphql/config/docs)
- [graphql-tag-pluck](https://the-guild.dev/graphql/tools/docs/graphql-tag-pluck)
- [graphql-eslint](https://the-guild.dev/graphql/eslint/docs)
- [vscode-graphql](https://github.com/graphql/vscode-graphql)

### Rust Libraries
- [apollo-parser](https://docs.rs/apollo-parser)
- [apollo-compiler](https://docs.rs/apollo-compiler)
- [tower-lsp](https://docs.rs/tower-lsp)
- [swc](https://swc.rs/)
- [clap](https://docs.rs/clap)
- [serde](https://docs.rs/serde)

---

## Development Workflow

### Building
```bash
# Build everything
cargo build --workspace

# Build LSP server only
cargo build --package graphql-lsp

# Build with release optimizations
cargo build --release
```

### Testing
```bash
# Run all tests
cargo test --workspace

# Run tests for a specific crate
cargo test -p graphql-config

# Run with output
cargo test -- --nocapture
```

### Linting and Formatting
```bash
# Format code
cargo fmt --all

# Check formatting
cargo fmt --all -- --check

# Run clippy
cargo clippy --workspace --all-targets

# Fix clippy suggestions
cargo clippy --workspace --all-targets --fix
```

### Running
```bash
# Run CLI
cargo run -p graphql-cli -- validate

# Run LSP server (for manual testing)
cargo run -p graphql-lsp

# Test LSP with example
./test-lsp.sh
```

### VS Code Extension
```bash
# Install dependencies
cd editors/vscode && npm install

# Compile TypeScript
npm run compile

# Watch mode
npm run watch

# Launch extension (or press F5 in VS Code)
```

---

## Open Questions

1. ⏳ Should we support TOML config format (`.graphqlrc.toml`)?
2. ⏳ Should we build a custom rule system or adopt graphql-eslint rules?
3. ⏳ Should we support JSON Schema validation for GraphQL configs?
4. ⏳ Should the CLI support code generation (TypeScript types, etc.)?
5. ⏳ Should we build a web-based playground/validator?
6. ⏳ Should we support Language Server Index Format (LSIF)?
7. ⏳ Should we integrate with schema registries from the start?
8. ⏳ Should we implement incremental parsing for better performance?
9. ⏳ Should we add support for GraphQL federation?

---

## Known Issues and Limitations

### Current Limitations
- Schema change watching not implemented yet
- No incremental validation (validates entire document on change)
- No debouncing for validation
- Configuration doesn't auto-reload on change
- No support for remote schema introspection yet
- No code generation features
- No breaking change detection

### Future Improvements
- Add file watching for schema changes
- Implement incremental validation
- Add debouncing to reduce validation frequency
- Support hot-reload of configuration
- Add remote schema introspection with caching
- Implement code generation (TypeScript, etc.)
- Add schema diff and breaking change detection
- Support for GraphQL federation schemas

---

**Last Updated**: 2025-11-24
**Status**: Phase 1-6 Complete, Phase 7 (Advanced Features) In Planning

**Next Priority**: Implement autocompletion, hover information, and go-to-definition for LSP
