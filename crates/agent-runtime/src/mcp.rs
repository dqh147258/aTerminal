//! Frozen MCP connections. No sampling handler or notification-to-model callback exists.
use anyhow::{Result, ensure};
use rmcp::{
    ServiceExt,
    model::{CallToolRequestParams, CallToolResult, ClientConfig, PaginatedRequestParams, Tool},
    service::RunningService,
    transport::{
        StreamableHttpClientTransport, TokioChildProcess,
        streamable_http_client::StreamableHttpClientTransportConfig,
    },
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, HashSet},
    path::PathBuf,
    sync::Arc,
    time::Duration,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "transport", rename_all = "snake_case", deny_unknown_fields)]
pub enum TransportConfig {
    Stdio {
        command: PathBuf,
        #[serde(default)]
        args: Vec<String>,
        cwd: Option<PathBuf>,
        #[serde(default)]
        env: BTreeMap<String, String>,
    },
    StreamableHttp {
        url: String,
        #[serde(default)]
        headers: BTreeMap<String, String>,
    },
}
pub struct CatalogSnapshot {
    pub server_id: String,
    pub revision: u64,
    pub tools: Vec<Tool>,
    pub connection: Arc<Connection>,
}
pub struct Connection {
    service: RunningService<rmcp::RoleClient, ClientConfig>,
    diagnostics: Diagnostics,
}
impl Connection {
    pub fn diagnostics(&self) -> Vec<String> {
        self.diagnostics
            .lines
            .lock()
            .unwrap()
            .iter()
            .cloned()
            .collect()
    }
    pub fn close(&self) {
        self.service.cancellation_token().cancel();
    }
    pub async fn connect(config: &TransportConfig, timeout: Duration) -> Result<Self> {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let open = async {
            let mut diagnostics = Diagnostics::default();
            let service = match config {
                TransportConfig::Stdio {
                    command,
                    args,
                    cwd,
                    env,
                } => {
                    ensure!(command.is_absolute(), "mcp_command_must_be_absolute");
                    let mut child = tokio::process::Command::new(command);
                    child
                        .args(args)
                        .env_clear()
                        .envs(env)
                        .kill_on_drop(true)
                        .stderr(std::process::Stdio::null());
                    if let Some(cwd) = cwd {
                        child.current_dir(cwd);
                    }
                    let mut command = process_wrap::tokio::CommandWrap::from(child);
                    #[cfg(unix)]
                    command.wrap(process_wrap::tokio::ProcessGroup::leader());
                    #[cfg(windows)]
                    command.wrap(process_wrap::tokio::JobObject);
                    command.wrap(process_wrap::tokio::KillOnDrop);
                    let (transport, stderr) = TokioChildProcess::builder(command)
                        .stderr(std::process::Stdio::piped())
                        .spawn()?;
                    if let Some(stderr) = stderr {
                        diagnostics.capture(
                            stderr,
                            env.values().filter(|v| !v.is_empty()).cloned().collect(),
                        );
                    }
                    ClientConfig::default().serve(transport).await?
                }
                TransportConfig::StreamableHttp { url, headers } => {
                    let parsed = reqwest::Url::parse(url)?;
                    ensure!(
                        ["http", "https"].contains(&parsed.scheme()),
                        "invalid_mcp_url"
                    );
                    let mut header_map = reqwest::header::HeaderMap::new();
                    for (name, value) in headers {
                        let mut value = reqwest::header::HeaderValue::from_str(value)?;
                        value.set_sensitive(true);
                        header_map.insert(
                            reqwest::header::HeaderName::from_bytes(name.as_bytes())?,
                            value,
                        );
                    }
                    let client = reqwest::Client::builder()
                        .default_headers(header_map)
                        .redirect(reqwest::redirect::Policy::none())
                        .retry(reqwest::retry::never())
                        .build()?;
                    ClientConfig::default()
                        .serve(StreamableHttpClientTransport::with_client(
                            client,
                            http_config(url),
                        ))
                        .await?
                }
            };
            Ok(Self {
                service,
                diagnostics,
            })
        };
        tokio::time::timeout(timeout, open)
            .await
            .map_err(|_| anyhow::anyhow!("mcp_initialize_timeout"))?
    }
    pub async fn catalog(
        self: Arc<Self>,
        server_id: String,
        revision: u64,
        timeout: Duration,
    ) -> Result<CatalogSnapshot> {
        let fetch = async {
            let mut cursor = None;
            let mut seen = HashSet::new();
            let mut tools = Vec::new();
            let mut bytes = 0;
            loop {
                let params = cursor.map(|c| {
                    let mut p = PaginatedRequestParams::default();
                    p.cursor = Some(c);
                    p
                });
                let page = self.service.list_tools(params).await?;
                bytes += serde_json::to_vec(&page)?.len();
                ensure!(
                    bytes <= 256 * 1024 && tools.len() + page.tools.len() <= 512,
                    "mcp_catalog_limit"
                );
                for tool in page.tools {
                    ensure!(
                        !tools.iter().any(|t: &Tool| t.name == tool.name),
                        "duplicate_mcp_tool"
                    );
                    tools.push(tool);
                }
                cursor = page.next_cursor;
                if let Some(c) = &cursor {
                    ensure!(
                        seen.insert(c.clone()) && seen.len() <= 32,
                        "mcp_cursor_cycle"
                    );
                } else {
                    break;
                }
            }
            tools.sort_by(|a, b| a.name.cmp(&b.name));
            Ok(tools)
        };
        let tools = tokio::time::timeout(timeout, fetch)
            .await
            .map_err(|_| anyhow::anyhow!("mcp_catalog_timeout"))??;
        Ok(CatalogSnapshot {
            server_id,
            revision,
            tools,
            connection: self,
        })
    }
    /// Unknown outcomes are not retried. Host must record an action before this call.
    pub async fn call(&self, name: &str, args: Value, timeout: Duration) -> Result<CallToolResult> {
        let args = args
            .as_object()
            .ok_or_else(|| anyhow::anyhow!("mcp_arguments_object_required"))?
            .clone();
        ensure!(
            serde_json::to_vec(&args)?.len() <= 64 * 1024,
            "mcp_input_limit"
        );
        let result = tokio::time::timeout(
            timeout,
            self.service
                .call_tool(CallToolRequestParams::new(name.to_owned()).with_arguments(args)),
        )
        .await
        .map_err(|_| anyhow::anyhow!("mcp_timeout_outcome_unknown"))??;
        ensure!(
            serde_json::to_vec(&result)?.len() <= 1024 * 1024,
            "mcp_output_limit"
        );
        Ok(result)
    }
}
#[derive(Default)]
struct Diagnostics {
    lines: Arc<std::sync::Mutex<std::collections::VecDeque<String>>>,
    task: Option<tokio::task::JoinHandle<()>>,
}
impl Diagnostics {
    fn capture(&mut self, mut stderr: tokio::process::ChildStderr, secrets: Vec<String>) {
        use tokio::io::AsyncReadExt;
        let lines = self.lines.clone();
        self.task = Some(tokio::spawn(async move {
            let mut chunk = [0u8; 1024];
            let mut line = Vec::new();
            let mut oversized = false;
            loop {
                let Ok(count) = stderr.read(&mut chunk).await else {
                    break;
                };
                if count == 0 {
                    break;
                }
                for byte in &chunk[..count] {
                    if *byte == b'\n' {
                        let mut text = if oversized {
                            "[oversized diagnostic omitted]".into()
                        } else {
                            String::from_utf8_lossy(&line)
                                .chars()
                                .filter(|c| !c.is_control() || *c == '\t')
                                .collect::<String>()
                        };
                        for secret in &secrets {
                            text = text.replace(secret, "[redacted]");
                        }
                        let mut saved = lines.lock().unwrap();
                        saved.push_back(text);
                        while saved.len() > 8 {
                            saved.pop_front();
                        }
                        line.clear();
                        oversized = false;
                    } else if !oversized {
                        if line.len() < 4096 {
                            line.push(*byte);
                        } else {
                            line.clear();
                            oversized = true;
                        }
                    }
                }
            }
        }));
    }
}
impl Drop for Diagnostics {
    fn drop(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}
fn http_config(url: &str) -> StreamableHttpClientTransportConfig {
    let mut config =
        StreamableHttpClientTransportConfig::with_uri(url.to_owned()).max_concurrent_requests(1);
    config.reinit_on_expired_session = false;
    config.max_sse_event_size = 1024 * 1024;
    config.channel_buffer_capacity = 8;
    config
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmcp::{ErrorData, RoleServer, ServerHandler, model::*, service::RequestContext};
    struct Server;
    impl ServerHandler for Server {
        fn get_info(&self) -> ServerConfig {
            ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
        }
        async fn list_tools(
            &self,
            _: Option<PaginatedRequestParams>,
            _: RequestContext<RoleServer>,
        ) -> std::result::Result<ListToolsResult, ErrorData> {
            let page = ListToolsResult {
                tools: vec![Tool::new(
                    "read_terminal",
                    "read",
                    serde_json::json!({"type":"object"})
                        .as_object()
                        .unwrap()
                        .clone(),
                )],
                ..Default::default()
            };
            Ok(page)
        }
        async fn call_tool(
            &self,
            _: CallToolRequestParams,
            _: RequestContext<RoleServer>,
        ) -> std::result::Result<CallToolResponse, ErrorData> {
            Ok(CallToolResult::success(vec![ContentBlock::text("terminal evidence")]).into())
        }
    }
    #[tokio::test]
    async fn builtin_uses_real_initialize_catalog_call_and_shutdown() {
        let (a, b) = tokio::io::duplex(4096);
        let server = tokio::spawn(async move { Server.serve(a).await.unwrap() });
        let client = ClientConfig::default().serve(b).await.unwrap();
        let server = server.await.unwrap();
        let connection = Arc::new(Connection {
            service: client,
            diagnostics: Diagnostics::default(),
        });
        let snapshot = connection
            .clone()
            .catalog("builtin/terminal".into(), 1, Duration::from_secs(1))
            .await
            .unwrap();
        assert_eq!(snapshot.tools[0].name, "read_terminal");
        let output = snapshot
            .connection
            .call(
                "read_terminal",
                serde_json::json!({}),
                Duration::from_secs(1),
            )
            .await
            .unwrap();
        assert!(!output.is_error.unwrap_or(false));
        drop(snapshot);
        Arc::try_unwrap(connection)
            .ok()
            .unwrap()
            .service
            .cancel()
            .await
            .unwrap();
        server.cancel().await.unwrap();
    }
    #[test]
    fn session_expiry_cannot_replay_side_effects() {
        assert!(!http_config("http://127.0.0.1/mcp").reinit_on_expired_session);
    }
}

#[cfg(all(test, unix))]
mod transport_contracts {
    use super::*;
    use serde_json::json;
    #[tokio::test]
    async fn stdio_pages_catalog_calls_once_and_redacts_bounded_diagnostics() {
        let script = r#"import sys,json,os
print('diagnostic '+os.environ['TEST_SECRET'],file=sys.stderr,flush=True)
for line in sys.stdin:
 r=json.loads(line)
 if 'id' not in r: continue
 method=r['method']
 if method=='initialize': result={'protocolVersion':r['params']['protocolVersion'],'capabilities':{'tools':{}},'serverInfo':{'name':'fixture','version':'1'}}
 elif method=='tools/list':
  last=r.get('params',{}).get('cursor')=='second'
  result={'tools':[{'name':'second' if last else 'first','inputSchema':{'type':'object'}}]}
  if not last: result['nextCursor']='second'
 elif method=='tools/call': result={'content':[{'type':'text','text':'called once'}]}
 else: result={}
 print(json.dumps({'jsonrpc':'2.0','id':r['id'],'result':result}),flush=True)
"#;
        let python = if std::path::Path::new("/usr/bin/python3").exists() {
            "/usr/bin/python3"
        } else {
            "/usr/local/bin/python3"
        };
        let connection = Arc::new(
            Connection::connect(
                &TransportConfig::Stdio {
                    command: python.into(),
                    args: vec!["-u".into(), "-c".into(), script.into()],
                    cwd: None,
                    env: BTreeMap::from([("TEST_SECRET".into(), "fake-secret-for-test".into())]),
                },
                Duration::from_secs(5),
            )
            .await
            .unwrap(),
        );
        let catalog = connection
            .clone()
            .catalog("fake".into(), 7, Duration::from_secs(5))
            .await
            .unwrap();
        assert_eq!(catalog.tools.len(), 2);
        let result = connection
            .call("first", json!({}), Duration::from_secs(2))
            .await
            .unwrap();
        assert!(
            serde_json::to_string(&result)
                .unwrap()
                .contains("called once")
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
        let diagnostics = connection.diagnostics().join("\n");
        assert!(diagnostics.contains("[redacted]"));
        assert!(!diagnostics.contains("fake-secret-for-test"));
        connection.close();
    }
    #[tokio::test]
    async fn http_transport_preserves_image_payload_and_does_not_retry_calls() {
        use axum::{Json, Router, extract::State, response::IntoResponse, routing::post};
        use std::sync::atomic::{AtomicUsize, Ordering};
        async fn serve(
            State(calls): State<Arc<AtomicUsize>>,
            Json(request): Json<Value>,
        ) -> axum::response::Response {
            if request.get("id").is_none() {
                return axum::http::StatusCode::ACCEPTED.into_response();
            }
            let result = match request["method"].as_str().unwrap() {
                "initialize" => {
                    json!({"protocolVersion":request["params"]["protocolVersion"],"capabilities":{"tools":{}},"serverInfo":{"name":"http-fixture","version":"1"}})
                }
                "tools/list" => json!({"tools":[{"name":"image","inputSchema":{"type":"object"}}]}),
                "tools/call" => {
                    calls.fetch_add(1, Ordering::AcqRel);
                    json!({"content":[{"type":"image","mimeType":"image/png","data":"aW1hZ2U="}]})
                }
                _ => json!({}),
            };
            Json(json!({"jsonrpc":"2.0","id":request["id"],"result":result})).into_response()
        }
        let calls = Arc::new(AtomicUsize::new(0));
        let app = Router::new()
            .route("/mcp", post(serve))
            .with_state(calls.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let connection = Arc::new(
            Connection::connect(
                &TransportConfig::StreamableHttp {
                    url: format!("http://{address}/mcp"),
                    headers: BTreeMap::new(),
                },
                Duration::from_secs(5),
            )
            .await
            .unwrap(),
        );
        let catalog = connection
            .clone()
            .catalog("http".into(), 1, Duration::from_secs(5))
            .await
            .unwrap();
        assert_eq!(catalog.tools.len(), 1);
        let result = connection
            .call("image", json!({}), Duration::from_secs(2))
            .await
            .unwrap();
        assert_eq!(
            serde_json::to_value(&result).unwrap()["content"][0]["data"],
            "aW1hZ2U="
        );
        assert_eq!(calls.load(Ordering::Acquire), 1);
        connection.close();
        task.abort();
    }
}
