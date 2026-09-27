//! Immutable, bounded reading copy. Never moves the authority's viewport.
use super::*;

pub const MAX_CAPTURE_BYTES: usize = 64 * 1024 * 1024;
const MAX_CAPTURE_CELLS: usize = 1_500_000;

pub struct Scrollback {
    template: Snapshot,
    lines: Vec<Vec<Cell>>,
    pub bytes: usize,
    pub truncated: bool,
}
impl Engine {
    pub fn capture_scrollback(&self) -> Result<Scrollback, wire::ProtocolError> {
        self.capture_scrollback_bounded(MAX_CAPTURE_BYTES, MAX_CAPTURE_CELLS)
    }

    fn capture_scrollback_bounded(
        &self,
        max_bytes: usize,
        max_cells: usize,
    ) -> Result<Scrollback, wire::ProtocolError> {
        let mut template = self.snapshot();
        template.validate()?;
        template.cells.clear();
        template.cells.shrink_to_fit();
        template.cursor.as_mut().unwrap().visible = false;
        let available = self.term.grid().history_size();
        let mut lines = Vec::new();
        let mut bytes = 0;
        for row in (-(available as i32)..template.rows as i32).rev() {
            let line = self.cells(row, row + 1);
            let size = line.capacity() * std::mem::size_of::<Cell>()
                + line.iter().map(|cell| cell.text.capacity()).sum::<usize>()
                + std::mem::size_of::<Vec<Cell>>();
            if bytes + size > max_bytes || (lines.len() + 1) * template.cols as usize > max_cells {
                break;
            }
            bytes += size;
            lines.push(line);
        }
        if lines.len() < template.rows as usize {
            return Err(wire::ProtocolError::Invalid);
        }
        let truncated = lines.len() < available + template.rows as usize || available >= 10_000;
        lines.reverse();
        Ok(Scrollback {
            template,
            lines,
            bytes,
            truncated,
        })
    }
}
impl Scrollback {
    pub fn line_count(&self) -> u32 {
        self.lines.len() as u32
    }
    pub fn text_page(
        &self,
        offset: u32,
        limit: u32,
    ) -> Result<(Vec<String>, u32), wire::ProtocolError> {
        let end = self.lines.len().saturating_sub(offset as usize);
        let start = end.saturating_sub(limit.clamp(1, 200) as usize);
        let mut lines = Vec::new();
        let mut bytes = 0;
        for row in self.lines[start..end].iter().rev() {
            let text: String = row
                .iter()
                .filter(|cell| cell.width != 0)
                .map(|cell| cell.text.as_str())
                .collect();
            let text = text.trim_end().to_owned();
            if bytes + text.len() + 16 > wire::MAX_MESSAGE_BYTES / 2 {
                if lines.is_empty() {
                    return Err(wire::ProtocolError::Invalid);
                }
                break;
            }
            bytes += text.len() + 16;
            lines.push(text);
        }
        let next = offset.min(self.line_count()) + lines.len() as u32;
        lines.reverse();
        Ok((lines, next))
    }
    pub fn compatible(&self, engine: &Engine) -> bool {
        self.template.dimensions_epoch == engine.dimensions_epoch
            && self.template.alternate_screen == engine.term.mode().contains(TermMode::ALT_SCREEN)
    }
    pub fn total(&self) -> u32 {
        (self.lines.len() - self.template.rows as usize) as u32
    }
    pub fn page(&self, offset: u32) -> Result<(Snapshot, u32), wire::ProtocolError> {
        let offset = offset.min(self.total());
        let end = self.lines.len() - offset as usize;
        let start = end - self.template.rows as usize;
        let mut frame = self.template.clone();
        frame.cells = self.lines[start..end].iter().flatten().cloned().collect();
        frame.seal();
        frame.validate()?;
        Ok((frame, offset))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn paginated_text_matches_the_same_desktop_capture_during_output() {
        for count in [0, 200, 450, 10100] {
            let mut engine = Engine::new(8, 30, 1).unwrap();
            for n in 0..count {
                engine.feed(format!("ROW_{n:05} 中e\u{301}\r\n").as_bytes());
            }
            let view = engine.capture_scrollback().unwrap();
            let total = view.line_count();
            let mut offset = 0;
            let mut combined = Vec::new();
            loop {
                let (page, next) = view.text_page(offset, 200).unwrap();
                assert_eq!(next - offset, page.len() as u32);
                combined.splice(0..0, page);
                if next == total {
                    break;
                }
                assert!(next > offset);
                offset = next;
                engine.feed(b"new output while reading\r\n");
            }
            assert_eq!(combined.len(), total as usize);
            let desktop = view.page(u32::MAX).unwrap().0;
            let first: String = desktop.cells[..30]
                .iter()
                .filter(|c| c.width != 0)
                .map(|c| c.text.as_str())
                .collect();
            assert_eq!(combined[0], first.trim_end());
            assert_eq!(view.text_page(total, 200).unwrap(), (vec![], total));
            assert_eq!(view.truncated, count >= 10000);
        }
    }
    #[test]
    fn full_history_is_bounded_after_ten_thousand_lines() {
        let mut engine = Engine::new(24, 80, 1).unwrap();
        for n in 0..10_100 {
            engine.feed(format!("{n:05}\r\n").as_bytes());
        }
        let start = Instant::now();
        let history = engine.capture_scrollback().unwrap();
        eprintln!(
            "10000-line capture: {:?}, {} bytes",
            start.elapsed(),
            history.bytes
        );
        assert_eq!(history.total(), 10_000);
        assert!(history.truncated);
        assert!(history.bytes <= MAX_CAPTURE_BYTES);
        let (first, offset) = history.page(u32::MAX).unwrap();
        assert_eq!(offset, 10_000);
        assert!(
            first
                .cells
                .iter()
                .map(|c| c.text.as_str())
                .collect::<String>()
                .starts_with("00077")
        );
    }
    #[test]
    fn frozen_styled_history_does_not_move_authority() {
        let mut engine = Engine::new(3, 20, 1).unwrap();
        for n in 0..50 {
            engine.feed(format!("\x1b[31m{n:03} 中e\u{301}\r\n").as_bytes());
        }
        let live = engine.snapshot();
        let history = engine.capture_scrollback().unwrap();
        let (first, offset) = history.page(u32::MAX).unwrap();
        assert_eq!(offset, history.total());
        assert!(
            first
                .cells
                .iter()
                .filter(|c| c.width != 0)
                .map(|c| c.text.as_str())
                .collect::<String>()
                .contains("000 中e\u{301}")
        );
        assert_eq!(first.cells[0].foreground, palette(1));
        assert!(!first.cursor.as_ref().unwrap().visible);
        assert_eq!(engine.snapshot(), live);
        engine.feed(b"more\r\nmore\r\n");
        assert_eq!(history.page(u32::MAX).unwrap().0, first);
        engine.resize(4, 21).unwrap();
        assert!(!history.compatible(&engine));
    }
    #[test]
    fn capture_limits_and_clear_semantics() {
        let mut engine = Engine::new(3, 8, 1).unwrap();
        for _ in 0..20 {
            engine.feed(b"hello\r\n");
        }
        let bounded = engine
            .capture_scrollback_bounded(MAX_CAPTURE_BYTES, 40)
            .unwrap();
        assert_eq!(bounded.total(), 2);
        assert!(bounded.truncated);
        assert!(engine.capture_scrollback_bounded(1, 40).is_err());
        engine.feed(b"\x1b[2J");
        assert!(engine.capture_scrollback().unwrap().total() > 0);
        engine.feed(b"\x1b[3J");
        assert_eq!(engine.capture_scrollback().unwrap().total(), 0);
        let primary = engine.capture_scrollback().unwrap();
        engine.feed(b"\x1b[?1049h");
        assert!(!primary.compatible(&engine));
        assert_eq!(engine.capture_scrollback().unwrap().total(), 0);
    }
}
