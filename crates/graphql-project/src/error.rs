use thiserror::Error;

pub type Result<T> = std::result::Result<T, ProjectError>;

#[derive(Debug, Error)]
pub enum ProjectError {
    #[error("Configuration error: {0}")]
    Config(#[from] graphql_config::ConfigError),

    #[error("Extraction error: {0}")]
    Extract(#[from] graphql_extract::ExtractError),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Schema loading error: {0}")]
    SchemaLoad(String),

    #[error("Schema parse error: {0}")]
    SchemaParse(String),

    #[error("Document loading error: {0}")]
    DocumentLoad(String),

    #[error("Document parse error: {0}")]
    DocumentParse(String),

    #[error("Validation error: {0}")]
    Validation(String),

    #[error("HTTP request error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("Project not found: {0}")]
    ProjectNotFound(String),
}
