use anyhow::{Result, bail};
use datachannel::{
    DataChannelHandler, DataChannelInfo, IceCandidate, PeerConnectionHandler, RtcConfig,
    RtcDataChannel, RtcPeerConnection, SessionDescription,
};
use serde::{Deserialize, Serialize};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver, SyncSender, TryRecvError},
};

const RECORD: usize = 16 * 1024;
const MAX_PENDING: usize = 256 * 1024;
#[derive(Serialize, Deserialize)]
enum Signal {
    Description(Box<SessionDescription>),
    Candidate(IceCandidate),
}
enum Internal {
    Signal(Signal),
    Channel(Box<RtcDataChannel<Handler>>),
    Open,
    Closed,
    Bytes(Vec<u8>),
}
pub enum Event {
    Signal(Vec<u8>),
    Open,
    Closed,
    Message(Vec<u8>),
}
#[derive(Clone)]
struct Sink {
    tx: SyncSender<Internal>,
    overflow: Arc<AtomicBool>,
}
impl Sink {
    fn push(&self, e: Internal) {
        if self.tx.try_send(e).is_err() {
            self.overflow.store(true, Ordering::Release);
        }
    }
}
struct Handler(Sink);
impl DataChannelHandler for Handler {
    fn on_open(&mut self) {
        self.0.push(Internal::Open)
    }
    fn on_closed(&mut self) {
        self.0.push(Internal::Closed)
    }
    fn on_error(&mut self, _: &str) {
        self.0.push(Internal::Closed)
    }
    fn on_message(&mut self, bytes: &[u8]) {
        if bytes.len() > RECORD {
            self.0.overflow.store(true, Ordering::Release);
            return;
        }
        self.0.push(Internal::Bytes(bytes.to_vec()));
    }
}
struct Connection(Sink);
impl PeerConnectionHandler for Connection {
    type DCH = Handler;
    fn data_channel_handler(&mut self, _: DataChannelInfo) -> Handler {
        Handler(self.0.clone())
    }
    fn on_description(&mut self, d: SessionDescription) {
        self.0
            .push(Internal::Signal(Signal::Description(Box::new(d))))
    }
    fn on_candidate(&mut self, c: IceCandidate) {
        self.0.push(Internal::Signal(Signal::Candidate(c)))
    }
    fn on_data_channel(&mut self, c: Box<RtcDataChannel<Handler>>) {
        self.0.push(Internal::Channel(c));
    }
}
pub struct Peer {
    channel: Option<Box<RtcDataChannel<Handler>>>,
    connection: Box<RtcPeerConnection<Connection>>,
    events: Receiver<Internal>,
    overflow: Arc<AtomicBool>,
    open: bool,
    pending: Vec<u8>,
}
impl Peer {
    pub fn new(initiator: bool, ice_servers: &[String]) -> Result<Self> {
        if ice_servers.len() > 8
            || ice_servers
                .iter()
                .any(|s| s.len() > 2048 || s.contains('\0'))
        {
            bail!("invalid ICE configuration")
        }
        let (tx, events) = mpsc::sync_channel(64);
        let overflow = Arc::new(AtomicBool::new(false));
        let sink = Sink {
            tx,
            overflow: overflow.clone(),
        };
        let config = RtcConfig::new(ice_servers).max_message_size(RECORD as i32);
        let mut connection = RtcPeerConnection::new(&config, Connection(sink.clone()))?;
        let channel = if initiator {
            Some(connection.create_data_channel("terminal", Handler(sink))?)
        } else {
            None
        };
        Ok(Self {
            channel,
            connection,
            events,
            overflow,
            open: false,
            pending: Vec::new(),
        })
    }
    pub fn apply_signal(&mut self, bytes: &[u8]) -> Result<()> {
        if bytes.len() > 64 * 1024 {
            bail!("oversized signaling")
        }
        match serde_json::from_slice::<Signal>(bytes)? {
            Signal::Description(d) => self.connection.set_remote_description(&d)?,
            Signal::Candidate(c) => self.connection.add_remote_candidate(&c)?,
        }
        Ok(())
    }
    pub fn is_open(&self) -> bool {
        self.open && !self.overflow.load(Ordering::Acquire)
    }
    pub fn buffered_bytes(&self) -> usize {
        self.channel.as_ref().map_or(0, |c| c.buffered_amount())
    }
    pub fn send(&mut self, bytes: &[u8]) -> Result<()> {
        if !self.is_open() || bytes.len() + 4 + self.buffered_bytes() > MAX_PENDING {
            bail!("direct channel unavailable or backpressured")
        }
        let mut framed = Vec::with_capacity(bytes.len() + 4);
        framed.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
        framed.extend_from_slice(bytes);
        let channel = self
            .channel
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("direct channel missing"))?;
        for chunk in framed.chunks(RECORD) {
            if let Err(e) = channel.send(chunk) {
                self.open = false;
                return Err(e.into());
            }
        }
        Ok(())
    }
    pub fn poll(&mut self) -> Result<Option<Event>> {
        if self.overflow.load(Ordering::Acquire) {
            bail!("direct receive queue overflow")
        }
        loop {
            match self.events.try_recv() {
                Ok(Internal::Signal(s)) => return Ok(Some(Event::Signal(serde_json::to_vec(&s)?))),
                Ok(Internal::Channel(c)) => {
                    if self.channel.is_some() {
                        bail!("unexpected second data channel")
                    };
                    self.channel = Some(c);
                }
                Ok(Internal::Open) => {
                    self.open = true;
                    return Ok(Some(Event::Open));
                }
                Ok(Internal::Closed) => {
                    self.open = false;
                    return Ok(Some(Event::Closed));
                }
                Ok(Internal::Bytes(bytes)) => {
                    if self.pending.len() + bytes.len() > MAX_PENDING {
                        bail!("direct message exceeds reassembly limit")
                    }
                    self.pending.extend_from_slice(&bytes);
                    if self.pending.len() >= 4 {
                        let len =
                            u32::from_be_bytes(self.pending[..4].try_into().unwrap()) as usize;
                        if len + 4 > MAX_PENDING || self.pending.len() > len + 4 {
                            bail!("invalid direct message length")
                        }
                        if self.pending.len() == len + 4 {
                            let data = self.pending.split_off(4);
                            self.pending.clear();
                            return Ok(Some(Event::Message(data)));
                        }
                    }
                }
                Err(TryRecvError::Empty) => return Ok(None),
                Err(TryRecvError::Disconnected) => return Ok(Some(Event::Closed)),
            }
        }
    }
}
