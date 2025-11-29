# Pre-commit Hooks

This directory contains the git pre-commit hook configuration.

## How it works

- `Cargo.toml` configures [cargo-husky](https://github.com/rhysd/cargo-husky) to automatically install git hooks during build
- cargo-husky runs `cargo clippy` and `cargo fmt --check` on Rust code
- `pre-commit.sh` contains additional custom checks (e.g., linting VSCode extension TypeScript files)

## Setup

The hooks are automatically installed when you build the project with `cargo build`.

To manually update the git hook after modifying `pre-commit.sh`, rebuild the project or manually edit `.git/hooks/pre-commit` to source the custom script.

## Custom checks

- **VSCode extension formatting**: Runs `prettier --check` when staging changes to `editors/vscode/`
- **VSCode extension linting**: Runs `oxlint` when staging changes to `editors/vscode/`
