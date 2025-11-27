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

## Important LSP Features

- **Goto Definition**: Navigate from fragment spreads to their definitions across files
  - Works in both pure GraphQL files (.graphql, .gql) and embedded GraphQL in TypeScript/JavaScript
  - Handles TypeScript/JavaScript by extracting GraphQL and adjusting positions automatically
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

# Instructions for Claude

- Please read and understand the contents of this file before assisting with any questions related to the GraphQL LSP project.
- Suggest updates to this file as the project evolves to keep it current and useful.
- Default to creating a new branch, committing changes, and opening a pull request for any modifications suggested.
- Don't add needless comments in source code; code should describe itself. Use comments to call out things that are subtle, confusing, or surprising.
- After finishing making changes, make sure the debug binary is built and the editor extensions are rebuilt if necessary to enable human testing.
- Put user reported bugs in .claude/notes/BUGS.md
