use crate::Client;
use ai_terminal_protocol::local::{Operation, Reply, Request};
use ai_terminal_remote::{Channel, HostPair};
use anyhow::{Context, Result, bail};
use prost::Message;
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

pub fn spawn(state_dir: PathBuf, stop: Arc<AtomicBool>) {
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .max_blocking_threads(2)
            .enable_all()
            .build()
            .expect("remote runtime");
        runtime.block_on(async move {
            let mut running = std::collections::HashMap::new();
            while !stop.load(Ordering::Acquire) {
                if !state_dir.join("account-mode").exists()
                    && let Ok(entries) = std::fs::read_dir(state_dir.join("pairs"))
                {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if path.extension().is_none_or(|e| e != "json") {
                            continue;
                        }
                        if running.contains_key(&path) {
                            continue;
                        }
                        if let Ok(pair) = load(&path) {
                            let state = state_dir.clone();
                            let enabled = stop.clone();
                            let source = path.clone();
                            running.insert(
                                path,
                                tokio::spawn(async move {
                                    while !enabled.load(Ordering::Acquire) && source.exists() {
                                        if now() >= pair.expires_at {
                                            break;
                                        }
                                        if let Ok(local) = Client::connect(&state) {
                                            let _ = serve(&pair, local).await;
                                        }
                                        tokio::time::sleep(Duration::from_secs(1)).await;
                                    }
                                }),
                            );
                        }
                    }
                }
                running.retain(|path, task| {
                    if state_dir.join("account-mode").exists() || !path.exists() {
                        task.abort();
                        false
                    } else {
                        !task.is_finished()
                    }
                });
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
            for (_, task) in running {
                task.abort();
            }
        });
    });
}
fn load(path: &Path) -> Result<HostPair> {
    Ok(serde_json::from_slice(&std::fs::read(path)?)?)
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
async fn serve(pair: &HostPair, client: Client) -> Result<()> {
    let _lease = Lease(client.clone());
    run_channel(pair, client).await
}
pub(crate) struct Lease(pub(crate) Client);
impl Drop for Lease {
    fn drop(&mut self) {
        let client = self.0.clone();
        // Also runs when a task is aborted by local pair revocation.
        std::thread::spawn(move || {
            if let Ok(list) = client.call(Request::default()) {
                for s in list.sessions {
                    if s.controller == client.id {
                        let _ = client.call(Request {
                            session: s.id,
                            operation: Operation::Detach as i32,
                            ..Request::default()
                        });
                    }
                }
            }
        });
    }
}
async fn run_channel(pair: &HostPair, mut client: Client) -> Result<()> {
    client.device_scope = format!("pair/{}", pair.room);
    let channel = Channel::accept(pair).await?;
    serve_channel(channel, client, pair.read_only).await
}
pub(crate) async fn serve_channel(
    mut channel: Channel,
    client: Client,
    read_only: bool,
) -> Result<()> {
    let client = std::sync::Arc::new(client);
    loop {
        let bytes = channel.receive().await?;
        let mut request = Request::decode(bytes.as_slice()).context("invalid remote request")?;
        if request.operation == Operation::Streaming as i32 {
            anyhow::ensure!(request.text == "stream/2", "unsupported stream protocol");
            channel
                .send(
                    &Reply {
                        history: vec!["stream/2".into()],
                        ..Reply::default()
                    }
                    .encode_to_vec(),
                )
                .await?;
            return crate::stream::serve(channel, client, read_only).await;
        }
        request.token.clear();
        request.client = 0;
        let reply = match authorize(read_only, &request) {
            Ok(()) => {
                let local = client.clone();
                tokio::task::spawn_blocking(move || local.call(request)).await?
            }
            Err(e) => Err(e),
        }
        .unwrap_or_else(|e| Reply {
            error: e.to_string(),
            ..Reply::default()
        });
        channel.send(&reply.encode_to_vec()).await?;
    }
}
pub(crate) fn authorize(read_only: bool, request: &Request) -> Result<()> {
    let operation = Operation::try_from(request.operation)?;
    if matches!(
        operation,
        Operation::Shutdown
            | Operation::Account
            | Operation::Watch
            | Operation::Streaming
            | Operation::AssistantInput
            | Operation::AttachDesktop
            | Operation::ObserveTerminal
            | Operation::AgentAcquire
            | Operation::AgentWrite
            | Operation::AgentClose
            | Operation::AgentResize
            | Operation::AgentRelease
    ) {
        bail!("remote clients cannot stop the desktop Agent")
    }
    if operation == Operation::Agent {
        let value: serde_json::Value = serde_json::from_str(&request.text)
            .map_err(|_| anyhow::anyhow!("invalid_agent_request"))?;
        anyhow::ensure!(
            !["clean", "retention"].contains(&value["action"].as_str().unwrap_or("")),
            "history management is local-only"
        );
        if read_only {
            anyhow::ensure!(
                [
                    "list",
                    "global_list",
                    "state",
                    "context",
                    "history",
                    "record"
                ]
                .contains(&value["action"].as_str().unwrap_or(""))
                    && value["allow_input"] != true,
                "paired device has read-only permission"
            );
        }
        return Ok(());
    }
    if operation == Operation::Configuration {
        let command: crate::config::Command = serde_json::from_str(&request.text)
            .map_err(|_| anyhow::anyhow!("invalid_configuration_request"))?;
        anyhow::ensure!(
            !matches!(command, crate::config::Command::Reload),
            "configuration reload is local-only"
        );
        if read_only {
            anyhow::ensure!(
                matches!(
                    command,
                    crate::config::Command::Show
                        | crate::config::Command::Validate
                        | crate::config::Command::Discover { .. }
                ),
                "paired device cannot manage configuration"
            );
        }
        return Ok(());
    }
    if read_only
        && !matches!(
            operation,
            Operation::List
                | Operation::Poll
                | Operation::Subscribe
                | Operation::History
                | Operation::Scrollback
                | Operation::ReleaseScrollback
                | Operation::Detach
        )
    {
        bail!("paired device has read-only permission")
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn image_uploads_and_sends_respect_paired_read_only_permissions() {
        for action in [
            "list",
            "global_list",
            "state",
            "context",
            "history",
            "record",
        ] {
            let request = Request {
                operation: Operation::Agent as i32,
                text: serde_json::json!({"version":1,"action":action}).to_string(),
                ..Default::default()
            };
            assert!(authorize(true, &request).is_ok());
        }
        for action in [
            "global_create",
            "send",
            "cancel",
            "image_begin",
            "image_chunk",
            "image_release",
        ] {
            let request = Request {
                operation: Operation::Agent as i32,
                text: serde_json::json!({"version":1,"action":action}).to_string(),
                ..Default::default()
            };
            assert!(authorize(true, &request).is_err());
            assert!(authorize(false, &request).is_ok());
        }
    }
    #[test]
    fn configuration_read_and_write_permissions_are_enforced() {
        let request = |text: &str| Request {
            operation: Operation::Configuration as i32,
            text: text.into(),
            ..Default::default()
        };
        assert!(authorize(true, &request(r#"{"action":"show"}"#)).is_ok());
        let replace = request(
            r#"{"action":"replace","expected_revision":0,"config":{"providers":{},"models":{},"bindings":{}}}"#,
        );
        assert!(authorize(true, &replace).is_err());
        assert!(authorize(false, &replace).is_ok());
        assert!(authorize(false, &request(r#"{"action":"reload"}"#)).is_err());
    }
    #[test]
    fn observe_permission_is_enforced_outside_the_ui() {
        for operation in [Operation::Scrollback, Operation::ReleaseScrollback] {
            assert!(
                authorize(
                    false,
                    &Request {
                        operation: operation as i32,
                        ..Request::default()
                    }
                )
                .is_ok()
            );
            assert!(
                authorize(
                    true,
                    &Request {
                        operation: operation as i32,
                        ..Request::default()
                    }
                )
                .is_ok()
            );
        }
        assert!(
            authorize(
                false,
                &Request {
                    operation: Operation::AssistantInput as i32,
                    ..Request::default()
                }
            )
            .is_err()
        );
        assert!(
            authorize(
                true,
                &Request {
                    operation: Operation::Poll as i32,
                    ..Request::default()
                }
            )
            .is_ok()
        );
        for operation in [
            Operation::Input,
            Operation::Acquire,
            Operation::AttachDesktop,
            Operation::Create,
            Operation::Resize,
            Operation::Close,
        ] {
            assert!(
                authorize(
                    true,
                    &Request {
                        operation: operation as i32,
                        ..Request::default()
                    }
                )
                .is_err()
            );
        }
        assert!(
            authorize(
                false,
                &Request {
                    operation: Operation::Shutdown as i32,
                    ..Request::default()
                }
            )
            .is_err()
        );
    }
}
