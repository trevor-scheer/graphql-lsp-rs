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
    // Define diagnostic output structure for collecting warnings and errors
    struct DiagnosticOutput {
        file_path: String,
        line: usize,
        column: usize,
        end_line: usize,
        end_column: usize,
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
    let mut total_warnings = 0;

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

        // Validate all loaded documents
        let document_index = project.get_document_index();

        // Check for duplicate operation and fragment names across the entire project
        let duplicate_diagnostics = document_index.check_duplicate_names();

        // Collect unique file paths that contain operations or fragments
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

        let mut all_warnings = Vec::new();
        let mut all_errors = Vec::new();

        // Add project-wide duplicate name errors
        for (file_path, diag) in duplicate_diagnostics {
            use graphql_project::Severity;

            // These diagnostics now have accurate line/column info
            let diag_output = DiagnosticOutput {
                file_path,
                // graphql-project uses 0-based, CLI output uses 1-based
                line: diag.range.start.line + 1,
                column: diag.range.start.character + 1,
                end_line: diag.range.end.line + 1,
                end_column: diag.range.end.character + 1,
                message: diag.message,
            };

            match diag.severity {
                Severity::Warning | Severity::Information | Severity::Hint => {
                    all_warnings.push(diag_output);
                }
                Severity::Error => {
                    all_errors.push(diag_output);
                }
            }
        }

        // Validate each file using the centralized validation logic
        for file_path in all_file_paths {
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

            // Use the centralized validation logic from graphql-project
            let diagnostics = project.validate_extracted_documents(&extracted, file_path);

            // Convert graphql-project diagnostics to CLI output format
            for diag in diagnostics {
                use graphql_project::Severity;

                let diag_output = DiagnosticOutput {
                    file_path: file_path.clone(),
                    // graphql-project uses 0-based, CLI output uses 1-based
                    line: diag.range.start.line + 1,
                    column: diag.range.start.character + 1,
                    end_line: diag.range.end.line + 1,
                    end_column: diag.range.end.character + 1,
                    message: diag.message,
                };

                match diag.severity {
                    Severity::Warning | Severity::Information | Severity::Hint => {
                        all_warnings.push(diag_output);
                    }
                    Severity::Error => {
                        all_errors.push(diag_output);
                    }
                }
            }
        }

        // Now display all warnings first, then all errors
        total_warnings = all_warnings.len();
        total_errors = all_errors.len();

        match format {
            OutputFormat::Human => {
                // Print all warnings
                for warning in &all_warnings {
                    println!(
                        "\n{}:{}:{}: {} {}",
                        warning.file_path,
                        warning.line,
                        warning.column,
                        "warning:".yellow().bold(),
                        warning.message.yellow()
                    );
                }

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
                        println!("\n{}", error.message);
                    }
                }
            }
            OutputFormat::Json => {
                // Print all warnings as JSON
                for warning in &all_warnings {
                    println!(
                        "{}",
                        serde_json::json!({
                            "file": warning.file_path,
                            "severity": "warning",
                            "message": warning.message,
                            "location": {
                                "start": {
                                    "line": warning.line,
                                    "column": warning.column
                                },
                                "end": {
                                    "line": warning.end_line,
                                    "column": warning.end_column
                                }
                            }
                        })
                    );
                }

                // Print all errors as JSON
                for error in &all_errors {
                    let location = if error.line > 0 {
                        Some(serde_json::json!({
                            "start": {
                                "line": error.line,
                                "column": error.column
                            },
                            "end": {
                                "line": error.end_line,
                                "column": error.end_column
                            }
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
        if total_errors == 0 && total_warnings == 0 {
            println!("{}", "✓ All validations passed!".green().bold());
        } else if total_errors == 0 {
            println!(
                "{}",
                format!("✓ Validation passed with {total_warnings} warning(s)")
                    .yellow()
                    .bold()
            );
        } else if total_warnings == 0 {
            println!("{}", format!("✗ Found {total_errors} error(s)").red());
        } else {
            println!(
                "{}",
                format!("✗ Found {total_errors} error(s) and {total_warnings} warning(s)").red()
            );
        }
    }

    if total_errors > 0 {
        process::exit(1);
    }

    Ok(())
}
