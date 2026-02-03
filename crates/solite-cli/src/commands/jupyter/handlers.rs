//! Handlers for dot commands in Jupyter cells.
//!
//! This module extracts the dot command handling logic from the main
//! kernel code into dedicated handler functions.

use anyhow::Result;
use jupyter_protocol::{DisplayData, JupyterMessage, MediaType};
use solite_core::{
    dot::{sh::ShellResult, DotCommand, LoadCommandSource},
    Runtime,
};
use std::fmt::Write;
use tokio::sync::mpsc;

use super::kernel::ExecutionMessage;
use super::protocol::JupyterSender;
use super::render::render_sql_html;

/// Handle a dot command and send appropriate output to the frontend.
pub async fn handle_dot_command(
    cmd: DotCommand,
    runtime: &mut Runtime,
    sender: &mpsc::Sender<ExecutionMessage>,
    parent: &JupyterMessage,
) -> Result<()> {
    match cmd {
        DotCommand::Ask(_) => {
            sender
                .send_plain("Ask command not yet implemented in Jupyter", parent)
                .await?;
        }
        DotCommand::Graphviz(cmd) => match cmd.execute(runtime) {
            Ok(dot) => {
                sender.send_plain(dot, parent).await?;
            }
            Err(e) => {
                sender
                    .send_plain(format!("Graphviz error: {}", e), parent)
                    .await?;
            }
        },
        DotCommand::Dotenv(dotenv_cmd) => match dotenv_cmd.execute() {
            Ok(result) => {
                let mut output = String::new();
                let relative = result
                    .path
                    .strip_prefix(std::env::current_dir().unwrap_or_default())
                    .unwrap_or(&result.path);

                if result.loaded.is_empty() {
                    writeln!(
                        &mut output,
                        "No environment variables loaded from `{}`",
                        relative.display()
                    )?;
                } else if result.loaded.len() == 1 {
                    writeln!(
                        &mut output,
                        "Loaded `{}` from `{}`",
                        result.loaded[0],
                        relative.display()
                    )?;
                } else {
                    writeln!(
                        &mut output,
                        "Loaded {} environment variables from `{}`:",
                        result.loaded.len(),
                        relative.display()
                    )?;
                    for key in result.loaded {
                        writeln!(&mut output, "- `{}`", key)?;
                    }
                }
                sender.send_markdown(output, parent).await?;
            }
            Err(e) => {
                sender
                    .send_plain(format!("Dotenv error: {}", e), parent)
                    .await?;
            }
        },
        DotCommand::Tui(_) => {
            sender
                .send_plain("TUI command not available in Jupyter", parent)
                .await?;
        }
        DotCommand::Clear(_) => {
            sender
                .send_plain("Clear command not yet implemented in Jupyter", parent)
                .await?;
        }
        DotCommand::Print(print_cmd) => {
            sender.send_plain(print_cmd.message, parent).await?;
        }
        DotCommand::Shell(shell_cmd) => match shell_cmd.execute() {
            Ok(ShellResult::Background(child)) => {
                sender
                    .send_stdout(
                        &format!("Started background process with PID {}", child.id()),
                        parent,
                    )
                    .await?;
            }
            Ok(ShellResult::Stream(rx)) => {
                while let Ok(msg) = rx.recv() {
                    sender.send_stdout(&format!("{msg}\n"), parent).await?;
                }
            }
            Err(e) => {
                sender
                    .send_plain(format!("Shell error: {}", e), parent)
                    .await?;
            }
        },
        DotCommand::Timer(_) => {
            sender
                .send_plain("Timer command not yet implemented", parent)
                .await?;
        }
        DotCommand::Parameter(param_cmd) => {
            let msg = match param_cmd {
                solite_core::dot::ParameterCommand::Set { key, value } => {
                    match runtime.define_parameter(key.clone(), value) {
                        Ok(()) => format!("Set parameter: {}", key),
                        Err(e) => format!("Failed to set parameter {}: {}", key, e),
                    }
                }
                solite_core::dot::ParameterCommand::Unset(key) => {
                    format!("Unset parameter not yet implemented: {}", key)
                }
                solite_core::dot::ParameterCommand::List => {
                    "List parameters not yet implemented".to_string()
                }
                solite_core::dot::ParameterCommand::Clear => {
                    "Clear parameters not yet implemented".to_string()
                }
            };
            sender.send_plain(msg, parent).await?;
        }
        DotCommand::Env(env_cmd) => {
            let action = env_cmd.execute();
            let msg = match action {
                solite_core::dot::EnvAction::Set { name, .. } => {
                    format!("Set environment variable: {}", name)
                }
                solite_core::dot::EnvAction::Unset { name } => {
                    format!("Unset environment variable: {}", name)
                }
            };
            sender.send_plain(msg, parent).await?;
        }
        DotCommand::Open(open_cmd) => {
            let path = open_cmd.path.clone();
            match open_cmd.execute(runtime) {
                Ok(()) => {
                    sender
                        .send_plain(format!("Opened database at {}", path), parent)
                        .await?;
                }
                Err(e) => {
                    sender
                        .send_plain(format!("Open error: {}", e), parent)
                        .await?;
                }
            }
        }
        DotCommand::Load(load_cmd) => {
            let msg = match load_cmd.execute(&mut runtime.connection) {
                Ok(LoadCommandSource::Path(v)) => format!("Loaded '{v}'"),
                Ok(LoadCommandSource::Uv { directory, package }) => {
                    format!("Loaded '{package}' with uv from {directory}")
                }
                Err(error) => format!("Load failed: {}", error),
            };
            sender.send_plain(msg, parent).await?;
        }
        DotCommand::Tables(cmd) => match cmd.execute(runtime) {
            Ok(tables) => {
                sender.send_plain(tables.join("\n"), parent).await?;
            }
            Err(e) => {
                sender
                    .send_plain(format!("Tables error: {}", e), parent)
                    .await?;
            }
        },
        DotCommand::Schema(cmd) => match cmd.execute(runtime) {
            Ok(creates) => {
                let html = creates
                    .iter()
                    .map(|s| render_sql_html(s))
                    .collect::<Vec<String>>()
                    .join("\n");
                sender.send_html(html, parent).await?;
            }
            Err(e) => {
                sender
                    .send_plain(format!("Schema error: {}", e), parent)
                    .await?;
            }
        },
        DotCommand::Vegalite(mut cmd) => {
            match cmd.execute() {
                Ok(data) => {
                    sender.send_clear(true, parent).await?;
                    sender
                        .send_display(DisplayData::from(MediaType::VegaLiteV4(data)), parent)
                        .await?;
                    sender.send_clear(true, parent).await?;
                }
                Err(e) => {
                    sender
                        .send_plain(format!("Vega-Lite error: {}", e), parent)
                        .await?;
                }
            }
        }
        DotCommand::Export(mut cmd) => {
            match cmd.execute() {
                Ok(()) => {
                    sender
                        .send_plain(
                            format!("Export successfully to {}", cmd.target.to_string_lossy()),
                            parent,
                        )
                        .await?;
                }
                Err(e) => {
                    sender
                        .send_plain(format!("Export failed: {}", e), parent)
                        .await?;
                }
            }
        }
        DotCommand::Bench(mut cmd) => {
            let sender_clone = sender.clone();
            let parent_clone = parent.clone();
            let callback = move |interval: jiff::Span| {
                let msg = format!("Benchmark running... elapsed: {:?}", interval);
                let sender = sender_clone.clone();
                let parent = parent_clone.clone();
                tokio::spawn(async move {
                    let _ = sender.send_clear(false, &parent).await;
                    let _ = sender.send_plain(msg, &parent).await;
                });
            };

            match cmd.execute(Some(Box::new(callback))) {
                Ok(result) => {
                    sender.send_clear(false, parent).await?;
                    sender.send_plain(result.report(), parent).await?;
                }
                Err(_) => {
                    sender.send_error("BenchmarkError", "Benchmark failed").await?;
                }
            }
        }
    }
    Ok(())
}
