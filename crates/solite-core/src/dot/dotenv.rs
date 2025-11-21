use std::path::PathBuf;

use serde::Serialize;

#[derive(Serialize, Debug, PartialEq)]
pub struct DotenvCommand {
}

impl DotenvCommand {
  pub fn execute(&self)-> DotenvResult {
    let mut loaded = vec![];
    let path = std::env::current_dir().unwrap().join(".env");
        let iter = dotenvy::from_path_iter(&path).unwrap();
        for item in iter {
          let (key, value) = item.unwrap();
          std::env::set_var(&key, &value);
          loaded.push(key);
        }


    DotenvResult {
      path,
      loaded  
    }

    }
}

pub struct DotenvResult {
  pub path: PathBuf,
  pub loaded: Vec<String>
}