#![cfg(not(target_os = "windows"))]

use graphql_config::{DocumentsConfig, ProjectConfig, SchemaConfig};
use graphql_project::GraphQLProject;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

/// Helper to create a test project with schema and documents
async fn create_test_project() -> (TempDir, GraphQLProject) {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let base_path = temp_dir.path();

    // Copy schema
    let schema_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("schema.graphql");
    let schema_content = fs::read_to_string(schema_path).expect("Failed to read schema");
    fs::write(base_path.join("schema.graphql"), schema_content).expect("Failed to write schema");

    // Copy fragment files
    let fragment_user_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("fragment_user.graphql");
    if fragment_user_path.exists() {
        let fragment_content =
            fs::read_to_string(&fragment_user_path).expect("Failed to read fragment");
        fs::write(base_path.join("fragment_user.graphql"), fragment_content)
            .expect("Failed to write fragment");
    }

    let fragment_with_posts_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("fragment_with_posts.graphql");
    if fragment_with_posts_path.exists() {
        let fragment_content =
            fs::read_to_string(&fragment_with_posts_path).expect("Failed to read fragment");
        fs::write(
            base_path.join("fragment_with_posts.graphql"),
            fragment_content,
        )
        .expect("Failed to write fragment");
    }

    // Create GraphQL config
    let config = ProjectConfig {
        schema: SchemaConfig::Path(base_path.join("schema.graphql").display().to_string()),
        documents: Some(DocumentsConfig::Patterns(vec![
            "**/*.graphql".to_string(),
            "**/*.tsx".to_string(),
        ])),
        include: None,
        exclude: None,
        extensions: None,
    };

    // Create and load the project
    let project = GraphQLProject::new(config).with_base_dir(base_path.to_path_buf());

    project.load_schema().await.expect("Failed to load schema");

    // Load documents to index fragments
    let _ = project.load_documents();

    (temp_dir, project)
}

#[tokio::test]
async fn test_validate_document_source_with_operation_no_fragments() {
    let (_temp_dir, project) = create_test_project().await;

    let document = r"
query GetUser($id: ID!) {
  user(id: $id) {
    id
    name
    email
  }
}
";

    let diagnostics = project.validate_document_source(document, "test.graphql");
    assert!(
        diagnostics.is_empty(),
        "Valid query without fragments should have no errors: {diagnostics:?}"
    );
}

#[tokio::test]
async fn test_validate_document_source_with_valid_fields() {
    let (_temp_dir, project) = create_test_project().await;

    // Test a query with nested selection sets
    let operation = r"
query GetPostsWithAuthors {
  posts {
    id
    title
    content
    author {
      id
      name
      email
    }
  }
}
";

    let diagnostics = project.validate_document_source(operation, "operation.graphql");

    // Should validate successfully
    assert!(
        diagnostics.is_empty(),
        "Operation with valid nested fields should have no errors: {diagnostics:?}"
    );
}

#[tokio::test]
async fn test_validate_document_source_standalone_fragment_valid() {
    let (_temp_dir, project) = create_test_project().await;

    let fragment = r"
fragment PostFields on Post {
  id
  title
  content
  author {
    id
    name
  }
}
";

    let diagnostics = project.validate_document_source(fragment, "fragment.graphql");

    // Standalone fragments should be validated for schema correctness
    assert!(
        diagnostics.is_empty(),
        "Valid standalone fragment should have no errors: {diagnostics:?}"
    );
}

#[tokio::test]
async fn test_validate_document_source_standalone_fragment_invalid() {
    let (_temp_dir, project) = create_test_project().await;

    let fragment = r"
fragment InvalidPostFields on Post {
  id
  title
  nonExistentField
}
";

    let diagnostics = project.validate_document_source(fragment, "fragment.graphql");

    // Should have an error about the nonExistentField
    assert!(
        !diagnostics.is_empty(),
        "Invalid standalone fragment should have errors"
    );

    let error_messages: Vec<String> = diagnostics.iter().map(|d| d.message.clone()).collect();
    assert!(
        error_messages
            .iter()
            .any(|msg| msg.contains("nonExistentField") || msg.contains("field")),
        "Should have error about invalid field, got: {error_messages:?}"
    );
}

#[tokio::test]
async fn test_validate_document_source_operation_without_fragment_spread() {
    let (_temp_dir, project) = create_test_project().await;

    // Operation without fragment spreads (no ...) shouldn't load project fragments
    let operation = r"
query GetUsers {
  users {
    id
    name
    email
  }
}
";

    let diagnostics = project.validate_document_source(operation, "operation.graphql");

    assert!(
        diagnostics.is_empty(),
        "Valid operation without fragment spreads should have no errors: {diagnostics:?}"
    );
}

#[tokio::test]
async fn test_validate_document_source_undefined_fragment() {
    let (_temp_dir, project) = create_test_project().await;

    let operation = r"
query GetUsers {
  users {
    ...UndefinedFragment
  }
}
";

    let diagnostics = project.validate_document_source(operation, "operation.graphql");

    // Should have error about undefined fragment
    assert!(
        !diagnostics.is_empty(),
        "Operation with undefined fragment should have errors"
    );

    let error_messages: Vec<String> = diagnostics.iter().map(|d| d.message.clone()).collect();
    assert!(
        error_messages
            .iter()
            .any(|msg| msg.contains("UndefinedFragment") || msg.contains("fragment")),
        "Should have error about undefined fragment, got: {error_messages:?}"
    );
}

#[tokio::test]
async fn test_validate_extracted_documents_valid() {
    let (temp_dir, project) = create_test_project().await;
    let base_path = temp_dir.path();

    // Write TypeScript file with simple query
    let ts_content = r"
import { gql } from '@apollo/client';

const GET_USERS = gql`
  query GetUsersTS {
    users {
      id
      name
      email
      posts {
        id
        title
      }
    }
  }
`;
";

    let ts_path = base_path.join("test.tsx");
    fs::write(&ts_path, ts_content).expect("Failed to write TypeScript file");

    // Extract GraphQL from TypeScript
    let extracted =
        graphql_extract::extract_from_file(&ts_path, &graphql_extract::ExtractConfig::default())
            .expect("Failed to extract GraphQL");

    let diagnostics = project.validate_extracted_documents(&extracted, ts_path.to_str().unwrap());

    // Should validate successfully
    assert!(
        diagnostics.is_empty(),
        "Valid TypeScript should have no errors: {diagnostics:?}"
    );
}

#[tokio::test]
async fn test_validate_extracted_documents_with_invalid_field() {
    let (temp_dir, project) = create_test_project().await;
    let base_path = temp_dir.path();

    // Write TypeScript file with invalid field
    let ts_content = r"
import { gql } from '@apollo/client';

const GET_USERS = gql`
  query GetUsersBad {
    users {
      id
      name
      invalidField
    }
  }
`;
";

    let ts_path = base_path.join("test_invalid.tsx");
    fs::write(&ts_path, ts_content).expect("Failed to write TypeScript file");

    // Extract GraphQL from TypeScript
    let extracted =
        graphql_extract::extract_from_file(&ts_path, &graphql_extract::ExtractConfig::default())
            .expect("Failed to extract GraphQL");

    let diagnostics = project.validate_extracted_documents(&extracted, ts_path.to_str().unwrap());

    // Should have error about invalid field
    assert!(
        !diagnostics.is_empty(),
        "Invalid TypeScript should have errors"
    );

    let error_messages: Vec<String> = diagnostics.iter().map(|d| d.message.clone()).collect();
    assert!(
        error_messages
            .iter()
            .any(|msg| msg.contains("invalidField") || msg.contains("field")),
        "Should have error about invalid field, got: {error_messages:?}"
    );
}

#[tokio::test]
async fn test_validate_extracted_documents_preserves_line_offsets() {
    let (temp_dir, project) = create_test_project().await;
    let base_path = temp_dir.path();

    // Write TypeScript file with GraphQL at specific line
    let ts_content = r"
import { gql } from '@apollo/client';

// Some code here
// More code

const GET_USERS = gql`
  query GetUsers {
    users {
      id
      invalidField
    }
  }
`;
";

    let ts_path = base_path.join("test_offset.tsx");
    fs::write(&ts_path, ts_content).expect("Failed to write TypeScript file");

    // Extract GraphQL from TypeScript
    let extracted =
        graphql_extract::extract_from_file(&ts_path, &graphql_extract::ExtractConfig::default())
            .expect("Failed to extract GraphQL");

    let diagnostics = project.validate_extracted_documents(&extracted, ts_path.to_str().unwrap());

    // Should have error with correct line number (accounting for offset)
    assert!(!diagnostics.is_empty(), "Should have validation errors");

    // The error should be on a line > 7 (because of the offset in the TypeScript file)
    let has_offset_error = diagnostics.iter().any(|d| d.range.start.line > 7);

    assert!(
        has_offset_error,
        "Error should preserve line offset from TypeScript file, got: {diagnostics:?}"
    );
}
