use crate::OutputFormat;
use anyhow::{Context, Result};
use colored::Colorize;
use graphql_config::{find_config, load_config};
use graphql_project::GraphQLProject;
use std::path::PathBuf;
use std::process;

#[allow(clippy::too_many_lines)]
pub async fn run(
    config_path: Option<PathBuf>,
    project_name: Option<String>,
    format: OutputFormat,
    watch: bool,
) -> Result<()> {
    // Define diagnostic output structure for collecting errors
    struct DiagnosticOutput {
        file_path: String,
        line: usize,
        column: usize,
        message: String,
    }

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
                    println!("{}", "✓ Schema loaded successfully".green());
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

        // Get extract config
        let extract_config = project.get_extract_config();

        // Collect unique file paths that contain operations or fragments
        let document_index = project.get_document_index();
        let mut all_file_paths = std::collections::HashSet::new();
        for op_infos in document_index.operations.values() {
            for op_info in op_infos {
                all_file_paths.insert(&op_info.file_path);
            }
        }
        for frag_infos in document_index.fragments.values() {
            for frag_info in frag_infos {
                all_file_paths.insert(&frag_info.file_path);
            }
        }

        let mut all_errors = Vec::new();

        // Validate each file using Apollo compiler only
        for file_path in all_file_paths {
            // Use graphql-extract to extract GraphQL from the file
            let extracted = match graphql_extract::extract_from_file(
                std::path::Path::new(file_path),
                &extract_config,
            ) {
                Ok(items) => items,
                Err(e) => {
                    if matches!(format, OutputFormat::Human) {
                        eprintln!(
                            "{} {}: {}",
                            "✗ Failed to extract GraphQL from".red(),
                            file_path,
                            e
                        );
                    }
                    continue;
                }
            };

            if extracted.is_empty() {
                continue;
            }

            // Use Apollo compiler validation only
            let diagnostics = project.validate_extracted_documents(&extracted, file_path);

            // Convert diagnostics to CLI output format
            for diag in diagnostics {
                use graphql_project::Severity;

                // Only process errors (Apollo compiler validation)
                if diag.severity == Severity::Error {
                    let diag_output = DiagnosticOutput {
                        file_path: file_path.clone(),
                        // graphql-project uses 0-based, CLI output uses 1-based
                        line: diag.range.start.line + 1,
                        column: diag.range.start.character + 1,
                        message: diag.message,
                    };

                    all_errors.push(diag_output);
                }
            }
        }

        // Display errors
        total_errors = all_errors.len();

        match format {
            OutputFormat::Human => {
                // Print all errors
                for error in &all_errors {
                    if error.line > 0 {
                        println!(
                            "\n{}:{}:{}: {} {}",
                            error.file_path,
                            error.line,
                            error.column,
                            "error:".red().bold(),
                            error.message.red()
                        );
                    } else {
                        // No location info
                        println!("\n{}", error.message.red());
                    }
                }
            }
            OutputFormat::Json => {
                // Print all errors as JSON
                for error in &all_errors {
                    let location = if error.line > 0 {
                        Some(serde_json::json!({
                            "line": error.line,
                            "column": error.column
                        }))
                    } else {
                        None
                    };

                    println!(
                        "{}",
                        serde_json::json!({
                            "file": error.file_path,
                            "severity": "error",
                            "message": error.message,
                            "location": location
                        })
                    );
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
            println!("{}", format!("✗ Found {total_errors} error(s)").red());
        }
    }

    if total_errors > 0 {
        process::exit(1);
    }

    Ok(())
}
