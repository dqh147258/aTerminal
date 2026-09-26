//! Desktop-only authoritative terminal engine. It alone answers application queries.
pub mod reading;
use ai_terminal_protocol::{self as wire, Cell, Cursor, Snapshot};
use alacritty_terminal::{
    Term,
    event::{Event, EventListener, WindowSize},
    grid::Dimensions,
    index::{Column, Line, Point},
    term::{Config, TermMode, cell::Flags},
    vte::ansi::{Color, CursorShape, Processor, Rgb},
};
use std::sync::{Arc, Mutex};
use std::time::Instant;

#[derive(Clone)]
struct Events(Arc<Mutex<Vec<Event>>>);
impl EventListener for Events {
    fn send_event(&self, event: Event) {
        self.0.lock().expect("event sink poisoned").push(event);
    }
}
struct Size {
    rows: usize,
    cols: usize,
}
impl Dimensions for Size {
    fn total_lines(&self) -> usize {
        self.rows
    }
    fn screen_lines(&self) -> usize {
        self.rows
    }
    fn columns(&self) -> usize {
        self.cols
    }
}

pub struct Engine {
    term: Term<Events>,
    parser: Processor,
    events: Events,
    epoch: u64,
    revision: u64,
    dimensions_epoch: u64,
}
impl Engine {
    pub fn new(rows: u16, cols: u16, epoch: u64) -> Result<Self, wire::ProtocolError> {
        check_size(rows, cols)?;
        if epoch == 0 {
            return Err(wire::ProtocolError::Epoch);
        }
        let events = Events(Arc::new(Mutex::new(Vec::new())));
        let config = Config {
            scrolling_history: 10_000,
            ..Config::default()
        };
        Ok(Self {
            term: Term::new(
                config,
                &Size {
                    rows: rows as usize,
                    cols: cols as usize,
                },
                events.clone(),
            ),
            parser: Processor::new(),
            events,
            epoch,
            revision: 1,
            dimensions_epoch: 1,
        })
    }
    pub fn feed(&mut self, bytes: &[u8]) -> Vec<Vec<u8>> {
        self.parser.advance(&mut self.term, bytes);
        self.revision += 1;
        self.drain_responses()
    }
    pub fn tick(&mut self) -> Vec<Vec<u8>> {
        if self
            .parser
            .sync_timeout()
            .sync_timeout()
            .is_some_and(|at| Instant::now() >= at)
        {
            self.parser.stop_sync(&mut self.term);
            self.revision += 1;
        }
        self.drain_responses()
    }
    fn drain_responses(&mut self) -> Vec<Vec<u8>> {
        let events = std::mem::take(&mut *self.events.0.lock().expect("event sink poisoned"));
        events
            .into_iter()
            .filter_map(|event| match event {
                Event::PtyWrite(s) => Some(s.into_bytes()),
                Event::ColorRequest(i, f) => {
                    let c = self.color_index(i);
                    Some(
                        f(Rgb {
                            r: (c >> 16) as u8,
                            g: (c >> 8) as u8,
                            b: c as u8,
                        })
                        .into_bytes(),
                    )
                }
                Event::TextAreaSizeRequest(f) => Some(
                    f(WindowSize {
                        num_lines: self.term.screen_lines() as u16,
                        num_cols: self.term.columns() as u16,
                        cell_width: 0,
                        cell_height: 0,
                    })
                    .into_bytes(),
                ),
                // Clipboard is deliberately not forwarded to the outer terminal.
                Event::ClipboardLoad(_, f) => Some(f("").into_bytes()),
                _ => None,
            })
            .collect()
    }
    pub fn resize(&mut self, rows: u16, cols: u16) -> Result<(), wire::ProtocolError> {
        check_size(rows, cols)?;
        if self.term.screen_lines() != rows as usize || self.term.columns() != cols as usize {
            self.term.resize(Size {
                rows: rows as usize,
                cols: cols as usize,
            });
            self.dimensions_epoch += 1;
            self.revision += 1;
        }
        Ok(())
    }
    pub fn application_cursor(&self) -> bool {
        self.term.mode().contains(TermMode::APP_CURSOR)
    }
    pub fn snapshot_revision(&self) -> u64 {
        self.revision
    }
    /// Flush an unfinished synchronized update when the PTY reaches EOF.
    pub fn finish(&mut self) {
        self.parser.stop_sync(&mut self.term);
        self.revision += 1;
    }
    pub fn bracketed_paste(&self) -> bool {
        self.term.mode().contains(TermMode::BRACKETED_PASTE)
    }
    pub fn focus_reporting(&self) -> bool {
        self.term.mode().contains(TermMode::FOCUS_IN_OUT)
    }
    /// Stable input flags, independent of the terminal library's bit representation.
    pub fn mouse_modes(&self) -> (bool, bool, bool, bool) {
        let m = self.term.mode();
        (
            m.intersects(TermMode::MOUSE_MODE),
            m.contains(TermMode::SGR_MOUSE),
            m.contains(TermMode::MOUSE_DRAG),
            m.contains(TermMode::MOUSE_MOTION),
        )
    }
    pub fn input_modes(&self) -> u32 {
        let (mouse, sgr, drag, motion) = self.mouse_modes();
        let bits = [
            self.application_cursor(),
            self.bracketed_paste(),
            self.focus_reporting(),
            mouse,
            sgr,
            drag,
            motion,
        ];
        bits.iter()
            .enumerate()
            .fold(0, |v, (i, on)| v | u32::from(*on) << i)
    }
    /// Bounded point-in-time history page. Offsets count backwards from the live grid.
    pub fn history(&self, offset: usize, limit: usize) -> (Vec<String>, bool) {
        let available = self.term.grid().history_size();
        let end = available.saturating_sub(offset);
        let start = end.saturating_sub(limit.min(200));
        let lines = (start..end)
            .map(|n| {
                let row = Line(n as i32 - available as i32);
                let mut text = String::new();
                for col in 0..self.term.columns() {
                    let cell = &self.term.grid()[Point::new(row, Column(col))];
                    if !cell
                        .flags
                        .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
                    {
                        text.push(cell.c);
                        if let Some(z) = cell.zerowidth() {
                            text.extend(z)
                        }
                    }
                }
                text.trim_end().to_owned()
            })
            .collect();
        (lines, start > 0 || available >= 10_000)
    }
    /// Capture once on the terminal actor. Matching never borrows the live grid.
    pub fn read_view(&self, max_lines: usize, max_bytes: usize) -> reading::ReadView {
        let alternate = self.term.mode().contains(TermMode::ALT_SCREEN);
        let history = if alternate {
            0
        } else {
            self.term.grid().history_size()
        };
        let end = self.term.screen_lines() as i32;
        let mut lines = Vec::new();
        let mut bytes = 0;
        let mut partial = history >= 10_000;
        for row in (-(history as i32)..end).rev() {
            if lines.len() >= max_lines.min(12_000) {
                partial = true;
                break;
            }
            let mut text = String::new();
            let mut wrapped = false;
            for col in 0..self.term.columns() {
                let cell = &self.term.grid()[Point::new(Line(row), Column(col))];
                wrapped |= cell.flags.contains(Flags::WRAPLINE);
                if cell
                    .flags
                    .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
                {
                    continue;
                }
                if cell.flags.contains(Flags::HIDDEN) || cell.c.is_control() {
                    text.push(' ');
                } else {
                    text.push(cell.c);
                    if let Some(z) = cell.zerowidth() {
                        text.extend(z.iter().copied().filter(|c| !c.is_control()));
                    }
                }
            }
            // Only grid padding is removed; indentation and non-space characters survive.
            let text = text.trim_end_matches(' ').to_owned();
            if bytes + text.len() + 1 > max_bytes.min(8 * 1024 * 1024) {
                partial = true;
                break;
            }
            bytes += text.len() + 1;
            lines.push(reading::TextLine { text, wrapped });
        }
        lines.reverse();
        let screen_start = lines.len().saturating_sub(self.term.screen_lines());
        reading::ReadView {
            epoch: self.epoch,
            revision: self.revision,
            dimensions_epoch: self.dimensions_epoch,
            alternate_screen: alternate,
            screen_start,
            source_partial: partial,
            lines,
        }
    }
    pub fn snapshot(&self) -> Snapshot {
        let rows = self.term.screen_lines();
        let cols = self.term.columns();
        let mut cells = Vec::with_capacity(rows * cols);
        for row in 0..rows {
            for col in 0..cols {
                let c = &self.term.grid()[Point::new(Line(row as i32), Column(col))];
                let mut fg = self.color(c.fg);
                let mut bg = self.color(c.bg);
                if c.flags.contains(Flags::INVERSE) {
                    std::mem::swap(&mut fg, &mut bg)
                }
                let width = if c.flags.contains(Flags::WIDE_CHAR_SPACER) {
                    0
                } else if c.flags.contains(Flags::WIDE_CHAR) {
                    2
                } else {
                    1
                };
                let mut text = if c
                    .flags
                    .intersects(Flags::HIDDEN | Flags::LEADING_WIDE_CHAR_SPACER)
                    || width == 0
                {
                    " ".into()
                } else {
                    if c.c.is_control() {
                        " ".into()
                    } else {
                        c.c.to_string()
                    }
                };
                if !c.flags.contains(Flags::HIDDEN)
                    && width != 0
                    && let Some(chars) = c.zerowidth()
                {
                    text.extend(chars.iter().copied().filter(|ch| !ch.is_control()))
                }
                let mut style = 0;
                for (flag, bit) in [
                    (Flags::BOLD, wire::BOLD),
                    (Flags::ITALIC, wire::ITALIC),
                    (Flags::ALL_UNDERLINES, wire::UNDERLINE),
                    (Flags::STRIKEOUT, wire::STRIKE),
                    (Flags::DIM, wire::DIM),
                ] {
                    if c.flags.intersects(flag) {
                        style |= bit;
                    }
                }
                cells.push(Cell {
                    text,
                    width,
                    foreground: fg,
                    background: bg,
                    style,
                });
            }
        }
        let cursor = self.term.renderable_content().cursor;
        let mut state = Snapshot {
            version: wire::VERSION,
            epoch: self.epoch,
            revision: self.revision,
            rows: rows as u32,
            cols: cols as u32,
            cells,
            cursor: Some(Cursor {
                row: cursor.point.line.0.max(0) as u32,
                col: cursor.point.column.0 as u32,
                visible: cursor.shape != CursorShape::Hidden,
                shape: match cursor.shape {
                    CursorShape::Beam => 1,
                    CursorShape::Underline => 2,
                    CursorShape::HollowBlock => 3,
                    _ => 0,
                },
            }),
            alternate_screen: self.term.mode().contains(TermMode::ALT_SCREEN),
            hash: Vec::new(),
            dimensions_epoch: self.dimensions_epoch,
            input_modes: self.input_modes(),
        };
        state.seal();
        state
    }
    fn color(&self, color: Color) -> u32 {
        match color {
            Color::Spec(c) => (u32::from(c.r) << 16) | (u32::from(c.g) << 8) | u32::from(c.b),
            Color::Indexed(i) => self.color_index(i as usize),
            Color::Named(i) => self.color_index(i as usize),
        }
    }
    fn color_index(&self, index: usize) -> u32 {
        if let Some(c) = self.term.colors()[index] {
            return (u32::from(c.r) << 16) | (u32::from(c.g) << 8) | u32::from(c.b);
        }
        palette(index)
    }
}
pub fn check_size(rows: u16, cols: u16) -> Result<(), wire::ProtocolError> {
    if rows == 0 || cols < 2 || rows as usize * cols as usize > wire::MAX_CELLS {
        Err(wire::ProtocolError::Invalid)
    } else {
        Ok(())
    }
}
fn palette(i: usize) -> u32 {
    const ANSI: [u32; 16] = [
        0x000000, 0xcd0000, 0x00cd00, 0xcdcd00, 0x0000ee, 0xcd00cd, 0x00cdcd, 0xe5e5e5, 0x7f7f7f,
        0xff0000, 0x00ff00, 0xffff00, 0x5c5cff, 0xff00ff, 0x00ffff, 0xffffff,
    ];
    match i {
        0..=15 => ANSI[i],
        16..=231 => {
            let n = i - 16;
            let c = |v: usize| if v == 0 { 0 } else { 55 + 40 * v as u32 };
            (c(n / 36) << 16) | (c(n / 6 % 6) << 8) | c(n % 6)
        }
        232..=255 => {
            let v = 8 + 10 * (i as u32 - 232);
            v * 0x010101
        }
        257 => 0x101014,
        259..=266 => {
            let n = ANSI[i - 259];
            (((n >> 16 & 255) * 2 / 3) << 16)
                | (((n >> 8 & 255) * 2 / 3) << 8)
                | ((n & 255) * 2 / 3)
        }
        268 => 0xaaaaaa,
        _ => 0xe5e5e5,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn chunk_boundaries_do_not_change_terminal_semantics() {
        let bytes = "hello 中 e\u{301}\r\n\x1b[31mred\x1b[0m\x1b[2;4H!".as_bytes();
        let mut full = Engine::new(8, 20, 1).unwrap();
        full.feed(bytes);
        let expected = full.snapshot();
        expected.validate().unwrap();
        for split in 0..=bytes.len() {
            let mut e = Engine::new(8, 20, 1).unwrap();
            e.feed(&bytes[..split]);
            e.feed(&bytes[split..]);
            let s = e.snapshot();
            s.validate().unwrap();
            assert_eq!(s.cells, expected.cells);
            assert_eq!(s.cursor, expected.cursor);
        }
    }
    #[test]
    fn git_status_tab_cells_are_safe_in_every_intermediate_frame() {
        let mut e = Engine::new(8, 32, 1).unwrap();
        for chunk in [
            b"On branch main\r\n".as_slice(),
            b"\tmodified: file\r\n",
            b"$ ",
        ] {
            e.feed(chunk);
            let frame = e.snapshot();
            frame.validate().unwrap();
            assert!(
                frame
                    .cells
                    .iter()
                    .all(|c| !c.text.chars().any(char::is_control))
            );
        }
    }
    #[test]
    fn only_authority_answers_query_and_restores_alternate_screen() {
        let mut e = Engine::new(4, 12, 1).unwrap();
        assert_eq!(e.feed(b"abc\x1b[6n"), vec![b"\x1b[1;4R".to_vec()]);
        let original = e.snapshot();
        e.feed(b"\x1b[?1049hother");
        assert!(e.snapshot().alternate_screen);
        e.feed(b"\x1b[?1049l");
        assert_eq!(e.snapshot().cells, original.cells);
        e.resize(6, 20).unwrap();
        let s = e.snapshot();
        s.validate().unwrap();
        assert_eq!(s.dimensions_epoch, 2);
    }
    #[test]
    fn palette_updates_recolor_existing_cells_and_replica_recovers() {
        let mut e = Engine::new(3, 12, 1).unwrap();
        e.feed(b"\x1b[31mx");
        let a = e.snapshot();
        e.feed(b"\x1b]4;1;rgb:12/34/56\x07");
        let b = e.snapshot();
        assert_eq!(b.cells[0].foreground, 0x123456);
        let mut r = wire::Replica::default();
        r.snapshot(a.clone()).unwrap();
        r.delta(b.delta_from(&a).unwrap()).unwrap();
        assert_eq!(r.state(), Some(&b));
    }
    #[test]
    fn eof_flushes_unfinished_synchronized_updates() {
        let mut engine = Engine::new(4, 12, 1).unwrap();
        engine.feed(b"\x1b[?2026hfinished");
        engine.finish();
        let frame = engine.snapshot();
        frame.validate().unwrap();
        let text = frame
            .cells
            .iter()
            .map(|c| c.text.as_str())
            .collect::<String>();
        assert!(text.starts_with("finished"));
    }
    #[test]
    fn history_is_bounded_and_does_not_move_the_live_cursor() {
        let mut e = Engine::new(3, 20, 1).unwrap();
        for i in 0..20 {
            e.feed(format!("line-{i}\r\n").as_bytes());
        }
        let before = e.snapshot();
        let (history, truncated) = e.history(0, 4);
        assert_eq!(history.len(), 4);
        assert!(truncated);
        assert!(history.iter().all(|s| s.starts_with("line-")));
        assert_eq!(e.snapshot(), before);
    }
}
