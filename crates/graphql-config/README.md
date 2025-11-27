# graphql-config

GraphQL configuration file parsing and discovery.

## Purpose

This crate handles loading and parsing GraphQL configuration files. It:
- Discovers configuration files in a project directory
- Parses multiple configuration formats (YAML, JSON)
- Provides a unified configuration API
- Supports multi-project configurations
- Resolves glob patterns for schema and document files

## How it Fits

This is a foundational crate used by all other components to understand project structure:

```
graphql-lsp -----> graphql-config
graphql-cli -----> graphql-config
graphql-project -> graphql-config
```

All GraphQL tooling starts by loading configuration through this crate.

## Configuration Format

The crate supports the standard GraphQL configuration format used by popular tools like GraphQL Code Generator and GraphQL ESLint.

### Single Project

```yaml
# .graphqlrc.yml
schema: schema.graphql
documents: src/**/*.graphql
```

### Multiple Projects

```yaml
# .graphqlrc.yml
projects:
  api:
    schema: api/schema.graphql
    documents: api/**/*.graphql
  client:
    schema:
      - client/schema.graphql
      - client/schema/*.graphql
    documents:
      - client/**/*.graphql
      - client/**/*.tsx
```

### Schema Sources

Schemas can be loaded from:
- Local files: `schema.graphql`
- Glob patterns: `schema/**/*.graphql`
- HTTP endpoints: `https://api.example.com/graphql` (introspection)
- Multiple sources: `["schema.graphql", "extensions/*.graphql"]`

### Document Patterns

Documents can include:
- GraphQL files: `**/*.graphql`, `**/*.gql`
- Embedded GraphQL in code: `**/*.tsx`, `**/*.ts`, `**/*.jsx`, `**/*.js`

## Usage

### Loading Configuration

```rust
use graphql_config::{load_config, find_config};

// Discover and load config from a directory
let config = load_config("/path/to/project")?;

// Get a specific project
let project = config.projects.get("my-project").unwrap();

println!("Schema: {:?}", project.schema);
println!("Documents: {:?}", project.documents);
```

### Finding Config Files

```rust
use graphql_config::find_config;

// Search for config file
let config_path = find_config("/path/to/project")?;
println!("Found config at: {}", config_path.display());
```

### Parsing from String

```rust
use graphql_config::load_config_from_str;

let yaml = r#"
schema: schema.graphql
documents: "**/*.graphql"
"#;

let config = load_config_from_str(yaml, "yaml")?;
```

## Supported Configuration Files

The crate searches for these files in order:
1. `.graphqlrc` (YAML or JSON)
2. `.graphqlrc.yml`
3. `.graphqlrc.yaml`
4. `.graphqlrc.json`
5. `graphql.config.js` (future)
6. `graphql.config.ts` (future)
7. `graphql` section in `package.json` (future)

Currently, only YAML and JSON formats are fully supported.

## Key Types

### GraphQLConfig

The top-level configuration:

```rust
pub struct GraphQLConfig {
    pub projects: HashMap<String, ProjectConfig>,
}
```

For single-project configs, there's an implicit "default" project.

### ProjectConfig

Configuration for a single GraphQL project:

```rust
pub struct ProjectConfig {
    pub schema: SchemaConfig,
    pub documents: Option<DocumentsConfig>,
}
```

### SchemaConfig

Schema source configuration:

```rust
pub enum SchemaConfig {
    File(String),
    Files(Vec<String>),
    Url(String),
}
```

### DocumentsConfig

Document pattern configuration:

```rust
pub struct DocumentsConfig {
    pub patterns: Vec<String>,
}
```

## Technical Details

### Glob Pattern Resolution

The crate uses the `glob` crate to resolve file patterns. Patterns are resolved relative to the configuration file location.

### File Discovery

Uses `walkdir` to recursively search for configuration files starting from a given directory and walking up to parent directories.

### Error Handling

Provides detailed error messages for:
- Missing configuration files
- Invalid YAML/JSON syntax
- Invalid configuration structure
- Missing required fields

## Development

Key files to understand:
- [src/config.rs](src/config.rs) - Configuration data structures
- [src/loader.rs](src/loader.rs) - Configuration loading and discovery
- [src/error.rs](src/error.rs) - Error types

When adding new features:
1. Update the configuration structs in config.rs
2. Add parsing logic in loader.rs
3. Update this README with examples
