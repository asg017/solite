//! Solite Jupyter kernel implementation.
//!
//! This module contains the `SoliteKernel` struct and related functionality
//! for handling Jupyter protocol messages.

use anyhow::{Context as _, Result};
use jupyter_protocol::{
    CodeMirrorMode, CommInfoReply, CompleteReply, ConnectionInfo, DisplayData, ErrorOutput,
    ExecuteInput, ExecuteReply, ExecutionCount, HelpLink, HistoryReply, InspectReply,
    InterruptReply, IsCompleteReply, IsCompleteReplyStatus, JupyterMessage,
    JupyterMessageContent, KernelInfoReply, LanguageInfo, Media, MediaType, ReplyError,
    ReplyStatus, ShutdownReply, Status,
};

/// Messages sent from the runtime task back to the shell handler.
#[allow(clippy::large_enum_variant)]
pub enum ExecutionMessage {
    /// A display message to send to iopub.
    Display(JupyterMessage),
    /// An error occurred during execution.
    Error { ename: String, evalue: String },
}

/// Requests sent from the shell handler to the runtime task, which owns the
/// `Runtime` (and its non-`Sync` connection).
#[allow(clippy::large_enum_variant)]
enum RuntimeRequest {
    Execute {
        code: String,
        parent: JupyterMessage,
        response: mpsc::Sender<ExecutionMessage>,
    },
    Complete {
        code: String,
        /// Cursor position in unicode code points, per the Jupyter spec.
        cursor_pos: usize,
        reply: tokio::sync::oneshot::Sender<CompleteReply>,
    },
    Inspect {
        code: String,
        /// Cursor position in unicode code points, per the Jupyter spec.
        cursor_pos: usize,
        reply: tokio::sync::oneshot::Sender<InspectReply>,
    },
}
use runtimelib::{KernelIoPubConnection, KernelShellConnection};
use solite_core::sqlite::InterruptHandle;
use solite_core::{Runtime, StepError, StepResult};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use uuid::Uuid;

use super::handlers::handle_dot_command;
use super::protocol::JupyterSender;
use super::render::render_statement;

/// The Solite Jupyter kernel.
pub struct SoliteKernel {
    execution_count: ExecutionCount,
    iopub: KernelIoPubConnection,
    runtime: mpsc::Sender<RuntimeRequest>,
}

impl SoliteKernel {
    /// Start the kernel with the given connection info.
    pub async fn start(connection_info: &ConnectionInfo) -> Result<()> {
        let runtime = Runtime::new(None)?;
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

        let (tx, mut rx) = mpsc::channel::<RuntimeRequest>(10);

        // Shared handle for interrupting the in-flight statement from outside
        // the runtime task. Refreshed by the runtime task whenever the
        // connection may have changed (e.g. `.open`).
        let interrupt_handle = Arc::new(Mutex::new(runtime.connection.interrupt_handle()));

        // Interrupt the running statement on SIGINT. The kernelspec declares
        // `"interrupt_mode": "signal"`, so Jupyter frontends deliver interrupts
        // as SIGINT to this process; without a handler the whole kernel dies.
        let sigint_handle = Arc::clone(&interrupt_handle);
        tokio::spawn(async move {
            while tokio::signal::ctrl_c().await.is_ok() {
                sigint_handle.lock().unwrap().interrupt();
            }
        });

        // Spawn the runtime handler task
        let runtime_interrupt = Arc::clone(&interrupt_handle);
        tokio::spawn(async move {
            let mut rt = runtime;
            // Per-session `.timer on/off` state, like the REPL's.
            let mut timer = false;
            while let Some(request) = rx.recv().await {
                let (code, parent, response) = match request {
                    RuntimeRequest::Complete {
                        code,
                        cursor_pos,
                        reply,
                    } => {
                        let _ = reply.send(completions(&rt, &code, cursor_pos));
                        continue;
                    }
                    RuntimeRequest::Inspect {
                        code,
                        cursor_pos,
                        reply,
                    } => {
                        let _ = reply.send(inspect(&rt, &code, cursor_pos));
                        continue;
                    }
                    RuntimeRequest::Execute {
                        code,
                        parent,
                        response,
                    } => (code, parent, response),
                };

                // Debugging backdoor: with SOLITE_KERNEL_DEBUG set, prefix a
                // cell with @@ to dump the raw execute_request message.
                if code.starts_with("@@") && std::env::var_os("SOLITE_KERNEL_DEBUG").is_some() {
                    let cell_id = parent
                        .metadata
                        .get("cellId")
                        .unwrap_or(&serde_json::Value::Null);
                    let r = format!("{}\n{:?}", cell_id, parent);
                    let _ = response
                        .send(ExecutionMessage::Display(
                            DisplayData::new(vec![MediaType::Plain(r)].into())
                                .as_child_of(&parent),
                        ))
                        .await;
                    continue;
                }

                // Jupyter sugar: a cell that is exactly one (optionally
                // schema-qualified, optionally quoted) identifier naming an
                // existing table/view is rewritten to `.describe`. Core
                // stays ignorant of this; `execute_input` and history above
                // already captured the original `code`.
                let code = match bare_identifier(&code) {
                    Some((schema, name))
                        if solite_core::dot::describe::table_kind(&rt, schema.as_deref(), &name)
                            .ok()
                            .flatten()
                            .is_some() =>
                    {
                        format!(".describe {}", quote_for_dot(schema.as_deref(), &name))
                    }
                    _ => code,
                };

                rt.enqueue(
                    "<anonymous>",
                    code.as_str(),
                    solite_core::BlockSource::JupyterCell,
                );
                if let Err(e) =
                    handle_code(&mut rt, &response, &parent, &runtime_interrupt, &mut timer).await
                {
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

        let control_interrupt = Arc::clone(&interrupt_handle);
        let control_handle = tokio::spawn({
            async move {
                while let Ok(message) = control_connection.read().await {
                    match &message.content {
                        JupyterMessageContent::KernelInfoRequest(_) => {
                            let sent = control_connection
                                .send(Self::kernel_info().as_child_of(&message))
                                .await;

                            if let Err(err) = sent {
                                eprintln!("Error on control: {}", err);
                            }
                        }
                        // Sent when the kernelspec uses `"interrupt_mode": "message"`;
                        // ours uses "signal" (SIGINT, handled above), but support
                        // both so either kernelspec works.
                        JupyterMessageContent::InterruptRequest(_) => {
                            control_interrupt.lock().unwrap().interrupt();
                            let sent = control_connection
                                .send(InterruptReply::default().as_child_of(&message))
                                .await;
                            if let Err(err) = sent {
                                eprintln!("Error on control: {}", err);
                            }
                        }
                        JupyterMessageContent::ShutdownRequest(req) => {
                            let reply = ShutdownReply {
                                restart: req.restart,
                                status: ReplyStatus::Ok,
                                error: None,
                            };
                            if let Err(err) =
                                control_connection.send(reply.as_child_of(&message)).await
                            {
                                eprintln!("Error on control: {}", err);
                            }
                            // The frontend owns restarting; either way this
                            // process is done.
                            std::process::exit(0);
                        }
                        _ => {}
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
            .map_err(|e| anyhow::anyhow!(e))
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
            let request = RuntimeRequest::Execute {
                code,
                parent,
                response: resp_tx,
            };
            if let Err(e) = cmd_tx.send(request).await {
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

            JupyterMessageContent::ExecuteRequest(req) => {
                let execution_count = self.one_up_execution_count();

                // Broadcast what is about to run, so other attached clients
                // see it and the cell's In[n] counter updates promptly.
                self.iopub
                    .send(
                        ExecuteInput {
                            code: req.code.clone(),
                            execution_count,
                        }
                        .as_child_of(parent),
                    )
                    .await?;

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

            JupyterMessageContent::CompleteRequest(req) => {
                let (tx, rx) = tokio::sync::oneshot::channel();
                self.runtime
                    .send(RuntimeRequest::Complete {
                        code: req.code.clone(),
                        cursor_pos: req.cursor_pos,
                        reply: tx,
                    })
                    .await?;
                let reply = rx.await.unwrap_or_else(|_| CompleteReply {
                    matches: vec![],
                    cursor_start: req.cursor_pos,
                    cursor_end: req.cursor_pos,
                    metadata: Default::default(),
                    status: ReplyStatus::Ok,
                    error: None,
                });
                shell.send(reply.as_child_of(parent)).await?;
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

            JupyterMessageContent::InspectRequest(req) => {
                let (tx, rx) = tokio::sync::oneshot::channel();
                self.runtime
                    .send(RuntimeRequest::Inspect {
                        code: req.code.clone(),
                        cursor_pos: req.cursor_pos,
                        reply: tx,
                    })
                    .await?;
                let reply = rx.await.unwrap_or_else(|_| InspectReply {
                    found: false,
                    data: Media::default(),
                    metadata: Default::default(),
                    status: ReplyStatus::Ok,
                    error: None,
                });
                shell.send(reply.as_child_of(parent)).await?;
            }

            JupyterMessageContent::IsCompleteRequest(req) => {
                // A bare identifier is complete input even without a `;` —
                // it will either become `.describe` or fail as SQL, neither
                // of which needs more lines. Grammar-only (no existence
                // check): this runs on every keystroke in the console.
                let status = if bare_identifier(&req.code).is_some()
                    || solite_core::sqlite::input_complete(&req.code)
                {
                    IsCompleteReplyStatus::Complete
                } else {
                    IsCompleteReplyStatus::Incomplete
                };
                let reply = IsCompleteReply {
                    status,
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
                mimetype: Some("text/x.sqlite".to_string()),
                file_extension: Some(".sql".to_string()),
                pygments_lexer: Some("sqlite".to_string()),
                codemirror_mode: Some(CodeMirrorMode::Simple("sql".to_string())),
                nbconvert_exporter: Some("script".to_string()),
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

/// Compute completions for a complete_request. Runs on the runtime task,
/// which owns the connection used for live schema lookups.
fn completions(runtime: &Runtime, code: &str, cursor_pos: usize) -> CompleteReply {
    use crate::commands::repl::completer::{
        find_completion_start, item_matches_prefix, LiveSchemaSource, DOT_COMMAND_NAMES,
    };
    use solite_completion::{detect_context, get_completions};

    // Jupyter cursor_pos is in unicode code points; the engine wants bytes.
    let byte_offset = code
        .char_indices()
        .nth(cursor_pos)
        .map(|(i, _)| i)
        .unwrap_or(code.len());
    let to_char_pos = |byte: usize| code[..byte].chars().count();

    // Dot command name completion, when the cursor's line is `.<partial>`.
    let line_start = code[..byte_offset].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let line = &code[line_start..byte_offset];
    if line.starts_with('.') && !line.contains(' ') {
        let prefix = &line[1..];
        let matches: Vec<String> = DOT_COMMAND_NAMES
            .iter()
            .filter(|name| name.starts_with(prefix))
            .map(|name| name.to_string())
            .collect();
        return CompleteReply {
            matches,
            cursor_start: to_char_pos(line_start + 1),
            cursor_end: cursor_pos,
            metadata: Default::default(),
            status: ReplyStatus::Ok,
            error: None,
        };
    }

    let context = detect_context(code, byte_offset);
    let start = find_completion_start(code, byte_offset);
    let prefix = &code[start..byte_offset];
    let prefix_opt = if prefix.is_empty() { None } else { Some(prefix) };

    let schema = LiveSchemaSource::new(runtime);
    // The engine leaves most contexts unfiltered; apply the same typed-prefix
    // filter the REPL uses.
    let matches: Vec<String> = get_completions(&context, Some(&schema), prefix_opt)
        .into_iter()
        .filter(|item| item_matches_prefix(item, prefix))
        .map(|item| item.insert_text.unwrap_or(item.label))
        .collect();

    CompleteReply {
        matches,
        cursor_start: to_char_pos(start),
        cursor_end: cursor_pos,
        metadata: Default::default(),
        status: ReplyStatus::Ok,
        error: None,
    }
}

/// Build an analyzer schema from the live database by parsing the DDL in
/// sqlite_master. Returns None when there is nothing usable.
fn schema_from_connection(runtime: &Runtime) -> Option<solite_analyzer::Schema> {
    let mut stmt = match runtime.connection.prepare(
        "SELECT sql FROM sqlite_master WHERE sql IS NOT NULL AND type IN ('table', 'view')",
    ) {
        Ok((_, Some(stmt))) => stmt,
        _ => return None,
    };

    let mut ddl = String::new();
    while let Ok(Some(row)) = stmt.next() {
        if let Some(sql) = row.first() {
            ddl.push_str(sql.as_str());
            ddl.push_str(";\n");
        }
    }

    let program = solite_parser::parse_program(&ddl).ok()?;
    Some(solite_analyzer::build_schema(&program))
}

/// Answer an inspect_request (Shift-Tab hover docs). Runs on the runtime
/// task, which owns the connection used to build the live schema.
fn inspect(runtime: &Runtime, code: &str, cursor_pos: usize) -> InspectReply {
    let not_found = || InspectReply {
        found: false,
        data: Media::default(),
        metadata: Default::default(),
        status: ReplyStatus::Ok,
        error: None,
    };

    // Jupyter cursor_pos is in unicode code points; spans/offsets are bytes.
    let byte_offset = code
        .char_indices()
        .nth(cursor_pos)
        .map(|(i, _)| i)
        .unwrap_or(code.len());

    // Cells containing dot commands won't parse as pure SQL; reply not-found.
    let Ok(program) = solite_parser::parse_program(code) else {
        return not_found();
    };

    let schema = schema_from_connection(runtime);
    let Some(stmt) = solite_analyzer::find_statement_at_offset(&program, byte_offset) else {
        return not_found();
    };
    let Some((symbol, _span)) =
        solite_analyzer::find_symbol_at_offset(stmt, code, byte_offset, schema.as_ref())
    else {
        return not_found();
    };

    let content = solite_analyzer::format_hover_content(&symbol, schema.as_ref());
    InspectReply {
        found: true,
        data: vec![
            MediaType::Markdown(content.clone()),
            MediaType::Plain(content),
        ]
        .into(),
        metadata: Default::default(),
        status: ReplyStatus::Ok,
        error: None,
    }
}

/// Render a statement's results and send them as DisplayData, or report a
/// RenderError. Returns whether rendering succeeded.
///
/// Takes the statement by value: `&Statement` is not `Send` (the type is
/// `Send` but not `Sync`), so holding a reference across the sends would
/// make the calling futures non-`Send`.
pub(super) async fn send_statement_result(
    mut stmt: solite_core::sqlite::Statement,
    response: &mpsc::Sender<ExecutionMessage>,
    parent: &JupyterMessage,
    timer: bool,
) -> Result<bool> {
    let started_at = std::time::Instant::now();
    match render_statement(&mut stmt) {
        Ok(tbl) => {
            let elapsed = started_at.elapsed();
            response
                .send_display(
                    DisplayData::new(
                        vec![MediaType::Plain(tbl.text), MediaType::Html(tbl.html)].into(),
                    ),
                    parent,
                )
                .await?;
            if timer {
                response
                    .send_plain(
                        format!(
                            "Took {}",
                            crate::commands::run::format_duration(elapsed)
                        ),
                        parent,
                    )
                    .await?;
            }
            Ok(true)
        }
        Err(err) => {
            response
                .send_error("RenderError", &format!("{:?}", err))
                .await?;
            Ok(false)
        }
    }
}

/// Step the runtime to completion, rendering each step's output. Used for
/// cell execution and (via the `.run` dot command) for whole-file runs.
pub(super) async fn handle_code(
    runtime: &mut Runtime,
    response: &mpsc::Sender<ExecutionMessage>,
    parent: &JupyterMessage,
    interrupt_handle: &Mutex<InterruptHandle>,
    timer: &mut bool,
) -> Result<()> {
    loop {
        // Re-fetch before every step: a previous step (e.g. `.open`) may have
        // swapped the connection, and interrupts must target the live one.
        *interrupt_handle.lock().unwrap() = runtime.connection.interrupt_handle();
        match runtime.next_stepx() {
            Some(Ok(step)) => match step.result {
                StepResult::SqlStatement { stmt, .. } => {
                    if !send_statement_result(stmt, response, parent, *timer).await? {
                        return Ok(());
                    }
                }
                StepResult::DotCommand(cmd) => {
                    handle_dot_command(cmd, runtime, response, parent, interrupt_handle, timer)
                        .await?;
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
                StepError::ParseDot { error, .. } => {
                    response
                        .send_error("DotCommandError", &error.to_string())
                        .await?;
                    return Ok(());
                }
            },
        }
    }
}

/// A cell that is exactly one SQL object name — `users`, `temp.users`,
/// `"my table"`, `` `x` ``, `[x]`, `main."a.b"` — with optional surrounding
/// whitespace and at most one trailing `;`. Returns `(schema, name)` with
/// quotes stripped. Anything else — trailing tokens, a second `.`, an
/// unterminated quote — returns `None`. Pure function, no runtime: the
/// caller decides whether the name actually exists.
pub(super) fn bare_identifier(cell: &str) -> Option<(Option<String>, String)> {
    let trimmed = cell.trim();
    let trimmed = trimmed.strip_suffix(';').unwrap_or(trimmed).trim();
    if trimmed.is_empty() || !is_single_qualified_name(trimmed) {
        return None;
    }
    let cmd = solite_core::dot::describe::DescribeCommand::parse_args(trimmed).ok()?;
    Some((cmd.schema, cmd.name))
}

/// Whether `s` is exactly `part` or `part.part` and nothing else — no
/// trailing tokens/comments, no second `.`. Each `part` is an unquoted
/// identifier (`[A-Za-z_][A-Za-z0-9_]*`) or a quoted identifier (`"…"`,
/// `` `…` ``, `[…]`).
fn is_single_qualified_name(s: &str) -> bool {
    let rest = match consume_name_part(s) {
        Some(rest) => rest,
        None => return false,
    };
    let rest = match rest.strip_prefix('.') {
        Some(after_dot) => match consume_name_part(after_dot) {
            Some(rest) => rest,
            None => return false,
        },
        None => rest,
    };
    rest.is_empty()
}

/// Consume one unquoted or quoted identifier from the start of `s` and
/// return what follows it, or `None` if `s` doesn't start with a valid part.
fn consume_name_part(s: &str) -> Option<&str> {
    let bytes = s.as_bytes();
    match bytes.first()? {
        b'A'..=b'Z' | b'a'..=b'z' | b'_' => {
            let end = s
                .char_indices()
                .find(|(_, c)| !(c.is_ascii_alphanumeric() || *c == '_'))
                .map(|(i, _)| i)
                .unwrap_or(s.len());
            Some(&s[end..])
        }
        b'"' => consume_quoted(s, b'"'),
        b'`' => consume_quoted(s, b'`'),
        b'[' => consume_quoted(s, b']'),
        _ => None,
    }
}

/// Consume a quoted span starting at `s[0]` (the opening quote) up to and
/// including `close`. When the opening byte and `close` match (`"…"`,
/// `` `…` ``), a doubled `close` is an escaped literal, not the terminator;
/// `[…]` has no escape. Returns the remainder after the closing quote, or
/// `None` if unterminated.
fn consume_quoted(s: &str, close: u8) -> Option<&str> {
    let bytes = s.as_bytes();
    let open = bytes[0];
    let mut i = 1;
    while i < bytes.len() {
        if bytes[i] == close {
            if open == close && bytes.get(i + 1) == Some(&close) {
                i += 2;
                continue;
            }
            return Some(&s[i + 1..]);
        }
        i += 1;
    }
    None
}

/// Re-quote `schema`/`name` with `"` (doubling embedded `"`) so they survive
/// `parse_dot`'s whitespace split as `.describe` arguments, regardless of
/// what quoting (if any) the original cell used.
fn quote_for_dot(schema: Option<&str>, name: &str) -> String {
    let quote = |s: &str| format!("\"{}\"", s.replace('"', "\"\""));
    match schema {
        Some(schema) => format!("{}.{}", quote(schema), quote(name)),
        None => quote(name),
    }
}

#[cfg(test)]
mod bare_identifier_tests {
    use super::bare_identifier;

    #[test]
    fn accepts_plain_and_whitespace_variants() {
        assert_eq!(
            bare_identifier("users"),
            Some((None, "users".to_string()))
        );
        assert_eq!(
            bare_identifier("  users  "),
            Some((None, "users".to_string()))
        );
        assert_eq!(
            bare_identifier("users;"),
            Some((None, "users".to_string()))
        );
    }

    #[test]
    fn accepts_schema_qualified() {
        assert_eq!(
            bare_identifier("temp.users"),
            Some((Some("temp".to_string()), "users".to_string()))
        );
        assert_eq!(
            bare_identifier(r#"main."a.b""#),
            Some((Some("main".to_string()), "a.b".to_string()))
        );
    }

    #[test]
    fn accepts_quoted_forms() {
        assert_eq!(
            bare_identifier(r#""my table""#),
            Some((None, "my table".to_string()))
        );
        assert_eq!(bare_identifier("`x`"), Some((None, "x".to_string())));
        assert_eq!(bare_identifier("[x]"), Some((None, "x".to_string())));
        assert_eq!(
            bare_identifier(r#""say ""hi""""#),
            Some((None, "say \"hi\"".to_string()))
        );
    }

    #[test]
    fn rejects_non_single_identifier_cells() {
        let rejects = [
            "",
            ";",
            "users where",
            "users;;",
            "users -- c",
            "-- users",
            "a.b.c",
            "users(",
            "select 1",
            ".tables",
            "\"unterminated",
        ];
        for cell in rejects {
            assert_eq!(bare_identifier(cell), None, "expected None for {cell:?}");
        }
    }

    #[test]
    fn accepts_bare_keyword_shape() {
        // Grammar-only: `select` alone has the shape of a single identifier.
        // Existence-checking (and thus SQL fallthrough) happens at the
        // rewrite site, not here.
        assert_eq!(
            bare_identifier("select"),
            Some((None, "select".to_string()))
        );
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
