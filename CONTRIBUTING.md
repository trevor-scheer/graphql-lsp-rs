# Contributing to GraphQL LSP

Thank you for your interest in contributing! This document provides guidelines and instructions for contributing to this project.

## Development Setup

### Prerequisites

- Rust 1.70 or later
- Git

### Getting Started

1. Fork the repository
2. Clone your fork:
   ```bash
   git clone https://github.com/YOUR_USERNAME/graphql-lsp.git
   cd graphql-lsp
   ```

3. Build the project:
   ```bash
   cargo build --workspace
   ```

4. Run tests:
   ```bash
   cargo test --workspace
   ```

## Code Style

### Formatting

We use `rustfmt` for code formatting. Run before committing:

```bash
cargo fmt --all
```

### Linting

We enforce strict linting with Clippy:

```bash
cargo clippy --workspace --all-targets --all-features
```

All Clippy warnings must be resolved before merging.

### Code Quality Standards

- Write clear, self-documenting code
- Add comments only where logic isn't self-evident
- Keep functions small and focused
- Prefer composition over inheritance
- Use descriptive variable and function names

## Testing

### Running Tests

```bash
# Run all tests
cargo test --workspace

# Run tests for a specific crate
cargo test -p graphql-config

# Run a specific test
cargo test test_name
```

### Writing Tests

- Write unit tests for all new functionality
- Add integration tests for complex features
- Use meaningful test names that describe what's being tested
- Follow the Arrange-Act-Assert pattern

## Pull Request Process

1. **Create a feature branch**:
   ```bash
   git checkout -b feature/your-feature-name
   ```

2. **Make your changes**:
   - Write code following our style guidelines
   - Add tests for new functionality
   - Update documentation as needed

3. **Run the full test suite**:
   ```bash
   cargo test --workspace
   cargo clippy --workspace --all-targets --all-features
   cargo fmt --all -- --check
   ```

4. **Commit your changes**:
   ```bash
   git add .
   git commit -m "feat: add new feature"
   ```

   Use conventional commit messages:
   - `feat:` new features
   - `fix:` bug fixes
   - `docs:` documentation changes
   - `test:` test additions/changes
   - `refactor:` code refactoring
   - `chore:` maintenance tasks

5. **Push and create a PR**:
   ```bash
   git push origin feature/your-feature-name
   ```

   Then create a pull request on GitHub.

## Code Review

All submissions require review before merging. We look for:

- Code quality and clarity
- Test coverage
- Documentation updates
- Adherence to project conventions
- No breaking changes (unless intentional and documented)

## Project Structure

```
graphql-lsp/
├── crates/
│   ├── graphql-config/       # Configuration parsing
│   ├── graphql-extract/      # GraphQL extraction from source files
│   ├── graphql-project/      # Core validation and indexing
│   ├── graphql-lsp/          # LSP server
│   └── graphql-cli/          # CLI tool
├── .github/workflows/        # CI/CD workflows
└── docs/                     # Documentation
```

## Getting Help

- Check the [project plan](.claude/project-plan.md) for architecture details
- Open an issue for questions or discussions
- Join our community discussions

## License

By contributing, you agree that your contributions will be licensed under the same license as the project (MIT OR Apache-2.0).
