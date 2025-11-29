# Release Process

This document describes how releases are created and distributed for the GraphQL CLI.

## Overview

Binary releases are automatically built and published using [cargo-dist](https://github.com/axodotdev/cargo-dist) through GitHub Actions. When a version tag is pushed, the release workflow builds optimized binaries for multiple platforms and creates a GitHub Release.

## Supported Platforms

The CLI is distributed for the following platforms:

- **macOS**
  - Intel (x86_64-apple-darwin)
  - Apple Silicon (aarch64-apple-darwin)
- **Linux**
  - x86_64 (x86_64-unknown-linux-gnu)
  - ARM64 (aarch64-unknown-linux-gnu)
- **Windows**
  - x86_64 (x86_64-pc-windows-msvc)

## Creating a Release

### 1. Update Version

Update the version in the workspace `Cargo.toml`:

```toml
[workspace.package]
version = "0.2.0"
```

### 2. Update CHANGELOG (if exists)

Document the changes in this release.

### 3. Commit the Version Bump

```bash
git add Cargo.toml Cargo.lock
git commit -m "chore: bump version to 0.2.0"
```

### 4. Create and Push Tag

```bash
git tag v0.2.0
git push origin main --tags
```

### 5. Automatic Build and Release

The GitHub Actions workflow will automatically:
1. Build binaries for all supported platforms
2. Generate checksums (SHA256) for verification
3. Create shell and PowerShell installers
4. Create a GitHub Release with all artifacts
5. Upload all binaries and installers

## Release Artifacts

Each release includes:

### Binaries
- Platform-specific compressed archives (.tar.xz for Unix, .zip for Windows)
- Each archive contains:
  - The `graphql` binary
  - README.md

### Installers
- `graphql-cli-installer.sh` - Shell installer for macOS/Linux
- `graphql-cli-installer.ps1` - PowerShell installer for Windows

### Checksums
- Individual `.sha256` files for each binary archive
- `sha256.sum` - Combined checksums file

### Source
- `source.tar.gz` - Source code archive

## Installation Methods

Users can install the CLI in several ways:

### 1. Shell/PowerShell Installer (Recommended)

**macOS/Linux:**
```bash
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/trevor-scheer/graphql-lsp/releases/latest/download/graphql-cli-installer.sh | sh
```

**Windows:**
```powershell
irm https://github.com/trevor-scheer/graphql-lsp/releases/latest/download/graphql-cli-installer.ps1 | iex
```

### 2. Direct Download

Download the appropriate binary from the [releases page](https://github.com/trevor-scheer/graphql-lsp/releases).

### 3. From Source

```bash
cargo install --git https://github.com/trevor-scheer/graphql-lsp graphql-cli
```

## Binary Optimization

Binaries are built with the `dist` profile which includes:
- Thin LTO (Link Time Optimization)
- Size optimization (`opt-level = "z"`)
- Debug symbol stripping
- Single codegen unit for maximum optimization

## Testing Releases

### Test Locally Before Release

```bash
# Plan a release (doesn't build, just shows what would happen)
dist plan

# Build release artifacts locally
dist build
```

### Test on Pull Requests

The release workflow runs in "plan" mode on pull requests to validate the configuration without publishing.

## Troubleshooting

### Release Workflow Failed

1. Check the GitHub Actions logs
2. Verify all platforms built successfully
3. Ensure the tag matches the version in Cargo.toml
4. Check that cargo-dist version is compatible

### Binary Size Too Large

The dist profile is already optimized for size. If binaries are still too large:
1. Check for unnecessary dependencies
2. Consider using `upx` for additional compression
3. Review feature flags to minimize included code

## Configuration

Release configuration is in [`dist-workspace.toml`](dist-workspace.toml):
- Target platforms
- Installer types
- cargo-dist version
- CI settings

Build optimization is in the workspace [`Cargo.toml`](Cargo.toml):
- `[profile.dist]` section
- LTO, optimization level, stripping, etc.
