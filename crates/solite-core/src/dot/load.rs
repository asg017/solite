//! SQLite extension loading.
//!
//! This module implements the `.load` command which loads SQLite extensions
//! from either local files or Python packages via uv.
//!
//! # Usage
//!
//! ```sql
//! .load ./myextension.so              -- Load from file
//! .load ./myextension.so entry_point  -- Load with custom entry point
//! .load uv:sqlite-vec                 -- Load from Python package via uv
//! ```
//!
//! # Sources
//!
//! - **Path**: Direct path to a `.so`, `.dll`, or `.dylib` file
//! - **UV**: Python package containing SQLite extensions, installed via `uv tool`

use crate::dot::DotError;
use crate::Connection;
use serde::Serialize;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Load an extension from a Python package's site-packages directory.
fn load_extension_from_sitepackages(
    site_package_directory: &Path,
    connection: &mut Connection,
    package: &str,
    entrypoint: &Option<String>,
) -> Result<String, DotError> {
    let pkg_subdir = package.replace('-', "_");
    // Strip out package name from a `==` specifier
    let pkg_subdir = pkg_subdir
        .split_once('=')
        .map_or(pkg_subdir.clone(), |(name, _)| name.to_owned());
    let pkg_directory = site_package_directory.join(&pkg_subdir);

    let entries = std::fs::read_dir(&pkg_directory).map_err(|e| {
        DotError::Extension(format!(
            "Failed to read package directory '{}': {}",
            pkg_directory.display(),
            e
        ))
    })?;

    let possible_extensions: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry
                .path()
                .extension()
                .is_some_and(|ext| ext == "so" || ext == "dll" || ext == "dylib")
        })
        .map(|entry| entry.path())
        .collect();

    if possible_extensions.is_empty() {
        return Err(DotError::Extension(format!(
            "No extension found for package '{}'",
            package
        )));
    }

    if possible_extensions.len() > 1 {
        return Err(DotError::Extension(format!(
            "Multiple extensions found for package '{}': {:?}",
            package, possible_extensions
        )));
    }

    let extension_path = possible_extensions[0]
        .to_str()
        .ok_or_else(|| DotError::Extension("Invalid extension path".into()))?;

    connection
        .load_extension(extension_path, entrypoint)
        .map_err(|e| DotError::Extension(e.to_string()))?;

    Ok(extension_path.to_string())
}

/// Find the site-packages directory for a uv tool.
fn find_sitepackages_uv_tool(package: &str) -> Result<Option<PathBuf>, DotError> {
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

    let output = command
        .output()
        .map_err(|e| DotError::Command(format!("Failed to execute uv: {}", e)))?;

    if !output.status.success() {
        // Write stderr for debugging
        let _ = std::io::stderr().write_all(&output.stderr);
        return Ok(None);
    }

    let site_package_dir = String::from_utf8(output.stdout)
        .map_err(|e| DotError::InvalidData(format!("Invalid UTF-8 in uv output: {}", e)))?
        .trim()
        .to_string();

    let path = Path::new(&site_package_dir);
    if !path.exists() {
        return Err(DotError::FileNotFound(format!(
            "Site-packages directory not found: {}",
            site_package_dir
        )));
    }

    Ok(Some(path.to_path_buf()))
}

/// Load an extension using uv.
pub(crate) fn uv_load(
    connection: &mut Connection,
    package: &str,
    entrypoint: &Option<String>,
) -> Result<String, DotError> {
    let site_package_directory = find_sitepackages_uv_tool(package)?.ok_or_else(|| {
        DotError::Extension(format!("Failed to find site-packages for package '{}'", package))
    })?;

    load_extension_from_sitepackages(&site_package_directory, connection, package, entrypoint)
}

/// Command to load a SQLite extension.
#[derive(Serialize, Debug, PartialEq)]
pub struct LoadCommand {
    /// Path to the extension or package name.
    pub path: String,
    /// Optional entry point function name.
    pub entrypoint: Option<String>,
    /// Whether to use uv for loading.
    pub is_uv: bool,
}

/// Source from which an extension was loaded.
pub enum LoadCommandSource {
    /// Loaded from a file path.
    Path(String),
    /// Loaded from a uv package.
    Uv {
        /// Directory containing the extension.
        directory: String,
        /// Package name.
        package: String,
    },
}

impl LoadCommand {
    /// Create a new load command from arguments.
    ///
    /// # Arguments
    ///
    /// * `args` - Command arguments (path and optional entrypoint)
    ///
    /// # Syntax
    ///
    /// - `.load path` - Load from path
    /// - `.load path entrypoint` - Load from path with entry point
    /// - `.load uv:package` - Load from uv package
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

    /// Execute the load command.
    ///
    /// # Arguments
    ///
    /// * `connection` - The database connection
    ///
    /// # Returns
    ///
    /// Information about the loaded extension source, or an error if loading fails.
    pub fn execute(&self, connection: &mut Connection) -> Result<LoadCommandSource, DotError> {
        if self.is_uv {
            uv_load(connection, &self.path, &self.entrypoint).map(|path| LoadCommandSource::Uv {
                directory: path,
                package: self.path.clone(),
            })
        } else {
            connection
                .load_extension(&self.path, &self.entrypoint)
                .map_err(|e| DotError::Extension(e.to_string()))?;
            Ok(LoadCommandSource::Path(self.path.clone()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_simple_path() {
        let cmd = LoadCommand::new("./extension.so".to_string());
        assert_eq!(cmd.path, "./extension.so");
        assert!(cmd.entrypoint.is_none());
        assert!(!cmd.is_uv);
    }

    #[test]
    fn test_new_with_entrypoint() {
        let cmd = LoadCommand::new("./extension.so my_init".to_string());
        assert_eq!(cmd.path, "./extension.so");
        assert_eq!(cmd.entrypoint, Some("my_init".to_string()));
        assert!(!cmd.is_uv);
    }

    #[test]
    fn test_new_uv_package() {
        let cmd = LoadCommand::new("uv:sqlite-vec".to_string());
        assert_eq!(cmd.path, "sqlite-vec");
        assert!(cmd.entrypoint.is_none());
        assert!(cmd.is_uv);
    }

    #[test]
    fn test_new_uv_package_with_version() {
        let cmd = LoadCommand::new("uv:sqlite-vec==0.1.0".to_string());
        assert_eq!(cmd.path, "sqlite-vec==0.1.0");
        assert!(cmd.entrypoint.is_none());
        assert!(cmd.is_uv);
    }

    #[test]
    fn test_new_uv_package_with_entrypoint() {
        let cmd = LoadCommand::new("uv:mypackage init_func".to_string());
        assert_eq!(cmd.path, "mypackage");
        assert_eq!(cmd.entrypoint, Some("init_func".to_string()));
        assert!(cmd.is_uv);
    }
}
