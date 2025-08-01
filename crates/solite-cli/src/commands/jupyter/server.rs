use anyhow::{Context as _, Result};
use html_builder::*;
use jupyter_protocol::{
    datatable::{TableSchema, TableSchemaField},
    ClearOutput, CodeMirrorMode, CommInfoReply, ConnectionInfo, DisplayData, ErrorOutput,
    ExecuteReply, ExecutionCount, HelpLink, HistoryReply, InspectReply, IsCompleteReply,
    IsCompleteReplyStatus, JupyterMessage, JupyterMessageContent, KernelInfoReply, LanguageInfo,
    Media, MediaType, ReplyStatus, Status, StreamContent, TabularDataResource,
};
use runtimelib::{KernelIoPubConnection, KernelShellConnection};
use serde_json::json;
use solite_core::{
    dot::{DotCommand, LoadCommandSource},
    sqlite::{self, ColumnMeta, Statement, ValueRefX, ValueRefXValue},
    Runtime, StepError, StepResult,
};
use std::fmt::Write;
use std::path::PathBuf;
use tokio::sync::mpsc;
use uuid::Uuid;

pub struct UiResponse {
    text: String,
    html: Option<String>,
}

fn html_tr_from_row<'a>(tbody: &'a mut Node, row: &[ValueRefX]) -> anyhow::Result<Node<'a>> {
    let mut tr = tbody.tr();
    for value in row {
        //tr.
        let raw: String = match value.value {
            ValueRefXValue::Null => "".to_owned(),
            ValueRefXValue::Int(value) => value.to_string(),
            ValueRefXValue::Double(value) => value.to_string(),
            ValueRefXValue::Text(value) => unsafe { String::from_utf8_unchecked(value.to_vec()) },
            ValueRefXValue::Blob(value) => format!("Blob<{}>", value.len()),
        };
        let style: String = match value.value {
            ValueRefXValue::Double(_) | ValueRefXValue::Int(_) | ValueRefXValue::Null => {
                "".to_owned()
            }
            ValueRefXValue::Text(_) => match value.subtype() {
                Some(sqlite::JSON_SUBTYPE) => "color: red".to_owned(),
                Some(_) | None => "text-align: left".to_owned(),
            },
            ValueRefXValue::Blob(_) => match value.subtype() {
                Some(223) | Some(224) | Some(225) => "color: blue".to_owned(),
                Some(_) | None => "".to_owned(),
            },
        };
        let mut td = tr.td().attr(format!("style=\"{}\"", style).as_str());
        writeln!(td, "{}", raw)?;
    }
    Ok(tr)
}

fn html_thead_from_stmt(thead: &mut Node, columns: &Vec<ColumnMeta>) -> anyhow::Result<()> {
    let mut tr = thead.tr().attr("style=\"text-align: center;\"");
    for column in columns {
        let title = format!(
            "{} {}",
            // column type
            match column.decltype {
                Some(ref t) => format!("{t} • "),
                None => "".to_string(),
            },
            // "db.table.column"
            format!(
                "{}{}{}",
                match &column.origin_database {
                    None => "".to_string(),
                    Some(db) =>
                        if db == "main" {
                            "".to_string()
                        } else {
                            format!("{db}.")
                        },
                },
                match &column.origin_table {
                    None => "".to_string(),
                    Some(table) => format!("{table}."),
                },
                column.origin_column.as_ref().map_or("", |v| v)
            )
        )
        .replace("\"", "&quot;");
        let mut th = tr.th().attr(format!("title=\"{}\"", title).as_str());
        writeln!(th, "{}", column.name)?;
    }

    Ok(())
}

pub(crate) fn render_statementx(stmt: &Statement) -> anyhow::Result<UiResponse> {
    let mut txt_rows = vec![];

    let mut buf = Buffer::new();
    let mut htmlx = buf.html();

    let mut root = htmlx.div();
    writeln!(root.style(), "td {{text-align: right;}}")?;
    let mut table = root.table();

    let columns = stmt.column_meta();
    html_thead_from_stmt(&mut table.thead(), &columns)?;

    let mut row_count = 0;
    let column_count = columns.len();

    let mut tbody = table.tbody();
    loop {
        match stmt.next() {
            Ok(result) => match result {
                Some(row) => {
                    row_count += 1;
                    if row_count <= 20 {
                      txt_rows.push(crate::ui::ui_row(&row, false));
                        html_tr_from_row(&mut tbody, &row)?;
                    }
                }
                None => break,
            },
            Err(error) => return Err(anyhow::anyhow!(error)),
        }
    }

    // TODO: warning for text version as well
    if row_count > 20 {
        writeln!(
            tbody
                .tr()
                .attr("style=\"background: red\"")
                .td()
                .attr(format!("colspan=\"{column_count}\"").as_str()),
            "WARNING"
        )?;
    }

    writeln!(
        root.div(),
        "{} column{} \u{00d7} {} row{}",
        column_count,
        if column_count < 2 { "" } else { "s" },
        row_count,
        if row_count < 2 { "" } else { "s" },
    )?;

    Ok(UiResponse {
        text: crate::ui::ui_table(columns.iter().map(|c| c.name.clone()).collect(), txt_rows)
            .display()?
            .to_string(),
        html: Some(buf.finish()),
    })
}

async fn handle_code(
    runtime: &mut Runtime,
    response: mpsc::Sender<JupyterMessage>,
    parent: &JupyterMessage,
) -> anyhow::Result<()> {
    loop {
        match runtime.next_stepx() {
            Some(Ok(step)) => match step.result {
                StepResult::SqlStatement { stmt, .. } => match render_statementx(&stmt) {
                    Ok(tbl) => {
                        response
                            .send(
                                DisplayData::new(
                                    vec![
                                        MediaType::Plain(stmt.sql()),
                                        MediaType::Html(tbl.html.unwrap()),
                                    ]
                                    .into(),
                                )
                                .as_child_of(parent),
                            )
                            .await
                            .unwrap();
                    }
                    Err(err) => {
                        response
                            .send(
                                DisplayData::from(MediaType::Plain(format!("{:?}", err)))
                                    .as_child_of(parent),
                            )
                            .await
                            .unwrap();
                    }
                },
                StepResult::DotCommand(cmd) => match cmd {
                    DotCommand::Print(print_cmd) => {
                        response
                            .send(
                                DisplayData::from(MediaType::Plain(print_cmd.message))
                                    .as_child_of(parent),
                            )
                            .await
                            .unwrap();
                    }
                    DotCommand::Shell(sh_cmd) => {
                        let rx = sh_cmd.execute();
                        let mut out = String::new();
                        while let Ok(msg) = rx.recv() {
                            response
                                .send(
                                    StreamContent::stdout(&format!("{msg}\n")).as_child_of(parent),
                                )
                                .await
                                .unwrap();
                            out.push_str(msg.as_str());
                            out.push('\n');
                        }
                    }
                    DotCommand::Timer(_enabled) => todo!(),
                    DotCommand::Parameter(cmd) => {
                        let msg = match cmd {
                            solite_core::dot::ParameterCommand::Set { key, value } => {
                                runtime.define_parameter(key.clone(), value).unwrap();
                                DisplayData::from(MediaType::Plain(format!(
                                    "set parameter : {}",
                                    key,
                                )))
                            }
                            solite_core::dot::ParameterCommand::Unset(_) => todo!(),
                            solite_core::dot::ParameterCommand::List => todo!(),
                            solite_core::dot::ParameterCommand::Clear => todo!(),
                        };

                        response.send(msg.as_child_of(parent)).await.unwrap()
                    }
                    DotCommand::Open(open_cmd) => {
                        open_cmd.execute(runtime);
                        response
                            .send(
                                DisplayData::from(MediaType::Plain(format!(
                                    "Opened database at {}",
                                    open_cmd.path
                                )))
                                .as_child_of(parent),
                            )
                            .await
                            .unwrap()
                    }
                    DotCommand::Load(load_cmd) => {
                        let msg = match load_cmd.execute(&mut runtime.connection) {
                            Ok(LoadCommandSource::Path(v)) => {
                                MediaType::Plain(format!("Loaded '{v}'"))
                            }
                            Ok(LoadCommandSource::Uv { directory, package }) => MediaType::Plain(
                                format!("Loaded '{package}' with uv from {directory}"),
                            ),
                            Err(_) => todo!(),
                        };
                        response
                            .send(DisplayData::from(msg).as_child_of(parent))
                            .await
                            .unwrap();
                    }
                    DotCommand::Tables(cmd) => {
                        let tables = cmd.execute(&runtime);
                        response.send(
                            DisplayData::from(MediaType::Plain(format!(
                                "{}",
                                tables.join("\n")
                            )))
                            .as_child_of(parent),
                        ).await.unwrap();
                    }
                    DotCommand::Vegalite(mut vegalite_command) => {
                        match vegalite_command.execute() {
                            Ok(data) => {
                                response
                                    .send(ClearOutput { wait: true }.as_child_of(parent))
                                    .await
                                    .unwrap();
                                response
                                    .send(
                                        DisplayData::from(MediaType::VegaLiteV4(data))
                                            .as_child_of(parent),
                                    )
                                    .await
                                    .unwrap();
                                response
                                    .send(ClearOutput { wait: true }.as_child_of(parent))
                                    .await
                                    .unwrap();
                            }
                            Err(_) => todo!(),
                        }
                    }
                    DotCommand::Export(mut export_command) => match export_command.execute() {
                        Ok(()) => {
                            response
                                .send(
                                    DisplayData::from(MediaType::Plain(format!(
                                        "export successfully to {}",
                                        export_command.target.to_string_lossy()
                                    )))
                                    .as_child_of(parent),
                                )
                                .await
                                .unwrap();
                        }
                        Err(_) => todo!(),
                    },
                    DotCommand::Bench(mut cmd) => {
                      
                      match cmd.execute(None) {
                        Ok(result) => {
                            response
                                .send(
                                    DisplayData::from(MediaType::Plain(format!(
                                        "{}",
                                        result.report()
                                    )))
                                    .as_child_of(parent),
                                )
                                .await
                                .unwrap();
                        }
                        Err(_) => response
                                .send(
                                    DisplayData::from(MediaType::Plain(format!(
                                        "Benchmark fail",
                                    )))
                                    .as_child_of(parent),
                                )
                                .await
                                .unwrap()
                    }
                  },
                },
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
                    response
                        .send(DisplayData::from(MediaType::Plain(error_string)).as_child_of(parent))
                        .await
                        .unwrap();
                }
                StepError::ParseDot(error) => {
                    response
                        .send(
                            DisplayData::from(MediaType::Plain(error.to_string()))
                                .as_child_of(parent),
                        )
                        .await
                        .unwrap();
                }
            },
        }
    }
}

struct SoliteKernel {
    execution_count: ExecutionCount,
    iopub: KernelIoPubConnection,
    runtime: mpsc::Sender<(String, JupyterMessage, mpsc::Sender<JupyterMessage>)>,
    //runtime: mpsc::Sender<(String, mpsc::Sender<Option<Result<String, String>>>)>,
}

impl SoliteKernel {
    pub async fn start(connection_info: &ConnectionInfo) -> Result<()> {
        let runtime = Runtime::new(None);
        let session_id = Uuid::new_v4().to_string();

        let mut heartbeat = runtimelib::create_kernel_heartbeat_connection(connection_info).await?;
        let shell_connection =
            runtimelib::create_kernel_shell_connection(connection_info, &session_id).await?;
        let mut control_connection =
            runtimelib::create_kernel_control_connection(connection_info, &session_id).await?;
        let _stdin_connection =
            runtimelib::create_kernel_stdin_connection(connection_info, &session_id).await?;
        let iopub_connection =
            runtimelib::create_kernel_iopub_connection(connection_info, &session_id).await?;

        let (tx, mut rx) =
            //mpsc::channel::<(String, mpsc::Sender<Option<Result<String, String>>>)>(10);
            mpsc::channel::<(String, JupyterMessage, mpsc::Sender<JupyterMessage>)>(10);

        tokio::spawn(async move {
            let mut rt = runtime;
            while let Some((cmd, parent, response)) = rx.recv().await {
                match cmd {
                    code => {
                        rt.enqueue(
                            "<anonymous>",
                            code.as_str(),
                            solite_core::BlockSource::JupyerCell,
                        );
                        handle_code(&mut rt, response, &parent).await.unwrap();
                    }
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

                        match sent {
                            Ok(_) => {}
                            Err(err) => eprintln!("Error on control {}", err),
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

    async fn clear_output_after_next_output(
        &mut self,
        parent: &JupyterMessage,
    ) -> anyhow::Result<()> {
        self.iopub
            .send(ClearOutput { wait: true }.as_child_of(parent))
            .await
    }

    async fn send_markdown(
        &mut self,
        markdown: &str,
        parent: &JupyterMessage,
    ) -> anyhow::Result<()> {
        self.iopub
            .send(DisplayData::from(MediaType::Markdown(markdown.to_string())).as_child_of(parent))
            .await
    }
    async fn send_plaintext(
        &mut self,
        message: &str,
        parent: &JupyterMessage,
    ) -> anyhow::Result<()> {
        self.iopub
            .send(DisplayData::from(MediaType::Plain(message.to_string())).as_child_of(parent))
            .await
    }
    
    async fn send_error(
        &mut self,
        ename: &str,
        evalue: &str,
        parent: &JupyterMessage,
    ) -> anyhow::Result<()> {
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

    async fn push_stdout(&mut self, text: &str, parent: &JupyterMessage) -> anyhow::Result<()> {
        self.iopub
            .send(StreamContent::stdout(text).as_child_of(parent))
            .await
    }

    async fn command(&mut self, command: &str, parent: &JupyterMessage) -> anyhow::Result<()> {
        println!("command: {command}");
        anyhow::Ok(())
    }

    async fn execute(&mut self, request: &JupyterMessage) -> anyhow::Result<()> {
        let code = match &request.content {
            JupyterMessageContent::ExecuteRequest(req) => req.code.clone(),
            _ => return Err(anyhow::anyhow!("Invalid message type for execution")),
        };

        let cmd_tx = self.runtime.clone();
        let parent = request.clone();
        let handle = tokio::spawn(async move {
            let (resp_tx, resp_rx) = mpsc::channel(10);
            cmd_tx.send((code, parent, resp_tx)).await.unwrap();
            resp_rx
        });
        let mut x = handle.await.unwrap();
        while let Some(x) = x.recv().await {
            self.iopub.send(x).await?;
        }
        // Clear the progress message after the first tokens come in
        //self.clear_output_after_next_output(request).await?;

        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        anyhow::Ok(())
    }

    pub async fn handle_shell(&mut self, mut connection: KernelShellConnection) -> Result<()> {
        loop {
            let msg = connection.read().await?;
            match self.handle_shell_message(&msg, &mut connection).await {
                Ok(_) => {}
                Err(err) => eprintln!("Error on shell: {}", err),
            }
        }
    }

    pub async fn handle_shell_message(
        &mut self,
        parent: &JupyterMessage,
        shell: &mut KernelShellConnection,
    ) -> Result<()> {
        // Even with messages like `kernel_info_request`, you're required to send a busy and idle message
        self.iopub.send(Status::busy().as_child_of(parent)).await?;

        match &parent.content {
            JupyterMessageContent::CommInfoRequest(_) => {
                // Just tell the frontend we don't have any comms
                let reply = CommInfoReply {
                    status: ReplyStatus::Ok,
                    comms: Default::default(),
                    error: None,
                }
                .as_child_of(parent);
                shell.send(reply).await?;
            }

            JupyterMessageContent::ExecuteRequest(_) => {
                // Respond back with reply immediately
                let reply = ExecuteReply {
                    status: ReplyStatus::Ok,
                    execution_count: self.one_up_execution_count(),
                    user_expressions: Default::default(),
                    payload: Default::default(),
                    error: None,
                }
                .as_child_of(parent);
                shell.send(reply).await?;

                if let Err(err) = self.execute(parent).await {
                    self.send_error("OllamaFailure", &err.to_string(), parent)
                        .await?;
                }
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
                // Would be really cool to have the model inspect at the word,
                // kind of like an editor.

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
                // true, unconditionally
                let reply = IsCompleteReply {
                    status: IsCompleteReplyStatus::Complete,
                    indent: "".to_string(),
                }
                .as_child_of(parent);

                shell.send(reply).await?;
            }
            JupyterMessageContent::KernelInfoRequest(_) => {
                let reply = Self::kernel_info().as_child_of(parent);

                shell.send(reply).await?;
            }
            // Not implemented for shell includes DebugRequest
            // Not implemented for control (and sometimes shell...) includes InterruptRequest, ShutdownRequest
            _ => {}
        };

        self.iopub.send(Status::idle().as_child_of(parent)).await?;

        Ok(())
    }

    fn kernel_info() -> KernelInfoReply {
        KernelInfoReply {
            status: ReplyStatus::Ok,
            protocol_version: "5.3".to_string(),
            implementation: "Solite kernel".to_string(),
            implementation_version: "TODO".to_string(),
            language_info: LanguageInfo {
                name: "sqlite".to_string(),
                version: "TODO".to_string(),
                mimetype: "text/x.sqlite".to_string(),
                file_extension: ".sql".to_string(),
                pygments_lexer: "sqlite".to_string(),
                codemirror_mode: CodeMirrorMode::Simple("sql".to_string()),
                nbconvert_exporter: "script".to_string(),
            },
            banner: "Solite Kernel".to_string(),
            help_links: vec![HelpLink {
                text: "TODO".to_string(),
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

pub async fn start_kernel(connection_filepath: PathBuf) -> anyhow::Result<()> {
    let conn_file = std::fs::read_to_string(&connection_filepath)
        .with_context(|| format!("Couldn't read connection file: {:?}", connection_filepath))?;
    let spec: ConnectionInfo = serde_json::from_str(&conn_file).with_context(|| {
        format!(
            "Connection file is not a valid JSON: {:?}",
            connection_filepath
        )
    })?;

    SoliteKernel::start(&spec).await?;
    anyhow::Ok(())
}
