# GraphQL Rust Tooling - Comprehensive Project Plan

## Vision

Build a comprehensive GraphQL tooling ecosystem in Rust that provides:
- **LSP (Language Server Protocol)** for editor integration and DX
- **CLI** for CI/CD enforcement and validation
- Parity with TypeScript ecosystem tools (graphql-config, graphql-tag-pluck, etc.)

## Architecture Overview

```
graphql-lsp/
├── crates/
│   ├── graphql-config/       # .graphqlrc parser and loader
│   ├── graphql-extract/      # Extract GraphQL from source files
│   ├── graphql-project/      # Core: validation, indexing, diagnostics
│   ├── graphql-lsp/          # LSP server implementation
│   └── graphql-cli/          # CLI tool for CI/CD
├── editors/
│   └── vscode/               # VS Code extension
└── docs/                     # Documentation
```

---

## Core Components

### 1. graphql-config (Foundation)

**Purpose**: Parse and load `.graphqlrc` configuration files with parity to the npm `graphql-config` package.

**Supported Formats**:
- `.graphqlrc` (YAML/JSON)
- `.graphqlrc.yml` / `.graphqlrc.yaml`
- `.graphqlrc.json`
- `graphql.config.yml` / `graphql.config.json`
- Future: `.graphqlrc.toml`, `.graphqlrc.rs`

**Phase 1 - Core Fields**:
```yaml
schema: "./schema.graphql"
documents: "./src/**/*.{graphql,ts,tsx,js,jsx}"
```

**Phase 2 - Multi-Project Support**:
```yaml
projects:
  frontend:
    schema: "https://api.example.com/graphql"
    documents: "frontend/**/*.{graphql,ts,tsx}"
  backend:
    schema: "backend/schema.graphql"
    documents: "backend/**/*.graphql"
```

**Key Features**:
- Glob pattern support for schema and documents
- Multi-project configuration
- Schema loading from local files and remote URLs
- Configuration inheritance and defaults
- Validation of configuration structure

**Dependencies**:
- `serde` for deserialization
- `glob` for pattern matching
- `yaml-rust` or `serde_yaml` for YAML parsing

**API Design**:
```rust
pub enum GraphQLConfig {
    /// Single project configuration
    Single(ProjectConfig),
    /// Multi-project configuration
    Multi(HashMap<String, ProjectConfig>),
}

impl GraphQLConfig {
    /// Get all projects as an iterator
    pub fn projects(&self) -> impl Iterator<Item = (&str, &ProjectConfig)>;

    /// Get a specific project by name (returns default project for Single variant)
    pub fn get_project(&self, name: &str) -> Option<&ProjectConfig>;
}

pub struct ProjectConfig {
    pub schema: SchemaConfig,
    pub documents: Option<DocumentsConfig>,
    pub include: Option<Vec<String>>,
    pub exclude: Option<Vec<String>>,
    pub extensions: Option<HashMap<String, serde_json::Value>>,
}

pub enum SchemaConfig {
    Path(String),
    Url(String),
    Paths(Vec<String>),  // Multiple schema files/globs
}

pub enum DocumentsConfig {
    Pattern(String),
    Patterns(Vec<String>),
}

pub fn load_config(path: &Path) -> Result<GraphQLConfig, ConfigError>;
pub fn find_config(start_dir: &Path) -> Result<Option<PathBuf>, IoError>;
```

---

### 2. graphql-extract (GraphQL Extraction)

**Purpose**: Extract GraphQL queries, mutations, fragments from source files. Similar to `graphql-tag-pluck` in the TypeScript ecosystem.

**Supported File Types**:
- Phase 1: `.graphql`, `.gql`, `.gqls` (raw GraphQL files)
- Phase 2: `.ts`, `.tsx`, `.js`, `.jsx` (TypeScript/JavaScript)
- Phase 3: `.vue`, `.svelte`, `.astro` (framework files)

**Extraction Methods**:

1. **Raw GraphQL Files**: Direct parsing
2. **Template Tag Literals**:
   ```typescript
   import gql from 'graphql-tag';
   const query = gql`query { ... }`;
   ```
3. **Magic Comments**:
   ```typescript
   const query = /* GraphQL */ `query { ... }`;
   ```
4. **String Literals**:
   ```typescript
   const query = "query { ... }";
   ```

**Supported Module Imports**:
- `graphql-tag`
- `@apollo/client`
- `gatsby`
- `apollo-server-*`
- `react-relay`
- Custom/configurable imports

**Key Features**:
- Preserve source location mapping (for diagnostics)
- Handle template literal interpolations
- Support custom magic comments (configurable)
- Support global identifiers (no import needed)
- Incremental parsing for performance

**Technical Approach**:

**Option A: SWC (Recommended)**
- Fast, production-ready Rust parser
- Native AST traversal
- Good TypeScript/JSX support

**Option B: Tree-sitter**
- Universal parser generator
- Multi-language support out of the box
- Better for future language additions
- Query-based extraction

**API Design**:
```rust
pub struct ExtractConfig {
    pub magic_comment: String,  // default: "GraphQL"
    pub tag_identifiers: Vec<String>,  // default: ["gql", "graphql"]
    pub modules: Vec<String>,  // recognized import sources
}

pub struct ExtractedGraphQL {
    pub source: String,
    pub location: SourceLocation,
    pub tag_name: Option<String>,
}

pub fn extract_from_file(
    path: &Path,
    config: &ExtractConfig
) -> Result<Vec<ExtractedGraphQL>, ExtractError>;

pub fn extract_from_source(
    source: &str,
    language: Language,
    config: &ExtractConfig
) -> Result<Vec<ExtractedGraphQL>, ExtractError>;
```

---

### 3. graphql-project (Core Engine)

**Purpose**: Central library providing validation, indexing, caching, and diagnostics. Used by both LSP and CLI.

**Responsibilities**:

#### A. Schema Management
- Load schemas from files, URLs, introspection
- Parse and validate schema syntax
- Build schema index for fast lookups
- Watch for schema changes

#### B. Document Management
- Load documents using graphql-config
- Extract GraphQL using graphql-extract
- Parse and validate document syntax
- Build document index (operations, fragments)

#### C. Validation Engine
- Validate documents against schema
- Validate fragment usage
- Validate variable definitions
- Custom validation rules
- Similar to graphql-eslint rules

#### D. Indexing & Caching
- Fast lookups for autocomplete
- Type definitions index
- Field definitions index
- Fragment definitions index
- Operation definitions index
- Incremental updates on file changes

#### E. Diagnostics System
- Structured errors and warnings
- Severity levels (error, warning, info, hint)
- Source location with ranges
- Related information links
- Quick fixes / code actions
- Position mapping (source ↔ extracted GraphQL)

**Key Features**:
- Thread-safe caching (Arc, RwLock)
- Incremental updates
- Project workspace support
- Configuration hot-reloading

**Technical Decisions**:

**GraphQL Parser Options**:
1. **apollo-parser** (Recommended)
   - Official Apollo parser in Rust
   - Full spec compliance
   - Good error recovery
   - Active maintenance

2. **async-graphql-parser**
   - Part of async-graphql framework
   - Good performance

3. **graphql-parser**
   - Older, simpler
   - Less active

**API Design**:
```rust
pub struct GraphQLProject {
    config: GraphQLConfig,
    schema: Arc<RwLock<Schema>>,
    documents: Arc<RwLock<DocumentIndex>>,
    diagnostics: Arc<RwLock<Vec<Diagnostic>>>,
}

impl GraphQLProject {
    pub fn new(config: GraphQLConfig) -> Result<Self, ProjectError>;
    pub fn load_schema(&mut self) -> Result<(), SchemaError>;
    pub fn load_documents(&mut self) -> Result<(), DocumentError>;
    pub fn validate(&mut self) -> Vec<Diagnostic>;
    pub fn get_completions(&self, position: Position) -> Vec<CompletionItem>;
    pub fn get_definition(&self, position: Position) -> Option<Location>;
    pub fn get_references(&self, position: Position) -> Vec<Location>;
    pub fn get_hover(&self, position: Position) -> Option<Hover>;
}

pub struct Diagnostic {
    pub severity: Severity,
    pub range: Range,
    pub message: String,
    pub code: Option<String>,
    pub source: String,
    pub related_info: Vec<RelatedInfo>,
    pub quick_fixes: Vec<CodeAction>,
}

pub enum Severity {
    Error,
    Warning,
    Information,
    Hint,
}
```

---

### 4. graphql-cli (Command-Line Tool)

**Purpose**: Standalone CLI tool for CI/CD pipelines, pre-commit hooks, and local validation.

**Commands**:

```bash
# Validate schema and documents
graphql-cli validate [--config .graphqlrc.yml]

# Check schema for breaking changes
graphql-cli check --base main --head feature-branch

# Generate types (future)
graphql-cli codegen

# Format GraphQL files (future)
graphql-cli format

# Lint with custom rules (future)
graphql-cli lint
```

**Features**:
- Colored terminal output
- Exit codes for CI integration
- JSON output mode for tooling
- Watch mode for development
- Parallel validation for multi-project configs
- Configuration discovery (walk up directory tree)

**Dependencies**:
- `clap` for CLI argument parsing
- `colored` for terminal colors
- `indicatif` for progress bars

**API Design**:
```rust
pub struct ValidateCommand {
    pub config_path: Option<PathBuf>,
    pub project: Option<String>,
    pub format: OutputFormat,  // human, json
}

pub enum OutputFormat {
    Human,
    Json,
}

pub fn validate(cmd: ValidateCommand) -> Result<ExitCode, CliError>;
```

---

### 5. graphql-lsp (Language Server)

**Purpose**: LSP server providing editor integration for GraphQL files and embedded GraphQL.

**LSP Features**:

#### Phase 1 - Diagnostics
- Real-time validation
- Syntax errors
- Schema validation errors
- Push diagnostics to client

#### Phase 2 - Navigation
- Go to definition (types, fields, fragments)
- Find references
- Document symbols
- Workspace symbols

#### Phase 3 - Editing
- Autocomplete (fields, arguments, types)
- Hover information (type info, descriptions)
- Signature help (for arguments)

#### Phase 4 - Refactoring
- Rename symbol
- Code actions / quick fixes
- Format document

**Key Features**:
- Support for embedded GraphQL (TS/JS files)
- Multi-project workspace support
- Configuration auto-reload
- Incremental document updates
- Debounced validation

**Dependencies**:
- `tower-lsp` (LSP framework for Rust)
- `tokio` (async runtime)
- `dashmap` (concurrent HashMap)

**API Design**:
```rust
pub struct GraphQLLanguageServer {
    projects: Arc<DashMap<Url, GraphQLProject>>,
    client: Client,
}

#[tower_lsp::async_trait]
impl LanguageServer for GraphQLLanguageServer {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult>;
    async fn initialized(&self, params: InitializedParams);
    async fn shutdown(&self) -> Result<()>;

    async fn did_open(&self, params: DidOpenTextDocumentParams);
    async fn did_change(&self, params: DidChangeTextDocumentParams);
    async fn did_save(&self, params: DidSaveTextDocumentParams);
    async fn did_close(&self, params: DidCloseTextDocumentParams);

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>>;
    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>>;
    async fn goto_definition(&self, params: GotoDefinitionParams) -> Result<Option<GotoDefinitionResponse>>;
    async fn references(&self, params: ReferenceParams) -> Result<Option<Vec<Location>>>;
    async fn document_symbol(&self, params: DocumentSymbolParams) -> Result<Option<DocumentSymbolResponse>>;
}
```

---

### 6. VS Code Extension

**Purpose**: Client-side extension that spawns and communicates with the LSP server.

**Technology**: TypeScript (standard for VS Code extensions)

**Features**:
- Spawn graphql-lsp server
- Configure server settings
- Syntax highlighting (use existing TextMate grammar)
- Status bar indicators
- Commands for validation, formatting

**Structure**:
```
editors/vscode/
├── src/
│   ├── extension.ts         # Main entry point
│   ├── client.ts            # LSP client setup
│   └── commands.ts          # VS Code commands
├── syntaxes/
│   └── graphql.tmLanguage.json  # Syntax highlighting
├── package.json             # Extension manifest
└── tsconfig.json
```

**Key Configuration**:
```json
{
  "contributes": {
    "languages": [{
      "id": "graphql",
      "extensions": [".graphql", ".gql", ".gqls"]
    }],
    "grammars": [{
      "language": "graphql",
      "scopeName": "source.graphql",
      "path": "./syntaxes/graphql.tmLanguage.json"
    }],
    "configuration": {
      "title": "GraphQL",
      "properties": {
        "graphql.config": {
          "type": "string",
          "default": ".graphqlrc.yml"
        }
      }
    }
  }
}
```

---

## Additional Supporting Components

### 7. Schema Introspection

**Purpose**: Query remote GraphQL endpoints for schema information.

**Features**:
- Standard introspection query
- HTTP client with authentication support
- Caching introspection results
- Retry logic and error handling

**Dependencies**:
- `reqwest` (HTTP client)
- `tokio` (async runtime)

---

### 8. Position Mapping Utilities

**Purpose**: Map between different coordinate systems.

**Coordinate Systems**:
1. LSP Position (line, character) - 0-indexed
2. Source file byte offsets
3. Extracted GraphQL positions (offset by template literal location)

**API Design**:
```rust
pub struct PositionMapper {
    source: String,
    line_starts: Vec<usize>,
}

impl PositionMapper {
    pub fn offset_to_position(&self, offset: usize) -> Position;
    pub fn position_to_offset(&self, position: Position) -> usize;
    pub fn map_extracted_position(&self,
        extracted_offset: usize,
        template_start: usize
    ) -> Position;
}
```

---

## Implementation Roadmap

### Phase 1: Foundation (Weeks 1-2)
- [ ] Set up Cargo workspace structure
- [ ] Implement graphql-config (basic schema + documents)
- [ ] Implement graphql-extract for `.graphql` files only
- [ ] Choose and integrate GraphQL parser (apollo-parser recommended)

### Phase 2: Core Engine (Weeks 3-4)
- [ ] Implement schema loading and parsing
- [ ] Implement document loading and parsing
- [ ] Build basic validation engine
- [ ] Implement diagnostics system
- [ ] Add basic indexing (types, fields)

### Phase 3: CLI (Week 5)
- [ ] Build CLI structure with clap
- [ ] Implement `validate` command
- [ ] Add configuration discovery
- [ ] Add multi-project support
- [ ] Add output formatting (human, JSON)

### Phase 4: TS/JS Extraction (Week 6)
- [ ] Integrate SWC for TypeScript/JavaScript parsing
- [ ] Implement template literal extraction
- [ ] Implement magic comment extraction
- [ ] Add source position mapping
- [ ] Test with real-world codebases

### Phase 5: LSP Server (Weeks 7-9)
- [ ] Set up tower-lsp server
- [ ] Implement diagnostics publishing
- [ ] Implement go-to-definition
- [ ] Implement find references
- [ ] Implement autocomplete
- [ ] Implement hover information
- [ ] Add multi-project workspace support

### Phase 6: VS Code Extension (Week 10)
- [ ] Create extension scaffolding
- [ ] Implement LSP client
- [ ] Add syntax highlighting
- [ ] Add configuration settings
- [ ] Package and test

### Phase 7: Advanced Features (Future)
- [ ] Multi-project configuration
- [ ] Custom validation rules
- [ ] Code actions / quick fixes
- [ ] Rename refactoring
- [ ] Format document
- [ ] Breaking change detection
- [ ] Additional language support (.vue, .svelte, etc.)
- [ ] Schema registry integration (Apollo, Hive)

---

## Technical Decisions Summary

| Decision | Choice | Rationale |
|----------|--------|-----------|
| GraphQL Parser | apollo-parser | Official, spec-compliant, good error recovery |
| TS/JS Parser | SWC | Fast, production-ready, native Rust |
| LSP Framework | tower-lsp | Standard choice for Rust LSP servers |
| CLI Framework | clap | Feature-rich, ergonomic API |
| HTTP Client | reqwest | Most popular, good async support |
| Async Runtime | tokio | Industry standard for Rust async |
| Config Format | YAML + JSON | Match graphql-config behavior |

---

## Testing Strategy

### Unit Tests
- Each crate has comprehensive unit tests
- Parser tests with fixtures
- Validation rule tests
- Position mapping tests

### Integration Tests
- End-to-end CLI validation
- LSP server feature tests
- Multi-project configuration tests

### Fixtures
- Real-world schema examples
- Complex document examples
- TypeScript/JavaScript codebases
- Various configuration formats

### Performance Tests
- Large schema parsing
- Many document validation
- Autocomplete response time
- Memory usage profiling

---

## Success Metrics

1. **Correctness**: Pass all graphql-js validation test cases
2. **Performance**: Validate 1000+ documents in < 1 second
3. **Compatibility**: Load 100% of valid graphql-config files
4. **Extraction**: Match graphql-tag-pluck behavior on test suite
5. **LSP**: Autocomplete response < 100ms for typical schemas
6. **Adoption**: VS Code extension with positive reviews

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
- [tower-lsp](https://docs.rs/tower-lsp)
- [swc](https://swc.rs/)
- [tree-sitter](https://tree-sitter.github.io/)
- [clap](https://docs.rs/clap)
- [serde](https://docs.rs/serde)

---

## Open Questions

1. Should we support TOML config format (`.graphqlrc.toml`)?
2. Should we build a custom rule system or adopt graphql-eslint rules?
3. Should we support JSON Schema validation for GraphQL configs?
4. Should the CLI support code generation (TypeScript types, etc.)?
5. Should we build a web-based playground/validator?
6. Should we support Language Server Index Format (LSIF) for code intelligence?
7. Should we integrate with schema registries from the start?

---

## Contributing Guidelines (Future)

- Conventional commits
- PR templates
- CI/CD with GitHub Actions
- Automated testing and linting
- Release automation
- Documentation requirements

---

**Last Updated**: 2025-11-22
**Status**: Planning Phase
