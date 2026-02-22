//! Parameter command parsing and handling.
//!
//! Provides the `.param` / `.parameter` dot command for managing query parameters.

use serde::Serialize;

use crate::ParseDotError;

/// Parameter command variants.
#[derive(Serialize, Debug, PartialEq)]
pub enum ParameterCommand {
    /// Set a parameter value.
    Set { key: String, value: String },
    /// Unset a parameter.
    Unset(String),
    /// List all parameters.
    List,
    /// Clear all parameters.
    Clear,
}

/// Parse a parameter command from input.
///
/// # Examples
///
/// ```
/// use solite_core::dot::param::parse_parameter;
///
/// let cmd = parse_parameter("set foo bar".to_string());
/// // Returns ParameterCommand::Set { key: "foo", value: "bar" }
/// ```
pub fn parse_parameter(line: String) -> Result<ParameterCommand, ParseDotError> {
    let trimmed = line.trim_end();

    match trimmed.split_once(' ') {
        Some((word, rest)) => match word {
            "set" => {
                let (k, v) = rest.split_once(' ').ok_or_else(|| {
                    ParseDotError::InvalidArgument(
                        "set requires key and value: .param set <key> <value>".to_string(),
                    )
                })?;
                Ok(ParameterCommand::Set {
                    key: k.to_owned(),
                    value: v.to_owned(),
                })
            }
            "unset" => Ok(ParameterCommand::Unset(rest.to_owned())),
            _ => Err(ParseDotError::InvalidArgument(format!(
                "unknown parameter subcommand: {}",
                word
            ))),
        },
        None => match trimmed {
            "list" => Ok(ParameterCommand::List),
            "clear" => Ok(ParameterCommand::Clear),
            "" => Err(ParseDotError::InvalidArgument(
                "missing subcommand: set, unset, list, or clear".to_string(),
            )),
            _ => Err(ParseDotError::InvalidArgument(format!(
                "unknown parameter subcommand: {}",
                trimmed
            ))),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_set() {
        let result = parse_parameter("set foo bar".to_string()).unwrap();
        assert_eq!(
            result,
            ParameterCommand::Set {
                key: "foo".to_string(),
                value: "bar".to_string()
            }
        );
    }

    #[test]
    fn test_parse_set_with_spaces_in_value() {
        let result = parse_parameter("set name alex garcia".to_string()).unwrap();
        assert_eq!(
            result,
            ParameterCommand::Set {
                key: "name".to_string(),
                value: "alex garcia".to_string()
            }
        );
    }

    #[test]
    fn test_parse_unset() {
        let result = parse_parameter("unset foo".to_string()).unwrap();
        assert_eq!(result, ParameterCommand::Unset("foo".to_string()));
    }

    #[test]
    fn test_parse_list() {
        let result = parse_parameter("list".to_string()).unwrap();
        assert_eq!(result, ParameterCommand::List);
    }

    #[test]
    fn test_parse_clear() {
        let result = parse_parameter("clear".to_string()).unwrap();
        assert_eq!(result, ParameterCommand::Clear);
    }

    #[test]
    fn test_parse_set_missing_value() {
        let result = parse_parameter("set foo".to_string());
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_unknown_subcommand() {
        let result = parse_parameter("invalid".to_string());
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_empty() {
        let result = parse_parameter("".to_string());
        assert!(result.is_err());
    }
}
