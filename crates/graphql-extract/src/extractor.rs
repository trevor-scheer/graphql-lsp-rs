use crate::{ExtractError, Language, Result, SourceLocation, Position, Range};
use std::fs;
use std::path::Path;

/// Configuration for GraphQL extraction
#[derive(Debug, Clone)]
pub struct ExtractConfig {
    /// Magic comment to look for (default: "GraphQL")
    /// Matches comments like: /* GraphQL */ `query { ... }`
    pub magic_comment: String,

    /// Tag identifiers to extract (default: ["gql", "graphql"])
    /// Matches: gql`query { ... }` or graphql`query { ... }`
    pub tag_identifiers: Vec<String>,

    /// Module names to recognize as GraphQL sources
    /// Default includes: graphql-tag, @apollo/client, etc.
    pub modules: Vec<String>,

    /// Allow extraction without imports (global identifiers)
    pub allow_global_identifiers: bool,
}

impl Default for ExtractConfig {
    fn default() -> Self {
        Self {
            magic_comment: "GraphQL".to_string(),
            tag_identifiers: vec!["gql".to_string(), "graphql".to_string()],
            modules: vec![
                "graphql-tag".to_string(),
                "@apollo/client".to_string(),
                "apollo-server".to_string(),
                "apollo-server-express".to_string(),
                "gatsby".to_string(),
                "react-relay".to_string(),
            ],
            allow_global_identifiers: false,
        }
    }
}

/// Extracted GraphQL content with source location
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractedGraphQL {
    /// The extracted GraphQL source code
    pub source: String,

    /// Source location in the original file
    pub location: SourceLocation,

    /// The tag name used (e.g., "gql", "graphql"), if any
    pub tag_name: Option<String>,
}

/// Extract GraphQL from a file
pub fn extract_from_file(path: &Path, config: &ExtractConfig) -> Result<Vec<ExtractedGraphQL>> {
    let language = Language::from_path(path)
        .ok_or_else(|| ExtractError::UnsupportedFileType(path.to_path_buf()))?;

    let source = fs::read_to_string(path)?;
    extract_from_source(&source, language, config)
}

/// Extract GraphQL from source code string
pub fn extract_from_source(
    source: &str,
    language: Language,
    config: &ExtractConfig,
) -> Result<Vec<ExtractedGraphQL>> {
    match language {
        Language::GraphQL => {
            // Raw GraphQL file - return entire content
            Ok(vec![ExtractedGraphQL {
                source: source.to_string(),
                location: SourceLocation::new(
                    0,
                    source.len(),
                    Range::new(
                        Position::new(0, 0),
                        position_from_offset(source, source.len()),
                    ),
                ),
                tag_name: None,
            }])
        }
        Language::TypeScript | Language::JavaScript => {
            #[cfg(feature = "typescript")]
            {
                extract_from_js_family(source, language, config)
            }
            #[cfg(not(feature = "typescript"))]
            {
                Err(ExtractError::UnsupportedLanguage(language))
            }
        }
        _ => Err(ExtractError::UnsupportedLanguage(language)),
    }
}

#[cfg(feature = "typescript")]
fn extract_from_js_family(
    _source: &str,
    _language: Language,
    _config: &ExtractConfig,
) -> Result<Vec<ExtractedGraphQL>> {
    // TODO: Implement SWC-based extraction in Phase 4
    // This is a placeholder for now
    Ok(vec![])
}

/// Calculate position from byte offset
fn position_from_offset(source: &str, offset: usize) -> Position {
    let mut line = 0;
    let mut column = 0;

    for (idx, ch) in source.char_indices() {
        if idx >= offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            column = 0;
        } else {
            column += ch.len_utf16();
        }
    }

    Position::new(line, column)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = ExtractConfig::default();
        assert_eq!(config.magic_comment, "GraphQL");
        assert!(config.tag_identifiers.contains(&"gql".to_string()));
        assert!(config.modules.contains(&"graphql-tag".to_string()));
    }

    #[test]
    fn test_extract_raw_graphql() {
        let source = r#"
query GetUser {
  user {
    id
    name
  }
}
"#;
        let config = ExtractConfig::default();
        let result = extract_from_source(source, Language::GraphQL, &config).unwrap();

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].source, source);
        assert_eq!(result[0].tag_name, None);
    }

    #[test]
    fn test_position_from_offset() {
        let source = "line 0\nline 1\nline 2";
        let pos = position_from_offset(source, 0);
        assert_eq!(pos, Position::new(0, 0));

        let pos = position_from_offset(source, 7); // Start of "line 1"
        assert_eq!(pos, Position::new(1, 0));

        let pos = position_from_offset(source, 14); // Start of "line 2"
        assert_eq!(pos, Position::new(2, 0));
    }
}
