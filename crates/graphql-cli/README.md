# graphql-cli

Command-line interface for GraphQL validation and tooling.

## Purpose

This crate provides a CLI tool for working with GraphQL projects from the command line. It enables:
- Validating GraphQL schemas and documents
- Checking for breaking changes between schema versions
- Watch mode for continuous validation during development
- Multiple output formats (human-readable, JSON, GitHub Actions)

## How it Fits

The CLI is a command-line alternative to the LSP server, sharing the same core functionality:

```
graphql-cli (CLI) -> graphql-project (core) -> GraphQL files
                                            -> graphql-config
                                            -> graphql-extract
```

Both `graphql-cli` and `graphql-lsp` depend on `graphql-project` for GraphQL processing.

## Installation

Build the CLI:

```bash
cargo build --package graphql-cli --release
```

The binary will be at `target/release/graphql`.

## Usage

### Validate Command

Validate GraphQL documents against a schema:

```bash
# Validate with auto-discovered config
graphql validate

# Specify a config file
graphql --config .graphqlrc.yml validate

# Specify a project in a multi-project config
graphql --project my-api validate

# JSON output
graphql validate --format json

# Watch mode - re-validate on file changes
graphql validate --watch
```

### Check Command

Check for breaking changes between schema versions:

```bash
# Compare schemas from two git refs
graphql check --base main --head feature-branch
```

This is useful in CI to prevent breaking changes from being merged.

## Output Formats

### Human (default)

Colorized, human-readable output:

```
✗ Query validation error in src/queries.graphql:5:3
  Cannot query field "invalidField" on type "User"

  3 |   user(id: $id) {
  4 |     id
  5 |     invalidField
    |     ^^^^^^^^^^^^
  6 |   }
```

### JSON

Machine-readable JSON output:

```json
{
  "errors": [
    {
      "message": "Cannot query field \"invalidField\" on type \"User\"",
      "location": {
        "file": "src/queries.graphql",
        "line": 5,
        "column": 3
      }
    }
  ]
}
```

Useful for integrating with other tools or scripts.

### GitHub

GitHub Actions annotation format:

```
::error file=src/queries.graphql,line=5,col=3::Cannot query field "invalidField" on type "User"
```

Errors appear as annotations in GitHub pull requests.

## Configuration

The CLI uses the same configuration format as the LSP server. It searches for:
- `.graphqlrc` (YAML or JSON)
- `.graphqlrc.yml` / `.graphqlrc.yaml`
- `.graphqlrc.json`
- `graphql.config.js` / `graphql.config.ts`
- `graphql` section in `package.json`

Example configuration:

```yaml
schema: schema.graphql
documents: src/**/*.graphql
```

For multi-project configs:

```yaml
projects:
  api:
    schema: api/schema.graphql
    documents: api/**/*.graphql
  client:
    schema: client/schema.graphql
    documents: client/**/*.graphql
```

## Use Cases

### CI/CD Integration

Add validation to your CI pipeline:

```yaml
# GitHub Actions
- name: Validate GraphQL
  run: graphql validate --format github
```

```yaml
# GitLab CI
validate-graphql:
  script:
    - graphql validate
```

### Pre-commit Hook

Validate GraphQL before commits (using husky, lint-staged, or cargo-husky):

```bash
graphql validate
```

### Development Workflow

Use watch mode during development:

```bash
graphql validate --watch
```

This continuously validates as you edit GraphQL files.

## Technical Details

### Commands Module

[src/commands/](src/commands/) contains implementations for each command:
- `validate.rs`: Document validation logic
- `check.rs`: Schema breaking change detection (future)

### Terminal UI

Uses the `colored` crate for colorized output and `indicatif` for progress bars.

## Development

Key files to understand:
- [src/main.rs](src/main.rs) - CLI argument parsing and command dispatch
- [src/commands/validate.rs](src/commands/validate.rs) - Validation command implementation

When adding new commands:
1. Add the command variant to the `Commands` enum in main.rs
2. Create a new file in src/commands/
3. Implement the command logic using `graphql-project` APIs
4. Update this README with usage examples
