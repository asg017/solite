use serde::Serialize;

#[derive(Serialize, Debug, PartialEq)]
pub enum EnvCommand {
    Set { name: String, value: String },
    Unset(String),
}

#[derive(Serialize, Debug, PartialEq)]
pub enum EnvAction {
    Set { name: String, value: String },
    Unset { name: String },
}

impl EnvCommand {
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
                EnvAction::Unset {
                    name: name.clone(),
                }
            }
        }
    }
}

pub(crate) fn parse_env(line: String) -> EnvCommand {
    match line.trim_end().split_once(' ') {
        Some((word, rest)) => match word {
            "set" => {
                let (name, value) = rest.split_once(' ').unwrap();
                EnvCommand::Set {
                    name: name.to_owned(),
                    value: value.to_owned(),
                }
            }
            "unset" => EnvCommand::Unset(rest.to_owned()),
            _ => todo!(),
        },
        None => todo!(),
    }
}
