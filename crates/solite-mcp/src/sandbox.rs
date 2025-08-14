#![allow(dead_code)]
use std::{collections::HashMap, sync::Arc};

use base64::Engine;
use rmcp::{
    ErrorData as McpError, RoleServer, ServerHandler,
    handler::server::{router::tool::ToolRouter, tool::Parameters},
    model::*,
    schemars,
    service::RequestContext,
    tool, tool_handler, tool_router,
};
use serde_json::json;
use solite_core::sqlite::Connection;
use tokio::sync::Mutex;



#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ExecuteSqlRequest {
    pub sql: String,
    //pub parameters: Option<serde_json::Value>,
}

#[derive(Clone)]
pub struct Sandbox {
    connection: Arc<Mutex<Connection>>,
    pub(crate) tool_router: ToolRouter<Sandbox>,
    exports: Arc<Mutex<HashMap<String, tempfile::TempPath>>>,

}

#[tool_router]
impl Sandbox {
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self {
            connection: Arc::new(Mutex::new(Connection::open_in_memory().unwrap())),
            tool_router: Self::tool_router(),
            exports: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn _create_resource_text(&self, uri: &str, name: &str) -> Resource {
        RawResource::new(uri, name.to_string()).no_annotation()
    }

    #[tool(description = "Execute a single SQL statement, including writes")]
    async fn execute_sql(
        &self,
        Parameters(ExecuteSqlRequest { sql }): Parameters<ExecuteSqlRequest>,
    ) -> Result<CallToolResult, McpError> {
        let connection = self.connection.lock().await;
        let stmt = connection.prepare(&sql).map_err(|e| McpError::invalid_request(e.to_string(), None))?.1.unwrap();
        let columns = stmt.column_names().unwrap();
        /*if let Some(params) = parameters {
            1;
        }*/
        let mut result = String::new();
        loop {
            match stmt.nextx() {
                Ok(Some(row)) => {
                    for (idx, column) in columns.iter().enumerate() {
                        result += &format!("{}: {}\n", column, row.value_at(idx).as_str());
                    }
                }
                Ok(None) => break,
                Err(_) => todo!(),
            }
        }

        Ok(CallToolResult::success(vec![Content::text(result)]))
    }

    #[tool(description = "Repeat what you say")]
    fn echo(&self, Parameters(object): Parameters<JsonObject>) -> Result<CallToolResult, McpError> {
        Ok(CallToolResult::success(vec![Content::text(
            serde_json::Value::Object(object).to_string(),
        )]))
    }
    #[tool(description = "export the database")]
    async fn export_database(&self, Parameters(object): Parameters<JsonObject>) -> Result<CallToolResult, McpError> {
        let connection = self.connection.lock().await;
        let buffer = connection.serialize().unwrap();
        let export_uri = format!("solite://aaa");
        Ok(CallToolResult::success(
          vec![
            Content::resource(
              ResourceContents::BlobResourceContents {
                  uri: export_uri,
                  mime_type: Some("application/vnd.sqlite3".to_owned()),
                  blob: base64::engine::general_purpose::STANDARD.encode(buffer),
              },
            )
          ]
        )
    )
    }
}
#[tool_handler]
impl ServerHandler for Sandbox {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::V_2024_11_05,
            capabilities: ServerCapabilities::builder()
                .enable_prompts()
                .enable_resources()
                .enable_tools()
                .build(),
            server_info: Implementation::from_build_env(),
            instructions: Some("This server provides a sandbox SQLite database. Perform read or write SQL statements.".to_string()),
        }
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParam>,
        _: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, McpError> {
        Ok(ListResourcesResult {
            resources: vec![
                //self._create_resource_text("memo://insights", "memo-name"),
            ],
            next_cursor: None,
        })
    }

    async fn read_resource(
        &self,
        ReadResourceRequestParam { uri }: ReadResourceRequestParam,
        _: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResult, McpError> {
        match uri.as_str() {
            "solite://wit" => {
                Ok(ReadResourceResult {
                    contents: vec![ResourceContents::BlobResourceContents  {
                        uri: uri.clone(),
                        mime_type: Some("application/vnd.sqlite3".to_string()),
                        blob: base64::engine::general_purpose::STANDARD.encode(vec![0xaa, 0xbb, 0xcc, 0xdd])
                    }],
                })
            }
            "memo://insights" => {
                let memo = "Business Intelligence Memo\n\nAnalysis has revealed 5 key insights ...";
                Ok(ReadResourceResult {
                    contents: vec![ResourceContents::text(memo, uri)],
                })
            }
            _ => Err(McpError::resource_not_found(
                "resource_not_found",
                Some(json!({
                    "uri": uri
                })),
            )),
        }
    }

    async fn initialize(
        &self,
        _request: InitializeRequestParam,
        context: RequestContext<RoleServer>,
    ) -> Result<InitializeResult, McpError> {
        if let Some(http_request_part) = context.extensions.get::<axum::http::request::Parts>() {
            let initialize_headers = &http_request_part.headers;
            let initialize_uri = &http_request_part.uri;
            tracing::info!(?initialize_headers, %initialize_uri, "initialize from http server");
        }
        Ok(self.get_info())
    }
}
