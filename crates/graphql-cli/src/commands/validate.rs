use crate::OutputFormat;
use anyhow::{Context, Result};
use colored::Colorize;
use graphql_config::{find_config, load_config};
use graphql_project::{GraphQLProject, Severity};
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

    // Get projects
    let projects = GraphQLProject::from_config(&config)?;

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
                    println!("{}", "✓ Documents loaded successfully".green());
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

        // Validate
        let diagnostics = project.validate();

        for diagnostic in &diagnostics {
            match diagnostic.severity {
                Severity::Error => total_errors += 1,
                Severity::Warning => total_warnings += 1,
                _ => {}
            }

            match format {
                OutputFormat::Human => {
                    let severity_str = match diagnostic.severity {
                        Severity::Error => "error".red(),
                        Severity::Warning => "warning".yellow(),
                        Severity::Information => "info".blue(),
                        Severity::Hint => "hint".cyan(),
                    };
                    println!(
                        "[{}] {}:{} - {}",
                        severity_str,
                        diagnostic.range.start.line + 1,
                        diagnostic.range.start.character + 1,
                        diagnostic.message
                    );
                }
                OutputFormat::Json => {
                    println!("{}", serde_json::to_string(&diagnostic).unwrap());
                }
            }
        }
    }

    // Summary
    if matches!(format, OutputFormat::Human) {
        println!();
        if total_errors == 0 && total_warnings == 0 {
            println!("{}", "✓ All validations passed!".green().bold());
        } else {
            println!(
                "{}",
                format!("Found {total_errors} error(s) and {total_warnings} warning(s)").yellow()
            );
        }
    }

    if total_errors > 0 {
        process::exit(1);
    }

    Ok(())
}
