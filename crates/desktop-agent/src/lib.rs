//! Per-user local session host. Remote authorization is a separate P2 concern.
mod account;
pub mod pty;
mod remote_bridge;
mod service;
mod stream;
use anyhow::{Result, bail};
pub use service::{Client, default_state_dir, run_agent};
use std::collections::VecDeque;

pub fn random_id() -> u64 {
    loop {
        let n = getrandom::u64().expect("OS randomness unavailable");
        if n != 0 {
            return n;
        }
    }
}

/// One controller and a contiguous, bounded idempotency window per control epoch.
#[derive(Default)]
pub struct Control {
    pub owner: u64,
    pub epoch: u64,
    pub next: u64,
    recent: VecDeque<(u64, [u8; 32])>,
}
impl Control {
    pub fn acquire(&mut self, client: u64) -> Result<()> {
        if client == 0 {
            bail!("invalid client")
        }
        if self.owner != client {
            self.owner = client;
            self.epoch += 1;
            self.next = 1;
            self.recent.clear();
        }
        Ok(())
    }
    pub fn release(&mut self, client: u64) {
        if self.owner == client {
            self.owner = 0;
            self.epoch += 1;
            self.next = 1;
            self.recent.clear();
        }
    }
    pub fn check(&self, client: u64, epoch: u64) -> Result<()> {
        if client == 0 || self.owner != client || self.epoch != epoch {
            bail!("control lost; acquire again before writing")
        }
        Ok(())
    }
    /// true is a previously accepted identical request, false the next new input.
    pub fn input(&self, client: u64, epoch: u64, seq: u64, bytes: &[u8]) -> Result<bool> {
        self.check(client, epoch)?;
        if seq == self.next {
            return Ok(false);
        }
        if self
            .recent
            .iter()
            .any(|(s, b)| *s == seq && b == blake3::hash(bytes).as_bytes())
        {
            return Ok(true);
        }
        bail!("input sequence gap, expired dedup window, or conflicting retry")
    }
    /// Commit only after the bounded PTY writer accepts the request.
    pub fn commit(&mut self, seq: u64, bytes: Vec<u8>) {
        self.recent
            .push_back((seq, *blake3::hash(&bytes).as_bytes()));
        self.next += 1;
        if self.recent.len() > 128 {
            self.recent.pop_front();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn duplicate_input_and_old_controller_cannot_reexecute() {
        let mut c = Control::default();
        c.acquire(1).unwrap();
        let epoch = c.epoch;
        assert!(!c.input(1, epoch, 1, b"x").unwrap());
        c.commit(1, b"x".to_vec());
        assert!(c.input(1, epoch, 1, b"x").unwrap());
        assert!(c.input(1, epoch, 1, b"y").is_err());
        assert!(c.input(1, epoch, 3, b"x").is_err());
        c.acquire(2).unwrap();
        assert!(c.input(1, epoch, 2, b"x").is_err());
        assert!(!c.input(2, c.epoch, 1, b"z").unwrap());
    }
}
