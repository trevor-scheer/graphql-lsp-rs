# graphql-project

Core GraphQL project management, validation, and language service functionality.

## Purpose

This crate provides the core logic for managing GraphQL projects, including:
- Loading and managing GraphQL schemas and documents
- Project-wide validation
- Building indices of fragments, operations, and types
- Providing goto definition and hover information
- Supporting file watching for incremental updates

## How it Fits

This is the heart of the GraphQL language tooling. It sits between the LSP server and the raw GraphQL files:

```
graphql-lsp (LSP server) -> graphql-project (this crate) -> GraphQL files
                                                          -> graphql-config
                                                          -> graphql-extract
```

Both the LSP server (`graphql-lsp`) and CLI (`graphql-cli`) depend on this crate for their core GraphQL functionality.

## Key Components

### GraphQLProject

The main entry point ([src/project.rs](src/project.rs)):
- Manages a GraphQL project based on configuration
- Loads schemas and documents
- Maintains indices of fragments, operations, and types
- Provides validation and language service features
- Supports incremental updates when files change

### DocumentLoader

Document management ([src/document.rs](src/document.rs)):
- Loads GraphQL documents from files
- Handles both pure `.graphql` files and embedded GraphQL in TypeScript/JavaScript
- Manages document caching

### SchemaLoader

Schema management ([src/schema.rs](src/schema.rs)):
- Loads GraphQL schemas from files or introspection endpoints
- Builds schema using apollo-compiler
- Supports schema stitching across multiple files

### Index

Fast lookup structures ([src/index.rs](src/index.rs)):
- `DocumentIndex`: Maps fragments and operations to their locations
- `SchemaIndex`: Maps types, fields, and directives to schema locations
- Enables goto definition and autocomplete

### Validation

GraphQL validation ([src/validation.rs](src/validation.rs)):
- Validates documents against schema
- Project-wide validation (unique operation/fragment names)
- Converts apollo-compiler diagnostics to our format

### Language Features

- **Goto Definition** ([src/goto_definition.rs](src/goto_definition.rs)): Comprehensive navigation support
  - Fragment spreads and definitions
  - Operation names
  - Type references (fragments, inline fragments, implements, unions, fields, variables)
  - Field references to schema definitions
  - Variable references to operation variable definitions
  - Argument names to schema argument definitions
  - Enum values to their definitions
  - Directive names and their arguments
- **Hover** ([src/hover.rs](src/hover.rs)): Type information and documentation

## Usage

### Creating a Project

```rust
use graphql_project::{GraphQLProject, GraphQLConfig};

// Load configuration
let config = GraphQLConfig::load("path/to/project")?;
let project_config = config.projects.get("my-project").unwrap();

// Create project
let project = GraphQLProject::new(project_config, "path/to/root").await?;

// Validate all documents
let diagnostics = project.validate_all().await?;
```

### Getting Language Features

```rust
// Goto definition
let definition = project.goto_definition("file:///path/to/file.graphql", line, col).await?;

// Hover information
let hover_info = project.hover("file:///path/to/file.graphql", line, col).await?;
```

### Watching for Changes

```rust
// Update a document
project.update_document("file:///path/to/file.graphql", new_content).await?;

// Re-validate
let diagnostics = project.validate_document("file:///path/to/file.graphql").await?;
```

## Technical Details

### Parser Support

The crate uses apollo-compiler as the primary GraphQL parser, which provides:
- Accurate error messages
- Full GraphQL spec compliance
- Built-in validation rules

### Position Mapping

For TypeScript/JavaScript files with embedded GraphQL:
1. Extract GraphQL using `graphql-extract`
2. Track position mappings between extracted and original source
3. Translate positions for accurate goto definition and diagnostics

### Concurrency

Uses DashMap for concurrent access to project data, allowing:
- Multiple LSP requests to be handled in parallel
- Safe updates from file watchers

## Development

Key files to understand:
- [src/project.rs](src/project.rs) - Main GraphQLProject implementation
- [src/index.rs](src/index.rs) - Indexing structures
- [src/validation.rs](src/validation.rs) - Validation logic
- [src/goto_definition.rs](src/goto_definition.rs) - Goto definition implementation
- [src/hover.rs](src/hover.rs) - Hover information

When adding new features:
1. Consider if it needs indexing (add to `DocumentIndex` or `SchemaIndex`)
2. Implement the feature on `GraphQLProject`
3. Export public APIs in lib.rs
4. Update LSP server to expose the feature
