//! Authenticated, length-delimited local Agent RPC; not the remote pairing protocol.
use crate::{Delta, MAX_MESSAGE_BYTES, Snapshot};
use prost::Message;
use std::io::{self, Read, Write};

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
    /// 0 raw local-terminal bytes, 1 text/paste, 2 named key; encode using authority modes.
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
