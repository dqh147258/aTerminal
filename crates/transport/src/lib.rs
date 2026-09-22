//! Bounded native DataChannel adapter. Signaling must travel over an authenticated channel.
mod peer;
pub use peer::{Event, Peer};
pub fn initialize() {
    datachannel::preload();
}
