//! Configuration for lint rules
//!
//! This module handles loading lint configuration from TOML files,
//! with support for project-local and global configuration.

use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::RuleSeverity;

/// Lint configuration loaded from TOML files
#[derive(Debug, Clone, Default, Deserialize)]
pub struct LintConfig {
    /// Rule severity overrides: rule_id -> severity
    #[serde(default)]
    pub rules: HashMap<String, RuleSeverity>,
}

// Custom deserialize for RuleSeverity since it comes from strings like "off", "warning", "error"
impl<'de> Deserialize<'de> for RuleSeverity {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        match s.to_lowercase().as_str() {
            "off" => Ok(RuleSeverity::Off),
            "warning" | "warn" => Ok(RuleSeverity::Warning),
            "error" => Ok(RuleSeverity::Error),
            _ => Err(serde::de::Error::custom(format!("unknown severity: {}", s))),
        }
    }
}

impl LintConfig {
    /// Discover config file by looking in current dir, parent dirs, then ~/.config/solite/lint.toml
    pub fn discover() -> Self {
        Self::discover_from(std::env::current_dir().ok())
    }

    /// Discover config file starting from a specific directory
    pub fn discover_from(start_dir: Option<PathBuf>) -> Self {
        // Look for solite-lint.toml in current and parent directories
        if let Some(mut dir) = start_dir {
            loop {
                let config_path = dir.join("solite-lint.toml");
                if config_path.exists() {
                    if let Ok(config) = Self::load(&config_path) {
                        return config;
                    }
                }
                if !dir.pop() {
                    break;
                }
            }
        }

        // Try global config
        if let Some(home) = home_dir() {
            let global_config = home.join(".config/solite/lint.toml");
            if global_config.exists() {
                if let Ok(config) = Self::load(&global_config) {
                    return config;
                }
            }
        }

        // Return default if no config found
        Self::default()
    }

    /// Load configuration from a specific TOML file
    pub fn load(path: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let content = std::fs::read_to_string(path)?;
        let config: LintConfig = toml::from_str(&content)?;
        Ok(config)
    }

    /// Get the effective severity for a rule (config override or default)
    pub fn get_severity(&self, rule_id: &str, default: RuleSeverity) -> RuleSeverity {
        self.rules.get(rule_id).copied().unwrap_or(default)
    }
}

/// Get the user's home directory using environment variables
/// Fallback implementation that doesn't require the `dirs` crate
fn home_dir() -> Option<PathBuf> {
    // Try HOME first (Unix)
    if let Ok(home) = std::env::var("HOME") {
        return Some(PathBuf::from(home));
    }

    // Try USERPROFILE (Windows)
    if let Ok(profile) = std::env::var("USERPROFILE") {
        return Some(PathBuf::from(profile));
    }

    // Try HOMEDRIVE + HOMEPATH (Windows alternative)
    if let (Ok(drive), Ok(path)) = (std::env::var("HOMEDRIVE"), std::env::var("HOMEPATH")) {
        return Some(PathBuf::from(format!("{}{}", drive, path)));
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = LintConfig::default();
        assert!(config.rules.is_empty());
    }

    #[test]
    fn test_get_severity_with_default() {
        let config = LintConfig::default();
        assert_eq!(
            config.get_severity("some-rule", RuleSeverity::Warning),
            RuleSeverity::Warning
        );
    }

    #[test]
    fn test_get_severity_with_override() {
        let mut config = LintConfig::default();
        config.rules.insert("some-rule".to_string(), RuleSeverity::Error);
        assert_eq!(
            config.get_severity("some-rule", RuleSeverity::Warning),
            RuleSeverity::Error
        );
    }

    #[test]
    fn test_parse_toml_config() {
        let toml = r#"
            [rules]
            empty-blob-literal = "off"
            double-quoted-string = "error"
            some-warning = "warn"
        "#;

        let config: LintConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.rules.get("empty-blob-literal"), Some(&RuleSeverity::Off));
        assert_eq!(config.rules.get("double-quoted-string"), Some(&RuleSeverity::Error));
        assert_eq!(config.rules.get("some-warning"), Some(&RuleSeverity::Warning));
    }

    #[test]
    fn test_parse_invalid_severity() {
        let toml = r#"
            [rules]
            some-rule = "invalid"
        "#;

        let result: Result<LintConfig, _> = toml::from_str(toml);
        assert!(result.is_err());
    }
}
