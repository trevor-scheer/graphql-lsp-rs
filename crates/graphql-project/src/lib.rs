mod diagnostics;
mod document;
mod error;
mod index;
mod project;
mod schema;
mod validation;

pub use diagnostics::{Diagnostic, Position, Range, RelatedInfo, Severity};
pub use document::DocumentLoader;
pub use error::{ProjectError, Result};
pub use index::{DocumentIndex, FragmentInfo, OperationInfo, OperationType, SchemaIndex, TypeInfo};
pub use project::GraphQLProject;
pub use schema::SchemaLoader;
pub use validation::Validator;

// Re-export common types from dependencies
pub use graphql_config::{GraphQLConfig, ProjectConfig};
