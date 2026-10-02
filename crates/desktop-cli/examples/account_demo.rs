//! Ephemeral account + Desktop fixture for native-app integration; secrets go to a private file.
use ai_terminal_agent::Client;
use ai_terminal_protocol::local::{Operation, Request};
use ai_terminal_security::account::DesktopAccountCommand;
use anyhow::{Context, Result};
use axum::response::IntoResponse;
use std::{
    path::PathBuf,
    process::{Command, Stdio},
    time::{Duration, Instant},
};
#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
async fn main() -> Result<()> {
    let args: Vec<_> = std::env::args_os().collect();
    let dir = PathBuf::from(args.get(1).context("account_demo STATE CLI")?);
    let cli = PathBuf::from(args.get(2).context("CLI required")?);
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(&dir)?;
    let password = ai_terminal_security::random_secret()?;
    let assistant_test = args.iter().any(|value| value == "--assistant-test");
    let agent_test = args.iter().any(|value| value == "--agent-test");
    let db = dir.join("demo.db");
    ai_terminal_server::account::manage_user(&db, "demo", &password, false)?;
    let mut router = ai_terminal_server::router(&db, &ai_terminal_security::random_secret()?)?;
    if assistant_test {
        router = router.route("/model/chat/completions", axum::routing::post(|axum::Json(body): axum::Json<serde_json::Value>| async move {
            let task = body["messages"].as_array().and_then(|v| v.last()).and_then(|v| v["content"].as_str()).unwrap_or_default();
            let command = if task.contains("ls /Volumes/Code") {
                Some("ls /Volumes/Code")
            } else if task.contains("AI_DEVICE_OK") {
                Some("printf 'AI_DEVICE_OK\\n'; sleep 2; printf 'AI_DEVICE_DONE\\n'")
            } else { None };
            let message = if let Some(command) = command.filter(|_| body.get("tools").is_some()) {
                serde_json::json!({"role":"assistant","content":"Deterministic test model: requested terminal input.","tool_calls":[{"id":"fixture-input","type":"function","function":{"name":"terminal_input","arguments":serde_json::json!({"text":command,"submit":true}).to_string()}}]})
            } else {
                let observing = task.contains("Terminal snapshot revision");
                serde_json::json!({"role":"assistant","content":if observing { "Deterministic test model: terminal changes observed; this does not establish command success." } else { "Deterministic test model: no terminal input requested." }})
            };
            axum::Json(serde_json::json!({"choices":[{"message":message}]}))
        }));
    }
    if agent_test {
        let task_dir = dir.clone();
        router=router.route("/agent-model/chat/completions",axum::routing::post(move |axum::Json(body):axum::Json<serde_json::Value>| {
            let task_dir = task_dir.clone();
            async move{
            use serde_json::{Value,json};
            match task_result_fixture(&body, &task_dir).await {
                Ok(Some(response)) => return response,
                Err(error) => return fixture_sse(json!({"content":format!("TASK_RESULTS_FIXTURE_ERROR: {error:#}")})),
                Ok(None) => {},
            }
            let messages=body["messages"].as_array().unwrap();
            let analyzing=messages.last().and_then(|m|m["content"].as_str()).is_some_and(|s|s.starts_with("Application analysis stage"));
            let observed=messages.iter().rev().filter(|m|m["role"]=="tool").find_map(|m|serde_json::from_str::<Value>(m["content"].as_str()?).ok());
            let picture=messages.iter().rev().filter(|m|m["role"]=="user").find_map(|m|m["content"].as_array().and_then(|parts|parts.iter().find_map(|part|part["image_url"]["url"].as_str())));
            let vision=picture.is_some_and(|url|url.starts_with("data:image/png;base64,iVBOR"));
            let global_fixture = messages.iter().rev().filter(|m| m["role"] == "user").any(|m| m["content"].to_string().contains("GLOBAL_FIXTURE"));
            let tool=observed.is_none()&&!analyzing&&!vision&&!global_fixture;
            let delta=if analyzing {
                let text=observed.as_ref().and_then(|v|v["body"].as_str()).unwrap_or("");
                let tui=text.lines().filter(|line|line.starts_with("TUI status:")||line.trim_end().ends_with('$')).collect::<Vec<_>>();
                json!({"content":json!({"summary":"Read fixture logs; interactive rows are separate search exclusions.","key_quotes":[],"facts":[],"tui_lines":tui,"open_questions":[]}).to_string()})
            }else if tool {json!({"role":"assistant","tool_calls":[{"index":0,"id":"device-read","type":"function","function":{"name":"read_terminal","arguments":"{\"mode\":\"tail\",\"max_lines\":100}"}}]})}
            else{json!({"content":if vision {"VISION_FIXTURE_DONE"} else if global_fixture {"GLOBAL_FIXTURE_DONE"} else {"UI_FIXTURE_DONE"}})};
            let first=json!({"id":"device-response","object":"chat.completion.chunk","created":0,"model":"fixture","choices":[{"index":0,"delta":delta,"finish_reason":null}]});
            let end=json!({"id":"device-response","object":"chat.completion.chunk","created":0,"model":"fixture","choices":[{"index":0,"delta":{},"finish_reason":if tool{"tool_calls"}else{"stop"}}],"usage":{"prompt_tokens":20,"completion_tokens":10,"total_tokens":30}});
            ([("content-type","text/event-stream")],format!("data: {first}\n\ndata: {end}\n\ndata: [DONE]\n\n")).into_response()
        }}));
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let url = format!("http://{}", listener.local_addr()?);
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let mut command = Command::new(cli);
    if assistant_test {
        command
            .env("AI_TERMINAL_AI_BASE_URL", format!("{url}/model"))
            .env("AI_TERMINAL_AI_MODEL", "deterministic-device-test")
            .env_remove("AI_TERMINAL_AI_API_KEY");
    }
    if agent_test {
        command.env("HOME", &dir);
    }
    let mut child = command
        .env("AI_TERMINAL_CREDENTIAL_STORE", "file")
        .env("XDG_CONFIG_HOME", dir.join("config"))
        .args(["--agent", "--state-dir"])
        .arg(&dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let until = Instant::now() + Duration::from_secs(5);
    let local = loop {
        if let Ok(c) = Client::connect(&dir) {
            break c;
        }
        anyhow::ensure!(Instant::now() < until, "agent startup failed");
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    local.call(Request {
        operation: Operation::Account as i32,
        text: serde_json::to_string(&DesktopAccountCommand::Login {
            server: url.clone(),
            ca: None,
            username: "demo".into(),
            password: password.clone(),
            device_name: "Local Desktop".into(),
        })?,
        ..Request::default()
    })?;
    let info = local
        .call(Request {
            operation: Operation::Create as i32,
            cwd: if agent_test {
                dir.to_string_lossy().into_owned()
            } else {
                String::new()
            },
            command: if cfg!(unix) && agent_test {
                let lines = (0..60)
                    .map(|i| format!("UI_LOG_{i:03}"))
                    .collect::<Vec<_>>()
                    .join(" ");
                vec![
                    "/bin/sh".into(),
                    "-c".into(),
                    format!("printf '%s\\n' {lines}; printf 'TUI status: 01\\n'; exec /bin/sh -i"),
                ]
            } else if cfg!(unix) {
                vec!["/bin/sh".into(), "-i".into()]
            } else {
                Vec::new()
            },
            rows: 24,
            cols: 120,
            ..Request::default()
        })?
        .info
        .context("missing session")?;
    local.call(Request {
        operation: if agent_test {
            Operation::AttachDesktop as i32
        } else {
            Operation::Detach as i32
        },
        session: info.id.clone(),
        ..Request::default()
    })?;
    if agent_test {
        let view = local.call(Request {
            operation: Operation::Configuration as i32,
            text: serde_json::json!({"action":"show"}).to_string(),
            ..Default::default()
        })?;
        let view: serde_json::Value = serde_json::from_str(&view.history[0])?;
        let config = serde_json::json!({"providers":{"fixture":{"id":"fixture","name":"Isolated UI fixture","connection":{"protocol":"openai_chat","endpoint":format!("{url}/agent-model")},"credential_revision":0}},"models":{"fixture":{"id":"fixture","name":"Deterministic device fixture","provider_id":"fixture","model":"fixture","context_window":128000,"max_tokens":2048,"capabilities":{"tools":true,"streaming":true,"vision":true},"max_rounds":24,"max_seconds":60,"read_only":false}},"bindings":{"global":{"model_id":"fixture"},"session-default":{"model_id":"fixture"}}});
        local.call(Request{operation:Operation::Configuration as i32,text:serde_json::json!({"action":"replace","expected_revision":view["revision"],"config":config}).to_string(),..Default::default()})?;
    }
    let mut benchmarks = Vec::new();
    if args.iter().any(|value| value == "--bench") {
        for (name, rows, cols, output) in [
            ("idle-80x24", 24, 80, false),
            ("output-120x40", 40, 120, true),
            ("history-120x40", 40, 120, false),
        ] {
            let cwd = std::env::current_dir()?.join(&dir).join(name);
            std::fs::create_dir_all(&cwd)?;
            let code = r#"import os,tty,threading,time
 tty.setraw(0)
 lock=threading.Lock()
 os.write(1,b'history line\r\n'*200+b'\x1b[2J\x1b[H')
 def output():
  i=0
  while True:
   with lock: os.write(1,('\x1b7\x1b[ROW;1Hload:%08d\x1b8'%i).encode())
   i+=1
   time.sleep(1/30)
 if ENABLED: threading.Thread(target=output,daemon=True).start()
 while True:
  data=os.read(0,1024)
  if not data: break
  with lock: os.write(1,data)
"#
            .replace("\n ", "\n")
            .replace("ROW", &rows.to_string())
            .replace("ENABLED", if output { "True" } else { "False" });
            let session = local
                .call(Request {
                    operation: Operation::Create as i32,
                    command: vec!["python3".into(), "-u".into(), "-c".into(), code],
                    cwd: cwd.to_string_lossy().into(),
                    rows,
                    cols,
                    ..Request::default()
                })?
                .info
                .context("missing benchmark session")?;
            local.call(Request {
                operation: Operation::Detach as i32,
                session: session.id.clone(),
                ..Request::default()
            })?;
            benchmarks
                .push(serde_json::json!({"id":session.id,"name":name,"rows":rows,"cols":cols}));
        }
    }
    let mut options = std::fs::OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(dir.join("account-fixture.json"))?;
    serde_json::to_writer(
        file,
        &serde_json::json!({"server":url,"username":"demo","password":password,"session":info.id,"benchmarks":benchmarks,"assistant_test":assistant_test,"agent_test":agent_test}),
    )?;
    println!("Account fixture ready; credentials written to private fixture file");
    if agent_test {
        loop {
            tokio::select! {signal=tokio::signal::ctrl_c()=>{signal?;break;},_=tokio::time::sleep(Duration::from_secs(2))=>{local.call(Request{operation:Operation::Poll as i32,session:info.id.clone(),revision:u64::MAX,..Default::default()})?;}}
        }
    } else {
        tokio::signal::ctrl_c().await?;
    }
    let _ = local.call(Request {
        operation: Operation::Shutdown as i32,
        ..Request::default()
    });
    let _ = child.wait();
    server.abort();
    Ok(())
}

fn fixture_sse(delta: serde_json::Value) -> axum::response::Response {
    use serde_json::json;
    let tools = delta["tool_calls"].is_array();
    let first = json!({"id":"task-fixture","object":"chat.completion.chunk","created":0,"model":"fixture","choices":[{"index":0,"delta":delta,"finish_reason":null}]});
    let end = json!({"id":"task-fixture","object":"chat.completion.chunk","created":0,"model":"fixture","choices":[{"index":0,"delta":{},"finish_reason":if tools {"tool_calls"} else {"stop"}}],"usage":{"prompt_tokens":20,"completion_tokens":10,"total_tokens":30}});
    (
        [("content-type", "text/event-stream")],
        format!("data: {first}\n\ndata: {end}\n\ndata: [DONE]\n\n"),
    )
        .into_response()
}

/// Deterministic task workflow used by the opt-in Android encrypted-RPC test.
async fn task_result_fixture(
    body: &serde_json::Value,
    dir: &std::path::Path,
) -> Result<Option<axum::response::Response>> {
    use serde_json::{Value, json};
    let messages = body["messages"].as_array().context("messages required")?;
    let content = |message: &Value| -> String {
        message["content"]
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| {
                message["content"]
                    .as_array()
                    .map(|parts| {
                        parts
                            .iter()
                            .filter_map(|p| p["text"].as_str())
                            .collect::<Vec<_>>()
                            .join("\n")
                    })
                    .unwrap_or_default()
            })
    };
    let global = body["tools"].as_array().is_some_and(|tools| {
        tools
            .iter()
            .any(|t| t["function"]["name"] == "get_agent_task")
    });
    if !global {
        let latest = messages
            .iter()
            .rev()
            .filter(|m| m["role"] == "user")
            .map(content)
            .find(|text| text.contains("TASK_RESULTS_CHILD") || text.contains("TASK_ERROR_CHILD"));
        if let Some(text) = latest {
            tokio::time::sleep(Duration::from_secs(2)).await;
            if text.contains("TASK_ERROR_CHILD") {
                return Ok(Some((axum::http::StatusCode::BAD_REQUEST, axum::Json(json!({"error":{"message":format!("TASK_ERROR_LONG:{}", "upstream diagnostic ".repeat(3000)),"type":"fixture_error"}}))).into_response()));
            }
            return Ok(Some(fixture_sse(
                json!({"content":format!("TASK_RESULTS_CHILD_DONE:{}", "evidence ".repeat(1600))}),
            )));
        }
        return Ok(None);
    }
    let Some((start, prompt)) = messages
        .iter()
        .enumerate()
        .rev()
        .filter(|(_, m)| m["role"] == "user")
        .map(|(i, m)| (i, content(m)))
        .find(|(_, text)| {
            text.starts_with("TASK_RESULTS_GLOBAL:") || text.starts_with("TASK_ERROR_GLOBAL:")
        })
    else {
        return Ok(None);
    };
    let failure = prompt.starts_with("TASK_ERROR_GLOBAL:");
    let session = prompt.split_once(':').context("session required")?.1.trim();
    let tool = |id: &str, name: &str, args: Value| {
        fixture_sse(
            json!({"role":"assistant","tool_calls":[{"index":0,"id":id,"type":"function","function":{"name":name,"arguments":args.to_string()}}]}),
        )
    };
    let results = messages[start..]
        .iter()
        .filter(|m| m["role"] == "tool")
        .map(|m| {
            Ok((
                m["tool_call_id"]
                    .as_str()
                    .context("tool call ID required")?
                    .to_owned(),
                serde_json::from_str::<Value>(&content(m))?,
            ))
        })
        .collect::<Result<Vec<_>>>()?;
    let find = |id: &str| {
        results
            .iter()
            .find(|(call, _)| call == id)
            .map(|(_, value)| value)
    };
    let Some(sent) = find("task-send") else {
        return Ok(Some(tool(
            "task-send",
            "send_agent_message",
            json!({"session_id":session,"message":if failure {"TASK_ERROR_CHILD"} else {"TASK_RESULTS_CHILD"}}),
        )));
    };
    let task = sent["task_id"]
        .as_str()
        .context("delegation did not return task_id")?;
    if find("task-get").is_none() {
        return Ok(Some(tool(
            "task-get",
            "get_agent_task",
            json!({"task_id":task}),
        )));
    }
    if find("task-short-wait").is_none() {
        return Ok(Some(tool(
            "task-short-wait",
            "wait_agent_task",
            json!({"task_id":task,"timeout_ms":1}),
        )));
    }
    anyhow::ensure!(
        find("task-short-wait").unwrap()["timed_out"] == true,
        "short wait must time out while child is active"
    );
    let Some(waited) = find("task-wait") else {
        return Ok(Some(tool(
            "task-wait",
            "wait_agent_task",
            json!({"task_id":task,"timeout_ms":10000}),
        )));
    };
    anyhow::ensure!(
        waited["done"] == true && waited["timed_out"] == false,
        "task did not finish: {waited}"
    );
    if failure {
        anyhow::ensure!(
            waited["state"] == "paused" && waited["error_truncated"] == true,
            "long error was not durably bounded: {waited}"
        );
        let error = waited["error"].as_str().context("error missing")?;
        anyhow::ensure!(
            error.len() <= 2048 && error.contains("TASK_ERROR_LONG"),
            "wrong error diagnostic"
        );
        std::fs::write(
            dir.join("task-error-observations.json"),
            serde_json::to_vec_pretty(
                &json!({"task_id":task,"state":waited["state"],"error_truncated":true,"error_bytes":error.len(),"timeout_observed":true}),
            )?,
        )?;
        return Ok(Some(fixture_sse(
            json!({"content":"TASK_ERROR_GLOBAL_DONE"}),
        )));
    }
    anyhow::ensure!(
        waited["state"] == "completed" && waited["result_truncated"] == true,
        "expected a bounded completed result"
    );
    let record = waited["result_record_id"]
        .as_str()
        .context("result record missing")?;
    let pages = results
        .iter()
        .filter(|(id, _)| id.starts_with("task-read-"))
        .collect::<Vec<_>>();
    let analyzing = messages
        .last()
        .is_some_and(|m| content(m).starts_with("Application analysis"));
    let captured_path = dir.join(format!("task-pages-{task}.json"));
    if analyzing {
        let (_, page) = pages.last().context("analysis page missing")?;
        anyhow::ensure!(page["body"].is_string(), "analysis body missing");
        let mut captured: Vec<Value> = if captured_path.exists() {
            serde_json::from_slice(&std::fs::read(&captured_path)?)?
        } else {
            vec![]
        };
        captured.retain(|old| old["offset"] != page["offset"]);
        captured.push(json!({"offset":page["offset"],"body":page["body"]}));
        captured.sort_by_key(|part| part["offset"].as_u64().unwrap_or(0));
        std::fs::write(&captured_path, serde_json::to_vec(&captured)?)?;
        // Keep the opaque next-page cursor in the model's digest; raw pages leave the
        // context after Host analysis. Capture those actual HTTP pages for the assertion.
        let digest = json!({"summary":json!({"task_result_cursor":page["cursor"]}).to_string(),"key_quotes":[],"facts":[],"tui_lines":[],"open_questions":[]});
        return Ok(Some(fixture_sse(json!({"content":digest.to_string()}))));
    }
    if pages.is_empty() {
        // Exercise real concurrent local history maintenance before the model follows
        // the record reference. Only this fixture's Session is cleaned.
        let local = Client::connect(dir)?;
        let reply = local.call(Request { operation:Operation::Agent as i32, session:session.into(),
            text:json!({"version":1,"action":"clean","rule":{"selector":"before","utc_ms":chrono::Utc::now().timestamp_millis()+1000}}).to_string(), ..Default::default() })?;
        anyhow::ensure!(reply.error.is_empty(), "clean failed: {}", reply.error);
        let cleaned: Value =
            serde_json::from_str(reply.history.first().context("clean result missing")?)?;
        // The selected completion report belongs to the Global scope; this cleanup
        // visits only the child's scope, which holds the final answer.
        anyhow::ensure!(
            cleaned["pinned"].as_u64().unwrap_or(0) >= 1,
            "active task result was not protected: {cleaned}"
        );
        std::fs::write(
            dir.join("task-result-observations.json"),
            serde_json::to_vec_pretty(
                &json!({"task_id":task,"record_id":record,"retention":cleaned,"timeout_observed":true}),
            )?,
        )?;
    }
    let mut next = Value::Null;
    if let Some((_, page)) = pages.last() {
        let digest: Value = serde_json::from_str(
            page["digest"]["summary"]
                .as_str()
                .context("analyzed page missing")?,
        )?;
        next = digest["task_result_cursor"].clone();
        if next.is_null() {
            let captured: Vec<Value> = serde_json::from_slice(&std::fs::read(&captured_path)?)?;
            let full = captured
                .iter()
                .map(|p| p["body"].as_str().unwrap_or_default())
                .collect::<String>();
            let answer: Value = serde_json::from_str(&full)?;
            anyhow::ensure!(
                answer["text"] == format!("TASK_RESULTS_CHILD_DONE:{}", "evidence ".repeat(1600)),
                "result was not lossless"
            );
            let path = dir.join("task-result-observations.json");
            let mut observations: Value = serde_json::from_slice(&std::fs::read(&path)?)?;
            observations["read_pages"] = json!(captured.len());
            observations["read_bytes"] = json!(full.len());
            observations["lossless"] = json!(true);
            std::fs::write(path, serde_json::to_vec_pretty(&observations)?)?;
            return Ok(Some(fixture_sse(
                json!({"content":"TASK_RESULTS_GLOBAL_DONE"}),
            )));
        }
    }
    let mut args = json!({"record_id":record,"part":"body"});
    if !next.is_null() {
        args["cursor"] = next;
    }
    Ok(Some(tool(
        &format!("task-read-{}", pages.len()),
        "read_record",
        args,
    )))
}
