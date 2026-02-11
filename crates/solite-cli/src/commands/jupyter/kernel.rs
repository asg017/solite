//! Solite Jupyter kernel implementation.
//!
//! This module contains the `SoliteKernel` struct and related functionality
//! for handling Jupyter protocol messages.

use anyhow::{Context as _, Result};
use jupyter_protocol::{
    CodeMirrorMode, CommInfoReply, ConnectionInfo, DisplayData, ErrorOutput, ExecuteReply,
    ExecutionCount, HelpLink, HistoryReply, InspectReply, IsCompleteReply, IsCompleteReplyStatus,
    JupyterMessage, JupyterMessageContent, KernelInfoReply, LanguageInfo, Media, MediaType,
    ReplyError, ReplyStatus, Status,
};

/// Messages sent from the runtime task back to the shell handler.
pub enum ExecutionMessage {
    /// A display message to send to iopub.
    Display(JupyterMessage),
    /// An error occurred during execution.
    Error { ename: String, evalue: String },
}
use runtimelib::{KernelIoPubConnection, KernelShellConnection};
use solite_core::{Runtime, StepError, StepResult};
use std::path::PathBuf;
use tokio::sync::mpsc;
use uuid::Uuid;

use super::handlers::handle_dot_command;
use super::protocol::JupyterSender;
use super::render::{render_statement, UiResponse};

/// The Solite Jupyter kernel.
pub struct SoliteKernel {
    execution_count: ExecutionCount,
    iopub: KernelIoPubConnection,
    runtime: mpsc::Sender<(String, JupyterMessage, mpsc::Sender<ExecutionMessage>)>,
}

impl SoliteKernel {
    /// Start the kernel with the given connection info.
    pub async fn start(connection_info: &ConnectionInfo) -> Result<()> {
        let runtime = Runtime::new(None);
        let session_id = Uuid::new_v4().to_string();

        let mut heartbeat =
            runtimelib::create_kernel_heartbeat_connection(connection_info).await?;
        let shell_connection =
            runtimelib::create_kernel_shell_connection(connection_info, &session_id).await?;
        let mut control_connection =
            runtimelib::create_kernel_control_connection(connection_info, &session_id).await?;
        let _stdin_connection =
            runtimelib::create_kernel_stdin_connection(connection_info, &session_id).await?;
        let iopub_connection =
            runtimelib::create_kernel_iopub_connection(connection_info, &session_id).await?;

        let (tx, mut rx) =
            mpsc::channel::<(String, JupyterMessage, mpsc::Sender<ExecutionMessage>)>(10);

        // Spawn the runtime handler task
        tokio::spawn(async move {
            let mut rt = runtime;
            while let Some((code, parent, response)) = rx.recv().await {
                // Debugging mode: prefix code with @@ to see message details
                if code.starts_with("@@") {
                    let r = format!("{}\n{:?}", parent.metadata["cellId"], parent);
                    let _ = response
                        .send(ExecutionMessage::Display(
                            DisplayData::new(vec![MediaType::Plain(r)].into())
                                .as_child_of(&parent),
                        ))
                        .await;
                    continue;
                }

                rt.enqueue(
                    "<anonymous>",
                    code.as_str(),
                    solite_core::BlockSource::JupyerCell,
                );
                if let Err(e) = handle_code(&mut rt, &response, &parent).await {
                    eprintln!("Error handling code: {}", e);
                }
            }
        });

        let mut solite_kernel = Self {
            execution_count: Default::default(),
            iopub: iopub_connection,
            runtime: tx,
        };

        let heartbeat_handle = tokio::spawn({
            async move { while let Ok(()) = heartbeat.single_heartbeat().await {} }
        });

        let control_handle = tokio::spawn({
            async move {
                while let Ok(message) = control_connection.read().await {
                    if let JupyterMessageContent::KernelInfoRequest(_) = message.content {
                        let sent = control_connection
                            .send(Self::kernel_info().as_child_of(&message))
                            .await;

                        if let Err(err) = sent {
                            eprintln!("Error on control: {}", err);
                        }
                    }
                }
            }
        });

        let shell_handle = tokio::spawn(async move {
            if let Err(err) = solite_kernel.handle_shell(shell_connection).await {
                eprintln!("Shell error: {}\nBacktrace:\n{}", err, err.backtrace());
            }
        });

        let join_fut =
            futures::future::try_join_all(vec![heartbeat_handle, control_handle, shell_handle]);

        join_fut.await?;

        Ok(())
    }

    async fn send_error(
        &mut self,
        ename: &str,
        evalue: &str,
        parent: &JupyterMessage,
    ) -> Result<()> {
        self.iopub
            .send(
                ErrorOutput {
                    ename: ename.to_string(),
                    evalue: evalue.to_string(),
                    traceback: Default::default(),
                }
                .as_child_of(parent),
            )
            .await
    }

    /// Execute code and return any error that occurred.
    async fn execute(
        &mut self,
        request: &JupyterMessage,
    ) -> Result<Option<(String, String)>> {
        let code = match &request.content {
            JupyterMessageContent::ExecuteRequest(req) => req.code.clone(),
            _ => return Err(anyhow::anyhow!("Invalid message type for execution")),
        };

        let cmd_tx = self.runtime.clone();
        let parent = request.clone();
        let handle = tokio::spawn(async move {
            let (resp_tx, resp_rx) = mpsc::channel(10);
            if let Err(e) = cmd_tx.send((code, parent, resp_tx)).await {
                eprintln!("Failed to send code to runtime: {}", e);
            }
            resp_rx
        });

        let mut rx = handle.await?;
        let mut error_info: Option<(String, String)> = None;

        while let Some(msg) = rx.recv().await {
            match msg {
                ExecutionMessage::Display(jupyter_msg) => {
                    self.iopub.send(jupyter_msg).await?;
                }
                ExecutionMessage::Error { ename, evalue } => {
                    // Send error output to iopub
                    self.iopub
                        .send(
                            ErrorOutput {
                                ename: ename.clone(),
                                evalue: evalue.clone(),
                                traceback: Default::default(),
                            }
                            .as_child_of(request),
                        )
                        .await?;
                    error_info = Some((ename, evalue));
                }
            }
        }

        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        Ok(error_info)
    }

    /// Handle messages on the shell connection.
    pub async fn handle_shell(&mut self, mut connection: KernelShellConnection) -> Result<()> {
        loop {
            let msg = connection.read().await?;
            if let Err(err) = self.handle_shell_message(&msg, &mut connection).await {
                eprintln!("Error on shell: {}", err);
            }
        }
    }

    async fn handle_shell_message(
        &mut self,
        parent: &JupyterMessage,
        shell: &mut KernelShellConnection,
    ) -> Result<()> {
        // Send busy status
        self.iopub.send(Status::busy().as_child_of(parent)).await?;

        match &parent.content {
            JupyterMessageContent::CommInfoRequest(_) => {
                let reply = CommInfoReply {
                    status: ReplyStatus::Ok,
                    comms: Default::default(),
                    error: None,
                }
                .as_child_of(parent);
                shell.send(reply).await?;
            }

            JupyterMessageContent::ExecuteRequest(_) => {
                let execution_count = self.one_up_execution_count();

                // Execute code first, then send reply with appropriate status
                let (status, error) = match self.execute(parent).await {
                    Ok(None) => (ReplyStatus::Ok, None),
                    Ok(Some((ename, evalue))) => (
                        ReplyStatus::Error,
                        Some(Box::new(ReplyError {
                            ename,
                            evalue,
                            traceback: Default::default(),
                        })),
                    ),
                    Err(err) => {
                        self.send_error("ExecutionError", &err.to_string(), parent)
                            .await?;
                        (
                            ReplyStatus::Error,
                            Some(Box::new(ReplyError {
                                ename: "ExecutionError".to_string(),
                                evalue: err.to_string(),
                                traceback: Default::default(),
                            })),
                        )
                    }
                };

                let reply = ExecuteReply {
                    status,
                    execution_count,
                    user_expressions: Default::default(),
                    payload: Default::default(),
                    error,
                }
                .as_child_of(parent);
                shell.send(reply).await?;
            }

            JupyterMessageContent::HistoryRequest(_) => {
                let reply = HistoryReply {
                    history: Default::default(),
                    status: ReplyStatus::Ok,
                    error: None,
                }
                .as_child_of(parent);
                shell.send(reply).await?;
            }

            JupyterMessageContent::InspectRequest(_) => {
                let reply = InspectReply {
                    found: false,
                    data: Media::default(),
                    metadata: Default::default(),
                    status: ReplyStatus::Ok,
                    error: None,
                }
                .as_child_of(parent);
                shell.send(reply).await?;
            }

            JupyterMessageContent::IsCompleteRequest(_) => {
                let reply = IsCompleteReply {
                    status: IsCompleteReplyStatus::Complete,
                    indent: String::new(),
                }
                .as_child_of(parent);
                shell.send(reply).await?;
            }

            JupyterMessageContent::KernelInfoRequest(_) => {
                let reply = Self::kernel_info().as_child_of(parent);
                shell.send(reply).await?;
            }

            _ => {}
        };

        // Send idle status
        self.iopub.send(Status::idle().as_child_of(parent)).await?;

        Ok(())
    }

    fn kernel_info() -> KernelInfoReply {
        KernelInfoReply {
            status: ReplyStatus::Ok,
            protocol_version: "5.3".to_string(),
            implementation: "Solite kernel".to_string(),
            implementation_version: env!("CARGO_PKG_VERSION").to_string(),
            language_info: LanguageInfo {
                name: "sqlite".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                mimetype: "text/x.sqlite".to_string(),
                file_extension: ".sql".to_string(),
                pygments_lexer: "sqlite".to_string(),
                codemirror_mode: CodeMirrorMode::Simple("sql".to_string()),
                nbconvert_exporter: "script".to_string(),
            },
            banner: "Solite Kernel".to_string(),
            help_links: vec![HelpLink {
                text: "Solite Documentation".to_string(),
                url: "https://github.com/asg017/solite".to_string(),
            }],
            debugger: false,
            error: None,
        }
    }

    fn one_up_execution_count(&mut self) -> ExecutionCount {
        self.execution_count.0 += 1;
        self.execution_count
    }
}

/// Handle code execution within a cell.
async fn handle_code(
    runtime: &mut Runtime,
    response: &mpsc::Sender<ExecutionMessage>,
    parent: &JupyterMessage,
) -> Result<()> {
    loop {
        match runtime.next_stepx() {
            Some(Ok(step)) => match step.result {
                StepResult::SqlStatement { stmt, .. } => match render_statement(&stmt) {
                    Ok(tbl) => {
                        response
                            .send_display(
                                DisplayData::new(
                                    vec![
                                        MediaType::Plain(stmt.sql()),
                                        MediaType::Html(tbl.html.unwrap()),
                                    ]
                                    .into(),
                                ),
                                parent,
                            )
                            .await?;
                    }
                    Err(err) => {
                        response
                            .send_error("RenderError", &format!("{:?}", err))
                            .await?;
                        return Ok(());
                    }
                },
                StepResult::DotCommand(cmd) => {
                    handle_dot_command(cmd, runtime, response, parent).await?;
                }
                StepResult::ProcedureDefinition(_) => { /* already registered in runtime */ }
            },
            None => {
                return Ok(());
            }
            Some(Err(error)) => match error {
                StepError::Prepare {
                    error,
                    file_name,
                    src,
                    offset,
                } => {
                    let error_string = crate::errors::report_error_string(
                        file_name.as_str(),
                        &src,
                        &error,
                        Some(offset),
                    );
                    response.send_error("SQLError", &error_string).await?;
                    return Ok(());
                }
                StepError::ParseDot(error) => {
                    response
                        .send_error("DotCommandError", &error.to_string())
                        .await?;
                    return Ok(());
                }
            },
        }
    }
}

/// Start the Solite Jupyter kernel from a connection file path.
pub async fn start_kernel(connection_filepath: PathBuf) -> Result<()> {
    let conn_file = std::fs::read_to_string(&connection_filepath)
        .with_context(|| format!("Couldn't read connection file: {:?}", connection_filepath))?;
    let spec: ConnectionInfo = serde_json::from_str(&conn_file).with_context(|| {
        format!(
            "Connection file is not valid JSON: {:?}",
            connection_filepath
        )
    })?;

    SoliteKernel::start(&spec).await?;
    Ok(())
}
