use std::io;
use std::path::PathBuf;
use thiserror::Error;

pub type Result<T> = std::result::Result<T, ExtractError>;

#[derive(Debug, Error)]
pub enum ExtractError {
    #[error("IO error: {0}")]
    Io(#[from] io::Error),

    #[error("Unsupported file type: {0}")]
    UnsupportedFileType(PathBuf),

    #[error("Parse error in {path}: {message}")]
    Parse { path: PathBuf, message: String },

    #[error("Language not supported: {0:?}")]
    UnsupportedLanguage(crate::Language),
}
