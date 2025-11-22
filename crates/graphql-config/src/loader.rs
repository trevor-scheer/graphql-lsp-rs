use crate::{ConfigError, GraphQLConfig, Result};
use std::fs;
use std::path::{Path, PathBuf};

/// Config file names to search for, in order of preference
const CONFIG_FILES: &[&str] = &[
    ".graphqlrc.yml",
    ".graphqlrc.yaml",
    ".graphqlrc.json",
    ".graphqlrc",
    "graphql.config.yml",
    "graphql.config.yaml",
    "graphql.config.json",
];

/// Find a GraphQL config file by walking up the directory tree from the given start directory.
/// Returns the path to the config file if found.
pub fn find_config(start_dir: &Path) -> Result<Option<PathBuf>> {
    let mut current_dir = start_dir.to_path_buf();

    loop {
        for file_name in CONFIG_FILES {
            let config_path = current_dir.join(file_name);
            if config_path.exists() && config_path.is_file() {
                return Ok(Some(config_path));
            }
        }

        // Move to parent directory
        if !current_dir.pop() {
            // Reached root without finding config
            break;
        }
    }

    Ok(None)
}

/// Load a GraphQL config from the specified path.
/// Automatically detects the format based on file extension.
pub fn load_config(path: &Path) -> Result<GraphQLConfig> {
    let contents = fs::read_to_string(path)?;
    load_config_from_str(&contents, path)
}

/// Load a GraphQL config from a string.
/// The path is used for error messages and format detection.
pub fn load_config_from_str(contents: &str, path: &Path) -> Result<GraphQLConfig> {
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("");

    let file_name = path.file_name().and_then(|name| name.to_str()).unwrap_or("");

    let config = match extension {
        "yml" | "yaml" => parse_yaml(contents, path)?,
        "json" => parse_json(contents, path)?,
        "" if file_name == ".graphqlrc" => {
            // .graphqlrc without extension - try YAML first, then JSON
            parse_yaml(contents, path).or_else(|_| parse_json(contents, path))?
        }
        _ => return Err(ConfigError::UnsupportedFormat(path.to_path_buf())),
    };

    validate_config(&config, path)?;

    Ok(config)
}

/// Parse YAML configuration
fn parse_yaml(contents: &str, path: &Path) -> Result<GraphQLConfig> {
    serde_yaml::from_str(contents).map_err(|e| {
        ConfigError::Invalid {
            path: path.to_path_buf(),
            message: format!("YAML parse error: {}", e),
        }
    })
}

/// Parse JSON configuration
fn parse_json(contents: &str, path: &Path) -> Result<GraphQLConfig> {
    serde_json::from_str(contents).map_err(|e| {
        ConfigError::Invalid {
            path: path.to_path_buf(),
            message: format!("JSON parse error: {}", e),
        }
    })
}

/// Validate the loaded configuration
fn validate_config(config: &GraphQLConfig, path: &Path) -> Result<()> {
    for (project_name, project_config) in config.projects() {
        // Validate that schema is not empty
        let schema_paths = project_config.schema.paths();
        if schema_paths.is_empty() {
            return Err(ConfigError::Invalid {
                path: path.to_path_buf(),
                message: format!("Project '{}' has empty schema configuration", project_name),
            });
        }

        // Validate that schema paths are not empty strings
        for schema_path in schema_paths {
            if schema_path.trim().is_empty() {
                return Err(ConfigError::Invalid {
                    path: path.to_path_buf(),
                    message: format!(
                        "Project '{}' has empty schema path",
                        project_name
                    ),
                });
            }
        }

        // Validate documents if present
        if let Some(ref documents) = project_config.documents {
            let doc_patterns = documents.patterns();
            if doc_patterns.is_empty() {
                return Err(ConfigError::Invalid {
                    path: path.to_path_buf(),
                    message: format!(
                        "Project '{}' has empty documents configuration",
                        project_name
                    ),
                });
            }

            for pattern in doc_patterns {
                if pattern.trim().is_empty() {
                    return Err(ConfigError::Invalid {
                        path: path.to_path_buf(),
                        message: format!(
                            "Project '{}' has empty document pattern",
                            project_name
                        ),
                    });
                }
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_load_yaml_single_project() {
        let yaml = r#"
schema: "schema.graphql"
documents: "**/*.graphql"
"#;

        let mut file = NamedTempFile::with_suffix(".yml").unwrap();
        file.write_all(yaml.as_bytes()).unwrap();
        file.flush().unwrap();

        let config = load_config(file.path()).unwrap();
        assert!(!config.is_multi_project());
        assert_eq!(config.project_count(), 1);
    }

    #[test]
    fn test_load_yaml_multi_project() {
        let yaml = r#"
projects:
  frontend:
    schema: "frontend/schema.graphql"
    documents: "frontend/**/*.ts"
  backend:
    schema: "backend/schema.graphql"
    documents: "backend/**/*.graphql"
"#;

        let mut file = NamedTempFile::with_suffix(".yml").unwrap();
        file.write_all(yaml.as_bytes()).unwrap();
        file.flush().unwrap();

        let config = load_config(file.path()).unwrap();
        assert!(config.is_multi_project());
        assert_eq!(config.project_count(), 2);
    }

    #[test]
    fn test_load_json_single_project() {
        let json = r#"
{
  "schema": "schema.graphql",
  "documents": "**/*.graphql"
}
"#;

        let mut file = NamedTempFile::with_suffix(".json").unwrap();
        file.write_all(json.as_bytes()).unwrap();
        file.flush().unwrap();

        let config = load_config(file.path()).unwrap();
        assert!(!config.is_multi_project());
    }

    #[test]
    fn test_validation_empty_schema() {
        let yaml = r#"
schema: ""
"#;

        let mut file = NamedTempFile::with_suffix(".yml").unwrap();
        file.write_all(yaml.as_bytes()).unwrap();
        file.flush().unwrap();

        let result = load_config(file.path());
        assert!(result.is_err());
    }

    #[test]
    fn test_find_config_in_current_dir() {
        let temp_dir = tempfile::tempdir().unwrap();
        let config_path = temp_dir.path().join(".graphqlrc.yml");
        fs::write(&config_path, "schema: schema.graphql").unwrap();

        let found = find_config(temp_dir.path()).unwrap();
        assert_eq!(found, Some(config_path));
    }

    #[test]
    fn test_find_config_in_parent_dir() {
        let temp_dir = tempfile::tempdir().unwrap();
        let config_path = temp_dir.path().join(".graphqlrc.yml");
        fs::write(&config_path, "schema: schema.graphql").unwrap();

        let sub_dir = temp_dir.path().join("subdir");
        fs::create_dir(&sub_dir).unwrap();

        let found = find_config(&sub_dir).unwrap();
        assert_eq!(found, Some(config_path));
    }

    #[test]
    fn test_find_config_not_found() {
        let temp_dir = tempfile::tempdir().unwrap();
        let found = find_config(temp_dir.path()).unwrap();
        assert_eq!(found, None);
    }

    #[test]
    fn test_config_file_priority() {
        let temp_dir = tempfile::tempdir().unwrap();

        // Create multiple config files
        fs::write(
            temp_dir.path().join(".graphqlrc.yml"),
            "schema: yml.graphql",
        )
        .unwrap();
        fs::write(
            temp_dir.path().join("graphql.config.json"),
            r#"{"schema": "json.graphql"}"#,
        )
        .unwrap();

        let found = find_config(temp_dir.path()).unwrap().unwrap();

        // Should prefer .graphqlrc.yml over graphql.config.json
        assert_eq!(found.file_name().unwrap(), ".graphqlrc.yml");
    }
}
