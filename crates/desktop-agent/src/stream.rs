//! Stream/2: bounded request replay ledger plus acknowledged state subscriptions.
use crate::{Client, remote_bridge::authorize};
use ai_terminal_protocol::local::{Operation, Reply, Request};
use ai_terminal_remote::{Channel, PathKind, StreamEvent};
use anyhow::{Context, Result, bail, ensure};
use prost::Message;
use std::{
    collections::{BTreeMap, VecDeque},
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::task::JoinHandle;
struct Cached {
    id: u64,
    signature: [u8; 32],
    reply: Reply,
    screen_frame: bool,
}
type ScreenJob = (u64, [u8; 32], bool, JoinHandle<Result<Reply>>);
struct Subscription {
    client: Arc<Client>,
    request: Request,
    worker: Option<JoinHandle<Result<Reply>>>,
    pending: VecDeque<(u64, Reply, Instant, bool)>,
    ended: bool,
}
impl Drop for Subscription {
    fn drop(&mut self) {
        if let Some(task) = self.worker.take() {
            task.abort()
        }
    }
}
pub(crate) async fn serve(
    mut channel: Channel,
    client: Arc<Client>,
    read_only: bool,
) -> Result<()> {
    let mut next = 1u64;
    let mut cache = VecDeque::<Cached>::new();
    let mut queued = BTreeMap::<u64, Vec<u8>>::new();
    let mut subscription: Option<Subscription> = None;
    let mut update_id = 0u64;
    let mut screen: Option<ScreenJob> = None;
    let mut history: Option<(u64, [u8; 32], JoinHandle<Result<Reply>>)> = None;
    loop {
        if let Some(event) = channel.stream_next(Duration::from_millis(2)).await? {
            match event {
                StreamEvent::Request(id, bytes) => {
                    ensure!(bytes.len() <= 65536, "oversized streaming request");
                    let signature = *blake3::hash(&bytes).as_bytes();
                    if let Some(c) = cache.iter().find(|c| c.id == id) {
                        ensure!(c.signature == signature, "conflicting request retry");
                        channel.stream_reply(id, &c.reply).await?;
                        continue;
                    }
                    if let Some((pending, sig, _, _)) = &screen
                        && *pending == id
                    {
                        ensure!(*sig == signature, "conflicting screen retry");
                        continue;
                    }
                    if let Some((pending, sig, _)) = &history
                        && *pending == id
                    {
                        ensure!(*sig == signature, "conflicting history retry");
                        continue;
                    }
                    ensure!(
                        id >= next && id < next + 64,
                        "expired request retry or sequence gap"
                    );
                    if let Some(existing) = queued.get(&id) {
                        ensure!(existing == &bytes, "conflicting queued retry")
                    }
                    queued.insert(id, bytes);
                }
                StreamEvent::StateAck(id) => {
                    if let Some(s) = subscription.as_mut() {
                        ensure!(id <= update_id, "future state acknowledgement");
                        while s.pending.front().is_some_and(|p| p.0 <= id) {
                            s.pending.pop_front();
                        }
                    }
                }
                _ => bail!("invalid desktop stream event"),
            }
        }
        // Process a bounded batch, leaving time for liveness/revocation and state acknowledgements.
        for _ in 0..16 {
            let Some(bytes) = queued.remove(&next) else {
                break;
            };
            let id = next;
            next = next.checked_add(1).context("sequence exhausted")?;
            let signature = *blake3::hash(&bytes).as_bytes();
            let mut req = Request::decode(bytes.as_slice())?;
            req.token.clear();
            req.client = 0;
            let op = match Operation::try_from(req.operation) {
                Ok(op) => op,
                Err(_) => {
                    let reply = Reply {
                        error: "unsupported_operation: upgrade the Desktop".into(),
                        ..Default::default()
                    };
                    channel.stream_reply(id, &reply).await?;
                    record_reply(
                        &mut cache,
                        Cached {
                            id,
                            signature,
                            reply,
                            screen_frame: false,
                        },
                    );
                    continue;
                }
            };
            let allowed = authorize(read_only, &req);
            let mut reply = if let Err(e) = allowed {
                Reply {
                    error: e.to_string(),
                    ..Reply::default()
                }
            } else if op == Operation::Subscribe {
                req.operation = Operation::Poll as i32;
                let mut result = call(client.clone(), req.clone()).await?;
                result.state_sequence = update_id;
                if result.error.is_empty() {
                    let info = result.info.as_ref().context("missing subscription info")?;
                    req.session_epoch = info.epoch;
                    req.control_epoch = info.control_epoch;
                    req.availability_epoch = info.availability_epoch;
                    req.revision = result
                        .snapshot
                        .as_ref()
                        .map_or(req.revision, |s| s.revision);
                    subscription = Some(Subscription {
                        client: Arc::new((*client).clone()),
                        request: req,
                        worker: None,
                        pending: VecDeque::new(),
                        ended: false,
                    });
                }
                result
            } else if op == Operation::Close {
                let closes_subscription = subscription
                    .as_ref()
                    .is_some_and(|s| s.request.session == req.session);
                let mut result = call(client.clone(), req).await?;
                if result.error.is_empty() && closes_subscription {
                    // Closing one terminal must not leave a Watch worker polling a removed actor.
                    // Fence updates already sent before the close acknowledgement.
                    subscription = None;
                    result.state_sequence = update_id;
                }
                result
            } else if matches!(op, Operation::RemoteScreens | Operation::RemoteScreenFrame) {
                if screen.is_some() {
                    Reply {
                        error: "screen_capture_busy: another screen request is pending".into(),
                        ..Default::default()
                    }
                } else {
                    let local = Arc::new((*client).clone());
                    screen = Some((
                        id,
                        signature,
                        op == Operation::RemoteScreenFrame,
                        tokio::spawn(call(local, req)),
                    ));
                    continue;
                }
            } else if matches!(op, Operation::History | Operation::Scrollback) {
                if history.is_some() {
                    Reply {
                        error: "history request already pending".into(),
                        ..Reply::default()
                    }
                } else {
                    let local = Arc::new((*client).clone());
                    history = Some((id, signature, tokio::spawn(call(local, req))));
                    continue;
                }
            } else {
                call(client.clone(), req).await?
            };
            if op == Operation::Agent {
                crate::remote_bridge::annotate_agent_permissions(&mut reply, read_only);
            }
            channel.stream_reply(id, &reply).await?;
            record_reply(
                &mut cache,
                Cached {
                    id,
                    signature,
                    reply,
                    screen_frame: false,
                },
            );
        }
        if screen
            .as_ref()
            .is_some_and(|(_, _, _, task)| task.is_finished())
        {
            let (id, signature, screen_frame, task) = screen.take().unwrap();
            let reply = task.await??;
            channel.stream_reply(id, &reply).await?;
            record_reply(
                &mut cache,
                Cached {
                    id,
                    signature,
                    reply,
                    screen_frame,
                },
            );
        }
        if history.as_ref().is_some_and(|(_, _, h)| h.is_finished()) {
            let (id, signature, h) = history.take().unwrap();
            let reply = h.await??;
            channel.stream_reply(id, &reply).await?;
            record_reply(
                &mut cache,
                Cached {
                    id,
                    signature,
                    reply,
                    screen_frame: false,
                },
            );
        }
        ensure!(
            cache.iter().map(|c| c.reply.encoded_len()).sum::<usize>() <= 8 * 1024 * 1024,
            "response replay cache budget exceeded"
        );
        if let Some(s) = subscription.as_mut() {
            if s.pending.iter().any(|(_, _, sent, retried)| {
                !*retried && sent.elapsed() > Duration::from_millis(500)
            }) {
                for (id, reply, sent, retried) in &mut s.pending {
                    channel.stream_update(*id, reply, true).await?;
                    *sent = Instant::now();
                    *retried = true;
                }
            }
            ensure!(
                !s.pending.iter().any(
                    |(_, _, sent, retried)| *retried && sent.elapsed() > Duration::from_secs(5)
                ),
                "state acknowledgement timeout"
            );
            // Keep enough display updates in flight to cover a high RTT without waiting per frame.
            if s.pending.len() < 32
                && s.pending.iter().map(|p| p.1.encoded_len()).sum::<usize>() < 4 * 1024 * 1024
            {
                if s.worker.as_ref().is_some_and(|w| w.is_finished()) {
                    let reply = s.worker.take().unwrap().await??;
                    if reply.snapshot.is_some()
                        || reply.delta.is_some()
                        || !reply.error.is_empty()
                        || reply.info.as_ref().is_some_and(|i| {
                            i.control_epoch != s.request.control_epoch
                                || i.availability_epoch != s.request.availability_epoch
                                || i.exited
                        })
                    {
                        s.ended = !reply.error.is_empty()
                            || reply.info.as_ref().is_some_and(|info| info.exited);
                        update_id += 1;
                        channel.stream_update(update_id, &reply, false).await?;
                        s.request.revision = reply
                            .snapshot
                            .as_ref()
                            .map(|v| v.revision)
                            .or_else(|| reply.delta.as_ref().map(|v| v.revision))
                            .unwrap_or(s.request.revision);
                        if let Some(info) = &reply.info {
                            s.request.control_epoch = info.control_epoch;
                            s.request.availability_epoch = info.availability_epoch;
                        }
                        s.pending.push_back((
                            update_id,
                            reply,
                            Instant::now(),
                            channel.path() == PathKind::Relay,
                        ));
                    }
                }
                if s.worker.is_none() && !s.ended {
                    let local = s.client.clone();
                    let mut req = s.request.clone();
                    req.operation = Operation::Watch as i32;
                    s.worker = Some(tokio::spawn(call(local, req)));
                }
            }
        }
    }
}
// Preserve the replay signature/window without retaining 64 base64 images.
// A late retry of an evicted read-only frame asks the viewer to refresh.
fn record_reply(cache: &mut VecDeque<Cached>, reply: Cached) {
    cache.push_back(reply);
    if cache.len() > 64 {
        cache.pop_front();
    }
    let mut bytes: usize = cache
        .iter()
        .filter(|c| c.screen_frame)
        .map(|c| c.reply.encoded_len())
        .sum();
    for old in cache.iter_mut().filter(|c| c.screen_frame) {
        if bytes <= 512 * 1024 {
            break;
        }
        bytes -= old.reply.encoded_len();
        old.reply = Reply {
            error: "screen_frame_expired: refresh the screen".into(),
            ..Default::default()
        };
        bytes += old.reply.encoded_len();
    }
}
async fn call(client: Arc<Client>, req: Request) -> Result<Reply> {
    tokio::task::spawn_blocking(move || {
        client.call(req).unwrap_or_else(|e| Reply {
            error: e.to_string(),
            ..Reply::default()
        })
    })
    .await
    .map_err(Into::into)
}

#[cfg(test)]
mod screen_tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    #[test]
    fn screen_replay_retains_signatures_without_accumulating_images() {
        let mut cache = VecDeque::new();
        for id in 1..=64 {
            record_reply(
                &mut cache,
                Cached {
                    id,
                    signature: [id as u8; 32],
                    screen_frame: true,
                    reply: Reply {
                        history: vec!["A".repeat(164 * 1024)],
                        ..Default::default()
                    },
                },
            );
        }
        assert_eq!(cache.len(), 64);
        assert!(cache.iter().map(|c| c.reply.encoded_len()).sum::<usize>() <= 512 * 1024);
        assert_eq!(cache.front().unwrap().signature, [1; 32]);
        assert!(
            cache
                .front()
                .unwrap()
                .reply
                .error
                .contains("screen_frame_expired")
        );
        assert!(cache.back().unwrap().reply.error.is_empty());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn encrypted_screen_rpc_is_independent_of_terminal_and_history_and_unknown_ops() {
        use ai_terminal_protocol::local::{read_message, write_message};
        let dir = tempfile::tempdir().unwrap();
        let state = dir.path().join("desktop");
        crate::service::secure_dir(&state).unwrap();
        let local = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        local.set_nonblocking(true).unwrap();
        let token = "fixture-local-token";
        std::fs::write(
            state.join("endpoint.json"),
            serde_json::json!({"address":local.local_addr().unwrap(),"token":token}).to_string(),
        )
        .unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let release = Arc::new(AtomicBool::new(false));
        let started = Arc::new(AtomicUsize::new(0));
        let fake_stop = stop.clone();
        let fake_release = release.clone();
        let fake_started = started.clone();
        let fixture = std::thread::spawn(move || {
            let mut workers = vec![];
            while !fake_stop.load(Ordering::Acquire) {
                let (mut socket, _) = match local.accept() {
                    Ok(value) => value,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(2));
                        continue;
                    }
                    Err(error) => panic!("fixture accept failed: {error}"),
                };
                let release = fake_release.clone();
                let started = fake_started.clone();
                let stop = fake_stop.clone();
                workers.push(std::thread::spawn(move || {
                    socket.set_nonblocking(false).unwrap();
                    socket
                        .set_read_timeout(Some(Duration::from_secs(10)))
                        .unwrap();
                    while let Ok(request) = read_message::<_, Request>(&mut socket) {
                        assert_eq!(request.token, token);
                        let reply = if request.operation == Operation::RemoteScreenFrame as i32 {
                            started.fetch_add(1, Ordering::AcqRel);
                            while !release.load(Ordering::Acquire) && !stop.load(Ordering::Acquire)
                            {
                                std::thread::sleep(Duration::from_millis(2));
                            }
                            Reply {
                                history: vec!["frame".into()],
                                ..Default::default()
                            }
                        } else if request.operation == Operation::History as i32 {
                            Reply {
                                history: vec!["terminal history".into()],
                                ..Default::default()
                            }
                        } else {
                            Reply {
                                screen_protocol_version: 1,
                                ..Default::default()
                            }
                        };
                        if write_message(&mut socket, &reply).is_err() {
                            break;
                        }
                    }
                }));
            }
            for worker in workers {
                worker.join().unwrap();
            }
        });
        let local = Client::connect(&state).unwrap();
        let admin = ai_terminal_security::random_secret().unwrap();
        let router = ai_terminal_server::router(&dir.path().join("relay.db"), &admin).unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let relay = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        let (mut pair, mut invitation) = ai_terminal_remote::create_pair(&url, &admin, true)
            .await
            .unwrap();
        pair.ice_servers.clear();
        invitation.ice_servers.clear();
        let desktop = tokio::spawn(async move {
            let channel = Channel::accept(&pair).await.unwrap();
            crate::remote_bridge::serve_channel(channel, local, true).await
        });
        let mut mobile = Channel::connect(&invitation).await.unwrap();
        mobile.disable_direct();
        mobile.negotiate_stream().await.unwrap();
        let frame = Request {
            operation: Operation::RemoteScreenFrame as i32,
            screen_id: "native:1".into(),
            screen_max_width: 100,
            ..Default::default()
        };
        mobile.stream_request(1, &frame, false).await.unwrap();
        tokio::time::timeout(Duration::from_secs(1), async {
            while started.load(Ordering::Acquire) == 0 {
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
        })
        .await
        .unwrap();
        mobile.stream_request(1, &frame, true).await.unwrap(); // in-flight retry must not recapture
        mobile
            .stream_request(2, &Request::default(), false)
            .await
            .unwrap();
        mobile
            .stream_request(
                3,
                &Request {
                    operation: Operation::History as i32,
                    ..Default::default()
                },
                false,
            )
            .await
            .unwrap();
        mobile.stream_request(4, &frame, false).await.unwrap();
        mobile
            .stream_request(
                5,
                &Request {
                    operation: 999,
                    ..Default::default()
                },
                false,
            )
            .await
            .unwrap();
        mobile
            .stream_request(
                6,
                &Request {
                    operation: Operation::Input as i32,
                    ..Default::default()
                },
                false,
            )
            .await
            .unwrap();
        let mut replies = BTreeMap::new();
        tokio::time::timeout(Duration::from_millis(800), async {
            while replies.len() < 5 {
                if let Some(StreamEvent::Reply(id, reply)) =
                    mobile.stream_next(Duration::from_millis(10)).await.unwrap()
                {
                    assert_ne!(id, 1, "capture completed before test released it");
                    replies.insert(id, reply);
                }
            }
        })
        .await
        .expect("terminal/history RPC waited for screen capture");
        assert!(
            replies[&2].error.is_empty(),
            "list error: {}",
            replies[&2].error
        );
        assert_eq!(replies[&2].screen_protocol_version, 1);
        assert_eq!(replies[&3].history, ["terminal history"]);
        assert!(replies[&4].error.contains("screen_capture_busy"));
        assert!(replies[&5].error.contains("unsupported_operation"));
        assert!(replies[&6].error.contains("read-only permission"));
        assert_eq!(started.load(Ordering::Acquire), 1);
        release.store(true, Ordering::Release);
        let frame_reply = tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if let Some(StreamEvent::Reply(1, reply)) =
                    mobile.stream_next(Duration::from_millis(10)).await.unwrap()
                {
                    break reply;
                }
            }
        })
        .await
        .unwrap();
        assert_eq!(frame_reply.history, ["frame"]);
        mobile.stream_request(1, &frame, true).await.unwrap();
        let replay = tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if let Some(StreamEvent::Reply(1, reply)) =
                    mobile.stream_next(Duration::from_millis(10)).await.unwrap()
                {
                    break reply;
                }
            }
        })
        .await
        .unwrap();
        assert_eq!(replay, frame_reply);
        assert_eq!(started.load(Ordering::Acquire), 1);
        stop.store(true, Ordering::Release);
        desktop.abort();
        relay.abort();
        drop(mobile);
        tokio::task::spawn_blocking(move || fixture.join().unwrap())
            .await
            .unwrap();
    }
}
