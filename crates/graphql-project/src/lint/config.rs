use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Severity level for a lint rule
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LintSeverity {
    Off,
    Warn,
    Error,
}

/// Configuration for a single lint rule
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum LintRuleConfig {
    /// Just a severity level (simple case)
    Severity(LintSeverity),

    /// Detailed config with options (future)
    Detailed {
        severity: LintSeverity,
        #[serde(skip_serializing_if = "Option::is_none")]
        options: Option<serde_json::Value>,
    },
}

/// Overall lint configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum LintConfig {
    /// Use recommended preset
    Recommended(String), // "recommended"

    /// Custom rule configuration
    Rules {
        #[serde(flatten)]
        rules: HashMap<String, LintRuleConfig>,
    },
}

impl Default for LintConfig {
    fn default() -> Self {
        // Default is no lints enabled (opt-in)
        Self::Rules {
            rules: HashMap::new(),
        }
    }
}

impl LintConfig {
    /// Get the severity for a rule, considering recommended preset
    #[must_use]
    pub fn get_severity(&self, rule_name: &str) -> Option<LintSeverity> {
        match self {
            Self::Recommended(_) => Self::recommended_severity(rule_name),
            Self::Rules { rules } => {
                // Check if "recommended" is set
                if matches!(
                    rules.get("recommended"),
                    Some(LintRuleConfig::Severity(
                        LintSeverity::Warn | LintSeverity::Error
                    ))
                ) {
                    // Start with recommended, allow overrides
                    let recommended = Self::recommended_severity(rule_name);
                    rules
                        .get(rule_name)
                        .map(|config| match config {
                            LintRuleConfig::Severity(severity)
                            | LintRuleConfig::Detailed { severity, .. } => *severity,
                        })
                        .or(recommended)
                } else {
                    // No recommended, only explicit rules
                    rules.get(rule_name).map(|config| match config {
                        LintRuleConfig::Severity(severity)
                        | LintRuleConfig::Detailed { severity, .. } => *severity,
                    })
                }
            }
        }
    }

    /// Check if a rule is enabled (not Off and not None)
    #[must_use]
    pub fn is_enabled(&self, rule_name: &str) -> bool {
        matches!(
            self.get_severity(rule_name),
            Some(LintSeverity::Warn | LintSeverity::Error)
        )
    }

    /// Get recommended severity for a rule
    fn recommended_severity(rule_name: &str) -> Option<LintSeverity> {
        match rule_name {
            "unique_names" => Some(LintSeverity::Error),
            "deprecated_field" => Some(LintSeverity::Warn),
            _ => None,
        }
    }

    /// Get recommended configuration
    #[must_use]
    pub fn recommended() -> Self {
        Self::Recommended("recommended".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_recommended_string() {
        let yaml = r"recommended";
        let config: LintConfig = serde_yaml::from_str(yaml).unwrap();
        assert!(matches!(config, LintConfig::Recommended(_)));
        assert!(config.is_enabled("unique_names"));
        assert!(config.is_enabled("deprecated_field"));
    }

    #[test]
    fn test_parse_simple_rules() {
        let yaml = "\nunique_names: error\ndeprecated_field: off\n";
        let config: LintConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(
            config.get_severity("unique_names"),
            Some(LintSeverity::Error)
        );
        assert_eq!(
            config.get_severity("deprecated_field"),
            Some(LintSeverity::Off)
        );
        assert!(!config.is_enabled("deprecated_field"));
    }

    #[test]
    fn test_parse_recommended_with_overrides() {
        let yaml = "\nrecommended: error\ndeprecated_field: off\n";
        let config: LintConfig = serde_yaml::from_str(yaml).unwrap();
        // Should have recommended rules enabled
        assert!(config.is_enabled("unique_names"));
        // But deprecated_field is overridden to off
        assert!(!config.is_enabled("deprecated_field"));
    }

    #[test]
    fn test_default_no_rules_enabled() {
        let config = LintConfig::default();
        assert!(!config.is_enabled("unique_names"));
        assert!(!config.is_enabled("deprecated_field"));
    }

    #[test]
    fn test_recommended_constructor() {
        let config = LintConfig::recommended();
        assert_eq!(
            config.get_severity("unique_names"),
            Some(LintSeverity::Error)
        );
        assert_eq!(
            config.get_severity("deprecated_field"),
            Some(LintSeverity::Warn)
        );
    }
}
