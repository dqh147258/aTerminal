use anyhow::{Result, bail, ensure};
pub(crate) fn encode(key: &str, modifiers: &[String], application_cursor: bool) -> Result<Vec<u8>> {
    ensure!(modifiers.len() <= 3, "invalid_key_modifiers");
    let mut shift = false;
    let mut alt = false;
    let mut ctrl = false;
    for modifier in modifiers {
        match modifier.as_str() {
            "shift" if !shift => shift = true,
            "alt" if !alt => alt = true,
            "ctrl" if !ctrl => ctrl = true,
            _ => bail!("invalid_key_modifiers"),
        }
    }
    let key = key.to_ascii_lowercase();
    if let Some(letter) = key.strip_prefix("ctrl_") {
        ensure!(modifiers.is_empty(), "duplicate_key_modifiers");
        return encode(letter, &["ctrl".into()], application_cursor);
    }
    let parameter = 1 + u8::from(shift) + 2 * u8::from(alt) + 4 * u8::from(ctrl);
    if let Some(suffix) = match key.as_str() {
        "up" => Some('A'),
        "down" => Some('B'),
        "right" => Some('C'),
        "left" => Some('D'),
        "home" => Some('H'),
        "end" => Some('F'),
        _ => None,
    } {
        return Ok(if parameter == 1 {
            format!("\x1b{}{suffix}", if application_cursor { 'O' } else { '[' })
        } else {
            format!("\x1b[1;{parameter}{suffix}")
        }
        .into_bytes());
    }
    if let Some(number) = match key.as_str() {
        "insert" => Some(2),
        "delete" => Some(3),
        "page_up" => Some(5),
        "page_down" => Some(6),
        "f5" => Some(15),
        "f6" => Some(17),
        "f7" => Some(18),
        "f8" => Some(19),
        "f9" => Some(20),
        "f10" => Some(21),
        "f11" => Some(23),
        "f12" => Some(24),
        _ => None,
    } {
        return Ok(if parameter == 1 {
            format!("\x1b[{number}~")
        } else {
            format!("\x1b[{number};{parameter}~")
        }
        .into_bytes());
    }
    if let Some(suffix) = match key.as_str() {
        "f1" => Some('P'),
        "f2" => Some('Q'),
        "f3" => Some('R'),
        "f4" => Some('S'),
        _ => None,
    } {
        return Ok(if parameter == 1 {
            format!("\x1bO{suffix}")
        } else {
            format!("\x1b[1;{parameter}{suffix}")
        }
        .into_bytes());
    }
    if key == "tab" && shift && !alt && !ctrl {
        return Ok(b"\x1b[Z".to_vec());
    }
    let value = match key.as_str() {
        "enter" if !shift && !ctrl => 13,
        "tab" if !shift && !ctrl => 9,
        "escape" if !shift && !ctrl => 27,
        "backspace" if !shift => {
            if ctrl {
                8
            } else {
                127
            }
        }
        "space" => {
            if ctrl {
                0
            } else {
                32
            }
        }
        s if s.len() == 1 && s.as_bytes()[0].is_ascii_graphic() => {
            let c = s.as_bytes()[0];
            if ctrl {
                ensure!(
                    c.is_ascii_alphabetic() || b"@[\\]^_".contains(&c),
                    "unsupported_control_key"
                );
                c & 31
            } else if shift {
                c.to_ascii_uppercase()
            } else {
                c
            }
        }
        _ => bail!("unsupported_key"),
    };
    let mut bytes = Vec::new();
    if alt {
        bytes.push(27);
    }
    bytes.push(value);
    Ok(bytes)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mode_aware_keys_and_modifiers() {
        assert_eq!(encode("up", &[], true).unwrap(), b"\x1bOA");
        assert_eq!(encode("up", &["ctrl".into()], true).unwrap(), b"\x1b[1;5A");
        assert_eq!(encode("c", &["ctrl".into()], false).unwrap(), [3]);
        assert_eq!(encode("tab", &["shift".into()], false).unwrap(), b"\x1b[Z");
        assert!(encode("enter", &["shift".into()], false).is_err());
    }
}
