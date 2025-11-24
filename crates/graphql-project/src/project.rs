use crate::{DocumentIndex, DocumentLoader, Result, SchemaIndex, SchemaLoader, Validator};
use apollo_compiler::validation::DiagnosticList;
use graphql_config::{GraphQLConfig, ProjectConfig};
use std::sync::{Arc, RwLock};

/// Main project structure that manages schema, documents, and validation
pub struct GraphQLProject {
    config: ProjectConfig,
    base_dir: Option<std::path::PathBuf>,
    schema: Arc<RwLock<Option<String>>>,
    schema_index: Arc<RwLock<SchemaIndex>>,
    document_index: Arc<RwLock<DocumentIndex>>,
}

impl GraphQLProject {
    /// Create a new project from configuration
    #[must_use]
    pub fn new(config: ProjectConfig) -> Self {
        Self {
            config,
            base_dir: None,
            schema: Arc::new(RwLock::new(None)),
            schema_index: Arc::new(RwLock::new(SchemaIndex::new())),
            document_index: Arc::new(RwLock::new(DocumentIndex::new())),
        }
    }

    /// Create a new project with a base directory for resolving relative paths
    #[must_use]
    pub fn with_base_dir(mut self, base_dir: std::path::PathBuf) -> Self {
        self.base_dir = Some(base_dir);
        self
    }

    /// Create projects from GraphQL config (single or multi-project)
    pub fn from_config(config: &GraphQLConfig) -> Result<Vec<(String, Self)>> {
        let mut projects = Vec::new();

        for (name, project_config) in config.projects() {
            let project = Self::new(project_config.clone());
            projects.push((name.to_string(), project));
        }

        Ok(projects)
    }

    /// Create projects from GraphQL config with a base directory
    pub fn from_config_with_base(
        config: &GraphQLConfig,
        base_dir: &std::path::Path,
    ) -> Result<Vec<(String, Self)>> {
        let mut projects = Vec::new();

        for (name, project_config) in config.projects() {
            let project = Self::new(project_config.clone()).with_base_dir(base_dir.to_path_buf());
            projects.push((name.to_string(), project));
        }

        Ok(projects)
    }

    /// Load the schema from configured sources
    pub async fn load_schema(&self) -> Result<()> {
        let loader = SchemaLoader::new(self.config.schema.clone());
        let schema_content = loader.load().await?;

        // Build index from schema
        let index = SchemaIndex::from_schema(&schema_content);

        // Update state
        {
            let mut schema = self.schema.write().unwrap();
            *schema = Some(schema_content);
        }

        {
            let mut schema_index = self.schema_index.write().unwrap();
            *schema_index = index;
        }

        Ok(())
    }

    /// Load documents from configured sources
    pub fn load_documents(&self) -> Result<()> {
        // Return early if no documents configured
        let Some(ref documents_config) = self.config.documents else {
            return Ok(());
        };

        let mut loader = DocumentLoader::new(documents_config.clone());

        // Set base path if we have one
        if let Some(ref base_dir) = self.base_dir {
            loader = loader.with_base_path(base_dir);
        }

        let index = loader.load()?;

        // Update document index
        {
            let mut document_index = self.document_index.write().unwrap();
            *document_index = index;
        }

        Ok(())
    }

    /// Validate a single document string against the loaded schema
    ///
    /// Returns Ok(()) if valid, or Err with a `DiagnosticList` containing errors and warnings.
    /// This validates a single GraphQL document against the project's schema.
    ///
    /// The `DiagnosticList` can be used directly for CLI output or converted to LSP diagnostics
    /// by the language server package.
    pub fn validate_document(&self, document: &str) -> std::result::Result<(), DiagnosticList> {
        let schema_index = self.schema_index.read().unwrap();
        let validator = Validator::new();
        validator.validate_document(document, &schema_index)
    }

    /// Validate a document with file location information for accurate diagnostics
    ///
    /// This method adjusts the source to include line offsets, making apollo-compiler's
    /// diagnostics show the correct file name and line numbers.
    pub fn validate_document_with_location(
        &self,
        document: &str,
        file_name: &str,
        line_offset: usize,
    ) -> std::result::Result<(), DiagnosticList> {
        let schema_index = self.schema_index.read().unwrap();
        let validator = Validator::new();
        validator.validate_document_with_location(document, &schema_index, file_name, line_offset)
    }

    /// Get the schema index for advanced operations
    #[must_use]
    pub fn get_schema_index(&self) -> SchemaIndex {
        self.schema_index.read().unwrap().clone()
    }

    /// Get schema as string
    #[must_use]
    pub fn get_schema(&self) -> Option<String> {
        self.schema.read().unwrap().clone()
    }

    /// Get document index
    #[must_use]
    pub fn get_document_index(&self) -> DocumentIndex {
        let index = self.document_index.read().unwrap();
        // Clone the inner data
        DocumentIndex {
            operations: index.operations.clone(),
            fragments: index.fragments.clone(),
        }
    }

    /// Check if a file path matches the schema configuration
    ///
    /// This is used by the LSP to determine if a file should be validated
    /// as a schema file (type definitions) or as a document (executable operations).
    #[must_use]
    pub fn is_schema_file(&self, file_path: &std::path::Path) -> bool {
        use glob::Pattern;

        let schema_patterns = self.config.schema.paths();

        // Get the file path as a string for matching
        let Some(file_str) = file_path.to_str() else {
            return false;
        };

        // Check if file matches any schema pattern
        for pattern_str in schema_patterns {
            // Resolve the pattern to an absolute path if we have a base_dir
            if let Some(ref base) = self.base_dir {
                // Normalize the pattern by stripping leading ./ if present
                let normalized_pattern = pattern_str.strip_prefix("./").unwrap_or(pattern_str);

                // Join with base directory to get absolute path
                let full_path = base.join(normalized_pattern);

                // Canonicalize both paths if possible for comparison
                let canonical_full = full_path.canonicalize().ok();
                let canonical_file = file_path.canonicalize().ok();

                // Try exact match with canonicalized paths
                if let (Some(ref full), Some(ref file)) = (&canonical_full, &canonical_file) {
                    if full == file {
                        return true;
                    }
                }

                // Also try glob pattern matching
                if let (Some(full_str), Ok(pattern)) =
                    (full_path.to_str(), Pattern::new(normalized_pattern))
                {
                    if pattern.matches(file_str) {
                        return true;
                    }
                    // Try matching against the full resolved path
                    if let Ok(full_pattern) = Pattern::new(full_str) {
                        if full_pattern.matches(file_str) {
                            return true;
                        }
                    }
                }
            } else {
                // No base directory, try matching against the pattern directly
                if let Ok(pattern) = Pattern::new(pattern_str) {
                    if pattern.matches(file_str) {
                        return true;
                    }
                }
            }
        }

        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use graphql_config::{DocumentsConfig, SchemaConfig};

    #[test]
    fn test_create_project() {
        let config = ProjectConfig {
            schema: SchemaConfig::Path("schema.graphql".to_string()),
            documents: Some(DocumentsConfig::Pattern("**/*.graphql".to_string())),
            include: None,
            exclude: None,
            extensions: None,
        };

        let project = GraphQLProject::new(config);
        assert!(project.get_schema().is_none());
    }

    #[test]
    fn test_from_single_config() {
        let config = GraphQLConfig::Single(ProjectConfig {
            schema: SchemaConfig::Path("schema.graphql".to_string()),
            documents: None,
            include: None,
            exclude: None,
            extensions: None,
        });

        let projects = GraphQLProject::from_config(&config).unwrap();
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].0, "default");
    }
}
