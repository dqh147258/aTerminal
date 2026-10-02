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
}
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
            let op = Operation::try_from(req.operation)?;
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
            cache.push_back(Cached {
                id,
                signature,
                reply,
            });
            if cache.len() > 64 {
                cache.pop_front();
            }
        }
        if history.as_ref().is_some_and(|(_, _, h)| h.is_finished()) {
            let (id, signature, h) = history.take().unwrap();
            let reply = h.await??;
            channel.stream_reply(id, &reply).await?;
            cache.push_back(Cached {
                id,
                signature,
                reply,
            });
            if cache.len() > 64 {
                cache.pop_front();
            }
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
