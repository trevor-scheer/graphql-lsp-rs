mod diagnostics;
mod error;
mod index;
mod project;
mod schema;
mod validation;

pub use diagnostics::{Diagnostic, RelatedInfo, Severity};
pub use error::{ProjectError, Result};
pub use index::{DocumentIndex, SchemaIndex};
pub use project::GraphQLProject;
pub use schema::SchemaLoader;

// Re-export common types from dependencies
pub use graphql_config::{GraphQLConfig, ProjectConfig};
