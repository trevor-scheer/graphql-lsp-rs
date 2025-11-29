mod completion;
mod diagnostics;
mod document;
mod error;
mod find_references;
mod goto_definition;
mod hover;
mod index;
mod lint;
mod project;
mod schema;
mod validation;

// Export diagnostics types for LSP package to use when converting DiagnosticList
pub use completion::{CompletionItem, CompletionItemKind, CompletionProvider};
pub use diagnostics::{Diagnostic, Position, Range, RelatedInfo, Severity};
pub use document::DocumentLoader;
pub use error::{ProjectError, Result};
pub use find_references::{FindReferencesProvider, ReferenceLocation};
pub use goto_definition::{DefinitionLocation, GotoDefinitionProvider};
pub use hover::{HoverInfo, HoverProvider};
pub use index::{
    DocumentIndex, ExtractedBlock, FieldDefinitionLocation, FragmentInfo, OperationInfo,
    OperationType, SchemaIndex, TypeInfo,
};
pub use lint::{LintConfig, LintRuleConfig, LintSeverity, Linter};
pub use project::GraphQLProject;
pub use schema::SchemaLoader;
pub use validation::Validator;

// Re-export common types from dependencies
pub use apollo_compiler::validation::DiagnosticList;
pub use apollo_parser::SyntaxTree;
pub use graphql_config::{GraphQLConfig, ProjectConfig};
