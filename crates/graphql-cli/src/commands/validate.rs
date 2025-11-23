use crate::OutputFormat;
use anyhow::{Context, Result};
use colored::Colorize;
use graphql_config::{find_config, load_config};
use graphql_project::GraphQLProject;
use std::path::PathBuf;
use std::process;

#[allow(clippy::too_many_lines)] // Main validation logic - will refactor when more features are added
pub async fn run(
    config_path: Option<PathBuf>,
    project_name: Option<String>,
    format: OutputFormat,
    watch: bool,
) -> Result<()> {
    if watch {
        println!("{}", "Watch mode not yet implemented".yellow());
        return Ok(());
    }

    // Find and load config
    let config_path = if let Some(path) = config_path {
        path
    } else {
        let current_dir = std::env::current_dir()?;
        find_config(&current_dir)
            .context("Failed to search for config")?
            .context("No GraphQL config file found")?
    };

    let config = load_config(&config_path).context("Failed to load config")?;

    // Get the base directory from the config path
    let base_dir = config_path
        .parent()
        .context("Failed to get config directory")?
        .to_path_buf();

    // Get projects with base directory
    let projects = GraphQLProject::from_config_with_base(&config, &base_dir)?;

    // Filter by project name if specified
    let projects_to_validate: Vec<_> = if let Some(ref name) = project_name {
        projects.into_iter().filter(|(n, _)| n == name).collect()
    } else {
        projects
    };

    if projects_to_validate.is_empty() {
        if let Some(name) = project_name {
            eprintln!("{}", format!("Project '{name}' not found").red());
            process::exit(1);
        }
    }

    let mut total_errors = 0;

    for (name, project) in &projects_to_validate {
        if projects_to_validate.len() > 1 {
            println!("\n{}", format!("=== Project: {name} ===").bold().cyan());
        }

        // Load schema
        match project.load_schema().await {
            Ok(()) => {
                if matches!(format, OutputFormat::Human) {
                    let schema = project.get_schema();
                    if let Some(ref schema_str) = schema {
                        let line_count = schema_str.lines().count();
                        println!(
                            "{} ({} lines)",
                            "✓ Schema loaded successfully".green(),
                            line_count
                        );
                    } else {
                        println!("{}", "✓ Schema loaded successfully".green());
                    }
                }
            }
            Err(e) => {
                if matches!(format, OutputFormat::Human) {
                    eprintln!("{} {}", "✗ Schema error:".red(), e);
                } else {
                    eprintln!("{}", serde_json::json!({ "error": e.to_string() }));
                }
                process::exit(1);
            }
        }

        // Load documents
        match project.load_documents() {
            Ok(()) => {
                if matches!(format, OutputFormat::Human) {
                    let doc_index = project.get_document_index();
                    let op_count = doc_index.operations.len();
                    let frag_count = doc_index.fragments.len();
                    println!(
                        "{} ({} operations, {} fragments)",
                        "✓ Documents loaded successfully".green(),
                        op_count,
                        frag_count
                    );
                }
            }
            Err(e) => {
                if matches!(format, OutputFormat::Human) {
                    eprintln!("{} {}", "✗ Document error:".red(), e);
                } else {
                    eprintln!("{}", serde_json::json!({ "error": e.to_string() }));
                }
                process::exit(1);
            }
        }

        // Validate all loaded documents
        let document_index = project.get_document_index();

        // Collect all fragment definitions from the project
        // These will be included when validating operations
        let mut all_fragments = Vec::new();
        let mut fragment_file_paths = std::collections::HashSet::new();

        for frag_info in document_index.fragments.values() {
            fragment_file_paths.insert(&frag_info.file_path);
        }

        // Extract all fragment sources
        for frag_path in fragment_file_paths {
            if let Ok(extracted) = graphql_extract::extract_from_file(
                std::path::Path::new(frag_path),
                &graphql_extract::ExtractConfig::default(),
            ) {
                for item in extracted {
                    if item.source.trim_start().starts_with("fragment") {
                        all_fragments.push(item.source);
                    }
                }
            }
        }

        // Collect unique file paths that contain operations
        let mut operation_file_paths = std::collections::HashSet::new();
        for op_info in document_index.operations.values() {
            operation_file_paths.insert(&op_info.file_path);
        }

        // Validate each file containing operations
        for file_path in operation_file_paths {
            // Use graphql-extract to extract GraphQL from the file
            // This handles both .graphql files and embedded GraphQL in TypeScript/JavaScript
            let extracted = match graphql_extract::extract_from_file(
                std::path::Path::new(file_path),
                &graphql_extract::ExtractConfig::default(),
            ) {
                Ok(items) => items,
                Err(e) => {
                    eprintln!(
                        "{} {}: {}",
                        "✗ Failed to extract GraphQL from".red(),
                        file_path,
                        e
                    );
                    continue;
                }
            };

            if extracted.is_empty() {
                if matches!(format, OutputFormat::Human) {
                    eprintln!("{} {} (no GraphQL found)", "⚠".yellow(), file_path);
                }
                continue;
            }

            // Validate each extracted GraphQL document
            for item in &extracted {
                let source = &item.source;

                // Skip documents that only contain fragments
                // Fragments are validated in the context of operations
                if source.trim_start().starts_with("fragment")
                    && !source.contains("query")
                    && !source.contains("mutation")
                    && !source.contains("subscription")
                {
                    continue;
                }

                // Find all fragment spreads recursively (fragments can reference other fragments)
                let mut referenced_fragments = std::collections::HashSet::new();
                let mut to_process = vec![source.to_string()];

                while let Some(doc) = to_process.pop() {
                    for line in doc.lines() {
                        let trimmed = line.trim();
                        if let Some(stripped) = trimmed.strip_prefix("...") {
                            // Extract fragment name (everything after ... until whitespace or special char)
                            let frag_name = stripped
                                .split(|c: char| c.is_whitespace() || c == '@')
                                .next()
                                .unwrap_or("");
                            if !frag_name.is_empty() && !referenced_fragments.contains(frag_name) {
                                referenced_fragments.insert(frag_name.to_string());

                                // Find and queue the fragment definition for processing
                                if let Some(frag_def) = all_fragments
                                    .iter()
                                    .find(|f| f.contains(&format!("fragment {frag_name}")))
                                {
                                    to_process.push(frag_def.clone());
                                }
                            }
                        }
                    }
                }

                // Collect all referenced fragments
                let relevant_fragments: Vec<&String> = all_fragments
                    .iter()
                    .filter(|frag| {
                        referenced_fragments
                            .iter()
                            .any(|name| frag.contains(&format!("fragment {name}")))
                    })
                    .collect();

                // Combine the operation with all referenced fragments (including transitive dependencies)
                let combined_source = if relevant_fragments.is_empty() {
                    source.to_string()
                } else {
                    let fragments_str = relevant_fragments
                        .iter()
                        .map(|s| s.as_str())
                        .collect::<Vec<_>>()
                        .join("\n\n");
                    format!("{source}\n\n{fragments_str}")
                };

                // Validate with the actual file path and line offset
                // This makes apollo-compiler's diagnostics show the correct file:line:column
                let line_offset = item.location.range.start.line;
                let validation_result = project.validate_document_with_location(
                    &combined_source,
                    file_path,
                    line_offset,
                );

                match validation_result {
                    Ok(()) => {
                        // Valid document - no output in human mode unless verbose
                    }
                    Err(diagnostics) => {
                        // Found validation errors - diagnostics already have correct file and line numbers
                        for diagnostic in diagnostics.iter() {
                            total_errors += 1;

                            match format {
                                OutputFormat::Human => {
                                    // Just print the diagnostic - it already has the correct location
                                    println!("\n{diagnostic}");
                                }
                                OutputFormat::Json => {
                                    // For JSON output, extract location from diagnostic
                                    let location = diagnostic.line_column_range().map(|range| {
                                        serde_json::json!({
                                            "start": {
                                                "line": range.start.line,
                                                "column": range.start.column
                                            },
                                            "end": {
                                                "line": range.end.line,
                                                "column": range.end.column
                                            }
                                        })
                                    });

                                    println!(
                                        "{}",
                                        serde_json::json!({
                                            "file": file_path,
                                            "error": format!("{}", diagnostic.error),
                                            "location": location
                                        })
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Summary
    if matches!(format, OutputFormat::Human) {
        println!();
        if total_errors == 0 {
            println!("{}", "✓ All validations passed!".green().bold());
        } else {
            println!("{}", format!("Found {total_errors} error(s)").yellow());
        }
    }

    if total_errors > 0 {
        process::exit(1);
    }

    Ok(())
}
