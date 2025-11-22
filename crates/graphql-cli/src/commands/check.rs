use anyhow::Result;
use colored::Colorize;
use std::path::PathBuf;

#[allow(clippy::unused_async)] // Will be async when implemented
pub async fn run(
    _config_path: Option<PathBuf>,
    _project_name: Option<String>,
    base: String,
    head: String,
) -> Result<()> {
    println!(
        "{}",
        format!("Breaking change detection not yet implemented (comparing {base} -> {head})")
            .yellow()
    );

    // TODO: Implement breaking change detection
    // - Load schema from base ref
    // - Load schema from head ref
    // - Compare schemas for breaking changes
    // - Report breaking changes

    Ok(())
}
