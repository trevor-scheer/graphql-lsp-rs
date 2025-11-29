mod config;
mod linter;
mod rules;

pub use config::{LintConfig, LintRuleConfig, LintSeverity};
pub use linter::Linter;
