//! Versioned terminal display protocol. Application VT bytes never reach replicas.
pub mod local;
use prost::Message;
use thiserror::Error;

pub const VERSION: u32 = 1;
pub const MAX_CELLS: usize = 100_000;
pub const MAX_MESSAGE_BYTES: usize = 4 * 1024 * 1024;
pub const BOLD: u32 = 1;
pub const ITALIC: u32 = 2;
pub const UNDERLINE: u32 = 4;
pub const STRIKE: u32 = 8;
pub const DIM: u32 = 16;

#[derive(Clone, PartialEq, Eq, Message)]
pub struct Cell {
    #[prost(string, tag = "1")]
    pub text: String,
    /// 0 is the continuation of a two-column glyph; 1 or 2 is its advance.
    #[prost(uint32, tag = "2")]
    pub width: u32,
    #[prost(fixed32, tag = "3")]
    pub foreground: u32,
    #[prost(fixed32, tag = "4")]
    pub background: u32,
    #[prost(uint32, tag = "5")]
    pub style: u32,
}

#[derive(Clone, PartialEq, Eq, Message)]
pub struct Cursor {
    #[prost(uint32, tag = "1")]
    pub row: u32,
    #[prost(uint32, tag = "2")]
    pub col: u32,
    #[prost(bool, tag = "3")]
    pub visible: bool,
    /// 0 block, 1 beam, 2 underline, 3 hollow block.
    #[prost(uint32, tag = "4")]
    pub shape: u32,
}

#[derive(Clone, PartialEq, Eq, Message)]
pub struct Snapshot {
    #[prost(uint32, tag = "1")]
    pub version: u32,
    #[prost(fixed64, tag = "2")]
    pub epoch: u64,
    #[prost(uint64, tag = "3")]
    pub revision: u64,
    #[prost(uint32, tag = "4")]
    pub rows: u32,
    #[prost(uint32, tag = "5")]
    pub cols: u32,
    #[prost(message, repeated, tag = "6")]
    pub cells: Vec<Cell>,
    #[prost(message, optional, tag = "7")]
    pub cursor: Option<Cursor>,
    #[prost(bool, tag = "8")]
    pub alternate_screen: bool,
    #[prost(bytes = "vec", tag = "9")]
    pub hash: Vec<u8>,
    #[prost(uint64, tag = "10")]
    pub dimensions_epoch: u64,
    /// Stable app cursor / paste / focus / mouse-click / SGR / drag / motion flags.
    #[prost(uint32, tag = "11")]
    pub input_modes: u32,
}

#[derive(Clone, PartialEq, Eq, Message)]
pub struct Patch {
    #[prost(uint32, tag = "1")]
    pub index: u32,
    #[prost(message, optional, tag = "2")]
    pub cell: Option<Cell>,
}

#[derive(Clone, PartialEq, Eq, Message)]
pub struct Delta {
    #[prost(fixed64, tag = "1")]
    pub epoch: u64,
    #[prost(uint64, tag = "2")]
    pub base_revision: u64,
    #[prost(uint64, tag = "3")]
    pub revision: u64,
    #[prost(message, repeated, tag = "4")]
    pub patches: Vec<Patch>,
    #[prost(message, optional, tag = "5")]
    pub cursor: Option<Cursor>,
    #[prost(bool, tag = "6")]
    pub alternate_screen: bool,
    #[prost(bytes = "vec", tag = "7")]
    pub hash: Vec<u8>,
    #[prost(uint64, tag = "8")]
    pub dimensions_epoch: u64,
    #[prost(uint32, tag = "9")]
    pub input_modes: u32,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ProtocolError {
    #[error("invalid or oversized terminal frame")]
    Invalid,
    #[error("state checksum mismatch")]
    Checksum,
    #[error("session epoch mismatch; open the intended session explicitly")]
    Epoch,
    #[error("missing delta baseline; request a snapshot")]
    Baseline,
    #[error("malformed protobuf message")]
    Decode,
}

impl Snapshot {
    pub fn seal(&mut self) {
        self.hash = self.checksum();
    }
    pub fn checksum(&self) -> Vec<u8> {
        let mut unhashed = self.clone();
        unhashed.hash.clear();
        blake3::hash(&unhashed.encode_to_vec()).as_bytes().to_vec()
    }
    pub fn validate(&self) -> Result<(), ProtocolError> {
        let count = (self.rows as usize)
            .checked_mul(self.cols as usize)
            .ok_or(ProtocolError::Invalid)?;
        if self.version != VERSION
            || self.epoch == 0
            || self.rows == 0
            || self.cols == 0
            || count > MAX_CELLS
            || count != self.cells.len()
            || self.encoded_len() > MAX_MESSAGE_BYTES
        {
            return Err(ProtocolError::Invalid);
        }
        let c = self.cursor.as_ref().ok_or(ProtocolError::Invalid)?;
        if c.row >= self.rows || c.col >= self.cols || c.shape > 3 {
            return Err(ProtocolError::Invalid);
        }
        for (i, cell) in self.cells.iter().enumerate() {
            if cell.width > 2
                || cell.text.len() > 256
                || cell.text.chars().any(char::is_control)
                || cell.foreground > 0xffffff
                || cell.background > 0xffffff
                || cell.style & !31 != 0
            {
                return Err(ProtocolError::Invalid);
            }
            let col = i % self.cols as usize;
            if cell.width == 0 && (col == 0 || self.cells[i - 1].width != 2)
                || cell.width == 2
                    && (col + 1 == self.cols as usize || self.cells[i + 1].width != 0)
            {
                return Err(ProtocolError::Invalid);
            }
        }
        if self.hash != self.checksum() {
            return Err(ProtocolError::Checksum);
        }
        Ok(())
    }
    pub fn wire(&self) -> Vec<u8> {
        self.encode_to_vec()
    }
    pub fn from_wire(bytes: &[u8]) -> Result<Self, ProtocolError> {
        if bytes.len() > MAX_MESSAGE_BYTES {
            return Err(ProtocolError::Invalid);
        }
        let value = Self::decode(bytes).map_err(|_| ProtocolError::Decode)?;
        value.validate()?;
        Ok(value)
    }
    /// A size change always starts with an atomic snapshot, never a partial delta.
    pub fn delta_from(&self, base: &Self) -> Option<Delta> {
        if self.epoch != base.epoch
            || self.revision <= base.revision
            || self.rows != base.rows
            || self.cols != base.cols
            || self.dimensions_epoch != base.dimensions_epoch
            || self.cells.len() != base.cells.len()
        {
            return None;
        }
        Some(Delta {
            epoch: self.epoch,
            base_revision: base.revision,
            revision: self.revision,
            patches: self
                .cells
                .iter()
                .zip(&base.cells)
                .enumerate()
                .filter(|(_, (a, b))| a != b)
                .map(|(i, (cell, _))| Patch {
                    index: i as u32,
                    cell: Some(cell.clone()),
                })
                .collect(),
            cursor: self.cursor.clone(),
            alternate_screen: self.alternate_screen,
            hash: self.hash.clone(),
            dimensions_epoch: self.dimensions_epoch,
            input_modes: self.input_modes,
        })
    }
}

#[derive(Default)]
pub struct Replica {
    state: Option<Snapshot>,
}
impl Replica {
    pub fn state(&self) -> Option<&Snapshot> {
        self.state.as_ref()
    }
    pub fn reset(&mut self) {
        self.state = None;
    }
    pub fn snapshot(&mut self, next: Snapshot) -> Result<bool, ProtocolError> {
        next.validate()?;
        if let Some(old) = &self.state {
            if old.epoch != next.epoch {
                return Err(ProtocolError::Epoch);
            }
            if next.revision <= old.revision {
                return Ok(false);
            }
            if next.dimensions_epoch < old.dimensions_epoch {
                return Err(ProtocolError::Baseline);
            }
        }
        self.state = Some(next);
        Ok(true)
    }
    pub fn delta(&mut self, delta: Delta) -> Result<bool, ProtocolError> {
        let old = self.state.as_ref().ok_or(ProtocolError::Baseline)?;
        if old.epoch != delta.epoch {
            return Err(ProtocolError::Epoch);
        }
        if delta.revision <= old.revision {
            return Ok(false);
        }
        if delta.base_revision != old.revision
            || delta.dimensions_epoch != old.dimensions_epoch
            || delta.patches.len() > old.cells.len()
        {
            return Err(ProtocolError::Baseline);
        }
        let mut next = old.clone();
        let mut last = None;
        for patch in delta.patches {
            if last.is_some_and(|i| patch.index <= i) {
                return Err(ProtocolError::Invalid);
            }
            last = Some(patch.index);
            *next
                .cells
                .get_mut(patch.index as usize)
                .ok_or(ProtocolError::Invalid)? = patch.cell.ok_or(ProtocolError::Invalid)?;
        }
        next.revision = delta.revision;
        next.cursor = delta.cursor;
        next.alternate_screen = delta.alternate_screen;
        next.input_modes = delta.input_modes;
        next.hash = delta.hash;
        next.validate()?;
        self.state = Some(next);
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn frame() -> Snapshot {
        let mut s = Snapshot {
            version: VERSION,
            epoch: 7,
            revision: 1,
            rows: 1,
            cols: 2,
            cells: vec![
                Cell {
                    text: " ".into(),
                    width: 1,
                    foreground: 0xffffff,
                    ..Cell::default()
                };
                2
            ],
            cursor: Some(Cursor::default()),
            ..Snapshot::default()
        };
        s.seal();
        s
    }
    #[test]
    fn recovery_duplicate_and_late_snapshot_do_not_roll_back() {
        let a = frame();
        let mut b = a.clone();
        b.revision = 2;
        b.cells[0].text = "中".into();
        b.cells[0].width = 2;
        b.cells[1].width = 0;
        b.seal();
        let delta = b.delta_from(&a).unwrap();
        let mut r = Replica::default();
        assert_eq!(r.delta(delta.clone()), Err(ProtocolError::Baseline));
        r.snapshot(a.clone()).unwrap();
        assert!(r.delta(delta.clone()).unwrap());
        assert!(!r.delta(delta).unwrap());
        assert!(!r.snapshot(a).unwrap());
        assert_eq!(r.state(), Some(&b));
        assert_eq!(Snapshot::from_wire(&b.wire()).unwrap(), b);
    }
    #[test]
    fn corrupted_delta_is_atomic_and_wrong_session_rejected() {
        let a = frame();
        let mut b = a.clone();
        b.revision = 2;
        b.cells[0].text = "x".into();
        b.seal();
        let mut d = b.delta_from(&a).unwrap();
        d.hash[0] ^= 1;
        let mut r = Replica::default();
        r.snapshot(a.clone()).unwrap();
        assert_eq!(r.delta(d), Err(ProtocolError::Checksum));
        assert_eq!(r.state(), Some(&a));
        b.epoch = 8;
        b.seal();
        assert_eq!(r.snapshot(b), Err(ProtocolError::Epoch));
    }
    #[test]
    fn rejects_terminal_injection_and_invalid_wide_cell() {
        let mut a = frame();
        a.cells[0].text = "\x1b[2J".into();
        a.seal();
        assert_eq!(a.validate(), Err(ProtocolError::Invalid));
        a.cells[0].text = "中".into();
        a.cells[0].width = 2;
        a.seal();
        assert_eq!(a.validate(), Err(ProtocolError::Invalid));
    }

    #[test]
    fn missing_revision_recovers_by_snapshot_and_resize_is_atomic() {
        let a = frame();
        let mut b = a.clone();
        b.revision = 2;
        b.cells[0].text = "b".into();
        b.seal();
        let mut c = b.clone();
        c.revision = 3;
        c.cells[1].text = "c".into();
        c.seal();
        let mut r = Replica::default();
        r.snapshot(a.clone()).unwrap();
        assert_eq!(
            r.delta(c.delta_from(&b).unwrap()),
            Err(ProtocolError::Baseline)
        );
        assert_eq!(r.state(), Some(&a));
        r.snapshot(c.clone()).unwrap();
        assert!(!r.snapshot(b).unwrap());
        let mut resized = c.clone();
        resized.cols = 1;
        resized.cells.truncate(1);
        resized.revision = 4;
        resized.dimensions_epoch += 1;
        resized.seal();
        assert!(resized.delta_from(&c).is_none());
        r.snapshot(resized.clone()).unwrap();
        assert_eq!(r.state(), Some(&resized));
    }
}
