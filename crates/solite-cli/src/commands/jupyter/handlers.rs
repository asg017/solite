//! Handlers for dot commands in Jupyter cells.
//!
//! This module extracts the dot command handling logic from the main
//! kernel code into dedicated handler functions.

use anyhow::Result;
use jupyter_protocol::{DisplayData, JupyterMessage, MediaType};
use solite_core::{
    dot::{sh::ShellResult, DotCommand, LoadCommandSource},
    sqlite::InterruptHandle,
    Runtime,
};
use std::fmt::Write;
use std::sync::Mutex;
use tokio::sync::mpsc;

use super::kernel::{handle_code, send_statement_result, ExecutionMessage};
use super::protocol::JupyterSender;
use super::render::render_sql_html;

/// Handle a dot command and send appropriate output to the frontend.
pub fn handle_dot_command<'a>(
    cmd: DotCommand,
    runtime: &'a mut Runtime,
    sender: &'a mpsc::Sender<ExecutionMessage>,
    parent: &'a JupyterMessage,
    interrupt_handle: &'a Mutex<InterruptHandle>,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + 'a>> {
    Box::pin(handle_dot_command_inner(
        cmd,
        runtime,
        sender,
        parent,
        interrupt_handle,
    ))
}

async fn handle_dot_command_inner(
    cmd: DotCommand,
    runtime: &mut Runtime,
    sender: &mpsc::Sender<ExecutionMessage>,
    parent: &JupyterMessage,
    interrupt_handle: &Mutex<InterruptHandle>,
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
                    .send_error("GraphvizError", &format!("{}", e)).await?;
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
                    .send_error("DotenvError", &format!("{}", e)).await?;
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
        DotCommand::Help(help_cmd) => {
            sender.send_plain(help_cmd.execute(), parent).await?;
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
                    .send_error("ShellError", &format!("{}", e)).await?;
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
                        Err(e) => {
                            sender
                                .send_error(
                                    "ParameterError",
                                    &format!("Failed to set parameter {}: {}", key, e),
                                )
                                .await?;
                            return Ok(());
                        }
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
                        .send_error("OpenError", &format!("{}", e)).await?;
                }
            }
        }
        DotCommand::Load(load_cmd) => {
            match load_cmd.execute(&mut runtime.connection) {
                Ok(LoadCommandSource::Path(v)) => {
                    sender.send_plain(format!("Loaded '{v}'"), parent).await?;
                }
                Ok(LoadCommandSource::Uv { directory, package }) => {
                    sender
                        .send_plain(
                            format!("Loaded '{package}' with uv from {directory}"),
                            parent,
                        )
                        .await?;
                }
                Err(error) => {
                    sender
                        .send_error("LoadError", &format!("{}", error))
                        .await?;
                }
            }
        }
        DotCommand::Tables(cmd) => match cmd.execute(runtime) {
            Ok(tables) => {
                sender.send_plain(tables.join("\n"), parent).await?;
            }
            Err(e) => {
                sender
                    .send_error("TablesError", &format!("{}", e)).await?;
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
                    .send_error("SchemaError", &format!("{}", e)).await?;
            }
        },
        DotCommand::Vegalite(mut cmd) => {
            match cmd.execute() {
                Ok(data) => {
                    sender.send_clear(true, parent).await?;
                    sender
                        .send_display(DisplayData::from(MediaType::VegaLiteV4(serde_json::Value::Object(data))), parent)
                        .await?;
                    sender.send_clear(true, parent).await?;
                }
                Err(e) => {
                    sender
                        .send_error("VegaLiteError", &format!("{}", e)).await?;
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
                        .send_error("ExportError", &format!("{}", e)).await?;
                }
            }
        }
        #[cfg(feature = "ritestream")]
        DotCommand::Stream(stream_cmd) => {
            // Extract db_path before spawn_blocking to avoid Send issues
            let db_name = runtime.connection.db_name();
            let msg = tokio::task::spawn_blocking(move || {
                let db_path = match db_name {
                    Some(ref p) => std::path::Path::new(p),
                    None => return "Stream error: no database file open (in-memory?)".to_string(),
                };
                match &stream_cmd.action {
                    solite_core::dot::StreamAction::Sync { url } => {
                        match ritestream_api::sync(url, db_path) {
                            Ok(Some(r)) => {
                                format!("Synced (txid={}, {} pages)", r.txid, r.page_count)
                            }
                            Ok(None) => "Nothing to sync (database empty or missing)".to_string(),
                            Err(e) => format!("Stream sync error: {}", e),
                        }
                    }
                    solite_core::dot::StreamAction::Restore { url } => {
                        match ritestream_api::restore(url, db_path) {
                            Ok(()) => format!("Restored from {}", url),
                            Err(e) => format!("Stream restore error: {}", e),
                        }
                    }
                }
            })
            .await
            .unwrap_or_else(|e| format!("Stream task error: {}", e));
            sender.send_plain(msg, parent).await?;
        }
        DotCommand::Call(_) => { /* resolved to SqlStatement in next_stepx() */ }
        DotCommand::Run(run_cmd) => {
            if let Some(ref proc_name) = run_cmd.procedure {
                for (key, value) in &run_cmd.parameters {
                    if let Err(e) = runtime.define_parameter(key.clone(), value.clone()) {
                        sender
                            .send_error("RunError", &format!("Failed to set parameter {}: {}", key, e))
                            .await?;
                        return Ok(());
                    }
                }
                if let Err(e) = runtime.load_file(&run_cmd.file) {
                    sender
                        .send_error("RunError", &format!("Failed to load file '{}': {}", run_cmd.file, e))
                        .await?;
                    return Ok(());
                }
                let proc = match runtime.get_procedure(proc_name) {
                    Some(p) => p.clone(),
                    None => {
                        sender
                            .send_error("RunError", &format!("Unknown procedure: '{}'", proc_name))
                            .await?;
                        return Ok(());
                    }
                };
                match runtime.prepare_with_parameters(&proc.sql) {
                    Ok((_, Some(stmt))) => {
                        send_statement_result(stmt, sender, parent).await?;
                    }
                    Ok((_, None)) => {
                        sender
                            .send_error("RunError", &format!("Procedure '{}' prepared to empty statement", proc_name))
                            .await?;
                    }
                    Err(e) => {
                        sender
                            .send_error("RunError", &format!("Error preparing procedure '{}': {:?}", proc_name, e))
                            .await?;
                    }
                }
            } else {
                let saved = match runtime.run_file_begin(&run_cmd.file, &run_cmd.parameters) {
                    Ok(s) => s,
                    Err(e) => {
                        sender.send_error("RunError", &format!("{}", e)).await?;
                        return Ok(());
                    }
                };
                // run_file_begin swapped in a stack containing only the run
                // file's block, so the shared stepping loop drains exactly it.
                let result = handle_code(runtime, sender, parent, interrupt_handle).await;
                runtime.run_file_end(saved);
                result?;
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
