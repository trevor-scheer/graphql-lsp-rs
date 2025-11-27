mod diagnostics;
mod document;
mod error;
mod goto_definition;
mod hover;
mod index;
mod project;
mod schema;
mod validation;

// Export diagnostics types for LSP package to use when converting DiagnosticList
pub use diagnostics::{Diagnostic, Position, Range, RelatedInfo, Severity};
pub use document::DocumentLoader;
pub use error::{ProjectError, Result};
pub use goto_definition::{DefinitionLocation, GotoDefinitionProvider};
pub use hover::{HoverInfo, HoverProvider};
pub use index::{
    DocumentIndex, FieldDefinitionLocation, FragmentInfo, OperationInfo, OperationType,
    SchemaIndex, TypeInfo,
};
pub use project::GraphQLProject;
pub use schema::SchemaLoader;
pub use validation::Validator;

// Re-export common types from dependencies
pub use apollo_compiler::validation::DiagnosticList;
pub use graphql_config::{GraphQLConfig, ProjectConfig};
