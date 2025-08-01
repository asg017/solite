use serde::Serialize;

#[derive(Serialize, Debug, PartialEq)]
pub enum ParameterCommand {
    Set { key: String, value: String },
    Unset(String),
    List,
    Clear,
}

pub(crate) fn parse_parameter(line: String) -> ParameterCommand {
    match line.trim_end().split_once(' ') {
        Some((word, rest)) => match word {
            "set" => {
                let (k, v) = rest.split_once(' ').unwrap();
                ParameterCommand::Set {
                    key: k.to_owned(),
                    value: v.to_owned(),
                }
            }
            "unset" => ParameterCommand::Unset(rest.to_owned()),
            _ => todo!(),
        },
        None => match line.trim_end() {
            "list" => ParameterCommand::List,
            "clear" => ParameterCommand::Clear,
            _ => todo!(),
        },
    }
}