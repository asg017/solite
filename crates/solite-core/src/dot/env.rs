//! Environment variable command parsing and handling.
//!
//! Provides the `.env` dot command for managing environment variables.

use serde::Serialize;

use crate::ParseDotError;

/// Environment command variants.
#[derive(Serialize, Debug, PartialEq)]
pub enum EnvCommand {
    /// Set an environment variable.
    Set { name: String, value: String },
    /// Unset an environment variable.
    Unset(String),
}

/// Result of executing an environment command.
#[derive(Serialize, Debug, PartialEq)]
pub enum EnvAction {
    /// Environment variable was set.
    Set { name: String, value: String },
    /// Environment variable was unset.
    Unset { name: String },
}

impl EnvCommand {
    /// Execute the environment command.
    pub fn execute(&self) -> EnvAction {
        match self {
            EnvCommand::Set { name, value } => {
                std::env::set_var(name, value);
                EnvAction::Set {
                    name: name.clone(),
                    value: value.clone(),
                }
            }
            EnvCommand::Unset(name) => {
                std::env::remove_var(name);
                EnvAction::Unset { name: name.clone() }
            }
        }
    }
}

/// Parse an environment command from input.
///
/// # Examples
///
/// ```
/// use solite_core::dot::env::parse_env;
///
/// let cmd = parse_env("set FOO bar".to_string());
/// // Returns Ok(EnvCommand::Set { name: "FOO", value: "bar" })
/// ```
pub fn parse_env(line: String) -> Result<EnvCommand, ParseDotError> {
    let trimmed = line.trim_end();

    match trimmed.split_once(' ') {
        Some((word, rest)) => match word {
            "set" => {
                let (name, value) = rest.split_once(' ').ok_or_else(|| {
                    ParseDotError::InvalidArgument(
                        "set requires name and value: .env set <name> <value>".to_string(),
                    )
                })?;
                Ok(EnvCommand::Set {
                    name: name.to_owned(),
                    value: value.to_owned(),
                })
            }
            "unset" => Ok(EnvCommand::Unset(rest.to_owned())),
            _ => Err(ParseDotError::InvalidArgument(format!(
                "unknown env subcommand: {}",
                word
            ))),
        },
        None => Err(ParseDotError::InvalidArgument(
            "missing subcommand: set or unset".to_string(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_set() {
        let result = parse_env("set FOO bar".to_string()).unwrap();
        assert_eq!(
            result,
            EnvCommand::Set {
                name: "FOO".to_string(),
                value: "bar".to_string()
            }
        );
    }

    #[test]
    fn test_parse_set_with_spaces_in_value() {
        let result = parse_env("set PATH /usr/bin:/usr/local/bin".to_string()).unwrap();
        assert_eq!(
            result,
            EnvCommand::Set {
                name: "PATH".to_string(),
                value: "/usr/bin:/usr/local/bin".to_string()
            }
        );
    }

    #[test]
    fn test_parse_unset() {
        let result = parse_env("unset FOO".to_string()).unwrap();
        assert_eq!(result, EnvCommand::Unset("FOO".to_string()));
    }

    #[test]
    fn test_parse_set_missing_value() {
        let result = parse_env("set FOO".to_string());
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_unknown_subcommand() {
        let result = parse_env("invalid".to_string());
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_empty() {
        let result = parse_env("".to_string());
        assert!(result.is_err());
    }

    #[test]
    fn test_execute_set() {
        let cmd = EnvCommand::Set {
            name: "_SOLITE_TEST_VAR".to_string(),
            value: "test_value".to_string(),
        };
        let action = cmd.execute();
        assert_eq!(
            action,
            EnvAction::Set {
                name: "_SOLITE_TEST_VAR".to_string(),
                value: "test_value".to_string()
            }
        );
        assert_eq!(std::env::var("_SOLITE_TEST_VAR").unwrap(), "test_value");

        // Clean up
        std::env::remove_var("_SOLITE_TEST_VAR");
    }

    #[test]
    fn test_execute_unset() {
        // Set up
        std::env::set_var("_SOLITE_TEST_VAR2", "value");

        let cmd = EnvCommand::Unset("_SOLITE_TEST_VAR2".to_string());
        let action = cmd.execute();
        assert_eq!(
            action,
            EnvAction::Unset {
                name: "_SOLITE_TEST_VAR2".to_string()
            }
        );
        assert!(std::env::var("_SOLITE_TEST_VAR2").is_err());
    }
}
