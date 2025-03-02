mod html;
mod jupyer_msg;
pub(crate) mod notebook;
mod server;

use crate::cli::JupyterFlags;
use crate::jupyter::server::{ConnectionSpec, JupyterServer};

use serde_json::json;
use solite_core::Runtime;
use std::fs;
use tempfile::TempDir;

async fn serve(spec: ConnectionSpec) {
    let (_stdio_tx, stdio_rx) = futures::channel::mpsc::unbounded();
    JupyterServer::start(spec, stdio_rx, Runtime::new(None))
        .await
        .unwrap();
}

fn install() {
    println!("Installing jupyter kernel...");
    let tmpdir = TempDir::new().unwrap();
    let f = fs::File::create(tmpdir.path().join("kernel.json")).unwrap();
    serde_json::to_writer(
        f,
        &json!({
          "argv": [
            std::env::current_exe().unwrap().to_string_lossy(),
            "jupyter",
            "--connection",
            "{connection_file}"
          ],
          "env": {},
          "display_name": "Solite",
          "language": "sql",
          "interrupt_mode": "signal",
          "metadata": {}
        }),
    )
    .unwrap();

    let child_result = std::process::Command::new("jupyter")
        .args([
            "kernelspec",
            "install",
            "--user",
            "--name",
            "solite",
            &tmpdir.path().to_string_lossy(),
        ])
        .spawn();

    match child_result {
        Ok(mut child) => {
            let wait_result = child.wait();
            match wait_result {
                Ok(status) => {
                    if !status.success() {
                        eprintln!("Failed to install kernelspec, try again.");
                    }
                }
                Err(err) => {
                    eprintln!("Failed to install kernelspec: {}", err);
                }
            }
        }
        Err(err) => {
            eprintln!("Failed to install kernelspec: {}", err);
            return;
        }
        _ => {}
    }

    let _ = std::fs::remove_dir(tmpdir);
    println!("Successfully install solite jupyter kernel.")
}

pub(crate) fn cli_jupyter(flags: JupyterFlags) -> Result<(), ()> {
    if flags.install {
        install();
        return Ok(());
    }
    let config_path = flags.connection.ok_or(())?;

    let spec: ConnectionSpec =
        serde_json::from_str(std::fs::read_to_string(config_path).unwrap().as_str()).unwrap();
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        serve(spec).await;
    });
    Ok(())
}
