//! A bounded real ICE/DTLS/SCTP test with host candidates only, no public STUN service.
use datachannel::{
    DataChannelHandler, DataChannelInfo, IceCandidate, PeerConnectionHandler, RtcConfig,
    RtcDataChannel, RtcPeerConnection, SessionDescription,
};
use std::{
    sync::mpsc::{self, Sender},
    time::{Duration, Instant},
};

enum Event {
    Description(u8, Box<SessionDescription>),
    Candidate(u8, IceCandidate),
    Channel(Box<RtcDataChannel<Channel>>),
    Open(bool),
    Message(Vec<u8>),
    Error(String),
}
#[derive(Clone)]
struct Channel(Sender<Event>, bool);
impl DataChannelHandler for Channel {
    fn on_open(&mut self) {
        let _ = self.0.send(Event::Open(self.1));
    }
    fn on_message(&mut self, bytes: &[u8]) {
        let _ = self.0.send(Event::Message(bytes.to_vec()));
    }
    fn on_error(&mut self, e: &str) {
        let _ = self.0.send(Event::Error(e.into()));
    }
}
struct Peer {
    other: u8,
    tx: Sender<Event>,
}
impl PeerConnectionHandler for Peer {
    type DCH = Channel;
    fn data_channel_handler(&mut self, _: DataChannelInfo) -> Channel {
        Channel(self.tx.clone(), false)
    }
    fn on_description(&mut self, d: SessionDescription) {
        let _ = self.tx.send(Event::Description(self.other, Box::new(d)));
    }
    fn on_candidate(&mut self, c: IceCandidate) {
        let _ = self.tx.send(Event::Candidate(self.other, c));
    }
    fn on_data_channel(&mut self, dc: Box<RtcDataChannel<Channel>>) {
        let _ = self.tx.send(Event::Channel(dc));
    }
}

#[test]
fn reliable_data_channel_transfers_binary_payload_on_host_candidates() {
    // Exercise adapter startup and inherit its native platform link dependencies.
    ai_terminal_transport::initialize();
    let (tx, rx) = mpsc::channel();
    let config = RtcConfig::new(&[] as &[&str]);
    let mut a = RtcPeerConnection::new(
        &config,
        Peer {
            other: 1,
            tx: tx.clone(),
        },
    )
    .unwrap();
    let mut b = RtcPeerConnection::new(
        &config,
        Peer {
            other: 0,
            tx: tx.clone(),
        },
    )
    .unwrap();
    let mut outgoing = a.create_data_channel("screen", Channel(tx, true)).unwrap();
    let payload: Vec<u8> = (0..8192).map(|i| (i % 251) as u8).collect();
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut incoming = None;
    let mut sent = false;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        let event = rx
            .recv_timeout(left)
            .expect("local DataChannel did not converge within 15 seconds");
        match event {
            Event::Description(id, d) => {
                if id == 0 {
                    a.set_remote_description(&d)
                } else {
                    b.set_remote_description(&d)
                }
                .unwrap();
            }
            Event::Candidate(id, c) => {
                if id == 0 {
                    a.add_remote_candidate(&c)
                } else {
                    b.add_remote_candidate(&c)
                }
                .unwrap();
            }
            Event::Channel(dc) => incoming = Some(dc),
            Event::Open(true) if !sent => {
                outgoing.send(&payload).unwrap();
                sent = true;
            }
            Event::Open(_) => {}
            Event::Message(bytes) => {
                assert_eq!(bytes, payload);
                break;
            }
            Event::Error(e) => panic!("DataChannel error: {e}"),
        }
    }
    drop(incoming);
    drop(outgoing);
    drop(a);
    drop(b);
}
