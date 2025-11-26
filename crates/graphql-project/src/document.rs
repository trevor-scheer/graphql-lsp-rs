use crate::{DocumentIndex, FragmentInfo, OperationInfo, OperationType, ProjectError, Result};
use apollo_parser::{cst, Parser};
use graphql_config::DocumentsConfig;
use graphql_extract::{extract_from_file, ExtractConfig};
use std::path::{Path, PathBuf};

/// Document loader for loading GraphQL operations and fragments from various sources
pub struct DocumentLoader {
    config: DocumentsConfig,
    base_path: Option<PathBuf>,
    extract_config: ExtractConfig,
}

impl DocumentLoader {
    #[must_use]
    pub fn new(config: DocumentsConfig) -> Self {
        Self {
            config,
            base_path: None,
            extract_config: ExtractConfig::default(),
        }
    }

    #[must_use]
    pub fn with_base_path(mut self, path: impl AsRef<Path>) -> Self {
        self.base_path = Some(path.as_ref().to_path_buf());
        self
    }

    #[must_use]
    pub fn with_extract_config(mut self, config: ExtractConfig) -> Self {
        self.extract_config = config;
        self
    }

    /// Load all documents and build an index
    pub fn load(&self) -> Result<DocumentIndex> {
        let mut index = DocumentIndex::new();

        for pattern in self.config.patterns() {
            let paths = self.find_files(pattern)?;

            for path in paths {
                if let Err(e) = self.load_file(&path, &mut index) {
                    // Log error but continue with other files
                    eprintln!("Warning: Failed to load {}: {}", path.display(), e);
                }
            }
        }

        Ok(index)
    }

    /// Find files matching a glob pattern
    fn find_files(&self, pattern: &str) -> Result<Vec<PathBuf>> {
        // Expand brace patterns like {ts,tsx} since glob crate doesn't support them
        let expanded_patterns = Self::expand_braces(pattern);

        let mut files = Vec::new();

        for expanded_pattern in expanded_patterns {
            let full_pattern = self.base_path.as_ref().map_or_else(
                || expanded_pattern.clone(),
                |base| base.join(&expanded_pattern).display().to_string(),
            );

            for entry in glob::glob(&full_pattern)
                .map_err(|e| ProjectError::DocumentLoad(format!("Invalid glob pattern: {e}")))?
            {
                match entry {
                    Ok(path) if path.is_file() => {
                        if !files.contains(&path) {
                            files.push(path);
                        }
                    }
                    Ok(_) => {} // Skip directories
                    Err(e) => {
                        return Err(ProjectError::DocumentLoad(format!("Glob error: {e}")));
                    }
                }
            }
        }

        Ok(files)
    }

    /// Expand brace patterns like {ts,tsx} into multiple patterns
    fn expand_braces(pattern: &str) -> Vec<String> {
        // Simple brace expansion for patterns like **/*.{ts,tsx}
        if let Some(start) = pattern.find('{') {
            if let Some(end) = pattern.find('}') {
                let before = &pattern[..start];
                let after = &pattern[end + 1..];
                let options = &pattern[start + 1..end];

                return options
                    .split(',')
                    .map(|opt| format!("{before}{opt}{after}"))
                    .collect();
            }
        }

        vec![pattern.to_string()]
    }

    /// Load a single file and add operations/fragments to the index
    fn load_file(&self, path: &Path, index: &mut DocumentIndex) -> Result<()> {
        // Extract GraphQL from the file
        let extracted = extract_from_file(path, &self.extract_config)
            .map_err(|e| ProjectError::DocumentLoad(format!("Extract error: {e}")))?;

        let file_path = path.display().to_string();

        // Parse each extracted GraphQL document
        for item in extracted {
            Self::parse_and_index(&item, &file_path, index);
        }

        Ok(())
    }

    /// Parse GraphQL source and add operations/fragments to index
    fn parse_and_index(
        item: &graphql_extract::ExtractedGraphQL,
        file_path: &str,
        index: &mut DocumentIndex,
    ) {
        use apollo_parser::cst::CstNode;

        let source = &item.source;
        // Get the starting position in the original file for this extracted block
        let base_line = item.location.range.start.line;
        let base_column = item.location.range.start.column;

        let parser = Parser::new(source);
        let tree = parser.parse();

        // Skip if there are syntax errors
        if tree.errors().len() > 0 {
            return; // Silently skip invalid documents
        }

        let document = tree.document();

        for definition in document.definitions() {
            match definition {
                cst::Definition::OperationDefinition(op) => {
                    let operation_type = match op.operation_type() {
                        Some(op_type) if op_type.query_token().is_some() => OperationType::Query,
                        Some(op_type) if op_type.mutation_token().is_some() => {
                            OperationType::Mutation
                        }
                        Some(op_type) if op_type.subscription_token().is_some() => {
                            OperationType::Subscription
                        }
                        _ => OperationType::Query, // Default to query
                    };

                    let (name, line, column) = op.name().map_or((None, 0, 0), |name_node| {
                        let name_str = name_node.text().to_string();
                        let syntax_node = name_node.syntax();
                        let offset: usize = syntax_node.text_range().start().into();
                        let (rel_line, rel_col) = Self::offset_to_line_col(source, offset);

                        // Add the base position from the extracted block
                        let abs_line = base_line + rel_line;
                        let abs_col = if rel_line == 0 {
                            base_column + rel_col
                        } else {
                            rel_col
                        };

                        (Some(name_str), abs_line, abs_col)
                    });

                    let info = OperationInfo {
                        name: name.clone(),
                        operation_type,
                        file_path: file_path.to_string(),
                        line,
                        column,
                    };

                    index.add_operation(name, info);
                }
                cst::Definition::FragmentDefinition(frag) => {
                    if let (Some(name_node), Some(type_cond)) =
                        (frag.fragment_name(), frag.type_condition())
                    {
                        let name = name_node
                            .name()
                            .map_or_else(String::new, |n| n.text().to_string());
                        let type_condition = type_cond
                            .named_type()
                            .and_then(|nt| nt.name())
                            .map_or_else(String::new, |n| n.text().to_string());

                        // Get position of fragment name
                        let (line, column) = name_node.name().map_or((0, 0), |name_token| {
                            let syntax_node = name_token.syntax();
                            let offset: usize = syntax_node.text_range().start().into();
                            let (rel_line, rel_col) = Self::offset_to_line_col(source, offset);

                            // Add the base position from the extracted block
                            let abs_line = base_line + rel_line;
                            let abs_col = if rel_line == 0 {
                                base_column + rel_col
                            } else {
                                rel_col
                            };

                            (abs_line, abs_col)
                        });

                        let info = FragmentInfo {
                            name: name.clone(),
                            type_condition,
                            file_path: file_path.to_string(),
                            line,
                            column,
                        };

                        index.add_fragment(name, info);
                    }
                }
                _ => {} // Skip schema definitions in document files
            }
        }
    }

    /// Convert a byte offset to a line and column (0-indexed)
    fn offset_to_line_col(document: &str, offset: usize) -> (usize, usize) {
        let mut line = 0;
        let mut col = 0;
        let mut current_offset = 0;

        for ch in document.chars() {
            if current_offset >= offset {
                break;
            }

            if ch == '\n' {
                line += 1;
                col = 0;
            } else {
                col += 1;
            }

            current_offset += ch.len_utf8();
        }

        (line, col)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use graphql_config::DocumentsConfig;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_load_graphql_files() {
        let temp_dir = tempdir().unwrap();

        // Create test GraphQL files
        let query_file = temp_dir.path().join("queries.graphql");
        fs::write(
            &query_file,
            r"
            query GetUser($id: ID!) {
                user(id: $id) {
                    id
                    name
                }
            }

            mutation UpdateUser($id: ID!, $name: String!) {
                updateUser(id: $id, name: $name) {
                    id
                    name
                }
            }
        ",
        )
        .unwrap();

        let fragment_file = temp_dir.path().join("fragments.graphql");
        fs::write(
            &fragment_file,
            r"
            fragment UserFields on User {
                id
                name
                email
            }
        ",
        )
        .unwrap();

        let pattern = temp_dir.path().join("*.graphql").display().to_string();
        let config = DocumentsConfig::Pattern(pattern);
        let loader = DocumentLoader::new(config);
        let index = loader.load().unwrap();

        // Check operation
        let get_user = index.get_operation("GetUser");
        assert!(get_user.is_some());
        assert_eq!(get_user.unwrap().operation_type, OperationType::Query);

        let update_user = index.get_operation("UpdateUser");
        assert!(update_user.is_some());
        assert_eq!(update_user.unwrap().operation_type, OperationType::Mutation);

        // Check fragment
        let user_fields = index.get_fragment("UserFields");
        assert!(user_fields.is_some());
        assert_eq!(user_fields.unwrap().type_condition, "User");
    }

    #[test]
    fn test_load_typescript_files() {
        let temp_dir = tempdir().unwrap();

        // Create TypeScript file with embedded GraphQL
        let ts_file = temp_dir.path().join("queries.ts");
        fs::write(
            &ts_file,
            r"
            import { gql } from '@apollo/client';

            export const GET_USER = gql`
                query GetUser($id: ID!) {
                    user(id: $id) {
                        id
                        name
                    }
                }
            `;

            export const USER_FRAGMENT = gql`
                fragment UserInfo on User {
                    id
                    name
                    email
                }
            `;
        ",
        )
        .unwrap();

        let pattern = temp_dir.path().join("*.ts").display().to_string();
        let config = DocumentsConfig::Pattern(pattern);
        let loader = DocumentLoader::new(config);
        let index = loader.load().unwrap();

        // Check operation
        let get_user = index.get_operation("GetUser");
        assert!(get_user.is_some());

        // Check fragment
        let user_info = index.get_fragment("UserInfo");
        assert!(user_info.is_some());
    }

    #[test]
    fn test_load_multiple_patterns() {
        let temp_dir = tempdir().unwrap();

        let query_file = temp_dir.path().join("query.graphql");
        fs::write(&query_file, "query Test { __typename }").unwrap();

        let ts_file = temp_dir.path().join("query.ts");
        fs::write(
            &ts_file,
            r"
            import { gql } from '@apollo/client';
            const QUERY = gql`query TypeScript { __typename }`;
        ",
        )
        .unwrap();

        let patterns = vec![
            temp_dir.path().join("*.graphql").display().to_string(),
            temp_dir.path().join("*.ts").display().to_string(),
        ];
        let config = DocumentsConfig::Patterns(patterns);
        let loader = DocumentLoader::new(config);
        let index = loader.load().unwrap();

        assert!(index.get_operation("Test").is_some());
        assert!(index.get_operation("TypeScript").is_some());
    }

    #[test]
    fn test_skip_invalid_documents() {
        let temp_dir = tempdir().unwrap();

        // Create file with invalid GraphQL
        let invalid_file = temp_dir.path().join("invalid.graphql");
        fs::write(&invalid_file, "query { this is not valid }").unwrap();

        // Create file with valid GraphQL
        let valid_file = temp_dir.path().join("valid.graphql");
        fs::write(&valid_file, "query Valid { __typename }").unwrap();

        let pattern = temp_dir.path().join("*.graphql").display().to_string();
        let config = DocumentsConfig::Pattern(pattern);
        let loader = DocumentLoader::new(config);
        let index = loader.load().unwrap();

        // Should only have the valid query
        assert!(index.get_operation("Valid").is_some());
    }
}
