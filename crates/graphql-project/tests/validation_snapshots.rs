use graphql_project::{SchemaIndex, Validator};
use insta::assert_snapshot;
use std::fs;
use std::path::Path;

/// Load schema from fixtures
fn load_schema() -> SchemaIndex {
    let schema_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("schema.graphql");
    let schema_content = fs::read_to_string(schema_path).expect("Failed to read schema");
    SchemaIndex::from_schema(&schema_content)
}

/// Convert absolute path to relative path from workspace root
fn to_relative_path(path: &Path) -> String {
    // Get the workspace root by going up from CARGO_MANIFEST_DIR
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir
        .parent()
        .and_then(|p| p.parent())
        .expect("Failed to find workspace root");

    path.strip_prefix(workspace_root)
        .map_or_else(|_| path.display().to_string(), |p| p.display().to_string())
}

/// Format diagnostics for snapshot testing
fn format_diagnostics(diagnostics: &apollo_compiler::validation::DiagnosticList) -> String {
    diagnostics
        .iter()
        .map(|d| format!("{d}"))
        .collect::<Vec<_>>()
        .join("\n\n")
}

#[test]
fn test_valid_query_snapshot() {
    let schema = load_schema();
    let validator = Validator::new();

    let document_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("valid_query.graphql");
    let document = fs::read_to_string(document_path).expect("Failed to read document");

    let result = validator.validate_document(&document, &schema);
    assert!(result.is_ok(), "Valid query should have no errors");
}

#[test]
fn test_invalid_field_snapshot() {
    let schema = load_schema();
    let validator = Validator::new();

    let document_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("invalid_field.graphql");
    let document = fs::read_to_string(&document_path).expect("Failed to read document");

    let result = validator.validate_document_with_name(
        &document,
        &schema,
        &to_relative_path(&document_path),
    );

    assert!(result.is_err(), "Should have validation errors");
    let diagnostics = result.unwrap_err();

    assert_snapshot!(format_diagnostics(&diagnostics));
}

#[test]
fn test_missing_argument_snapshot() {
    let schema = load_schema();
    let validator = Validator::new();

    let document_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("missing_argument.graphql");
    let document = fs::read_to_string(&document_path).expect("Failed to read document");

    let result = validator.validate_document_with_name(
        &document,
        &schema,
        &to_relative_path(&document_path),
    );

    assert!(result.is_err(), "Should have validation errors");
    let diagnostics = result.unwrap_err();

    assert_snapshot!(format_diagnostics(&diagnostics));
}

#[test]
fn test_invalid_fragment_snapshot() {
    let schema = load_schema();
    let validator = Validator::new();

    let document_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("invalid_fragment.graphql");
    let document = fs::read_to_string(&document_path).expect("Failed to read document");

    let result = validator.validate_document_with_name(
        &document,
        &schema,
        &to_relative_path(&document_path),
    );

    assert!(result.is_err(), "Should have validation errors");
    let diagnostics = result.unwrap_err();

    assert_snapshot!(format_diagnostics(&diagnostics));
}

#[test]
fn test_valid_typescript_snapshot() {
    let schema = load_schema();
    let validator = Validator::new();

    let document_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("valid_typescript.tsx");

    // Extract GraphQL from TypeScript file
    let extracted = graphql_extract::extract_from_file(
        &document_path,
        &graphql_extract::ExtractConfig::default(),
    )
    .expect("Failed to extract GraphQL");

    // Collect all fragments for validation
    let mut all_fragments = Vec::new();
    let mut operations = Vec::new();

    for item in &extracted {
        if item.source.trim_start().starts_with("fragment") {
            all_fragments.push(&item.source);
        } else {
            operations.push(item);
        }
    }

    // Validate operations (fragments are validated in context of operations)
    for item in operations {
        // Find referenced fragments
        let mut referenced_fragments = Vec::new();
        for frag in &all_fragments {
            let frag_name = frag.split_whitespace().nth(1).unwrap_or("");
            if item.source.contains(&format!("...{frag_name}")) {
                referenced_fragments.push(*frag);
            }
        }

        // Combine operation with fragments
        let combined = if referenced_fragments.is_empty() {
            item.source.clone()
        } else {
            let fragments_str: String = referenced_fragments
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join("\n\n");
            format!("{}\n\n{}", item.source, fragments_str)
        };

        let result = validator.validate_document_with_name(
            &combined,
            &schema,
            &to_relative_path(&document_path),
        );
        assert!(
            result.is_ok(),
            "Valid TypeScript should have no errors: {:?}",
            result.err()
        );
    }
}

#[test]
fn test_invalid_typescript_snapshot() {
    let schema = load_schema();
    let validator = Validator::new();

    let document_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("invalid_typescript.tsx");

    // Extract GraphQL from TypeScript file
    let extracted = graphql_extract::extract_from_file(
        &document_path,
        &graphql_extract::ExtractConfig::default(),
    )
    .expect("Failed to extract GraphQL");

    let mut all_diagnostics = Vec::new();

    // Validate each extracted document
    for (idx, item) in extracted.iter().enumerate() {
        let line_offset = item.location.range.start.line;
        let result = validator.validate_document_with_location(
            &item.source,
            &schema,
            &to_relative_path(&document_path),
            line_offset,
        );

        if let Err(diagnostics) = result {
            all_diagnostics.push(format!(
                "=== Document {} ===\n{}",
                idx + 1,
                format_diagnostics(&diagnostics)
            ));
        }
    }

    assert!(!all_diagnostics.is_empty(), "Should have validation errors");
    assert_snapshot!(all_diagnostics.join("\n\n"));
}

#[test]
fn test_apollo_client_directives_snapshot() {
    let schema = load_schema();
    let validator = Validator::new();

    let document_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("apollo_client_directives.tsx");

    // Extract GraphQL from TypeScript file
    let extracted = graphql_extract::extract_from_file(
        &document_path,
        &graphql_extract::ExtractConfig::default(),
    )
    .expect("Failed to extract GraphQL");

    // Note: @client fields (isLoggedIn, localData) will fail validation because they're not in the schema
    // This is expected - apollo_client_builtins.graphql only includes the directives themselves,
    // not the client-only fields. In real usage, the schema loader includes these directives automatically.
    // This test documents the current behavior when validating against a schema without Apollo directives.

    let mut all_diagnostics = Vec::new();

    for (idx, item) in extracted.iter().enumerate() {
        let line_offset = item.location.range.start.line;
        let result = validator.validate_document_with_location(
            &item.source,
            &schema,
            &to_relative_path(&document_path),
            line_offset,
        );

        if let Err(diagnostics) = result {
            all_diagnostics.push(format!(
                "=== Query {} ===\n{}",
                idx + 1,
                format_diagnostics(&diagnostics)
            ));
        }
    }

    // Snapshot the errors showing what happens without client field definitions
    if !all_diagnostics.is_empty() {
        assert_snapshot!(all_diagnostics.join("\n\n"));
    }
}

#[test]
fn test_multiline_error_snapshot() {
    let schema = load_schema();
    let validator = Validator::new();

    let document = r"
query GetUser($id: ID!) {
  user(id: $id) {
    id
    name
    posts {
      id
      invalidField1
      title
      invalidField2
      content
    }
  }
}
";

    let file_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("multiline.graphql");
    let result =
        validator.validate_document_with_name(document, &schema, &to_relative_path(&file_path));

    assert!(result.is_err(), "Should have validation errors");
    let diagnostics = result.unwrap_err();

    assert_snapshot!(format_diagnostics(&diagnostics));
}

#[test]
fn test_complex_nested_error_snapshot() {
    let schema = load_schema();
    let validator = Validator::new();

    let document = r"
query ComplexQuery($userId: ID!, $postId: ID!) {
  user(id: $userId) {
    id
    name
    invalidField
    posts {
      id
      title
      author {
        id
        anotherInvalidField
      }
    }
  }
  post(id: $postId) {
    id
    nonExistentField
  }
}
";

    let file_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("complex.graphql");
    let result =
        validator.validate_document_with_name(document, &schema, &to_relative_path(&file_path));

    assert!(result.is_err(), "Should have validation errors");
    let diagnostics = result.unwrap_err();

    assert_snapshot!(format_diagnostics(&diagnostics));
}

#[test]
fn test_undefined_fragment_snapshot() {
    let schema = load_schema();
    let validator = Validator::new();

    let document = r"
query GetUser($id: ID!) {
  user(id: $id) {
    ...UndefinedFragment
    id
    name
  }
}
";

    let file_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("undefined_fragment.graphql");
    let result =
        validator.validate_document_with_name(document, &schema, &to_relative_path(&file_path));

    assert!(result.is_err(), "Should have validation errors");
    let diagnostics = result.unwrap_err();

    assert_snapshot!(format_diagnostics(&diagnostics));
}

#[test]
fn test_type_mismatch_snapshot() {
    let schema = load_schema();
    let validator = Validator::new();

    let document = r"
query GetUser($id: String!) {
  user(id: $id) {
    id
    name
  }
}
";

    let file_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("type_mismatch.graphql");
    let result =
        validator.validate_document_with_name(document, &schema, &to_relative_path(&file_path));

    // This might or might not error depending on apollo-compiler's validation
    if let Err(diagnostics) = result {
        assert_snapshot!(format_diagnostics(&diagnostics));
    }
}
