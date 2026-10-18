//! Wire-level regression: identical serialized prefix/config, tool IDs and cancellation.
use ai_terminal_agent_runtime::model::*;
use axum::{Json, Router, extract::State, response::IntoResponse, routing::post};
use rig_core::completion::{Message, ToolDefinition};
use serde_json::{Value, json};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::watch;

async fn reply(
    State(calls): State<Arc<Mutex<Vec<Value>>>>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    calls.lock().unwrap().push(body);
    (
        [("content-type", "text/event-stream")],
        "data: {\"id\":\"response-1\",\"object\":\"chat.completion.chunk\",\"created\":0,\"model\":\"test-model\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"ok\"},\"finish_reason\":null}]}\n\ndata: {\"id\":\"response-1\",\"object\":\"chat.completion.chunk\",\"created\":0,\"model\":\"test-model\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":10,\"completion_tokens\":1,\"total_tokens\":11}}\n\ndata: [DONE]\n\n",
    )
}
fn builder() -> RequestBuilder {
    RequestBuilder {
        settings: RequestSettings {
            model: "test-model".into(),
            temperature: Some(0.6),
            max_tokens: 512,
            additional_params: Some(json!({"top_p":0.9})),
        },
        system: "static system".into(),
        tools: vec![ToolDefinition {
            name: "read_terminal".into(),
            description: "Read immutable view".into(),
            parameters: json!({"type":"object","properties":{}}),
        }],
    }
}
fn entry() -> ContextEntry {
    ContextEntry {
        unit_id: None,
        id: "user-1".into(),
        origin: Origin::User,
        root_user_message_id: Some("user-1".into()),
        artifacts: vec![],
        message: Message::user("check the terminal"),
    }
}
#[tokio::test]
async fn actual_wire_analysis_preserves_prefix_and_request_parameters() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let app = Router::new()
        .route("/chat/completions", post(reply))
        .with_state(calls.clone());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let model = connect(
        &Connection {
            protocol: Protocol::OpenaiChat,
            endpoint: format!("http://{address}"),
            api_version: None,
        },
        "test-model",
        "fake-local-key",
    )
    .unwrap();
    let (_cancel, rx) = watch::channel(false);
    let builder = builder();
    let history = vec![entry()];
    for record in [None, Some("record-1")] {
        let mut text = String::new();
        let response = collect(
            model.as_ref(),
            builder.build(&history, record).unwrap(),
            rx.clone(),
            Duration::from_secs(5),
            |delta| text.push_str(delta),
        )
        .await
        .unwrap();
        assert_eq!(text, "ok");
        assert!(!response.choice.is_empty());
    }
    let calls = calls.lock().unwrap();
    assert_eq!(calls.len(), 2);
    let mut analysis = calls[1].clone();
    analysis["messages"].as_array_mut().unwrap().pop();
    assert_eq!(calls[0], analysis);
    server.abort();
}
#[tokio::test]
async fn cancellation_prevents_even_opening_a_request() {
    let model = connect(
        &Connection {
            protocol: Protocol::OpenaiChat,
            endpoint: "http://127.0.0.1:1".into(),
            api_version: None,
        },
        "test-model",
        "fake",
    )
    .unwrap();
    let (_cancel, rx) = watch::channel(true);
    let result = collect(
        model.as_ref(),
        builder().build(&[entry()], None).unwrap(),
        rx,
        Duration::from_secs(5),
        |_| {},
    )
    .await;
    assert_eq!(result.err().unwrap().to_string(), "cancelled");
}

#[tokio::test]
async fn actual_http_errors_are_sanitized_and_collect_keeps_one_attempt() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let calls = Arc::new(AtomicUsize::new(0));
    let received = calls.clone();
    let app = Router::new().route(
        "/{status}/chat/completions",
        post(
            move |axum::extract::Path(status): axum::extract::Path<u16>| {
                let received = received.clone();
                async move {
                    received.fetch_add(1, Ordering::AcqRel);
                    (
                        axum::http::StatusCode::from_u16(status).unwrap(),
                        [("retry-after", "10")],
                        "secret provider response: https://private.invalid/?api_key=do-not-display",
                    )
                }
            },
        ),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    for status in [401, 429, 503] {
        let model = connect(
            &Connection {
                protocol: Protocol::OpenaiChat,
                endpoint: format!("http://{address}/{status}"),
                api_version: None,
            },
            "test",
            "fake-secret",
        )
        .unwrap();
        let (_tx, rx) = watch::channel(false);
        let error = collect(
            model.as_ref(),
            builder().build(&[entry()], None).unwrap(),
            rx,
            Duration::from_secs(2),
            |_| {},
        )
        .await
        .err()
        .unwrap();
        assert_eq!(error.to_string(), format!("model_provider_http_{status}"));
    }
    assert_eq!(
        calls.load(Ordering::Acquire),
        3,
        "Only Host may schedule a retry"
    );
    server.abort();
}

#[tokio::test]
async fn actual_token_limit_and_truncated_streams_never_report_success() {
    let partial = "data: {\"id\":\"response-1\",\"object\":\"chat.completion.chunk\",\"created\":0,\"model\":\"test-model\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"partial\"},\"finish_reason\":null}]}\n\n";
    let capped = format!(
        "{partial}data: {{\"id\":\"response-1\",\"object\":\"chat.completion.chunk\",\"created\":0,\"model\":\"test-model\",\"choices\":[{{\"index\":0,\"delta\":{{}},\"finish_reason\":\"length\"}}]}}\n\ndata: [DONE]\n\n"
    );
    let app = Router::new()
        .route(
            "/truncated/chat/completions",
            post(move || async move { ([("content-type", "text/event-stream")], partial) }),
        )
        .route(
            "/capped/chat/completions",
            post(move || {
                let capped = capped.clone();
                async move { ([("content-type", "text/event-stream")], capped) }
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    for (path, expected) in [
        ("truncated", "model_stream_truncated"),
        ("capped", "model_output_token_limit"),
    ] {
        let model = connect(
            &Connection {
                protocol: Protocol::OpenaiChat,
                endpoint: format!("http://{address}/{path}"),
                api_version: None,
            },
            "test",
            "fake",
        )
        .unwrap();
        let (_tx, rx) = watch::channel(false);
        let mut text = String::new();
        let error = collect(
            model.as_ref(),
            builder().build(&[entry()], None).unwrap(),
            rx,
            Duration::from_secs(2),
            |delta| text.push_str(delta),
        )
        .await
        .err()
        .unwrap();
        assert_eq!(error.to_string(), expected);
        assert_eq!(text, "partial");
    }
    server.abort();
}

async fn capture_error(
    State(calls): State<Arc<Mutex<Vec<Value>>>>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    calls.lock().unwrap().push(body);
    (
        axum::http::StatusCode::BAD_REQUEST,
        Json(
            json!({"error":{"message":"intentional local capture","type":"invalid_request_error","code":400,"status":"INVALID_ARGUMENT"},"type":"error"}),
        ),
    )
}
#[tokio::test]
async fn every_provider_keeps_terminal_tool_pair_and_analysis_prefix() {
    use rig_core::message::{
        AssistantContent, ToolCall, ToolFunction, ToolResultContent, UserContent,
    };
    let calls = Arc::new(Mutex::new(Vec::new()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let app = Router::new()
        .fallback(post(capture_error))
        .with_state(calls.clone());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let call = ToolCall::from_wire(
        "call_stable",
        ToolFunction {
            name: "read_terminal".into(),
            arguments: json!({}),
        },
    );
    let result = UserContent::tool_result_for(
        call.id.clone(),
        call.provider.clone(),
        "read_terminal",
        vec![ToolResultContent::text(
            "UUID=record-1\nimmutable terminal body",
        )],
    );
    let mut history = vec![entry()];
    for (id, message) in [
        (
            "assistant-1",
            Message::Assistant {
                id: Some("message-stable".into()),
                content: vec![AssistantContent::ToolCall(call)],
            },
        ),
        (
            "tool-1",
            Message::User {
                content: vec![result],
            },
        ),
    ] {
        history.push(ContextEntry {
            unit_id: None,
            id: id.into(),
            origin: Origin::Tool,
            root_user_message_id: Some("user-1".into()),
            artifacts: vec!["record-1".into()],
            message,
        });
    }
    for protocol in [
        Protocol::OpenaiResponses,
        Protocol::OpenaiChat,
        Protocol::Anthropic,
        Protocol::Gemini,
        Protocol::AzureOpenai,
        Protocol::Ollama,
    ] {
        calls.lock().unwrap().clear();
        let model = connect(
            &Connection {
                protocol: protocol.clone(),
                endpoint: format!("http://{address}"),
                api_version: Some("2024-10-21".into()),
            },
            "test-model",
            "fake",
        )
        .unwrap();
        let (_cancel, rx) = watch::channel(false);
        for record in [None, Some("record-1")] {
            let _ = collect(
                model.as_ref(),
                builder().build(&history, record).unwrap(),
                rx.clone(),
                Duration::from_secs(3),
                |_| {},
            )
            .await;
        }
        let calls = calls.lock().unwrap();
        assert_eq!(calls.len(), 2, "{protocol:?}");
        let mut analysis = calls[1].clone();
        let key = if analysis.get("input").is_some() {
            "input"
        } else if analysis.get("contents").is_some() {
            "contents"
        } else {
            "messages"
        };
        analysis[key].as_array_mut().expect("messages array").pop();
        assert_eq!(
            calls[0], analysis,
            "serialized prefix/config changed: {protocol:?}"
        );
    }
    server.abort();
}

#[tokio::test]
async fn generation_does_not_follow_redirects_or_resend_context() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let received = Arc::new(AtomicUsize::new(0));
    let count = received.clone();
    let app = Router::new()
        .route(
            "/chat/completions",
            post(|| async {
                (
                    axum::http::StatusCode::TEMPORARY_REDIRECT,
                    [("location", "/unconfigured-endpoint")],
                )
            }),
        )
        .route(
            "/unconfigured-endpoint",
            post(move || {
                let count = count.clone();
                async move {
                    count.fetch_add(1, Ordering::SeqCst);
                    "unexpected request"
                }
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let model = connect(
        &Connection {
            protocol: Protocol::OpenaiChat,
            endpoint: format!("http://{address}"),
            api_version: None,
        },
        "test-model",
        "fake",
    )
    .unwrap();
    let (_cancel, rx) = watch::channel(false);
    assert!(
        collect(
            model.as_ref(),
            builder().build(&[entry()], None).unwrap(),
            rx,
            Duration::from_secs(2),
            |_| {}
        )
        .await
        .is_err()
    );
    assert_eq!(received.load(Ordering::SeqCst), 0);
    server.abort();
}

#[tokio::test]
async fn user_picture_is_serialized_as_vision_content_on_actual_http_request() {
    use rig_core::message::{ImageMediaType, UserContent};
    let calls = Arc::new(Mutex::new(Vec::new()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let app = Router::new()
        .route("/chat/completions", post(reply))
        .with_state(calls.clone());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let model = connect(
        &Connection {
            protocol: Protocol::OpenaiChat,
            endpoint: format!("http://{address}"),
            api_version: None,
        },
        "test-model",
        "local-test",
    )
    .unwrap();
    let mut picture = entry();
    picture.message = Message::User {
        content: vec![
            UserContent::text("inspect this"),
            UserContent::image_base64("aW1hZ2U=", Some(ImageMediaType::PNG), None),
        ],
    };
    let (_tx, rx) = watch::channel(false);
    collect(
        model.as_ref(),
        builder().build(&[picture], None).unwrap(),
        rx,
        Duration::from_secs(5),
        |_| {},
    )
    .await
    .unwrap();
    let calls = calls.lock().unwrap();
    let user = calls[0]["messages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["role"] == "user")
        .unwrap();
    assert_eq!(user["content"][0]["text"], "inspect this");
    assert_eq!(user["content"][1]["type"], "image_url");
    assert_eq!(
        user["content"][1]["image_url"]["url"],
        "data:image/png;base64,aW1hZ2U="
    );
    server.abort();
}
