use ai_terminal_protocol::{BOLD, Cell, DIM, ITALIC, STRIKE, Snapshot, UNDERLINE};
use crossterm::{
    cursor::{Hide, Show},
    event::{
        DisableBracketedPaste, DisableFocusChange, DisableMouseCapture, EnableBracketedPaste,
        EnableFocusChange, EnableMouseCapture,
    },
    execute,
    style::ResetColor,
    terminal::{self, DisableLineWrap, EnableLineWrap, EnterAlternateScreen, LeaveAlternateScreen},
};
use std::io::{self, Write};

pub struct TerminalGuard;
impl TerminalGuard {
    pub fn enter() -> io::Result<Self> {
        terminal::enable_raw_mode()?;
        let guard = Self;
        execute!(
            io::stdout(),
            EnterAlternateScreen,
            DisableLineWrap,
            EnableBracketedPaste,
            EnableMouseCapture,
            EnableFocusChange,
            Hide
        )?;
        Ok(guard)
    }
}
impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = execute!(
            io::stdout(),
            ResetColor,
            Show,
            DisableFocusChange,
            DisableMouseCapture,
            DisableBracketedPaste,
            EnableLineWrap,
            LeaveAlternateScreen
        );
        let _ = terminal::disable_raw_mode();
    }
}

#[derive(Default)]
pub struct Renderer {
    previous: Option<Snapshot>,
    viewport: Option<(u16, u16)>,
}
impl Renderer {
    pub fn viewport(&mut self, cols: u16, rows: u16) {
        if self.viewport != Some((cols, rows)) {
            self.viewport = Some((cols, rows));
            self.previous = None;
        }
    }
    pub fn revision(&self) -> u64 {
        self.previous.as_ref().map_or(0, |s| s.revision)
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
                    set_style(&mut out, c);
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
                    _ => "\x1b[2 q",
                });
                out.push_str("\x1b[?25h");
            }
        }
        out.into_bytes()
    }
}
fn set_style(s: &mut String, c: &Cell) {
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
    s.push_str(&format!(
        ";38;2;{};{};{};48;2;{};{};{}m",
        c.foreground >> 16,
        c.foreground >> 8 & 255,
        c.foreground & 255,
        c.background >> 16,
        c.background >> 8 & 255,
        c.background & 255
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use ai_terminal_engine::Engine;
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
