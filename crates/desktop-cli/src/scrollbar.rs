use ai_terminal_protocol::Snapshot;

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Bar {
    pub column: u16,
    pub rows: u16,
    pub top: u16,
    pub height: u16,
}
impl Bar {
    pub fn new(cols: u16, rows: u16, total: u32, offset: u32) -> Option<Self> {
        if cols < 2 || rows < 2 || total == 0 {
            return None;
        }
        let height = ((u64::from(rows) * u64::from(rows)) / (u64::from(total) + u64::from(rows)))
            .clamp(1, u64::from(rows - 1)) as u16;
        let top = ((u64::from(rows - height) * u64::from(total.saturating_sub(offset)))
            / u64::from(total)) as u16;
        Some(Self {
            column: cols - 1,
            rows,
            top,
            height,
        })
    }
    pub fn offset(&self, row: u16, grab: u16, total: u32) -> u32 {
        let top = row.saturating_sub(grab).min(self.rows - self.height);
        let travel = u64::from(self.rows - self.height);
        (u64::from(total) - (u64::from(top) * u64::from(total) + travel / 2) / travel) as u32
    }
    pub fn paint(&self, frame: &mut Snapshot) {
        for row in 0..self.rows as usize {
            let index = row * frame.cols as usize + self.column as usize;
            if frame.cells[index].width == 0 {
                frame.cells[index - 1].width = 1;
                frame.cells[index - 1].text = " ".into();
            }
            if frame.cells[index].width == 2 && self.column as u32 + 1 < frame.cols {
                frame.cells[index + 1].width = 1;
                frame.cells[index + 1].text = " ".into();
            }
            let cell = &mut frame.cells[index];
            cell.text = if (self.top..self.top + self.height).contains(&(row as u16)) {
                "█"
            } else {
                "│"
            }
            .into();
            cell.width = 1;
            cell.style = 0;
            cell.foreground = 0xb0b0b0;
            cell.background = 0x303030;
        }
        if let Some(cursor) = &mut frame.cursor
            && cursor.col >= self.column as u32
        {
            cursor.visible = false;
        }
        frame.seal();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn thumb_maps_top_bottom_and_drag_outside_track() {
        for rows in [2, 8, 24] {
            for total in [1, 200, 10000] {
                let bar = Bar::new(80, rows, total, 0).unwrap();
                assert_eq!(bar.offset(0, 0, total), total);
                assert_eq!(bar.offset(u16::MAX, 0, total), 0);
                assert_eq!(bar.offset(bar.top, 0, total), 0);
                assert_eq!(Bar::new(80, rows, total, total).unwrap().top, 0);
            }
        }
        assert!(Bar::new(1, 24, 100, 0).is_none());
        assert!(Bar::new(80, 1, 100, 0).is_none());
    }
    #[test]
    fn overlay_preserves_wide_cell_contract_and_source() {
        let mut engine = ai_terminal_engine::Engine::new(3, 8, 1).unwrap();
        engine.feed("123456中".as_bytes());
        let original = engine.snapshot();
        let mut display = original.clone();
        Bar::new(8, 3, 20, 4).unwrap().paint(&mut display);
        display.validate().unwrap();
        assert_eq!(engine.snapshot(), original);
        assert_eq!(display.cells[6].text, " ");
    }
}
