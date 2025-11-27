# graphql-extract

Extract GraphQL queries from TypeScript and JavaScript source files.

## Purpose

This crate extracts GraphQL queries, mutations, and fragments embedded in TypeScript/JavaScript code, enabling language tooling to work with GraphQL-in-code patterns. It:
- Parses TypeScript/JavaScript using SWC
- Extracts GraphQL template literals (tagged with `gql`, `graphql`, etc.)
- Tracks source location mappings between extracted GraphQL and original code
- Supports various GraphQL embedding patterns

## How it Fits

This crate is used by `graphql-project` to process non-GraphQL files:

```
TypeScript/JavaScript files -> graphql-extract -> Extracted GraphQL -> graphql-project
```

This enables features like:
- Validating GraphQL in `.ts/.tsx/.js/.jsx` files
- Goto definition from code to GraphQL definitions
- Hover information in embedded GraphQL

## Supported Patterns

### Tagged Template Literals

```typescript
import { gql } from 'graphql-tag';

const query = gql`
  query GetUser($id: ID!) {
    user(id: $id) {
      id
      name
    }
  }
`;
```

### Supported Tags

- `gql`
- `graphql`
- Custom tags (configurable)

### Multiple Queries

```typescript
const query1 = gql`query A { user { id } }`;
const query2 = gql`query B { posts { title } }`;
```

### Fragments

```typescript
const userFragment = gql`
  fragment UserFields on User {
    id
    name
    email
  }
`;

const query = gql`
  query GetUser {
    user {
      ...UserFields
    }
  }
  ${userFragment}
`;
```

## Usage

### Extract from File

```rust
use graphql_extract::{extract_from_file, Language};

// Extract GraphQL from a TypeScript file
let result = extract_from_file("src/queries.ts", Language::TypeScript)?;

for extracted in result.documents {
    println!("Found GraphQL:");
    println!("{}", extracted.content);
    println!("At line {} col {}", extracted.location.start.line, extracted.location.start.column);
}
```

### Extract from String

```rust
use graphql_extract::{extract_from_source, Language};

let source = r#"
const query = gql`
  query GetUser {
    user { id }
  }
`;
"#;

let result = extract_from_source(source, Language::TypeScript)?;
```

### Language Detection

```rust
use graphql_extract::Language;

let lang = Language::from_path("file.tsx")?;
// lang == Language::TypeScript
```

### Configuration

```rust
use graphql_extract::ExtractConfig;

let config = ExtractConfig {
    tags: vec!["gql".to_string(), "graphql".to_string(), "apollo".to_string()],
};

let result = extract_from_file_with_config("src/queries.ts", config)?;
```

## Source Location Mapping

The crate tracks precise source locations, mapping between:
- **Original source**: Position in the TypeScript/JavaScript file
- **Extracted content**: Position in the extracted GraphQL string

This enables accurate:
- Error reporting (show errors at the correct line in the original file)
- Goto definition (navigate from code to GraphQL definitions)
- Hover information (show type info in embedded GraphQL)

### ExtractedGraphQL

```rust
pub struct ExtractedGraphQL {
    pub content: String,              // The extracted GraphQL
    pub location: SourceLocation,     // Location in original file
}

pub struct SourceLocation {
    pub start: Position,
    pub end: Position,
}

pub struct Position {
    pub line: usize,    // 1-based line number
    pub column: usize,  // 0-based column offset
}
```

## Technical Details

### Parser

Uses SWC (Speedy Web Compiler) for parsing TypeScript/JavaScript:
- Fast, production-ready parser
- Full TypeScript support including JSX/TSX
- Provides accurate source location information

### AST Traversal

Traverses the SWC AST looking for:
- `TaggedTemplateExpression` nodes
- Template literals with matching tag names
- Nested template expressions (for fragment interpolation)

### Error Handling

Handles common edge cases:
- Malformed TypeScript/JavaScript (returns parse errors)
- Non-GraphQL tagged templates (ignored)
- Empty template literals (skipped)
- Interpolated values in templates (preserved as-is)

## Supported Languages

```rust
pub enum Language {
    TypeScript,
    JavaScript,
}
```

Detected from file extensions:
- `.ts`, `.tsx` → TypeScript
- `.js`, `.jsx` → JavaScript

## Development

Key files to understand:
- [src/extractor.rs](src/extractor.rs) - Main extraction logic
- [src/language.rs](src/language.rs) - Language detection
- [src/source_location.rs](src/source_location.rs) - Position tracking
- [src/error.rs](src/error.rs) - Error types

When adding new features:
1. Update the AST visitor in extractor.rs
2. Update ExtractConfig if adding new options
3. Document supported patterns in this README

## Limitations

- Template literal interpolation is preserved as-is (not evaluated)
- Dynamic tag names are not supported (must be static identifiers)
- Minified code may have inaccurate source locations
