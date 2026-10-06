//! WSS remains authenticated control/liveness; DTLS fingerprints travel inside Noise.
//! Requests have path-independent IDs; a lost direct reply is replayed, never re-executed.
use super::{HostPair, Socket, binary, ready, socket, wait_for_peer};
use ai_terminal_protocol::local::{Reply, Request};
use ai_terminal_security::{Cipher, Invitation};
#[cfg(feature = "webrtc")]
use ai_terminal_transport::{Event, Peer};
use anyhow::{Context, Result, bail};
use futures_util::{FutureExt, SinkExt, StreamExt};
use prost::Message as _;
use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};
use tokio_tungstenite::tungstenite::Message;

#[derive(Clone, PartialEq, prost::Message)]
struct Envelope {
    #[prost(enumeration = "Kind", tag = "1")]
    kind: i32,
    #[prost(uint64, tag = "2")]
    id: u64,
    #[prost(bytes = "vec", tag = "3")]
    body: Vec<u8>,
}
#[derive(Clone, Copy, PartialEq, Eq, Debug, prost::Enumeration)]
#[repr(i32)]
enum Kind {
    Request = 0,
    Reply = 1,
    Signal = 2,
    Ping = 3,
    Pong = 4,
    StreamRequest = 5,
    StreamReply = 6,
    Update = 7,
    StateAck = 8,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathKind {
    Relay,
    Direct,
}
struct Cached {
    id: u64,
    request: Vec<u8>,
    reply: Envelope,
}

pub enum StreamEvent {
    Request(u64, Vec<u8>),
    Reply(u64, Reply),
    Update(u64, Reply),
    StateAck(u64),
}
pub struct Channel {
    socket: Socket,
    cipher: Cipher,
    #[cfg(feature = "webrtc")]
    peer: Option<Peer>,
    initiator: bool,
    next_id: u64,
    pending: Option<(u64, Vec<u8>, PathKind)>,
    cached: Option<Cached>,
    path: PathKind,
    cooldown: Option<Instant>,
    direct_since: Option<Instant>,
    probe_id: u64,
    probes: VecDeque<(u64, Instant)>,
    last_probe: Instant,
    direct_rtt: Option<Duration>,
    relay_rtt: Option<Duration>,
    direct_ack: Option<Instant>,
    relay_ack: Instant,
    last_switch: Instant,
    direct_samples: VecDeque<Duration>,
    relay_samples: VecDeque<Duration>,
    bad_since: Option<Instant>,
}
impl Channel {
    pub async fn accept(pair: &HostPair) -> Result<Self> {
        let mut socket = socket(
            &pair.server,
            &pair.room,
            "desktop",
            &pair.relay_token,
            pair.server_ca_pem.as_deref(),
        )
        .await?;
        // Legacy pair listeners stay available until the mobile comes online.
        ready(&mut socket).await?;
        let mut noise = pair.identity.responder()?;
        let hello = tokio::time::timeout(Duration::from_secs(10), binary(&mut socket)).await??;
        noise.read_message(&hello, &mut [0; 256])?;
        let mut response = [0; 256];
        let n = noise.write_message(&[], &mut response)?;
        socket
            .send(Message::Binary(response[..n].to_vec().into()))
            .await?;
        Self::new(socket, Cipher::new(noise)?, false, &pair.ice_servers)
    }
    pub async fn connect(invitation: &Invitation) -> Result<Self> {
        let mut socket = socket(
            &invitation.server,
            &invitation.room,
            "mobile",
            &invitation.relay_token,
            invitation.server_ca_pem.as_deref(),
        )
        .await?;
        wait_for_peer(&mut socket).await?;
        let mut noise = invitation.initiator()?;
        let mut hello = [0; 256];
        let n = noise.write_message(&[], &mut hello)?;
        socket
            .send(Message::Binary(hello[..n].to_vec().into()))
            .await?;
        let response = tokio::time::timeout(Duration::from_secs(10), binary(&mut socket)).await??;
        noise.read_message(&response, &mut [0; 256])?;
        Self::new(socket, Cipher::new(noise)?, true, &invitation.ice_servers)
    }
    pub async fn account(
        session: &crate::account::AccountSession,
        grant: &ai_terminal_security::account::ConnectionGrant,
        initiator: bool,
        ice: &[String],
    ) -> Result<Self> {
        use ai_terminal_security::encode;
        let role = if initiator { "mobile" } else { "desktop" };
        let own_id = if initiator {
            &grant.mobile_id
        } else {
            &grant.desktop_id
        };
        anyhow::ensure!(
            grant.version == 2 && own_id == &session.tokens.device_id,
            "invalid grant identity"
        );
        let mut socket = super::socket_version(
            &session.server,
            &grant.room,
            role,
            &session.tokens.access_token,
            session.ca.as_deref(),
            2,
        )
        .await?;
        wait_for_peer(&mut socket).await?;
        let mut noise = session.identity.handshake(grant, initiator)?;
        let mut message = [0u8; 256];
        if initiator {
            let n = noise.write_message(&[], &mut message)?;
            socket
                .send(Message::Binary(message[..n].to_vec().into()))
                .await?;
            let reply =
                tokio::time::timeout(Duration::from_secs(10), binary(&mut socket)).await??;
            noise.read_message(&reply, &mut message)?;
        } else {
            let hello =
                tokio::time::timeout(Duration::from_secs(10), binary(&mut socket)).await??;
            noise.read_message(&hello, &mut message)?;
            anyhow::ensure!(
                noise
                    .get_remote_static()
                    .is_some_and(|key| encode(key) == grant.mobile_public),
                "unexpected mobile identity"
            );
            let n = noise.write_message(&[], &mut message)?;
            socket
                .send(Message::Binary(message[..n].to_vec().into()))
                .await?;
        }
        Self::new(socket, Cipher::new(noise)?, initiator, ice)
    }
    fn new(socket: Socket, cipher: Cipher, initiator: bool, _ice: &[String]) -> Result<Self> {
        let now = Instant::now();
        Ok(Self {
            socket,
            cipher,
            #[cfg(feature = "webrtc")]
            peer: Peer::new(initiator, _ice).ok(),
            initiator,
            next_id: 1,
            pending: None,
            cached: None,
            path: PathKind::Relay,
            cooldown: None,
            direct_since: None,
            probe_id: 0,
            probes: VecDeque::new(),
            last_probe: now - Duration::from_secs(2),
            direct_rtt: None,
            relay_rtt: None,
            direct_ack: None,
            relay_ack: now,
            last_switch: now,
            direct_samples: VecDeque::new(),
            relay_samples: VecDeque::new(),
            bad_since: None,
        })
    }
    pub fn path(&self) -> PathKind {
        self.path
    }
    /// Also used by a user's relay-only preference and deterministic fallback tests.
    pub fn disable_direct(&mut self) {
        #[cfg(feature = "webrtc")]
        {
            self.peer = None;
        }
        self.path = PathKind::Relay;
        self.direct_since = None;
        self.direct_ack = None;
    }
    fn direct_open(&self) -> bool {
        #[cfg(feature = "webrtc")]
        {
            self.peer.as_ref().is_some_and(Peer::is_open)
        }
        #[cfg(not(feature = "webrtc"))]
        {
            false
        }
    }
    async fn transmit(&mut self, envelope: &Envelope, path: PathKind) -> Result<PathKind> {
        let bytes = envelope.encode_to_vec();
        #[cfg(feature = "webrtc")]
        if path == PathKind::Direct
            && let Some(peer) = self.peer.as_mut()
            && peer.send(&bytes).is_ok()
        {
            return Ok(PathKind::Direct);
        }
        let _ = path;
        for record in self.cipher.encrypt(&bytes)? {
            tokio::time::timeout(
                Duration::from_secs(3),
                self.socket.send(Message::Binary(record.into())),
            )
            .await??;
        }
        Ok(PathKind::Relay)
    }
    async fn maintenance(&mut self) -> Result<()> {
        if !self.initiator {
            return Ok(());
        }
        let now = Instant::now();
        if now.duration_since(self.last_probe) > Duration::from_secs(5) {
            // No network pump runs while the UI is idle. Resume on the authenticated
            // relay first, and require fresh probes before reusing an old direct path.
            self.relay_ack = now;
            self.direct_ack = None;
            self.path = PathKind::Relay;
        }
        if now.duration_since(self.last_probe) >= Duration::from_secs(1) {
            self.probe_id += 1;
            self.probes.push_back((self.probe_id, now));
            if self.probes.len() > 16 {
                self.probes.pop_front();
            }
            self.last_probe = now;
            let ping = Envelope {
                kind: Kind::Ping as i32,
                id: self.probe_id,
                body: Vec::new(),
            };
            self.transmit(&ping, PathKind::Relay).await?;
            if self.direct_open() {
                let _ = self.transmit(&ping, PathKind::Direct).await?;
            }
        }
        if now.duration_since(self.relay_ack) > Duration::from_secs(10) {
            bail!("relay control lease expired; reconnect to revalidate authorization")
        }
        if self.path == PathKind::Direct
            && self
                .direct_ack
                .is_none_or(|at| now.duration_since(at) > Duration::from_secs(3))
        {
            self.fallback();
        }
        let cooled = self.cooldown.is_none_or(|until| now >= until);
        if self.direct_open()
            && cooled
            && let (Some(direct), Some(relay)) = (self.direct_rtt, self.relay_rtt)
        {
            let initial = self.cooldown.is_none();
            let advantage = relay > direct + Duration::from_millis(50)
                && direct.as_secs_f64() < relay.as_secs_f64() * 0.7;
            if self.path == PathKind::Relay
                && (initial || advantage)
                && self
                    .direct_ack
                    .is_some_and(|at| now.duration_since(at) < Duration::from_secs(2))
            {
                self.path = PathKind::Direct;
                self.last_switch = now;
            }
            if self.path == PathKind::Direct
                && direct > Duration::from_millis(250)
                && direct > relay + Duration::from_millis(50)
                && relay.as_secs_f64() < direct.as_secs_f64() * 0.7
            {
                let since = *self.bad_since.get_or_insert(now);
                if now.duration_since(since) > Duration::from_secs(3) {
                    self.fallback();
                }
            } else {
                self.bad_since = None;
            }
        }
        Ok(())
    }
    fn fallback(&mut self) {
        self.path = PathKind::Relay;
        self.cooldown = Some(Instant::now() + Duration::from_secs(10));
        self.last_switch = Instant::now();
    }
    async fn next(&mut self) -> Result<(Envelope, PathKind)> {
        self.next_until(None)
            .await?
            .context("unexpected receive deadline")
    }
    async fn next_until(
        &mut self,
        deadline: Option<tokio::time::Instant>,
    ) -> Result<Option<(Envelope, PathKind)>> {
        loop {
            if deadline.is_some_and(|at| tokio::time::Instant::now() >= at) {
                return Ok(None);
            }
            self.maintenance().await?;
            // A busy direct stream must never starve relay revocation/expiry messages.
            if let Some(message) = self.socket.next().now_or_never() {
                let message = message.context("relay disconnected")??;
                if let Some(envelope) = self.relay_message(message).await? {
                    return Ok(Some((envelope, PathKind::Relay)));
                }
                continue;
            }
            #[cfg(feature = "webrtc")]
            if let Some(peer) = self.peer.as_mut() {
                match peer.poll() {
                    Ok(Some(Event::Signal(signal))) => {
                        self.transmit(
                            &Envelope {
                                kind: Kind::Signal as i32,
                                id: 0,
                                body: signal,
                            },
                            PathKind::Relay,
                        )
                        .await?;
                        continue;
                    }
                    Ok(Some(Event::Open)) => {
                        self.direct_since = Some(Instant::now());
                        continue;
                    }
                    Ok(Some(Event::Closed)) | Err(_) => {
                        self.disable_direct();
                    }
                    Ok(Some(Event::Message(bytes))) => {
                        let envelope = Envelope::decode(bytes.as_slice())?;
                        if let Some(e) = self.process(envelope, PathKind::Direct).await? {
                            return Ok(Some((e, PathKind::Direct)));
                        }
                        continue;
                    }
                    Ok(None) => {}
                }
            }
            // WebSocket StreamExt::next and the 5ms timer are cancellation-safe; partial
            // encrypted message reassembly lives in Cipher, not in the select future.
            tokio::select! {
                message=self.socket.next()=>{
                    if let Some(envelope)=self.relay_message(message.context("relay disconnected")??).await?{return Ok(Some((envelope,PathKind::Relay)))}
                },
                _=tokio::time::sleep(if deadline.is_some() { Duration::from_millis(1) } else { Duration::from_millis(5) })=>{},
            }
        }
    }
    async fn relay_message(&mut self, message: Message) -> Result<Option<Envelope>> {
        match message {
            Message::Binary(record) => {
                if let Some(bytes) = self.cipher.decrypt(&record)? {
                    return self
                        .process(Envelope::decode(bytes.as_slice())?, PathKind::Relay)
                        .await;
                }
            }
            Message::Ping(bytes) => {
                tokio::time::timeout(
                    Duration::from_secs(3),
                    self.socket.send(Message::Pong(bytes)),
                )
                .await??;
            }
            Message::Pong(_) => {}
            _ => bail!("relay control connection closed"),
        }
        Ok(None)
    }
    async fn process(&mut self, envelope: Envelope, path: PathKind) -> Result<Option<Envelope>> {
        match Kind::try_from(envelope.kind)? {
            Kind::Signal => {
                if path != PathKind::Relay {
                    bail!("signaling must use authenticated Noise channel")
                }
                #[cfg(feature = "webrtc")]
                if let Some(peer) = self.peer.as_mut()
                    && peer.apply_signal(&envelope.body).is_err()
                {
                    self.disable_direct();
                }
            }
            Kind::Ping => {
                self.transmit(
                    &Envelope {
                        kind: Kind::Pong as i32,
                        id: envelope.id,
                        body: Vec::new(),
                    },
                    path,
                )
                .await?;
            }
            Kind::Pong => {
                if let Some((_, started)) = self.probes.iter().find(|(id, _)| *id == envelope.id) {
                    let now = Instant::now();
                    let rtt = now.duration_since(*started);
                    if path == PathKind::Direct {
                        self.direct_rtt = Some(sample_p95(&mut self.direct_samples, rtt));
                        self.direct_ack = Some(now)
                    } else {
                        self.relay_rtt = Some(sample_p95(&mut self.relay_samples, rtt));
                        self.relay_ack = now
                    }
                }
            }
            _ => return Ok(Some(envelope)),
        }
        Ok(None)
    }
    /// Desktop: obtain each request exactly once within this live channel.
    pub async fn receive(&mut self) -> Result<Vec<u8>> {
        if self.initiator {
            bail!("client cannot receive requests")
        }
        loop {
            let (envelope, path) = self.next().await?;
            if Kind::try_from(envelope.kind)? != Kind::Request {
                bail!("unexpected response on desktop")
            }
            if let Some(cache) = &self.cached
                && envelope.id == cache.id
            {
                if envelope.body != cache.request {
                    bail!("conflicting request retry")
                }
                let reply = cache.reply.clone();
                self.transmit(&reply, path).await?;
                continue;
            }
            if envelope.id < self.next_id {
                continue;
            }
            if envelope.id != self.next_id || self.pending.is_some() {
                bail!("remote request sequence gap")
            }
            self.pending = Some((envelope.id, envelope.body.clone(), path));
            return Ok(envelope.body);
        }
    }
    /// Desktop: record the result before transport send, so retries cannot repeat work.
    pub async fn send(&mut self, bytes: &[u8]) -> Result<()> {
        let (id, request, path) = self.pending.take().context("no pending request")?;
        let reply = Envelope {
            kind: Kind::Reply as i32,
            id,
            body: bytes.to_vec(),
        };
        self.cached = Some(Cached {
            id,
            request,
            reply: reply.clone(),
        });
        self.next_id = id.checked_add(1).context("request sequence exhausted")?;
        self.transmit(&reply, path).await?;
        Ok(())
    }
    pub async fn negotiate_stream(&mut self) -> Result<()> {
        let reply = self
            .request(Request {
                operation: ai_terminal_protocol::local::Operation::Streaming as i32,
                text: "stream/2".into(),
                ..Request::default()
            })
            .await?;
        anyhow::ensure!(
            reply.error.is_empty() && reply.history == ["stream/2"],
            "desktop does not support stream/2; upgrade the desktop"
        );
        Ok(())
    }
    pub async fn stream_request(&mut self, id: u64, request: &Request, retry: bool) -> Result<()> {
        if retry {
            self.fallback();
        }
        self.transmit(
            &Envelope {
                kind: Kind::StreamRequest as i32,
                id,
                body: request.encode_to_vec(),
            },
            self.path,
        )
        .await?;
        Ok(())
    }
    pub async fn stream_reply(&mut self, id: u64, reply: &Reply) -> Result<()> {
        self.transmit(
            &Envelope {
                kind: Kind::StreamReply as i32,
                id,
                body: reply.encode_to_vec(),
            },
            self.path,
        )
        .await?;
        Ok(())
    }
    pub async fn stream_update(&mut self, id: u64, reply: &Reply, retry: bool) -> Result<()> {
        if retry {
            self.fallback();
        }
        self.transmit(
            &Envelope {
                kind: Kind::Update as i32,
                id,
                body: reply.encode_to_vec(),
            },
            self.path,
        )
        .await?;
        Ok(())
    }
    pub async fn state_ack(&mut self, id: u64) -> Result<()> {
        self.transmit(
            &Envelope {
                kind: Kind::StateAck as i32,
                id,
                body: Vec::new(),
            },
            self.path,
        )
        .await?;
        Ok(())
    }
    pub async fn stream_next(&mut self, wait: Duration) -> Result<Option<StreamEvent>> {
        let Some((event, path)) = self
            .next_until(Some(tokio::time::Instant::now() + wait))
            .await?
        else {
            return Ok(None);
        };
        // The responder follows the path used by the initiator; liveness remains on WSS.
        if !self.initiator {
            self.path = path;
        }
        Ok(Some(match Kind::try_from(event.kind)? {
            Kind::StreamRequest if !self.initiator => StreamEvent::Request(event.id, event.body),
            Kind::StreamReply if self.initiator => {
                StreamEvent::Reply(event.id, Reply::decode(event.body.as_slice())?)
            }
            Kind::Update if self.initiator => {
                StreamEvent::Update(event.id, Reply::decode(event.body.as_slice())?)
            }
            Kind::StateAck if !self.initiator => StreamEvent::StateAck(event.id),
            _ => bail!("unexpected streaming message"),
        }))
    }
    pub async fn request(&mut self, request: Request) -> Result<Reply> {
        if !self.initiator {
            bail!("desktop cannot originate requests")
        }
        self.maintenance().await?;
        let id = self.next_id;
        self.next_id = id.checked_add(1).context("request sequence exhausted")?;
        let envelope = Envelope {
            kind: Kind::Request as i32,
            id,
            body: request.encode_to_vec(),
        };
        let path = self.transmit(&envelope, self.path).await?;
        let mut retried = false;
        let mut deadline = tokio::time::Instant::now()
            + if path == PathKind::Direct {
                Duration::from_millis(500)
            } else {
                Duration::from_secs(5)
            };
        loop {
            // Do not cancel a Noise send halfway through a record at the retry deadline.
            match self.next_until(Some(deadline)).await {
                Ok(Some((reply, _))) => {
                    if Kind::try_from(reply.kind)? != Kind::Reply {
                        bail!("unexpected remote request")
                    }
                    if reply.id < id {
                        continue;
                    }
                    if reply.id != id {
                        bail!("unexpected response sequence")
                    }
                    return Ok(Reply::decode(reply.body.as_slice())?);
                }
                Err(e) => return Err(e),
                Ok(None) if path == PathKind::Direct && !retried => {
                    self.fallback();
                    retried = true;
                    self.transmit(&envelope, PathKind::Relay).await?;
                    deadline = tokio::time::Instant::now() + Duration::from_secs(5);
                }
                Ok(None) => {
                    bail!("remote request timed out; outcome unknown, reconnect without replaying")
                }
            }
        }
    }
}

fn sample_p95(samples: &mut VecDeque<Duration>, sample: Duration) -> Duration {
    samples.push_back(sample);
    if samples.len() > 20 {
        samples.pop_front();
    }
    let mut sorted = samples.iter().copied().collect::<Vec<_>>();
    sorted.sort_unstable();
    sorted[(sorted.len() * 95).div_ceil(100) - 1]
}

#[cfg(all(test, feature = "webrtc"))]
mod tests {
    use super::*;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn direct_timeout_retries_without_reexecuting_then_relay_stays_usable() {
        let dir = tempfile::tempdir().unwrap();
        let admin = ai_terminal_security::random_secret().unwrap();
        let router = ai_terminal_server::router(&dir.path().join("server.db"), &admin).unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        let (mut host, mut invitation) = crate::create_pair(&url, &admin, false).await.unwrap();
        host.ice_servers.clear();
        invitation.ice_servers.clear();
        let count = Arc::new(AtomicUsize::new(0));
        let executed = count.clone();
        let desktop = tokio::spawn(async move {
            let mut channel = Channel::accept(&host).await.unwrap();
            while let Ok(bytes) = channel.receive().await {
                let request = Request::decode(bytes.as_slice()).unwrap();
                if request.text == "slow" {
                    executed.fetch_add(1, Ordering::SeqCst);
                    tokio::time::sleep(Duration::from_millis(750)).await;
                }
                let reply = Reply {
                    accepted_input_seq: executed.load(Ordering::SeqCst) as u64,
                    ..Reply::default()
                };
                if channel.send(&reply.encode_to_vec()).await.is_err() {
                    break;
                }
            }
        });
        let mut client = Channel::connect(&invitation).await.unwrap();
        let deadline = Instant::now() + Duration::from_secs(12);
        while client.path() != PathKind::Direct {
            client.request(Request::default()).await.unwrap();
            assert!(
                Instant::now() < deadline,
                "direct connection never became usable"
            );
            tokio::time::sleep(Duration::from_millis(30)).await;
        }
        let reply = client
            .request(Request {
                text: "slow".into(),
                ..Request::default()
            })
            .await
            .unwrap();
        assert_eq!(reply.accepted_input_seq, 1);
        assert_eq!(client.path(), PathKind::Relay);
        client.request(Request::default()).await.unwrap();
        assert_eq!(count.load(Ordering::SeqCst), 1);
        client.disable_direct();
        client.request(Request::default()).await.unwrap();
        desktop.abort();
        server.abort();
    }
}
