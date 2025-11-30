use crate::OutputFormat;
use anyhow::{Context, Result};
use colored::Colorize;
use graphql_config::{find_config, load_config};
use graphql_project::{GraphQLProject, Linter, Severity};
use std::path::PathBuf;
use std::process;

#[allow(clippy::too_many_lines)]
pub async fn run(
    config_path: Option<PathBuf>,
    project_name: Option<String>,
    format: OutputFormat,
    _watch: bool,
) -> Result<()> {
    // Define diagnostic output structure for collecting warnings and errors
    struct DiagnosticOutput {
        file_path: String,
        line: usize,
        column: usize,
        end_line: usize,
        end_column: usize,
        message: String,
        severity: String,
        rule: Option<String>,
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
    let projects_to_lint: Vec<_> = if let Some(ref name) = project_name {
        projects.into_iter().filter(|(n, _)| n == name).collect()
    } else {
        projects
    };

    if projects_to_lint.is_empty() {
        if let Some(name) = project_name {
            eprintln!("{}", format!("Project '{name}' not found").red());
            process::exit(1);
        }
    }

    let mut total_errors = 0;
    let mut total_warnings = 0;

    for (name, project) in &projects_to_lint {
        if projects_to_lint.len() > 1 {
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

        // Get lint config and create linter
        let lint_config = project.get_lint_config();
        let linter = Linter::new(lint_config);

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

        let mut all_warnings = Vec::new();
        let mut all_errors = Vec::new();

        // Run lints on each file
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

            let schema_index = project.get_schema_index();

            // Run lints on each extracted block
            for block in &extracted {
                let diagnostics = linter.lint_document(&block.source, &schema_index, file_path);

                // Convert diagnostics to output format
                for diag in diagnostics {
                    // Adjust positions for extracted blocks
                    let adjusted_line = block.location.range.start.line + diag.range.start.line;
                    let adjusted_col = if diag.range.start.line == 0 {
                        block.location.range.start.column + diag.range.start.character
                    } else {
                        diag.range.start.character
                    };

                    let adjusted_end_line = block.location.range.start.line + diag.range.end.line;
                    let adjusted_end_col = if diag.range.end.line == 0 {
                        block.location.range.start.column + diag.range.end.character
                    } else {
                        diag.range.end.character
                    };

                    let diag_output = DiagnosticOutput {
                        file_path: file_path.clone(),
                        // Convert from 0-based to 1-based for display
                        line: adjusted_line + 1,
                        column: adjusted_col + 1,
                        end_line: adjusted_end_line + 1,
                        end_column: adjusted_end_col + 1,
                        message: diag.message,
                        severity: match diag.severity {
                            Severity::Error => "error".to_string(),
                            Severity::Warning => "warning".to_string(),
                            Severity::Information => "info".to_string(),
                            Severity::Hint => "hint".to_string(),
                        },
                        rule: diag.code.clone(),
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
        }

        // Run project-wide lint rules (e.g., unused_fields, unique_names)
        let project_diagnostics = project.lint_project();
        for diag in project_diagnostics {
            // Extract file path from diagnostic source field (format: "graphql-linter:path")
            let file_path = if diag.source.starts_with("graphql-linter:") {
                diag.source
                    .strip_prefix("graphql-linter:")
                    .unwrap()
                    .to_string()
            } else {
                "(project)".to_string()
            };

            let diag_output = DiagnosticOutput {
                file_path,
                // Convert from 0-indexed to 1-indexed for display
                line: diag.range.start.line + 1,
                column: diag.range.start.character + 1,
                end_line: diag.range.end.line + 1,
                end_column: diag.range.end.character + 1,
                message: diag.message,
                severity: match diag.severity {
                    Severity::Error => "error".to_string(),
                    Severity::Warning => "warning".to_string(),
                    Severity::Information => "info".to_string(),
                    Severity::Hint => "hint".to_string(),
                },
                rule: diag.code.clone(),
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

        // Display results
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
                    if let Some(ref rule) = warning.rule {
                        println!("  {}: {}", "rule".dimmed(), rule.dimmed());
                    }
                }

                // Print all errors
                for error in &all_errors {
                    println!(
                        "\n{}:{}:{}: {} {}",
                        error.file_path,
                        error.line,
                        error.column,
                        "error:".red().bold(),
                        error.message.red()
                    );
                    if let Some(ref rule) = error.rule {
                        println!("  {}: {}", "rule".dimmed(), rule.dimmed());
                    }
                }
            }
            OutputFormat::Json => {
                // Print all diagnostics as JSON
                for warning in &all_warnings {
                    println!(
                        "{}",
                        serde_json::json!({
                            "file": warning.file_path,
                            "severity": warning.severity,
                            "rule": warning.rule,
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

                for error in &all_errors {
                    println!(
                        "{}",
                        serde_json::json!({
                            "file": error.file_path,
                            "severity": error.severity,
                            "rule": error.rule,
                            "message": error.message,
                            "location": {
                                "start": {
                                    "line": error.line,
                                    "column": error.column
                                },
                                "end": {
                                    "line": error.end_line,
                                    "column": error.end_column
                                }
                            }
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
            println!("{}", "✓ No linting issues found!".green().bold());
        } else if total_errors == 0 {
            println!(
                "{}",
                format!("✓ Linting passed with {total_warnings} warning(s)")
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
