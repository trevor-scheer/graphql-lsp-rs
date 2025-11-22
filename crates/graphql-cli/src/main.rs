mod commands;

use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "graphql")]
#[command(about = "GraphQL CLI for validation and linting", long_about = None)]
#[command(version)]
struct Cli {
    /// Path to GraphQL config file
    #[arg(short, long, value_name = "FILE")]
    config: Option<PathBuf>,

    /// Project name (for multi-project configs)
    #[arg(short, long)]
    project: Option<String>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Validate GraphQL schema and documents
    Validate {
        /// Output format
        #[arg(short, long, value_enum, default_value = "human")]
        format: OutputFormat,

        /// Watch mode - re-validate on file changes
        #[arg(short, long)]
        watch: bool,
    },

    /// Check for breaking changes between schemas
    Check {
        /// Base branch/ref to compare against
        #[arg(long)]
        base: String,

        /// Head branch/ref to compare
        #[arg(long)]
        head: String,
    },
}

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
enum OutputFormat {
    /// Human-readable output with colors
    Human,
    /// JSON output for tooling
    Json,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Validate { format, watch } => {
            commands::validate::run(cli.config, cli.project, format, watch).await?;
        }
        Commands::Check { base, head } => {
            commands::check::run(cli.config, cli.project, base, head).await?;
        }
    }

    Ok(())
}
