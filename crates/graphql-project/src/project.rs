use crate::{
    Diagnostic, DocumentIndex, ProjectError, Result, SchemaIndex, SchemaLoader,
};
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
    pub fn from_config(config: GraphQLConfig) -> Result<Vec<(String, Self)>> {
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
    pub async fn load_documents(&self) -> Result<()> {
        // TODO: Implement document loading
        // - Use glob patterns from config.documents
        // - Extract GraphQL from files using graphql-extract
        // - Build document index
        Ok(())
    }

    /// Validate all documents against the schema
    pub fn validate(&self) -> Vec<Diagnostic> {
        // TODO: Implement full validation
        // For now, return empty diagnostics
        self.diagnostics.read().unwrap().clone()
    }

    /// Get schema as string
    pub fn get_schema(&self) -> Option<String> {
        self.schema.read().unwrap().clone()
    }

    /// Get all diagnostics
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

        let projects = GraphQLProject::from_config(config).unwrap();
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].0, "default");
    }
}
