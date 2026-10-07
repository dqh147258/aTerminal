//! Authenticated, length-delimited local Agent RPC; not the remote pairing protocol.
use crate::{Delta, MAX_MESSAGE_BYTES, Snapshot};
use prost::Message;
use std::io::{self, Read, Write};

pub const SCROLLBACK_EXPIRED: &str = "scrollback expired";
pub const SCROLLBACK_BUSY: &str = "scrollback reader limit reached";

/// A session actor was removed after an explicit close. Clients may end attachment normally.
pub const SESSION_CLOSED_ERROR: &str = "session closed";

#[derive(Clone, PartialEq, Message)]
pub struct Request {
    #[prost(string, tag = "1")]
    pub token: String,
    #[prost(uint64, tag = "2")]
    pub client: u64,
    #[prost(string, tag = "3")]
    pub session: String,
    #[prost(enumeration = "Operation", tag = "4")]
    pub operation: i32,
    #[prost(string, repeated, tag = "5")]
    pub command: Vec<String>,
    #[prost(string, tag = "6")]
    pub cwd: String,
    #[prost(uint32, tag = "7")]
    pub rows: u32,
    #[prost(uint32, tag = "8")]
    pub cols: u32,
    #[prost(uint64, tag = "9")]
    pub control_epoch: u64,
    #[prost(uint64, tag = "10")]
    pub input_seq: u64,
    #[prost(bytes = "vec", tag = "11")]
    pub input: Vec<u8>,
    #[prost(uint64, tag = "12")]
    pub revision: u64,
    #[prost(uint64, tag = "13")]
    pub session_epoch: u64,
    #[prost(uint32, tag = "14")]
    pub history_offset: u32,
    #[prost(uint32, tag = "15")]
    pub history_limit: u32,
    /// 0 raw terminal/typing bytes, 1 text/paste, 2 named key; encode using authority modes.
    #[prost(uint32, tag = "16")]
    pub input_kind: u32,
    #[prost(string, tag = "17")]
    pub text: String,
    #[prost(string, tag = "18")]
    pub key: String,
    #[prost(bool, tag = "19")]
    pub submit: bool,
    /// Set only by the authenticated local bridge, never trusted from the network.
    #[prost(string, tag = "20")]
    pub account_scope: String,
    /// Last Desktop attachment transition observed by a Watch subscriber.
    #[prost(uint64, tag = "21")]
    pub availability_epoch: u64,
    #[prost(uint64, tag = "22")]
    pub manual_revision: u64,
    /// Trusted bridge identity, overwritten by Client::call.
    #[prost(string, tag = "23")]
    pub device_scope: String,
    #[prost(bool, tag = "24")]
    pub shell_integration: bool,
    #[prost(uint32, tag = "25")]
    pub key_repeat: u32,
    /// Zero captures a new reading copy; otherwise reads this client's existing copy.
    #[prost(uint64, tag = "26")]
    pub scrollback_id: u64,
    /// Screens use their own identity and size, independent of terminal session/control.
    #[prost(string, tag = "27")]
    pub screen_id: String,
    #[prost(uint32, tag = "28")]
    pub screen_max_width: u32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, prost::Enumeration)]
#[repr(i32)]
pub enum Operation {
    List = 0,
    Create = 1,
    Poll = 2,
    Acquire = 3,
    Input = 4,
    Resize = 5,
    Detach = 6,
    Close = 7,
    Shutdown = 8,
    History = 9,
    /// Local-only account management; never authorized over a remote channel.
    Account = 10,
    /// Local change subscription with a bounded wait, never an input channel.
    Watch = 11,
    Streaming = 12,
    Subscribe = 13,
    /// Desktop-owned asynchronous assistant; JSON request and response in text/history.
    Assistant = 14,
    /// Local-only assistant broker input; fenced separately from manual input sequences.
    AssistantInput = 15,
    /// Local Desktop CLI attachment; remote bridges must reject this operation.
    AttachDesktop = 16,
    /// Authenticated Desktop configuration; JSON contract, separate from model tools.
    Configuration = 17,
    Agent = 18,
    ObserveTerminal = 19,
    AgentAcquire = 20,
    AgentWrite = 21,
    AgentClose = 22,
    AgentResize = 23,
    AgentRelease = 24,
    /// Read-only frozen history. history_limit=0 selects a styled viewport;
    /// a positive limit selects a text page, newest page first.
    Scrollback = 25,
    ReleaseScrollback = 26,
    RemoteScreens = 27,
    RemoteScreenFrame = 28,
}

#[derive(Clone, PartialEq, Message)]
pub struct SessionInfo {
    #[prost(string, tag = "1")]
    pub id: String,
    #[prost(uint64, tag = "2")]
    pub epoch: u64,
    #[prost(string, tag = "3")]
    pub cwd: String,
    #[prost(bool, tag = "4")]
    pub exited: bool,
    #[prost(uint32, tag = "5")]
    pub exit_code: u32,
    #[prost(uint64, tag = "6")]
    pub controller: u64,
    #[prost(uint64, tag = "7")]
    pub control_epoch: u64,
    #[prost(uint64, tag = "8")]
    pub next_input_seq: u64,
    #[prost(string, tag = "9")]
    pub error: String,
    #[prost(bool, tag = "10")]
    pub desktop_attached: bool,
    #[prost(uint64, tag = "11")]
    pub availability_epoch: u64,
    #[prost(uint64, tag = "12")]
    pub manual_revision: u64,
    #[prost(uint32, tag = "13")]
    pub process_id: u32,
    #[prost(uint32, tag = "14")]
    pub foreground_group: u32,
    #[prost(string, tag = "15")]
    pub process_identity: String,
    #[prost(string, tag = "16")]
    pub shell_status: String,
}

#[derive(Clone, PartialEq, Message)]
pub struct Reply {
    #[prost(string, tag = "1")]
    pub error: String,
    #[prost(message, repeated, tag = "2")]
    pub sessions: Vec<SessionInfo>,
    #[prost(message, optional, tag = "3")]
    pub info: Option<SessionInfo>,
    #[prost(message, optional, tag = "4")]
    pub snapshot: Option<Snapshot>,
    #[prost(message, optional, tag = "5")]
    pub delta: Option<Delta>,
    /// Accepted into the PTY writer queue, not proof of command execution.
    #[prost(uint64, tag = "6")]
    pub accepted_input_seq: u64,
    #[prost(string, repeated, tag = "7")]
    pub history: Vec<String>,
    #[prost(bool, tag = "8")]
    pub history_truncated: bool,
    /// Stream subscription boundary; earlier state events belong to the previous subscription.
    #[prost(uint64, tag = "9")]
    pub state_sequence: u64,
    #[prost(uint64, tag = "10")]
    pub scrollback_id: u64,
    #[prost(uint32, tag = "11")]
    pub scrollback_offset: u32,
    #[prost(uint32, tag = "12")]
    pub scrollback_total: u32,
    #[prost(uint32, tag = "13")]
    pub history_total: u32,
    #[prost(uint32, tag = "14")]
    pub history_next: u32,
    #[prost(bool, tag = "15")]
    pub history_has_more: bool,
    /// Desktop-owned, account-scoped MRU of verified working directories.
    /// Absent on older Desktops; only populated by List.
    #[prost(string, repeated, tag = "16")]
    pub recent_directories: Vec<String>,
    /// Advertised by List. Probe before sending screen operations to older Desktops.
    #[prost(uint32, tag = "17")]
    pub screen_protocol_version: u32,
}
pub fn write_message<W: Write, M: Message>(out: &mut W, message: &M) -> io::Result<()> {
    let bytes = message.encode_to_vec();
    if bytes.len() > MAX_MESSAGE_BYTES + 4096 {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "oversized RPC"));
    }
    out.write_all(&(bytes.len() as u32).to_be_bytes())?;
    out.write_all(&bytes)?;
    out.flush()
}
pub fn read_message<R: Read, M: Message + Default>(input: &mut R) -> io::Result<M> {
    let mut len = [0; 4];
    input.read_exact(&mut len)?;
    let len = u32::from_be_bytes(len) as usize;
    if len > MAX_MESSAGE_BYTES + 4096 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "oversized RPC"));
    }
    let mut bytes = vec![0; len];
    input.read_exact(&mut bytes)?;
    M::decode(bytes.as_slice()).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

#[cfg(test)]
mod compatibility_tests {
    use super::*;
    #[derive(Clone, PartialEq, Message)]
    struct LegacyListReply {
        #[prost(string, tag = "1")]
        error: String,
        #[prost(message, repeated, tag = "2")]
        sessions: Vec<SessionInfo>,
    }
    #[test]
    fn recent_directories_are_an_optional_list_extension() {
        let old = LegacyListReply {
            error: String::new(),
            sessions: vec![SessionInfo {
                id: "terminal".into(),
                ..Default::default()
            }],
        };
        let new = Reply::decode(old.encode_to_vec().as_slice()).unwrap();
        assert_eq!(new.screen_protocol_version, 0);
        assert!(new.recent_directories.is_empty());
        assert_eq!(new.sessions, old.sessions);
        let new = Reply {
            recent_directories: vec!["/project space".into()],
            screen_protocol_version: crate::screens::SCREEN_PROTOCOL_VERSION,
            ..new
        };
        assert_eq!(
            LegacyListReply::decode(new.encode_to_vec().as_slice()).unwrap(),
            old
        );
    }
}
