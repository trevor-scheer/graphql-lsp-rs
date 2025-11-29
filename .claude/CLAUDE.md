# GraphQL LSP Project Context

## Overview

This is a GraphQL Language Server Protocol (LSP) implementation written in Rust. The project provides language server features for GraphQL files, including validation, diagnostics, and IDE integration.

## Project Structure

- `crates/graphql-lsp/` - Main LSP implementation
- `crates/graphql-language-service/` - Core language service functionality
- `editors/vscode/` - VSCode extension for the LSP
- `tests/` - Test suites

## Key Technologies

- **Language**: Rust
- **LSP Framework**: tower-lsp
- **GraphQL Parsing**: apollo-compiler and graphql-parser
- **Build System**: Cargo

## Code Quality Standards

- Run `cargo fmt` before committing
- Run `cargo clippy` and address warnings
- Ensure `cargo test` passes
- Pre-commit hooks are set up via cargo-husky

## PR Guidelines

- Don't include notes about testing or linting passing in PR descriptions
- Don't use excessive emoji in PR titles or descriptions

## Testing

- Unit tests are located alongside source files
- Integration tests are in the `tests/` directory
- Use `cargo test` to run all tests
- Use `cargo insta` for snapshot testing (if applicable)

## Documentation

- README.md files should be kept up to date
- Before starting a new task, read the relevant README.md files for context
- Add a README.md to each crate / major directory. README.md files should:
  - Explain the purpose of the crate/directory
  - Describe how it fits into the overall project
  - Provide instructions and examples for using the code within
  - Explain technical details that would help a new contributor understand the code

## Important LSP Features

- **Goto Definition**: Comprehensive navigation support for all GraphQL language constructs
  - Fragment spreads to their definitions across files
  - Operation names to their definitions
  - Type references (in fragments, inline fragments, implements clauses, union members, field types, variable types)
  - Field references to their schema definitions
  - Variable references to their operation variable definitions
  - Field argument names to their schema argument definitions
  - Enum values to their enum value definitions
  - Directive names to their directive definitions
  - Directive argument names to their argument definitions
  - Works in both pure GraphQL files (.graphql, .gql) and embedded GraphQL in TypeScript/JavaScript
  - Handles TypeScript/JavaScript by extracting GraphQL and adjusting positions automatically
- **Find References**: Find all usages of GraphQL elements across the project
  - Fragment definitions → All fragment spreads using that fragment
  - Type definitions → All usages in field types, union members, implements clauses, input fields, arguments
  - Supports List and NonNull type wrappers
  - Respects include/exclude declaration context from client
  - Works across all open documents in the workspace
- **Hover**: Type information and descriptions for GraphQL elements
- **Diagnostics**: Project-wide validation with accurate error reporting
  - Project-wide unique name validation for operations and fragments
  - Correct line/column positioning for extracted GraphQL from TypeScript/JavaScript
  - Support for multiple GraphQL parsers (apollo-compiler and graphql-parser)

## Development Notes

- The project uses Rust toolchain specified in `rust-toolchain.toml`
- Main branch: `main`
- Follow conventional commit messages
- PRs should be created using `gh pr create`

## Common Commands

- `cargo build` - Build the project
- `cargo test` - Run tests
- `cargo clippy` - Lint checks
- `cargo fmt` - Format code
- `target/debug/graphql validate` - Run validation CLI
- `RUST_LOG=debug target/debug/graphql-lsp` - Run LSP with debug logging

## Logging and Tracing Strategy

### Framework
- Uses `tracing` crate for structured logging and instrumentation
- Uses `tracing-subscriber` with env-filter for log level control
- Outputs to stderr (LSP protocol uses stdout for JSON-RPC)
- ANSI colors disabled for LSP compatibility

### Log Levels
- **ERROR**: Critical failures (schema load errors, document processing failures)
- **WARN**: Non-fatal issues (missing config, no project found, stale data)
- **INFO**: High-level operations (server lifecycle, document operations, validation completion)
- **DEBUG**: Detailed operations (cache hits, timing measurements, internal state)
- **TRACE**: Reserved for deep debugging (not currently used)

### Configuration
- Set `RUST_LOG` environment variable to control log levels
- Default: `info` if not specified
- Examples:
  - `RUST_LOG=debug` - Enable debug logging
  - `RUST_LOG=graphql_lsp=debug,graphql_project=info` - Module-specific levels
  - `RUST_LOG=off` - Disable logging

### Guidelines
- Log user-facing operations at INFO (document open/save, validation start/complete)
- Log performance metrics with timing at DEBUG level
- Include context in log messages (file paths, project names, positions)
- Log errors immediately when they occur, before propagating
- Use structured fields for searchable data: `tracing::info!(uri = ?doc_uri, "message")`
- Keep log messages concise but informative
- Avoid logging sensitive data (API keys, credentials)
- Log at entry/exit of complex operations for traceability

# Instructions for Claude

- Please read and understand the contents of this file before assisting with any questions related to the GraphQL LSP project.
- Suggest updates to this file as the project evolves to keep it current and useful.
- Default to creating a new branch, committing changes, and opening a pull request for any modifications suggested.
- Don't add needless comments in source code; code should describe itself. Use comments to call out things that are subtle, confusing, or surprising.
- After finishing making changes, make sure the debug binary is built and the editor extensions are rebuilt if necessary to enable human testing.
- Put user reported bugs in .claude/notes/BUGS.md
- When starting work in a new git worktree, copy over the .claude/ directory from the main worktree to include notes and local settings.
