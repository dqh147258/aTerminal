//! Per-user local session host. Remote authorization is a separate P2 concern.
mod account;
mod assistant;
mod builtin_skills;
mod catalog;
pub mod config;
pub mod extensions;
mod keys;
mod private_acl;
mod process;
mod recent_directories;
pub mod pty;
pub mod raster;
mod remote_bridge;
mod secrets;
mod service;
mod shell;
pub mod state;
mod stream;
use anyhow::{Result, bail};
pub use service::{Client, default_state_dir, run_agent};
use std::collections::{HashMap, VecDeque};

pub fn random_id() -> u64 {
    loop {
        let n = getrandom::u64().expect("OS randomness unavailable");
        if n != 0 {
            return n;
        }
    }
}

/// Independent ordered input streams for every attached client.
#[derive(Default)]
pub struct Control {
    generation: u64,
    clients: HashMap<u64, ClientInput>,
}
struct ClientInput {
    epoch: u64,
    next: u64,
    recent: VecDeque<(u64, [u8; 32])>,
}
impl Control {
    pub fn acquire(&mut self, client: u64) -> Result<()> {
        if client == 0 {
            bail!("invalid client")
        }
        self.clients.entry(client).or_insert_with(|| {
            self.generation += 1;
            ClientInput {
                epoch: self.generation,
                next: 1,
                recent: VecDeque::new(),
            }
        });
        Ok(())
    }
    pub fn release(&mut self, client: u64) {
        self.clients.remove(&client);
    }
    pub fn metadata(&self, client: u64) -> (u64, u64, u64) {
        self.clients
            .get(&client)
            .map_or((0, 0, 1), |s| (client, s.epoch, s.next))
    }
    pub fn check(&self, client: u64, epoch: u64) -> Result<()> {
        if client == 0 || self.clients.get(&client).is_none_or(|s| s.epoch != epoch) {
            bail!("input stream expired; reopen the session before writing")
        }
        Ok(())
    }
    /// true is a previously accepted identical request, false the next new input.
    pub fn input(&self, client: u64, epoch: u64, seq: u64, bytes: &[u8]) -> Result<bool> {
        self.check(client, epoch)?;
        let state = &self.clients[&client];
        if seq == state.next {
            return Ok(false);
        }
        if state
            .recent
            .iter()
            .any(|(s, b)| *s == seq && b == blake3::hash(bytes).as_bytes())
        {
            return Ok(true);
        }
        bail!("input sequence gap, expired dedup window, or conflicting retry")
    }
    /// Commit only after the bounded PTY writer accepts the request.
    pub fn commit(&mut self, client: u64, seq: u64, bytes: Vec<u8>) {
        let state = self.clients.get_mut(&client).expect("checked input stream");
        state
            .recent
            .push_back((seq, *blake3::hash(&bytes).as_bytes()));
        state.next += 1;
        if state.recent.len() > 128 {
            state.recent.pop_front();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn concurrent_clients_keep_independent_order_and_deduplication() {
        let mut c = Control::default();
        c.acquire(1).unwrap();
        let epoch = c.metadata(1).1;
        assert!(!c.input(1, epoch, 1, b"x").unwrap());
        c.commit(1, 1, b"x".to_vec());
        assert!(c.input(1, epoch, 1, b"x").unwrap());
        assert!(c.input(1, epoch, 1, b"y").is_err());
        assert!(c.input(1, epoch, 3, b"x").is_err());
        c.acquire(2).unwrap();
        let second_epoch = c.metadata(2).1;
        assert!(!c.input(1, epoch, 2, b"x").unwrap());
        assert!(!c.input(2, second_epoch, 1, b"z").unwrap());
        c.commit(2, 1, b"z".to_vec());
        c.release(1);
        assert!(c.input(1, epoch, 2, b"x").is_err());
        c.acquire(1).unwrap();
        assert_ne!(c.metadata(1).1, epoch);
        assert!(c.input(1, epoch, 2, b"x").is_err());
        assert!(c.input(2, second_epoch, 1, b"z").unwrap());
    }
}
