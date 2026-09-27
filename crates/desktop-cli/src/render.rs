use ai_terminal_protocol::{BOLD, Cell, DIM, ITALIC, STRIKE, Snapshot, UNDERLINE};
use crossterm::{
    cursor::{Hide, MoveTo, SetCursorStyle, Show},
    event::{
        DisableBracketedPaste, DisableFocusChange, DisableMouseCapture, EnableBracketedPaste,
        EnableFocusChange, EnableMouseCapture,
    },
    execute,
    style::ResetColor,
    terminal::{
        self, Clear, ClearType, DisableLineWrap, EnableLineWrap, EnterAlternateScreen,
        LeaveAlternateScreen,
    },
};
use std::io::{self, Write};

pub struct TerminalGuard;
impl TerminalGuard {
    pub fn enter() -> io::Result<Self> {
        terminal::enable_raw_mode()?;
        let guard = Self;
        enter_screen(&mut io::stdout())?;
        Ok(guard)
    }
}
fn enter_screen(out: &mut impl Write) -> io::Result<()> {
    execute!(
        out,
        EnterAlternateScreen,
        ResetColor,
        Clear(ClearType::All),
        MoveTo(0, 0),
        DisableLineWrap,
        EnableBracketedPaste,
        EnableMouseCapture,
        EnableFocusChange,
        Hide
    )
}
fn leave_screen(out: &mut impl Write) -> io::Result<()> {
    execute!(
        out,
        ResetColor,
        Show,
        SetCursorStyle::DefaultUserShape,
        DisableFocusChange,
        DisableMouseCapture,
        DisableBracketedPaste,
        EnableLineWrap,
        LeaveAlternateScreen
    )
}
impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = leave_screen(&mut io::stdout());
        let _ = terminal::disable_raw_mode();
    }
}

#[derive(Default)]
pub struct Renderer {
    previous: Option<Snapshot>,
    colors: ColorLevel,
    viewport: Option<(u16, u16)>,
    source: Option<(Vec<u8>, Option<crate::scrollbar::Bar>)>,
}
#[derive(Clone, Copy, Default)]
pub enum ColorLevel {
    #[default]
    TrueColor,
    Indexed,
    Ansi,
}
impl ColorLevel {
    pub fn detect() -> Self {
        match crossterm::style::available_color_count() {
            u16::MAX => Self::TrueColor,
            256 => Self::Indexed,
            _ => Self::Ansi,
        }
    }
}
impl Renderer {
    pub fn new(colors: ColorLevel) -> Self {
        Self {
            colors,
            ..Self::default()
        }
    }
    pub fn viewport(&mut self, cols: u16, rows: u16) {
        if self.viewport != Some((cols, rows)) {
            self.viewport = Some((cols, rows));
            self.previous = None;
            self.source = None;
        }
    }
    pub fn draw_view(
        &mut self,
        out: &mut impl Write,
        frame: &Snapshot,
        bar: Option<crate::scrollbar::Bar>,
    ) -> io::Result<()> {
        if self
            .source
            .as_ref()
            .is_some_and(|(hash, old_bar)| *hash == frame.hash && *old_bar == bar)
        {
            return Ok(());
        }
        let mut display = frame.clone();
        if let Some(bar) = bar {
            bar.paint(&mut display);
        }
        self.draw(out, display)?;
        self.source = Some((frame.hash.clone(), bar));
        Ok(())
    }
    #[cfg(test)]
    pub fn needs_draw(&self, frame: &Snapshot) -> bool {
        self.previous
            .as_ref()
            .is_none_or(|old| old.hash != frame.hash)
    }
    pub fn draw(&mut self, out: &mut impl Write, next: Snapshot) -> io::Result<()> {
        next.validate()
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        let bytes = self.encode(&next);
        out.write_all(&bytes)?;
        out.flush()?;
        self.previous = Some(next);
        Ok(())
    }
    fn encode(&self, next: &Snapshot) -> Vec<u8> {
        let mut out = String::from("\x1b[?25l");
        let old = self
            .previous
            .as_ref()
            .filter(|s| s.rows == next.rows && s.cols == next.cols);
        if old.is_none() {
            out.push_str("\x1b[0m\x1b[2J");
        }
        let (cols, rows) = self.viewport.map_or((next.cols, next.rows), |(c, r)| {
            (u32::from(c).min(next.cols), u32::from(r).min(next.rows))
        });
        for row in 0..rows as usize {
            let start = row * next.cols as usize;
            let end = start + cols as usize;
            let changed = |i: usize| old.is_none_or(|s| s.cells[i] != next.cells[i]);
            let Some(mut first) = (start..end).find(|&i| changed(i)) else {
                continue;
            };
            let last = (first..end).rfind(|&i| changed(i)).unwrap();
            if next.cells[first].width == 0 && first > start {
                first -= 1;
            }
            out.push_str(&format!("\x1b[{};{}H", row + 1, first - start + 1));
            let mut previous_style: Option<(u32, u32, u32)> = None;
            for (offset, c) in next.cells[first..=last].iter().enumerate() {
                if c.width == 0 {
                    continue;
                }
                let style = (c.foreground, c.background, c.style);
                if previous_style != Some(style) {
                    set_style(&mut out, c, self.colors);
                    previous_style = Some(style)
                }
                if c.width == 2 && (first + offset - start + 1) >= cols as usize {
                    out.push(' ')
                } else {
                    out.push_str(&c.text);
                }
            }
        }
        if let Some(c) = &next.cursor
            && c.row < rows
            && c.col < cols
        {
            out.push_str(&format!("\x1b[{};{}H", c.row + 1, c.col + 1));
            if c.visible {
                out.push_str(match c.shape {
                    1 => "\x1b[6 q",
                    2 => "\x1b[4 q",
                    _ => "\x1b[0 q",
                });
                out.push_str("\x1b[?25h");
            }
        }
        out.into_bytes()
    }
}
fn set_style(s: &mut String, c: &Cell, colors: ColorLevel) {
    s.push_str("\x1b[0");
    for (bit, code) in [
        (BOLD, 1),
        (DIM, 2),
        (ITALIC, 3),
        (UNDERLINE, 4),
        (STRIKE, 9),
    ] {
        if c.style & bit != 0 {
            s.push_str(&format!(";{code}"));
        }
    }
    for (rgb, foreground) in [(c.foreground, true), (c.background, false)] {
        let base = if foreground { 38 } else { 48 };
        match colors {
            ColorLevel::TrueColor => s.push_str(&format!(
                ";{base};2;{};{};{}",
                rgb >> 16,
                rgb >> 8 & 255,
                rgb & 255
            )),
            ColorLevel::Indexed => s.push_str(&format!(";{base};5;{}", nearest_color(rgb, 256))),
            ColorLevel::Ansi => s.push_str(&format!(
                ";{}",
                (if foreground { 30 } else { 40 }) + nearest_color(rgb, 8)
            )),
        }
    }
    s.push('m');
}
fn nearest_color(rgb: u32, count: usize) -> usize {
    (0..count)
        .min_by_key(|&index| {
            let color = ai_terminal_engine::palette(index);
            [0, 8, 16]
                .into_iter()
                .map(|shift| {
                    let difference =
                        ((rgb >> shift) & 255) as i32 - ((color >> shift) & 255) as i32;
                    difference * difference
                })
                .sum::<i32>()
        })
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ai_terminal_engine::Engine;
    #[test]
    fn hiding_scrollbar_restores_covered_cells_without_new_output() {
        let mut engine = Engine::new(3, 8, 1).unwrap();
        engine.feed("123456中".as_bytes());
        let frame = engine.snapshot();
        let mut host = Engine::new(3, 8, 2).unwrap();
        let mut renderer = Renderer::default();
        for bar in [None, crate::scrollbar::Bar::new(8, 3, 100, 0), None] {
            let mut bytes = Vec::new();
            renderer.draw_view(&mut bytes, &frame, bar).unwrap();
            assert!(!bytes.is_empty());
            host.feed(&bytes);
        }
        assert_eq!(host.snapshot().cells, frame.cells);
    }
    #[test]
    fn host_color_levels_and_quantization() {
        let mut source = Engine::new(2, 8, 1).unwrap();
        source.feed(b"\x1b[38;2;255;0;0;48;2;0;0;255;1mR");
        let frame = source.snapshot();
        let rgb = String::from_utf8(Renderer::new(ColorLevel::TrueColor).encode(&frame)).unwrap();
        assert!(rgb.contains(";38;2;255;0;0;48;2;0;0;255"));
        let indexed = String::from_utf8(Renderer::new(ColorLevel::Indexed).encode(&frame)).unwrap();
        assert!(indexed.contains(";38;5;9;48;5;21"));
        assert!(!indexed.contains(";38;2;"));
        let ansi = String::from_utf8(Renderer::new(ColorLevel::Ansi).encode(&frame)).unwrap();
        assert!(ansi.contains("\x1b[0;1;31;44m"));
        assert!(!ansi.contains(";38;"));
        assert_eq!(nearest_color(0x808080, 256), 244);
    }
    #[test]
    fn lifecycle_clears_only_alternate_screen_and_restores_primary() {
        let mut host = Engine::new(3, 12, 1).unwrap();
        host.feed(b"parent\r\nhistory\r\nkeep\r\nprompt");
        let before = host.snapshot();
        let history = host.history(0, 200);
        let mut enter = Vec::new();
        enter_screen(&mut enter).unwrap();
        assert!(!enter.windows(4).any(|x| x == b"\x1b[3J"));
        host.feed(&enter);
        let blank = host.snapshot();
        assert!(blank.cells.iter().all(|cell| cell.text == " "));
        assert_eq!(blank.cursor.as_ref().unwrap().row, 0);
        assert_eq!(blank.cursor.as_ref().unwrap().col, 0);
        host.feed(b"child output");
        let mut leave = Vec::new();
        leave_screen(&mut leave).unwrap();
        host.feed(&leave);
        assert_eq!(host.snapshot().cells, before.cells);
        assert_eq!(host.history(0, 200), history);
        assert!(!host.snapshot().alternate_screen);
    }
    #[test]
    fn browsing_redraws_without_live_revision_change() {
        let mut engine = Engine::new(2, 10, 1).unwrap();
        engine.feed(b"one\r\ntwo\r\nthree\r\nfour");
        let history = engine.capture_scrollback().unwrap();
        let first = history.page(1).unwrap().0;
        let second = history.page(2).unwrap().0;
        assert_eq!(first.revision, second.revision);
        let mut renderer = Renderer::default();
        renderer.draw(&mut Vec::new(), first).unwrap();
        assert!(renderer.needs_draw(&second));
    }
    #[test]
    fn default_cursor_uses_host_terminal_preference() {
        let snapshot = Engine::new(2, 8, 1).unwrap().snapshot();
        let encoded = Renderer::default().encode(&snapshot);
        assert!(encoded.windows(5).any(|chunk| chunk == b"\x1b[0 q"));
        assert!(!encoded.windows(5).any(|chunk| chunk == b"\x1b[2 q"));
    }
    #[test]
    fn emitted_ansi_reconstructs_authority_without_wide_ghosts() {
        let mut source = Engine::new(4, 12, 1).unwrap();
        let mut host = Engine::new(4, 12, 2).unwrap();
        host.feed(b"\x1b[?7l");
        let mut r = Renderer::default();
        for text in [
            "中abc\r\nhello",
            "\x1b[1;1Hx ",
            "\x1b[2;3H\x1b[31m中文",
            "\x1b[2J",
            "\x1b[4;12HZ",
        ] {
            source.feed(text.as_bytes());
            let s = source.snapshot();
            let encoded = r.encode(&s);
            host.feed(&encoded);
            assert_eq!(host.snapshot().cells, s.cells);
            r.previous = Some(s);
        }
    }
}
