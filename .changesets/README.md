# Changesets

This directory contains changesets for managing package versioning and changelog generation.

## What are Changesets?

Changesets are a way to track which packages need version bumps and why. Each changeset is a markdown file that describes:
- Which packages are affected
- What type of change it is (major, minor, patch)
- A description of the change for the changelog

## Installation

Install the changesets CLI:

```bash
cargo install changesets
```

## Workflow

### 1. Making Changes

When you make a change that should be released:

```bash
changesets add
```

This will prompt you to:
- Select which packages are affected
- Choose the bump type (major, minor, or patch)
- Write a description of the change

This creates a new changeset file in `.changesets/` with a unique ID.

### 2. Versioning and Releasing

When ready to release:

```bash
# Consume all changesets, bump versions, and update CHANGELOGs
changesets version

# Review the changes, then commit
git add .
git commit -m "chore: Version packages"

# Publish to crates.io (if configured)
changesets publish
```

## Example Changeset

A changeset file might look like:

```markdown
---
"graphql-lsp": minor
"graphql-language-service": patch
---

Add goto definition support for field references. This includes internal improvements to the language service for better position tracking.
```

## Configuration

Configuration is stored in [config.json](./config.json) which defines:
- Package locations
- Changelog paths
- Publishing settings

## Best Practices

- Create a changeset for every PR that changes functionality
- Write clear, user-facing descriptions
- Group related changes in a single changeset when possible
- Run `changesets version` on the main branch when ready to release
