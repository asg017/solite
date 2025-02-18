// Modified from deno project, original declaration:

// Copyright 2018-2023 the Deno authors. All rights reserved. MIT license.

// This file is forked/ported from <https://github.com/evcxr/evcxr>
// Copyright 2020 The Evcxr Authors. MIT license.

use std::collections::HashMap;
use std::sync::Arc;

use crate::jupyter::html::html_escape;
use crate::jupyter::jupyer_msg::{Connection, JupyterMessage};
use anyhow::Error as AnyError;
use futures::channel::mpsc;
use futures::StreamExt;
use serde::Deserialize;
use serde_json::json;
use solite_core::dot::DotCommand;
use solite_core::sqlite::{self, Statement, ValueRefXValue};
use solite_core::{Runtime, StepError, StepResult};
use tokio::sync::oneshot;
use tokio::sync::Mutex;
use zeromq::SocketRecv;
use zeromq::SocketSend;

#[derive(Debug, Deserialize)]
pub struct ConnectionSpec {
    pub(crate) ip: String,
    pub(crate) transport: String,
    pub(crate) control_port: u32,
    pub(crate) shell_port: u32,
    pub(crate) stdin_port: u32,
    pub(crate) hb_port: u32,
    pub(crate) iopub_port: u32,
    pub(crate) key: String,
}

pub enum StdioMsg {
    Stdout(String),
    #[allow(dead_code)]
    Stderr(String),
}

enum Command {
    Execute(String),
}

#[derive(Debug, Deserialize)]
struct UiResponse {
    text: String,
    html: Option<String>,
}

#[derive(Debug, Deserialize)]
struct UiError {
    ename: String,
    evalue: String,
}
impl UiError {
    fn new<S: Into<String>>(ename: S, evalue: S) -> Self {
        UiError {
            ename: ename.into(),
            evalue: evalue.into(),
        }
    }
}
pub struct JupyterServer {
    execution_count: usize,
    last_execution_request: Arc<Mutex<Option<JupyterMessage>>>,
    // This is Arc<Mutex<>>, so we don't hold RefCell borrows across await
    // points.
    iopub_socket: Arc<Mutex<Connection<zeromq::PubSocket>>>,
    solite_runtime:
        tokio::sync::mpsc::Sender<(Command, oneshot::Sender<Result<UiResponse, UiError>>)>,
}

fn render_statement(stmt: &Statement) -> Result<UiResponse, UiError> {
    //let mut text = String::new();
    let mut rows = vec![];
    let mut html = String::new();

    html.push_str("<div>\n");
    html.push_str("<style>td {text-align: right;}</style>");
    html.push_str("<table>\n");
    html.push_str("<thead>\n");
    html.push_str("<tr style=\"text-align: center;\">\n");
    let column_names = stmt
        .column_names()
        .map_err(|_e| UiError::new("error", "Error getting column names"))?;
    let column_count = column_names.len();
    let mut row_count = 0;
    for column in &column_names {
        html.push_str("<th>\n");
        let cleaned =
            html_escape(column).map_err(|_e| UiError::new("Some formatting error", ""))?;
        html.push_str(cleaned.as_str());
        html.push_str("\n</th>\n");
    }
    html.push_str("</tr>\n");
    html.push_str("</thead>\n");

    html.push_str("<tbody>\n");
    loop {
        match stmt.next() {
            Ok(result) => match result {
                Some(row) => {
                    row_count += 1;

                    rows.push(crate::ui::ui_row(&row, false));

                    html.push_str("<tr>\n");
                    for value in row {
                        let raw: String = match value.value {
                            ValueRefXValue::Null => "".to_owned(),
                            ValueRefXValue::Int(value) => value.to_string(),
                            ValueRefXValue::Double(value) => value.to_string(),
                            ValueRefXValue::Text(value) => unsafe {
                                String::from_utf8_unchecked(value.to_vec())
                            },
                            ValueRefXValue::Blob(value) => format!("Blob<{}>", value.len()),
                        };
                        let style: String = match value.value {
                            ValueRefXValue::Double(_)
                            | ValueRefXValue::Int(_)
                            | ValueRefXValue::Null => "".to_owned(),
                            ValueRefXValue::Text(_) => match value.subtype() {
                                Some(sqlite::JSON_SUBTYPE) => "style=\"color: red\"".to_owned(),
                                Some(_) | None => "".to_owned(),
                            },
                            ValueRefXValue::Blob(_) => match value.subtype() {
                                Some(223) | Some(224) | Some(225) => {
                                    "style=\"color: blue\"".to_owned()
                                }
                                Some(_) | None => "".to_owned(),
                            },
                        };
                        //let raw = value.as_str().to_string();
                        let value = html_escape(&raw)
                            .map_err(|_e| UiError::new("Some formatting error", ""))?;

                        html.push_str(format!("<td {}>\n", style).as_str());
                        html.push_str(value.as_str());
                        html.push_str("\n</td>\n");
                    }
                    html.push_str("</tr>\n");
                }
                None => break,
            },
            Err(error) => return Err(UiError::new(error.code_description, error.message)),
        }
    }
    html.push_str("</tbody>\n");
    html.push_str("</table>\n");

    html.push_str("<div>\n");
    html.push_str(
        format!(
            "{} row{} \u{00d7} {} column{}",
            row_count,
            if row_count < 2 { "" } else { "s" },
            column_count,
            if column_count < 2 { "" } else { "s" }
        )
        .as_str(),
    );
    html.push_str("\n</div>\n");
    html.push_str("</div>\n");

    Ok(UiResponse {
        text: crate::ui::ui_table(column_names, rows)
            .display()
            .map_err(|_e| UiError::new("Error displaying table", ""))?
            .to_string(),
        html: Some(html),
    })
}
fn handle_code(runtime: &mut Runtime, code: String) -> Result<UiResponse, UiError> {
    runtime.enqueue("TODO", code.as_str(), solite_core::BlockSource::JupyerCell);
    loop {
        match runtime.next_step() {
            Ok(Some(step)) => match step.result {
                StepResult::SqlStatement(stmt) => {
                    if !runtime.has_next() {
                        return render_statement(&stmt);
                    } else {
                        stmt.execute()
                            .map_err(|e| UiError::new(e.code_description.clone(), e.to_string()))?;
                    }
                }
                StepResult::DotCommand(cmd) => match cmd {
                    DotCommand::Print(_print_cmd) => {
                        return Err(UiError::new(
                            "Unsupported",
                            "The .print command is not supported in Jupyter.",
                        ))
                    }
                    DotCommand::Timer(_enabled) => {
                        return Err(UiError::new(
                            "Unsupported",
                            "The .timer command is not supported in Jupyter.",
                        ))
                    }
                    DotCommand::Parameter(param_cmd) => match param_cmd {
                        solite_core::dot::ParameterCommand::Set { key, value } => {
                            runtime.define_parameter(key, value).unwrap();
                        }
                        solite_core::dot::ParameterCommand::Unset(_) => todo!(),
                        solite_core::dot::ParameterCommand::List => todo!(),
                        solite_core::dot::ParameterCommand::Clear => todo!(),
                    },
                    DotCommand::Open(open_cmd) => {
                        open_cmd.execute(runtime);
                    }
                    DotCommand::Load(load_cmd) => {
                        load_cmd.execute(&mut runtime.connection);
                    }
                    DotCommand::Tables(cmd) => {
                        cmd.execute(&runtime);
                    }
                },
            },
            Ok(None) => {
                return Ok(UiResponse {
                    text: "[no code]".to_string(),
                    html: None,
                })
            }
            Err(error) => match error {
                StepError::Prepare {
                    error,
                    file_name,
                    src,
                    offset,
                } => {
                    let x =
                        crate::errors::report_error_string(file_name.as_str(), &src, &error, None);
                    return Err(UiError::new(error.code_description.clone(), x));
                }
                StepError::ParseDot(error) => {
                    return Err(UiError::new(
                        "Error parsing dot command",
                        &error.to_string(),
                    ))
                }
            },
        }
    }
}

impl JupyterServer {
    pub async fn start(
        spec: ConnectionSpec,
        mut stdio_rx: mpsc::UnboundedReceiver<StdioMsg>,
        runtime: Runtime,
    ) -> Result<StdioMsg, AnyError> {
        let mut heartbeat = bind_socket::<zeromq::RepSocket>(&spec, spec.hb_port).await?;
        let shell_socket = bind_socket::<zeromq::RouterSocket>(&spec, spec.shell_port).await?;
        let control_socket = bind_socket::<zeromq::RouterSocket>(&spec, spec.control_port).await?;
        let _stdin_socket = bind_socket::<zeromq::RouterSocket>(&spec, spec.stdin_port).await?;
        let iopub_socket = bind_socket::<zeromq::PubSocket>(&spec, spec.iopub_port).await?;
        let iopub_socket = Arc::new(Mutex::new(iopub_socket));
        let last_execution_request = Arc::new(Mutex::new(None));

        let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::channel::<(
            Command,
            oneshot::Sender<Result<UiResponse, UiError>>,
        )>(1);
        tokio::spawn(async move {
            let mut rt = runtime;
            while let Some((cmd, response)) = cmd_rx.recv().await {
                match cmd {
                    Command::Execute(code) => response.send(handle_code(&mut rt, code)).unwrap(),
                }
            }
        });

        let mut server = Self {
            execution_count: 0,
            iopub_socket: iopub_socket.clone(),
            last_execution_request: last_execution_request.clone(),
            solite_runtime: cmd_tx.clone(),
        };

        let handle1 = tokio::task::spawn(async move {
            if let Err(_err) = Self::handle_heartbeat(&mut heartbeat).await {}
        });

        let handle2 = tokio::task::spawn(async move {
            if let Err(_err) = Self::handle_control(control_socket).await {}
        });

        let handle3 = tokio::task::spawn(async move {
            if let Err(_err) = server.handle_shell(shell_socket).await {}
        });

        let handle4 = tokio::task::spawn(async move {
            while let Some(stdio_msg) = stdio_rx.next().await {
                Self::handle_stdio_msg(
                    iopub_socket.clone(),
                    last_execution_request.clone(),
                    stdio_msg,
                )
                .await;
            }
        });

        let join_fut = futures::future::try_join_all(vec![handle1, handle2, handle3, handle4]);

        if let Ok(_result) = join_fut.await {}

        Ok(StdioMsg::Stdout("yo".to_owned()))
    }

    async fn handle_stdio_msg<S: zeromq::SocketSend>(
        iopub_socket: Arc<Mutex<Connection<S>>>,
        last_execution_request: Arc<Mutex<Option<JupyterMessage>>>,
        stdio_msg: StdioMsg,
    ) {
        let exec_request = last_execution_request.clone();

        let (name, text) = match stdio_msg {
            StdioMsg::Stdout(text) => ("stdout", text),
            StdioMsg::Stderr(text) => ("stderr", text),
        };

        let mut x = exec_request.try_lock().unwrap();
        let result = x
            .as_mut()
            .unwrap()
            .new_message("stream")
            .with_content(json!({
                "name": name,
                "text": text
            }))
            .send(&mut *iopub_socket.lock().await)
            .await;

        if let Err(_err) = result {}
    }

    async fn handle_heartbeat(
        connection: &mut Connection<zeromq::RepSocket>,
    ) -> Result<(), AnyError> {
        loop {
            connection.socket.recv().await?;
            connection
                .socket
                .send(zeromq::ZmqMessage::from(b"ping".to_vec()))
                .await?;
        }
    }

    async fn handle_control(
        mut connection: Connection<zeromq::RouterSocket>,
    ) -> Result<(), AnyError> {
        loop {
            let msg = JupyterMessage::read(&mut connection).await?;

            match msg.message_type() {
                "kernel_info_request" => {
                    msg.new_reply()
                        .with_content(kernel_info())
                        .send(&mut connection)
                        .await?;
                }
                "shutdown_request" => {
                    //cancel_handle.cancel();
                }
                "interrupt_request" => {
                    eprintln!("Interrupt request currently not supported");
                }
                _ => {
                    eprintln!("Unrecognized control message type: {}", msg.message_type());
                }
            }
        }
    }

    async fn handle_shell(
        &mut self,
        mut connection: Connection<zeromq::RouterSocket>,
    ) -> Result<(), AnyError> {
        loop {
            let msg = JupyterMessage::read(&mut connection).await?;
            self.handle_shell_message(msg, &mut connection).await?;
        }
    }

    async fn handle_shell_message(
        &mut self,
        msg: JupyterMessage,
        connection: &mut Connection<zeromq::RouterSocket>,
    ) -> Result<(), AnyError> {
        msg.new_message("status")
            .with_content(json!({"execution_state": "busy"}))
            .send(&mut *self.iopub_socket.lock().await)
            .await?;

        match msg.message_type() {
            "kernel_info_request" => {
                msg.new_reply()
                    .with_content(kernel_info())
                    .send(connection)
                    .await?;
            }
            "is_complete_request" => {
                // TODO: also 'invalid' or 'unknown'
                let status = if solite_core::sqlite::complete(msg.code()) {
                    "complete"
                } else {
                    "incomplete"
                };
                msg.new_reply()
                    .with_content(json!({"status": status}))
                    .send(connection)
                    .await?;
            }
            "execute_request" => {
                self.handle_execution_request(msg.clone(), connection)
                    .await?;
            }
            "comm_open" => {
                msg.comm_close_message()
                    .send(&mut *self.iopub_socket.lock().await)
                    .await?;
            }
            "complete_request" => {
                //let user_code = msg.code();
                //let cursor_pos = msg.cursor_pos();
            }
            "comm_msg" | "comm_info_request" | "history_request" => {
                // We don't handle these messages
            }
            _ => {}
        }

        msg.new_message("status")
            .with_content(json!({"execution_state": "idle"}))
            .send(&mut *self.iopub_socket.lock().await)
            .await?;

        Ok(())
    }

    async fn handle_execution_request(
        &mut self,
        msg: JupyterMessage,
        connection: &mut Connection<zeromq::RouterSocket>,
    ) -> Result<(), AnyError> {
        self.execution_count += 1;
        //self.last_execution_request = Some(msg.clone());
        let mut guard = self.last_execution_request.try_lock().unwrap();
        *guard = Some(msg.clone());

        msg.new_message("execute_input")
            .with_content(json!({
                "execution_count": self.execution_count,
                "code": msg.code()
            }))
            .send(&mut *self.iopub_socket.lock().await)
            .await?;

        if msg.code().starts_with("! ") {
            msg.new_message("execute_result")
                .with_content(json!({
                    "execution_count": self.execution_count,
                    "data": {"text/plain": "aaaaa"},
                    "metadata": {},
                }))
                .send(&mut *self.iopub_socket.lock().await)
                .await?;
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            msg.new_message("execute_result")
                .with_content(json!({
                    "execution_count": self.execution_count,
                    "data": {"text/plain": "bbbbb"},
                    "metadata": {},
                }))
                .send(&mut *self.iopub_socket.lock().await)
                .await?;
            msg.new_reply()
                .with_content(json!({
                    "status": "ok",
                    "execution_count": self.execution_count,
                }))
                .send(connection)
                .await?;
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        } else {
            let mut data: HashMap<String, String> = HashMap::new();
            let cmd_tx = self.solite_runtime.clone();
            let code = msg.code().to_string();
            let handle = tokio::spawn(async move {
                let (resp_tx, resp_rx) = oneshot::channel();
                cmd_tx
                    .send((Command::Execute(code), resp_tx))
                    .await
                    .ok()
                    .unwrap();
                resp_rx.await.unwrap()
            });

            match handle.await {
                Ok(result) => match result {
                    Ok(response) => {
                        data.insert("text/plain".to_string(), response.text);
                        if let Some(html) = response.html {
                            data.insert("text/html".to_string(), html);
                        }
                        msg.new_message("execute_result")
                            .with_content(json!({
                                "execution_count": self.execution_count,
                                "data": data,
                                "metadata": {},
                            }))
                            .send(&mut *self.iopub_socket.lock().await)
                            .await?;
                    }
                    Err(error) => {
                        msg.new_message("error")
                            .with_content(json!({
                              "ename": error.ename,
                              "evalue": error.evalue,
                              "traceback": [

                              ]
                            }))
                            .send(&mut *self.iopub_socket.lock().await)
                            .await?;
                    }
                },
                Err(err) => {
                    msg.new_message("error")
                        .with_content(json!({
                          "ename": "some sort of error",
                          "evalue": err.to_string(),
                          "traceback": []
                        }))
                        .send(&mut *self.iopub_socket.lock().await)
                        .await?;
                }
            }

            /*match todo!() {
                Ok(stmt) => {
                    let value = stmt.unwrap().next().unwrap().unwrap();
                    data.insert("text/plain".to_string(), value.get(0).unwrap().x_as_text());
                    msg.new_message("execute_result")
                        .with_content(json!({
                            "execution_count": self.execution_count,
                            "data": data,
                            "metadata": {},
                        }))
                        .send(&mut *self.iopub_socket.lock().await)
                        .await?;
                }
                Err(_) => {
                    msg.new_message("error")
                        .with_content(json!({
                          "ename": "prepare eror TODO",
                          "evalue": " ",
                          "traceback": []
                        }))
                        .send(&mut *self.iopub_socket.lock().await)
                        .await?;
                }
            }*/

            msg.new_reply()
                .with_content(json!({
                    "status": "ok",
                    "execution_count": self.execution_count,
                }))
                .send(connection)
                .await?;
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }

        Ok(())
    }
}

async fn bind_socket<S: zeromq::Socket>(
    config: &ConnectionSpec,
    port: u32,
) -> Result<Connection<S>, AnyError> {
    let endpoint = format!("{}://{}:{}", config.transport, config.ip, port);
    let mut socket = S::new();
    socket.bind(&endpoint).await?;
    Ok(Connection::new(socket, &config.key))
}

fn kernel_info() -> serde_json::Value {
    json!({
      "status": "ok",
      "protocol_version": "5.3",
      "implementation_version": "TODO",
      "implementation": "Solite kernel",
      "language_info": {
        "name": "sqlite",
        "version": "TODO",
        "mimetype": "text/x.sqlite",
        "file_extension": ".sql",
        "pygments_lexer": "sql",
        "nb_converter": "script"
      },
      "help_links": [{
        "text": "TODO",
        "url": "https://github.com/asg017/solite"
      }],
      "banner": "Welcome to the Solite kernel!",
    })
}
