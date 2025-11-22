use std::io;
use std::path::PathBuf;
use thiserror::Error;

pub type Result<T> = std::result::Result<T, ConfigError>;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("IO error: {0}")]
    Io(#[from] io::Error),

    #[error("YAML parse error: {0}")]
    YamlParse(#[from] serde_yaml::Error),

    #[error("JSON parse error: {0}")]
    JsonParse(#[from] serde_json::Error),

    #[error("Configuration file not found")]
    NotFound,

    #[error("Invalid configuration at {path}: {message}")]
    Invalid { path: PathBuf, message: String },

    #[error("Unsupported config file format: {0}")]
    UnsupportedFormat(PathBuf),

    #[error("Configuration validation error: {0}")]
    Validation(String),
}
