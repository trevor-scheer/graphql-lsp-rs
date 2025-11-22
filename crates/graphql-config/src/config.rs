use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Top-level GraphQL configuration.
/// Either a single project or multiple named projects.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum GraphQLConfig {
    /// Single project configuration
    Single(ProjectConfig),
    /// Multi-project configuration
    Multi {
        projects: HashMap<String, ProjectConfig>,
    },
}

impl GraphQLConfig {
    /// Get all projects as an iterator.
    /// For single project configs, yields a single item with name "default".
    #[must_use]
    pub fn projects(&self) -> Box<dyn Iterator<Item = (&str, &ProjectConfig)> + '_> {
        match self {
            Self::Single(config) => Box::new(std::iter::once(("default", config))),
            Self::Multi { projects } => Box::new(
                projects
                    .iter()
                    .map(|(name, config)| (name.as_str(), config)),
            ),
        }
    }

    /// Get a specific project by name.
    /// For single project configs, returns the project if name is "default".
    #[must_use]
    pub fn get_project(&self, name: &str) -> Option<&ProjectConfig> {
        match self {
            Self::Single(config) if name == "default" => Some(config),
            Self::Single(_) => None,
            Self::Multi { projects } => projects.get(name),
        }
    }

    /// Check if this is a multi-project configuration
    #[must_use]
    pub const fn is_multi_project(&self) -> bool {
        matches!(self, Self::Multi { .. })
    }

    /// Get the number of projects
    #[must_use]
    pub fn project_count(&self) -> usize {
        match self {
            Self::Single(_) => 1,
            Self::Multi { projects } => projects.len(),
        }
    }
}

/// Configuration for a single GraphQL project
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectConfig {
    /// Schema source(s)
    pub schema: SchemaConfig,

    /// Document patterns (queries, mutations, fragments)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub documents: Option<DocumentsConfig>,

    /// File patterns to include
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include: Option<Vec<String>>,

    /// File patterns to exclude
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exclude: Option<Vec<String>>,

    /// Tool-specific extensions
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extensions: Option<HashMap<String, serde_json::Value>>,
}

/// Schema source configuration
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SchemaConfig {
    /// Single file path or glob pattern
    Path(String),
    /// Multiple file paths or glob patterns
    Paths(Vec<String>),
}

impl SchemaConfig {
    /// Get all schema paths/patterns as a slice
    #[must_use]
    pub fn paths(&self) -> Vec<&str> {
        match self {
            Self::Path(path) => vec![path.as_str()],
            Self::Paths(paths) => paths.iter().map(String::as_str).collect(),
        }
    }

    /// Check if this schema config contains URLs (HTTP/HTTPS)
    #[must_use]
    pub fn has_remote_schema(&self) -> bool {
        self.paths()
            .iter()
            .any(|p| p.starts_with("http://") || p.starts_with("https://"))
    }
}

/// Documents source configuration
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum DocumentsConfig {
    /// Single pattern
    Pattern(String),
    /// Multiple patterns
    Patterns(Vec<String>),
}

impl DocumentsConfig {
    /// Get all document patterns as a slice
    #[must_use]
    pub fn patterns(&self) -> Vec<&str> {
        match self {
            Self::Pattern(pattern) => vec![pattern.as_str()],
            Self::Patterns(patterns) => patterns.iter().map(String::as_str).collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_single_project_config() {
        let config = GraphQLConfig::Single(ProjectConfig {
            schema: SchemaConfig::Path("schema.graphql".to_string()),
            documents: Some(DocumentsConfig::Pattern("**/*.graphql".to_string())),
            include: None,
            exclude: None,
            extensions: None,
        });

        assert!(!config.is_multi_project());
        assert_eq!(config.project_count(), 1);
        assert!(config.get_project("default").is_some());
        assert!(config.get_project("other").is_none());
    }

    #[test]
    fn test_multi_project_config() {
        let mut projects = HashMap::new();
        projects.insert(
            "frontend".to_string(),
            ProjectConfig {
                schema: SchemaConfig::Path("frontend/schema.graphql".to_string()),
                documents: Some(DocumentsConfig::Pattern("frontend/**/*.ts".to_string())),
                include: None,
                exclude: None,
                extensions: None,
            },
        );
        projects.insert(
            "backend".to_string(),
            ProjectConfig {
                schema: SchemaConfig::Path("backend/schema.graphql".to_string()),
                documents: Some(DocumentsConfig::Pattern("backend/**/*.graphql".to_string())),
                include: None,
                exclude: None,
                extensions: None,
            },
        );

        let config = GraphQLConfig::Multi { projects };

        assert!(config.is_multi_project());
        assert_eq!(config.project_count(), 2);
        assert!(config.get_project("frontend").is_some());
        assert!(config.get_project("backend").is_some());
        assert!(config.get_project("default").is_none());
    }

    #[test]
    fn test_schema_config_paths() {
        let single = SchemaConfig::Path("schema.graphql".to_string());
        assert_eq!(single.paths(), vec!["schema.graphql"]);

        let multiple = SchemaConfig::Paths(vec![
            "schema1.graphql".to_string(),
            "schema2.graphql".to_string(),
        ]);
        assert_eq!(multiple.paths(), vec!["schema1.graphql", "schema2.graphql"]);
    }

    #[test]
    fn test_remote_schema_detection() {
        let local = SchemaConfig::Path("schema.graphql".to_string());
        assert!(!local.has_remote_schema());

        let remote = SchemaConfig::Path("https://api.example.com/graphql".to_string());
        assert!(remote.has_remote_schema());

        let mixed = SchemaConfig::Paths(vec![
            "schema.graphql".to_string(),
            "https://api.example.com/graphql".to_string(),
        ]);
        assert!(mixed.has_remote_schema());
    }

    #[test]
    fn test_documents_config_patterns() {
        let single = DocumentsConfig::Pattern("**/*.graphql".to_string());
        assert_eq!(single.patterns(), vec!["**/*.graphql"]);

        let multiple =
            DocumentsConfig::Patterns(vec!["**/*.graphql".to_string(), "**/*.ts".to_string()]);
        assert_eq!(multiple.patterns(), vec!["**/*.graphql", "**/*.ts"]);
    }
}
