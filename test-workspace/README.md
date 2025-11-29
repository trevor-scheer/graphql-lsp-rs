# GraphQL LSP Test Workspace - Pokemon Edition

A fully configured TypeScript + GraphQL project for testing and developing the GraphQL LSP, featuring a Pokemon-themed schema with realistic project structure.

## Quick Start

```bash
# Install dependencies
npm install

# Type check TypeScript
npm run type-check

# Validate GraphQL operations
npm run graphql:validate
# Or directly:
../target/release/graphql validate
```

## Project Structure

```
test-workspace/
├── schema.graphql              # Pokemon GraphQL schema
├── graphql.config.yaml         # GraphQL LSP configuration
├── tsconfig.json              # TypeScript configuration
├── package.json               # Dependencies (Apollo Client, GraphQL, React)
└── src/
    ├── components/            # React components with embedded GraphQL
    │   ├── BattleViewer.tsx
    │   ├── PokemonCard.tsx
    │   └── TrainerProfile.tsx
    ├── fragments/             # Reusable GraphQL fragments
    │   ├── pokemon.graphql
    │   ├── trainer.graphql
    │   └── battle.graphql
    ├── queries/              # GraphQL queries
    │   ├── pokemon-queries.graphql
    │   ├── trainer-queries.graphql
    │   ├── battle-queries.graphql
    │   └── misc-queries.graphql
    ├── mutations/            # GraphQL mutations
    │   ├── trainer-mutations.graphql
    │   └── battle-mutations.graphql
    └── services/             # TypeScript services with embedded GraphQL
        ├── pokemon-service.ts
        ├── trainer-service.ts
        └── battle-service.ts
```

**Statistics**: 49 operations, 20 fragments across .graphql and .ts/.tsx files

## GraphQL LSP Features Tested

### Project-Wide Linting (NEW!)

The workspace is configured with the new project-wide lint rules:

- **`unique_names: error`** - Ensures operation and fragment names are unique across the **entire project**
  - Checks all files, not just within a single document
  - Provides rich diagnostics showing all duplicate locations

- **`unused_fields: warn`** - Detects schema fields that are never used
  - Analyzes all operations and fragments project-wide
  - Helps identify dead code in your schema

- **`deprecated_field: warn`** - Warns when using deprecated fields
  - Per-document check for deprecated field usage

### Validation

- Schema validation with apollo-compiler
- Query/mutation/subscription validation
- Fragment validation and resolution across files
- Type checking
- Variable usage checking
- Project-wide duplicate name detection

### Language Features

- **Go to Definition**: Fields, types, fragments, operations, variables, arguments
- **Find References**: Find all usages across the project
- **Hover**: Type information and descriptions
- **Completion**: Context-aware completions
- **Diagnostics**: Real-time error and warning reporting

## Configuration

GraphQL config in `graphql.config.yaml`:

```yaml
schema: schema.graphql
documents: "**/*.{graphql,gql,ts,tsx,js,jsx}"
extensions:
  project:
    lint:
      recommended: error
      unique_names: error      # Project-wide unique names
      unused_fields: warn      # Detect unused schema fields
      deprecated_field: warn   # Warn on deprecated field usage
  extractConfig:
    tagIdentifiers:
      - gql
      - graphql
    requireImport: false  # Allow gql without import
```

## Testing the LSP

### 1. Build the Project

```bash
# From project root
cargo build --release
```

### 2. Validate GraphQL

```bash
# From test-workspace directory
npm run graphql:validate
```

This runs validation and linting on all GraphQL operations.

### 3. VSCode Extension

Open this workspace in VSCode with the GraphQL LSP extension installed to test:
- Real-time diagnostics
- Go to definition
- Find references
- Hover information
- Auto-completion

### 4. Manual LSP Server

```bash
# From project root
./target/release/graphql-lsp
```

## Example Operations

The workspace includes comprehensive examples:

### Pokemon Operations
- Search by type, region, stats
- Evolution chain queries
- Detailed Pokemon info with fragments
- Batch operations

### Trainer Management
- Trainer profiles with Pokemon teams
- Badge collections
- Trainer battles

### Battle System
- Battle creation and updates
- Turn-by-turn battle logs
- Battle history queries

### Mutations
- Create/update trainers
- Add Pokemon to teams
- Start/end battles
- Award badges

## Schema Overview

The `schema.graphql` defines a comprehensive Pokemon API:

- **Types**: Pokemon, Trainer, Battle, Stats, Moves, Abilities, Evolutions
- **Queries**: Search, filters, pagination
- **Mutations**: CRUD operations for trainers and battles
- **Enums**: PokemonType, Region, BattleStatus, BattleOutcome
- **Interfaces**: BattleAction, EvolutionRequirement

## Dependencies

```json
{
  "dependencies": {
    "@apollo/client": "^3.11.8",
    "graphql": "^16.9.0",
    "graphql-tag": "^2.12.6",
    "react": "^18.3.1"
  },
  "devDependencies": {
    "@types/node": "^22.10.1",
    "@types/react": "^18.3.12",
    "typescript": "^5.7.2"
  }
}
```

## Development

This workspace serves as:
1. **Test suite** for GraphQL LSP features
2. **Example project** showing best practices
3. **Development environment** for LSP work
4. **Regression testing** for new features

When adding new LSP features, add corresponding test cases to this workspace!
