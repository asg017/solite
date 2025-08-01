use serde::Serialize;
use crate::{
    Connection
};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

fn load_extension_from_sitepackages(
    site_package_directory: &Path,
    connection: &mut Connection,
    package: &str,
    entrypoint: &Option<String>,
) -> anyhow::Result<String> {
    let pkg_subdir = package.replace('-', "_");
    // Strip out package name from a `==` specifier
    // TODO support other constraints like `>=` or whatever
    let pkg_subdir = pkg_subdir
        .split_once('=')
        .map_or(pkg_subdir.clone(), |(name, _)| name.to_owned());
    let pkg_directory = site_package_directory.join(pkg_subdir);
    let possible_extensions = std::fs::read_dir(pkg_directory)
        .unwrap()
        .filter_map(|entry| {
            let entry = entry.unwrap();
            if entry
                .path()
                .extension()
                .map_or(false, |ext| ext == "so" || ext == "dll" || ext == "dylib")
            {
                Some(entry.path())
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    if possible_extensions.len() == 0 {
        panic!("No extension found for package {package}");
    } else if possible_extensions.len() > 1 {
        panic!("Multiple extensions found for package {package}");
    }
    let extension_path = possible_extensions[0].to_str().unwrap();
    match connection.load_extension(extension_path, entrypoint) {
        Ok(()) => Ok(extension_path.to_string()),
        Err(err) => Err(err),
    }
}

fn find_sitepackages_uv_tool(package: &str) -> anyhow::Result<Option<PathBuf>> {
    let mut command = Command::new("uv");
    command.args([
        "tool",
        "run",
        "--from",
        package,
        "python",
        "-c",
        "import site; print(site.getsitepackages()[0])",
    ]);

    let output = command.output().expect("Failed to execute command");
    if !output.status.success() {
        std::io::stderr().write_all(&output.stderr).unwrap();
        return Ok(None);
    }
    let site_package_directory = String::from_utf8(output.stdout).unwrap().trim().to_string();
    let site_package_directory = Path::new(&site_package_directory);
    assert!(site_package_directory.exists());

    Ok(Some(site_package_directory.to_path_buf()))
}
pub(crate) fn uv_load(
    connection: &mut Connection,
    package: &str,
    entrypoint: &Option<String>,
) -> anyhow::Result<String> {
    let site_package_directory = find_sitepackages_uv_tool(package)?.unwrap();
    load_extension_from_sitepackages(&site_package_directory, connection, package, entrypoint)
}


#[derive(Serialize, Debug, PartialEq)]
pub struct LoadCommand {
    pub path: String,
    pub entrypoint: Option<String>,
    pub is_uv: bool,
}

pub enum LoadCommandSource {
    Path(String),
    Uv { directory: String, package: String },
}

impl LoadCommand {
    pub fn new(args: String) -> Self {
        let (args, is_uv) = match args.strip_prefix("uv:") {
            Some(args) => (args, true),
            None => (args.as_str(), false),
        };

        let (path, entrypoint) = match args.split_once(' ') {
            Some((path, entrypoint)) => (path.to_string(), Some(entrypoint.trim().to_string())),
            None => (args.to_owned(), None),
        };
        Self {
            path,
            entrypoint,
            is_uv,
        }
    }
    pub fn execute(&self, connection: &mut Connection) -> anyhow::Result<LoadCommandSource> {
        if self.is_uv {
            uv_load(connection, &self.path, &self.entrypoint).map(|path| {
                LoadCommandSource::Uv {
                    directory: path,
                    package: self.path.clone(),
                }
            })
        } else {
            connection
                .load_extension(&self.path, &self.entrypoint)
                .map(|_| LoadCommandSource::Path(self.path.clone()))
        }
    }
}
