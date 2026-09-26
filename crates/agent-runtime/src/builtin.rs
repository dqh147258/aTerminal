//! Every built-in call crosses an actual in-memory MCP session before reaching Broker.
use crate::host::{TerminalBackend, ToolContext, ToolOutput};
use anyhow::{Context, Result, ensure};
use rig_core::completion::ToolDefinition;
use rmcp::{
    ErrorData, RoleClient, RoleServer, ServerHandler, ServiceExt,
    model::*,
    service::{RequestContext, RunningService},
};
use serde_json::Value;
use std::sync::{Arc, Mutex};
struct Server {
    backend: Arc<dyn TerminalBackend>,
    tools: Vec<Tool>,
    context: Arc<Mutex<Option<ToolContext>>>,
}
impl ServerHandler for Server {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build()).with_server_info(
            Implementation::new("builtin/terminal", env!("CARGO_PKG_VERSION")),
        )
    }
    async fn list_tools(
        &self,
        _: Option<PaginatedRequestParams>,
        _: RequestContext<RoleServer>,
    ) -> std::result::Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult {
            tools: self.tools.clone(),
            ..Default::default()
        })
    }
    fn get_tool(&self, name: &str) -> Option<Tool> {
        self.tools.iter().find(|t| t.name == name).cloned()
    }
    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _: RequestContext<RoleServer>,
    ) -> std::result::Result<CallToolResponse, ErrorData> {
        let context = self
            .context
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| ErrorData::invalid_request("inactive user run", None))?;
        if *context.cancel.borrow() {
            return Ok(CallToolResult::error(vec![ContentBlock::text("cancelled")]).into());
        }
        let result = self
            .backend
            .invoke(
                context,
                &request.name,
                Value::Object(request.arguments.unwrap_or_default()),
            )
            .await;
        match result {
            Ok(result) => Ok(CallToolResult::success(vec![ContentBlock::text(
                serde_json::to_string(&result)
                    .map_err(|_| ErrorData::internal_error("encode tool result", None))?,
            )])
            .into()),
            Err(error) => {
                Ok(CallToolResult::error(vec![ContentBlock::text(error.to_string())]).into())
            }
        }
    }
}
pub(crate) struct Gateway {
    client: RunningService<RoleClient, ClientConfig>,
    _server: RunningService<RoleServer, Server>,
    context: Arc<Mutex<Option<ToolContext>>>,
}
impl Gateway {
    pub async fn open(
        backend: Arc<dyn TerminalBackend>,
        definitions: &[ToolDefinition],
    ) -> Result<Self> {
        let context = Arc::new(Mutex::new(None));
        let tools = definitions
            .iter()
            .map(|t| {
                Ok(Tool::new(
                    t.name.clone(),
                    t.description.clone(),
                    t.parameters
                        .as_object()
                        .context("invalid_tool_schema")?
                        .clone(),
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        let server = Server {
            backend,
            tools,
            context: context.clone(),
        };
        let (a, b) = tokio::io::duplex(64 * 1024);
        let server_task = tokio::spawn(async move { server.serve(a).await });
        let client = ClientConfig::default().serve(b).await?;
        let server = server_task.await??;
        let advertised = client.list_tools(None).await?;
        ensure!(
            advertised.tools.len() == definitions.len(),
            "builtin_catalog_mismatch"
        );
        Ok(Self {
            client,
            _server: server,
            context,
        })
    }
    pub async fn call(&self, context: ToolContext, name: &str, args: Value) -> Result<ToolOutput> {
        *self.context.lock().unwrap() = Some(context);
        let result = self
            .client
            .call_tool(
                CallToolRequestParams::new(name.to_owned()).with_arguments(
                    args.as_object()
                        .context("tool_arguments_object_required")?
                        .clone(),
                ),
            )
            .await?;
        let text = result
            .content
            .first()
            .and_then(ContentBlock::as_text)
            .context("invalid_builtin_result")?;
        ensure!(!result.is_error.unwrap_or(false), "{}", text.text);
        Ok(serde_json::from_str(&text.text)?)
    }
}
