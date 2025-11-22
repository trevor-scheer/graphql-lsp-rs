use serde::{Deserialize, Serialize};

/// Diagnostic severity levels
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Severity {
    Error,
    Warning,
    Information,
    Hint,
}

/// Position in a document (0-indexed)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Position {
    pub line: usize,
    pub character: usize,
}

/// Range in a document
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Range {
    pub start: Position,
    pub end: Position,
}

/// Related information for a diagnostic
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelatedInfo {
    pub message: String,
    pub location: Location,
}

/// Location in a file
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Location {
    pub uri: String,
    pub range: Range,
}

/// A diagnostic message (error, warning, etc.)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnostic {
    /// Severity level
    pub severity: Severity,

    /// Range where the diagnostic applies
    pub range: Range,

    /// Diagnostic message
    pub message: String,

    /// Optional diagnostic code
    pub code: Option<String>,

    /// Source of the diagnostic (e.g., "graphql-validator")
    pub source: String,

    /// Related information
    pub related_info: Vec<RelatedInfo>,
}

impl Diagnostic {
    pub fn error(range: Range, message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Error,
            range,
            message: message.into(),
            code: None,
            source: "graphql-project".to_string(),
            related_info: Vec::new(),
        }
    }

    pub fn warning(range: Range, message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Warning,
            range,
            message: message.into(),
            code: None,
            source: "graphql-project".to_string(),
            related_info: Vec::new(),
        }
    }

    #[must_use]
    pub fn with_code(mut self, code: impl Into<String>) -> Self {
        self.code = Some(code.into());
        self
    }

    #[must_use]
    pub fn with_source(mut self, source: impl Into<String>) -> Self {
        self.source = source.into();
        self
    }

    #[must_use]
    pub fn with_related_info(mut self, info: RelatedInfo) -> Self {
        self.related_info.push(info);
        self
    }
}
