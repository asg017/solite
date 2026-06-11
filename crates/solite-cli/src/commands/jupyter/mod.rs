//! Jupyter kernel support for Solite.
//!
//! This module provides:
//! - `install`: Install the Solite kernel specification
//! - `up`: Start the kernel from a connection file
//!
//! Submodules:
//! - `kernel`: The main kernel implementation
//! - `handlers`: Dot command handlers
//! - `protocol`: Jupyter message sending utilities
//! - `render`: HTML and table rendering

mod handlers;
mod kernel;
mod protocol;
pub(crate) mod render;

use crate::cli::{
    JupyterCommand, JupyterInstallArgs, JupyterNamespace, JupyterUninstallArgs, JupyterUpArgs,
};
use kernel::start_kernel;
use serde_json::json;
use std::env::current_exe;

/// Whether a kernel.json's argv[0] looks like a solite binary.
fn argv0_is_solite(spec: &serde_json::Value) -> bool {
    spec["argv"][0]
        .as_str()
        .and_then(|argv0| std::path::Path::new(argv0).file_stem().map(|s| s.to_owned()))
        .map(|stem| stem.to_string_lossy().starts_with("solite"))
        .unwrap_or(false)
}

fn install(args: JupyterInstallArgs) -> anyhow::Result<()> {
    let user_data_dir = runtimelib::dirs::user_data_dir()?;
    let kernelspec_path = user_data_dir
        .join("kernels")
        .join(args.name.unwrap_or_else(|| "solite".to_string()))
        .join("kernel.json");
    std::fs::create_dir_all(kernelspec_path.parent().unwrap())?;

    if kernelspec_path.exists() && !args.force {
        return Err(anyhow::anyhow!(
            "Kernel spec already exists at {}. Use --force to overwrite.",
            kernelspec_path.display()
        ));
    }

    let kernel_json = json!({
      "argv": [
        current_exe()?.to_string_lossy().to_string(),
        "jupyter",
        "up",
        "--connection",
        "{connection_file}"
      ],
      "env": {},
      "display_name": args.display.unwrap_or_else(|| "Solite".to_string()),
      "language": "sql",
      "interrupt_mode": "signal",
      "metadata": {}
    });

    let f = std::fs::File::create(&kernelspec_path)?;
    serde_json::to_writer(f, &kernel_json)?;
    println!(
        "Successfully installed Solite Jupyter kernel at {}",
        kernelspec_path.display()
    );
    Ok(())
}

fn uninstall(args: JupyterUninstallArgs) -> anyhow::Result<()> {
    let name = args.name.unwrap_or_else(|| "solite".to_string());
    let kernel_dir = runtimelib::dirs::user_data_dir()?.join("kernels").join(&name);
    if !kernel_dir.exists() {
        return Err(anyhow::anyhow!(
            "No kernelspec named '{}' found at {}",
            name,
            kernel_dir.display()
        ));
    }

    // Refuse to delete a kernelspec that isn't ours (e.g. --name python3).
    let kernel_json_path = kernel_dir.join("kernel.json");
    let contents = std::fs::read_to_string(&kernel_json_path).map_err(|e| {
        anyhow::anyhow!("Couldn't read {}: {}", kernel_json_path.display(), e)
    })?;
    let spec: serde_json::Value = serde_json::from_str(&contents)?;
    if !argv0_is_solite(&spec) {
        return Err(anyhow::anyhow!(
            "Kernelspec '{}' does not appear to be a Solite kernel (argv[0] = {}); refusing to remove it",
            name,
            spec["argv"][0]
        ));
    }

    std::fs::remove_dir_all(&kernel_dir)?;
    println!("Removed kernelspec at {}", kernel_dir.display());
    Ok(())
}

fn list() -> anyhow::Result<()> {
    let kernels_dir = runtimelib::dirs::user_data_dir()?.join("kernels");
    let entries = match std::fs::read_dir(&kernels_dir) {
        Ok(entries) => entries,
        Err(_) => {
            println!("No kernelspecs found at {}", kernels_dir.display());
            return Ok(());
        }
    };

    for entry in entries.flatten() {
        let kernel_json_path = entry.path().join("kernel.json");
        let Ok(contents) = std::fs::read_to_string(&kernel_json_path) else {
            continue;
        };
        let Ok(spec) = serde_json::from_str::<serde_json::Value>(&contents) else {
            continue;
        };
        let display_name = spec["display_name"].as_str().unwrap_or("?");
        let marker = if argv0_is_solite(&spec) { " (solite)" } else { "" };
        println!(
            "{:<24} {}{}",
            entry.file_name().to_string_lossy(),
            display_name,
            marker
        );
    }
    Ok(())
}

fn up(args: JupyterUpArgs) -> anyhow::Result<()> {
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async { start_kernel(args.connection).await })?;
    Ok(())
}

pub(crate) fn jupyter(cmd: JupyterNamespace) -> Result<(), ()> {
    match cmd.command {
        JupyterCommand::Install(args) => match install(args) {
            Ok(_) => Ok(()),
            Err(error) => {
                eprintln!("{error}");
                Err(())
            }
        },
        JupyterCommand::Uninstall(args) => match uninstall(args) {
            Ok(_) => Ok(()),
            Err(error) => {
                eprintln!("{error}");
                Err(())
            }
        },
        JupyterCommand::List => match list() {
            Ok(_) => Ok(()),
            Err(error) => {
                eprintln!("{error}");
                Err(())
            }
        },
        JupyterCommand::Up(args) => match up(args) {
            Ok(_) => Ok(()),
            Err(error) => {
                eprintln!("{error}");
                Err(())
            }
        },
    }
}
