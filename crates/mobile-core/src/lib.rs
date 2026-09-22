//! Shared mobile state replica. UI receives batches, never raw application VT.
use ai_terminal_protocol::{Replica, Snapshot};
use prost::Message;
use std::sync::Mutex;
uniffi::setup_scaffolding!();
mod account;
mod remote;
pub use account::{Account, AccountDevice};
pub use remote::{RemoteSession, RemoteTerminal};

/// P0 build probe: force the native WebRTC symbols to link into mobile artifacts.
#[uniffi::export]
pub fn native_webrtc_available() -> bool {
    #[cfg(feature = "webrtc")]
    {
        ai_terminal_transport::initialize();
        true
    }
    #[cfg(not(feature = "webrtc"))]
    {
        false
    }
}

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum CoreError {
    #[error("{reason}")]
    InvalidFrame { reason: String },
}

#[derive(Clone, uniffi::Record)]
pub struct RenderCell {
    pub text: String,
    pub width: u32,
    pub foreground: u32,
    pub background: u32,
    pub style: u32,
}
#[derive(Clone, uniffi::Record)]
pub struct RenderFrame {
    pub rows: u32,
    pub cols: u32,
    pub revision: u64,
    pub cells: Vec<RenderCell>,
    pub cursor_row: u32,
    pub cursor_col: u32,
    pub cursor_visible: bool,
    pub cursor_shape: u32,
}
#[derive(uniffi::Record)]
pub struct RenderPatch {
    pub index: u32,
    pub cell: RenderCell,
}
#[derive(uniffi::Record)]
pub struct RenderUpdate {
    pub generation: u64,
    pub epoch: u64,
    pub revision: u64,
    pub rows: u32,
    pub cols: u32,
    pub full: bool,
    pub patches: Vec<RenderPatch>,
    pub cursor_row: u32,
    pub cursor_col: u32,
    pub cursor_visible: bool,
    pub cursor_shape: u32,
}
#[derive(uniffi::Record)]
pub struct DisplayBatch {
    pub update: Option<RenderUpdate>,
    pub controlled: bool,
    pub path: String,
}
pub(crate) fn render_cell(c: &ai_terminal_protocol::Cell) -> RenderCell {
    RenderCell {
        text: c.text.clone(),
        width: c.width,
        foreground: c.foreground,
        background: c.background,
        style: c.style,
    }
}
pub(crate) fn render_frame(s: &Snapshot) -> RenderFrame {
    let c = s.cursor.as_ref().expect("validated snapshot cursor");
    RenderFrame {
        rows: s.rows,
        cols: s.cols,
        revision: s.revision,
        cells: s
            .cells
            .iter()
            .map(|c| RenderCell {
                text: c.text.clone(),
                width: c.width,
                foreground: c.foreground,
                background: c.background,
                style: c.style,
            })
            .collect(),
        cursor_row: c.row,
        cursor_col: c.col,
        cursor_visible: c.visible,
        cursor_shape: c.shape,
    }
}

#[derive(uniffi::Object, Default)]
pub struct TerminalReplica {
    inner: Mutex<Replica>,
}
#[uniffi::export]
impl TerminalReplica {
    #[uniffi::constructor]
    pub fn new() -> Self {
        Self::default()
    }
    pub fn apply_snapshot(&self, bytes: Vec<u8>) -> Result<bool, CoreError> {
        let frame = Snapshot::from_wire(&bytes).map_err(|e| CoreError::InvalidFrame {
            reason: e.to_string(),
        })?;
        self.inner
            .lock()
            .map_err(|_| CoreError::InvalidFrame {
                reason: "replica unavailable".into(),
            })?
            .snapshot(frame)
            .map_err(|e| CoreError::InvalidFrame {
                reason: e.to_string(),
            })
    }
    pub fn apply_delta(&self, bytes: Vec<u8>) -> Result<bool, CoreError> {
        if bytes.len() > ai_terminal_protocol::MAX_MESSAGE_BYTES {
            return Err(CoreError::InvalidFrame {
                reason: "oversized delta".into(),
            });
        }
        let delta = ai_terminal_protocol::Delta::decode(bytes.as_slice()).map_err(|e| {
            CoreError::InvalidFrame {
                reason: e.to_string(),
            }
        })?;
        self.inner
            .lock()
            .map_err(|_| CoreError::InvalidFrame {
                reason: "replica unavailable".into(),
            })?
            .delta(delta)
            .map_err(|e| CoreError::InvalidFrame {
                reason: e.to_string(),
            })
    }
    pub fn frame(&self) -> Result<Option<RenderFrame>, CoreError> {
        let guard = self.inner.lock().map_err(|_| CoreError::InvalidFrame {
            reason: "replica unavailable".into(),
        })?;
        Ok(guard.state().map(|s| {
            let c = s.cursor.as_ref().expect("validated frame has cursor");
            RenderFrame {
                rows: s.rows,
                cols: s.cols,
                revision: s.revision,
                cells: s
                    .cells
                    .iter()
                    .map(|c| RenderCell {
                        text: c.text.clone(),
                        width: c.width,
                        foreground: c.foreground,
                        background: c.background,
                        style: c.style,
                    })
                    .collect(),
                cursor_row: c.row,
                cursor_col: c.col,
                cursor_visible: c.visible,
                cursor_shape: c.shape,
            }
        }))
    }
    pub fn reset(&self) -> Result<(), CoreError> {
        self.inner
            .lock()
            .map_err(|_| CoreError::InvalidFrame {
                reason: "replica unavailable".into(),
            })?
            .reset();
        Ok(())
    }
}
