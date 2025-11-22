use crate::{Diagnostic, DocumentIndex, DocumentLoader, Result, SchemaIndex, SchemaLoader, Validator};
use graphql_config::{GraphQLConfig, ProjectConfig};
use std::sync::{Arc, RwLock};

/// Main project structure that manages schema, documents, and validation
pub struct GraphQLProject {
    config: ProjectConfig,
    schema: Arc<RwLock<Option<String>>>,
    schema_index: Arc<RwLock<SchemaIndex>>,
    document_index: Arc<RwLock<DocumentIndex>>,
    diagnostics: Arc<RwLock<Vec<Diagnostic>>>,
}

impl GraphQLProject {
    /// Create a new project from configuration
    #[must_use]
    pub fn new(config: ProjectConfig) -> Self {
        Self {
            config,
            schema: Arc::new(RwLock::new(None)),
            schema_index: Arc::new(RwLock::new(SchemaIndex::new())),
            document_index: Arc::new(RwLock::new(DocumentIndex::new())),
            diagnostics: Arc::new(RwLock::new(Vec::new())),
        }
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

        let loader = DocumentLoader::new(documents_config.clone());
        let index = loader.load()?;

        // Update document index
        {
            let mut document_index = self.document_index.write().unwrap();
            *document_index = index;
        }

        Ok(())
    }

    /// Validate all loaded documents against the schema
    ///
    /// Returns a list of validation diagnostics (errors and warnings) from all documents.
    /// Currently returns empty diagnostics - will be implemented when document loading is complete.
    #[must_use]
    pub fn validate(&self) -> Vec<Diagnostic> {
        // TODO: Iterate over all documents in document_index and validate each
        // For now, return stored diagnostics
        self.diagnostics.read().unwrap().clone()
    }

    /// Validate a single document string against the loaded schema
    ///
    /// Returns a list of validation diagnostics (errors and warnings).
    /// This validates a single GraphQL document against the project's schema.
    #[must_use]
    pub fn validate_document(&self, document: &str) -> Vec<Diagnostic> {
        let schema_index = self.schema_index.read().unwrap();
        let validator = Validator::new();
        validator.validate_document(document, &schema_index)
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

    /// Get all diagnostics
    #[must_use]
    pub fn get_diagnostics(&self) -> Vec<Diagnostic> {
        self.diagnostics.read().unwrap().clone()
    }

    /// Clear all diagnostics
    pub fn clear_diagnostics(&self) {
        let mut diagnostics = self.diagnostics.write().unwrap();
        diagnostics.clear();
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
