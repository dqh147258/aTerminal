use crossterm::event::{
    Event, KeyCode, KeyEventKind, KeyModifiers as M, MouseButton as MB, MouseEventKind as MK,
};

struct Modes(u32);
impl Modes {
    fn application_cursor(&self) -> bool {
        self.0 & 1 != 0
    }
    fn bracketed_paste(&self) -> bool {
        self.0 & 2 != 0
    }
    fn focus_reporting(&self) -> bool {
        self.0 & 4 != 0
    }
    fn mouse_modes(&self) -> (bool, bool, bool, bool) {
        (
            self.0 & 8 != 0,
            self.0 & 16 != 0,
            self.0 & 32 != 0,
            self.0 & 64 != 0,
        )
    }
}
pub fn encode(event: Event, modes: u32) -> Option<Vec<u8>> {
    let engine = Modes(modes);
    match event {
        Event::Key(k) if k.kind != KeyEventKind::Release => {
            let modifier = 1
                + u8::from(k.modifiers.contains(M::SHIFT))
                + 2 * u8::from(k.modifiers.contains(M::ALT))
                + 4 * u8::from(k.modifiers.contains(M::CONTROL));
            let mut bytes = match k.code {
                KeyCode::Char(c) if k.modifiers.contains(M::CONTROL) => {
                    let c = c.to_ascii_uppercase();
                    let b = match c {
                        '?' => 127,
                        '@'..='_' => (c as u8) & 31,
                        ' ' => 0,
                        _ => return None,
                    };
                    vec![b]
                }
                KeyCode::Char(c) => c.to_string().into_bytes(),
                KeyCode::Enter => vec![13],
                KeyCode::Backspace => vec![127],
                KeyCode::Tab => vec![9],
                KeyCode::Esc => vec![27],
                KeyCode::BackTab => b"\x1b[Z".to_vec(),
                KeyCode::Up
                | KeyCode::Down
                | KeyCode::Right
                | KeyCode::Left
                | KeyCode::Home
                | KeyCode::End => {
                    let suffix = match k.code {
                        KeyCode::Up => 'A',
                        KeyCode::Down => 'B',
                        KeyCode::Right => 'C',
                        KeyCode::Left => 'D',
                        KeyCode::Home => 'H',
                        _ => 'F',
                    };
                    if modifier > 1 {
                        format!("\x1b[1;{modifier}{suffix}").into_bytes()
                    } else {
                        format!(
                            "\x1b{}{suffix}",
                            if engine.application_cursor() {
                                'O'
                            } else {
                                '['
                            }
                        )
                        .into_bytes()
                    }
                }
                KeyCode::Delete | KeyCode::Insert | KeyCode::PageUp | KeyCode::PageDown => {
                    let n = match k.code {
                        KeyCode::Insert => 2,
                        KeyCode::Delete => 3,
                        KeyCode::PageUp => 5,
                        _ => 6,
                    };
                    tilde(n, modifier)
                }
                KeyCode::F(n @ 1..=4) => {
                    if modifier == 1 {
                        vec![27, b'O', b'P' + n - 1]
                    } else {
                        format!("\x1b[1;{}{}", modifier, (b'P' + n - 1) as char).into_bytes()
                    }
                }
                KeyCode::F(n @ 5..=12) => {
                    tilde([15, 17, 18, 19, 20, 21, 23, 24][n as usize - 5], modifier)
                }
                _ => return None,
            };
            if k.modifiers.contains(M::ALT)
                && matches!(
                    k.code,
                    KeyCode::Char(_) | KeyCode::Enter | KeyCode::Backspace
                )
            {
                bytes.insert(0, 27)
            }
            Some(bytes)
        }
        Event::Paste(s) => Some(if engine.bracketed_paste() {
            format!("\x1b[200~{}\x1b[201~", s.replace('\x1b', "")).into_bytes()
        } else {
            s.replace("\r\n", "\r").replace('\n', "\r").into_bytes()
        }),
        Event::FocusGained if engine.focus_reporting() => Some(b"\x1b[I".to_vec()),
        Event::FocusLost if engine.focus_reporting() => Some(b"\x1b[O".to_vec()),
        Event::Mouse(m) => {
            let (enabled, sgr, drag, motion) = engine.mouse_modes();
            if !enabled {
                return None;
            }
            let button = |b| match b {
                MB::Left => 0,
                MB::Middle => 1,
                MB::Right => 2,
            };
            let mut code = match m.kind {
                MK::Down(b) | MK::Up(b) => button(b),
                MK::Drag(b) if drag || motion => 32 + button(b),
                MK::Moved if motion => 35,
                MK::ScrollUp => 64,
                MK::ScrollDown => 65,
                _ => return None,
            };
            if m.modifiers.contains(M::SHIFT) {
                code += 4
            }
            if m.modifiers.contains(M::ALT) {
                code += 8
            }
            if m.modifiers.contains(M::CONTROL) {
                code += 16
            }
            let release = matches!(m.kind, MK::Up(_));
            if sgr {
                Some(
                    format!(
                        "\x1b[<{};{};{}{}",
                        code,
                        u32::from(m.column) + 1,
                        u32::from(m.row) + 1,
                        if release { 'm' } else { 'M' }
                    )
                    .into_bytes(),
                )
            } else if m.column < 223 && m.row < 223 {
                Some(vec![
                    27,
                    b'[',
                    b'M',
                    if release { 35 } else { code + 32 },
                    m.column as u8 + 33,
                    m.row as u8 + 33,
                ])
            } else {
                None
            }
        }
        _ => None,
    }
}
fn tilde(n: u8, modifier: u8) -> Vec<u8> {
    if modifier == 1 {
        format!("\x1b[{n}~").into_bytes()
    } else {
        format!("\x1b[{n};{modifier}~").into_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ai_terminal_engine::Engine;
    use crossterm::event::KeyEvent;
    #[test]
    fn arrows_follow_inner_modes_and_ctrl_c_is_forwarded() {
        let mut e = Engine::new(4, 12, 1).unwrap();
        let up = Event::Key(KeyEvent::new(KeyCode::Up, M::NONE));
        assert_eq!(encode(up.clone(), e.input_modes()).unwrap(), b"\x1b[A");
        e.feed(b"\x1b[?1h");
        assert_eq!(encode(up, e.input_modes()).unwrap(), b"\x1bOA");
        assert_eq!(
            encode(
                Event::Key(KeyEvent::new(KeyCode::Char('c'), M::CONTROL)),
                e.input_modes()
            )
            .unwrap(),
            vec![3]
        );
    }
}
