//! Deterministic off-screen rendering of the authoritative terminal grid.
use ai_terminal_protocol::{BOLD, DIM, ITALIC, STRIKE, Snapshot, UNDERLINE};
use anyhow::{Result, ensure};
use serde_json::{Value, json};
use std::{collections::BTreeSet, sync::OnceLock};
static FONT: OnceLock<Result<fontdue::Font, String>> = OnceLock::new();
const FONT_BYTES: &[u8] = include_bytes!("../assets/fonts/NotoSansMonoCJKsc-Regular.otf");
pub struct Capture {
    pub png: Vec<u8>,
    pub metadata: Value,
    pub text: String,
}
pub fn capture(snapshot: &Snapshot) -> Result<Capture> {
    snapshot.validate()?;
    let font = FONT
        .get_or_init(|| {
            fontdue::Font::from_bytes(FONT_BYTES, fontdue::FontSettings::default())
                .map_err(str::to_owned)
        })
        .as_ref()
        .map_err(|e| anyhow::anyhow!("font unavailable: {e}"))?;
    let scale = if u64::from(snapshot.rows) * u64::from(snapshot.cols) * 8 * 22 > 4 * 1024 * 1024 {
        2usize
    } else {
        1
    };
    let cw = 8 / scale;
    let ch = 22 / scale;
    let px = 16.0 / scale as f32;
    let baseline = 18 / scale;
    let width = snapshot.cols as usize * cw;
    let height = snapshot.rows as usize * ch;
    ensure!(
        width * height <= 4 * 1024 * 1024,
        "screenshot_dimensions_limit"
    );
    let mut pixels = vec![0u8; width * height * 3];
    let mut missing = BTreeSet::new();
    for (index, cell) in snapshot.cells.iter().enumerate() {
        let x = index % snapshot.cols as usize * cw;
        let y = index / snapshot.cols as usize * ch;
        fill(&mut pixels, width, height, x, y, cw, ch, cell.background);
    }
    for (index, cell) in snapshot.cells.iter().enumerate() {
        if cell.width == 0 {
            continue;
        }
        let left = index % snapshot.cols as usize * cw;
        let top = index / snapshot.cols as usize * ch;
        let right = (left + cell.width as usize * cw).min(width);
        let mut advance = 0.0f32;
        let fg = if cell.style & DIM != 0 {
            dim(cell.foreground)
        } else {
            cell.foreground
        };
        for character in cell.text.chars() {
            if character == '\u{200d}' || ('\u{fe00}'..='\u{fe0f}').contains(&character) {
                continue;
            }
            let glyph = if font.lookup_glyph_index(character) == 0 {
                if missing.len() < 64 {
                    missing.insert(format!("U+{:04X}", character as u32));
                }
                '\u{fffd}'
            } else {
                character
            };
            let (metrics, mask) = font.rasterize(glyph, px);
            for y in 0..metrics.height {
                for x in 0..metrics.width {
                    let shear = if cell.style & ITALIC != 0 {
                        ((metrics.height - y) as f32 * 0.18) as i32
                    } else {
                        0
                    };
                    let gx = left as i32 + advance.round() as i32 + metrics.xmin + x as i32 + shear;
                    let gy = top as i32 + baseline as i32 - metrics.ymin - metrics.height as i32
                        + y as i32;
                    if gx < left as i32
                        || gx >= right as i32
                        || gy < top as i32
                        || gy >= (top + ch) as i32
                    {
                        continue;
                    }
                    let alpha = mask[y * metrics.width + x];
                    blend(&mut pixels, width, gx as usize, gy as usize, fg, alpha);
                    if cell.style & BOLD != 0 && (gx as usize + 1) < right {
                        blend(&mut pixels, width, gx as usize + 1, gy as usize, fg, alpha);
                    }
                }
            }
            advance += metrics.advance_width;
        }
        if cell.style & UNDERLINE != 0 {
            fill(
                &mut pixels,
                width,
                height,
                left,
                top + (baseline + 1).min(ch - 1),
                right - left,
                1,
                fg,
            );
        }
        if cell.style & STRIKE != 0 {
            fill(
                &mut pixels,
                width,
                height,
                left,
                top + baseline.saturating_sub(5 / scale),
                right - left,
                1,
                fg,
            );
        }
    }
    if let Some(cursor) = &snapshot.cursor
        && cursor.visible
    {
        let x = cursor.col as usize * cw;
        let y = cursor.row as usize * ch;
        let color = snapshot.cells[(cursor.row * snapshot.cols + cursor.col) as usize].foreground;
        match cursor.shape {
            0 => {
                for dy in 0..ch {
                    for dx in 0..cw {
                        let i = ((y + dy) * width + x + dx) * 3;
                        for c in &mut pixels[i..i + 3] {
                            *c = 255 - *c;
                        }
                    }
                }
            }
            1 => fill(&mut pixels, width, height, x, y, 1, ch, color),
            2 => fill(&mut pixels, width, height, x, y + ch - 2, cw, 2, color),
            3 => {
                fill(&mut pixels, width, height, x, y, cw, 1, color);
                fill(&mut pixels, width, height, x, y + ch - 1, cw, 1, color);
                fill(&mut pixels, width, height, x, y, 1, ch, color);
                fill(&mut pixels, width, height, x + cw - 1, y, 1, ch, color);
            }
            _ => {}
        }
    }
    let mut png = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut png, width as u32, height as u32);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header()?;
        writer.write_image_data(&pixels)?;
    }
    ensure!(png.len() <= 4 * 1024 * 1024, "screenshot_byte_limit");
    let text = snapshot
        .cells
        .chunks(snapshot.cols as usize)
        .map(|row| {
            row.iter()
                .filter(|c| c.width != 0)
                .map(|c| c.text.as_str())
                .collect::<String>()
                .trim_end_matches(' ')
                .to_owned()
        })
        .collect::<Vec<_>>()
        .join("\n");
    Ok(Capture {
        png,
        metadata: json!({"source":"rendered_terminal","epoch":snapshot.epoch,"revision":snapshot.revision,"dimensions_epoch":snapshot.dimensions_epoch,"rows":snapshot.rows,"cols":snapshot.cols,"width":width,"height":height,"cell_width":cw,"cell_height":ch,"scale":1.0/scale as f64,"font":"Noto Sans Mono CJK SC Regular","font_sha256":"ec04cc376b34887cedbdf84074e2e226ed2761eeabdcb9173fc1dd7bfd153ef7","missing_glyphs":missing,"synthetic_bold_italic":true,"text":text}),
        text,
    })
}
#[allow(clippy::too_many_arguments)]
fn fill(
    pixels: &mut [u8],
    width: usize,
    height: usize,
    x: usize,
    y: usize,
    w: usize,
    h: usize,
    color: u32,
) {
    for row in y..(y + h).min(height) {
        for col in x..(x + w).min(width) {
            let i = (row * width + col) * 3;
            pixels[i..i + 3].copy_from_slice(&[
                (color >> 16) as u8,
                (color >> 8) as u8,
                color as u8,
            ]);
        }
    }
}
fn blend(pixels: &mut [u8], width: usize, x: usize, y: usize, color: u32, alpha: u8) {
    let i = (y * width + x) * 3;
    for (n, c) in [(color >> 16) as u8, (color >> 8) as u8, color as u8]
        .iter()
        .enumerate()
    {
        pixels[i + n] = ((u32::from(*c) * u32::from(alpha)
            + u32::from(pixels[i + n]) * (255 - u32::from(alpha)))
            / 255) as u8;
    }
}
fn dim(c: u32) -> u32 {
    ((c >> 16 & 255) / 2) << 16 | ((c >> 8 & 255) / 2) << 8 | ((c & 255) / 2)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn png_is_deterministic_and_renders_wide_combining_color_and_cursor() {
        let mut engine = ai_terminal_engine::Engine::new(4, 20, 7).unwrap();
        engine.feed("\x1b[31m中e\u{301}\x1b[0m\x1b[2;2H\x1b[6 q".as_bytes());
        let snapshot = engine.snapshot();
        let a = capture(&snapshot).unwrap();
        let b = capture(&snapshot).unwrap();
        assert_eq!(a.png, b.png);
        assert!(a.text.contains("中e\u{301}"));
        assert_eq!(a.metadata["revision"], snapshot.revision);
        let mut reader = png::Decoder::new(std::io::Cursor::new(a.png))
            .read_info()
            .unwrap();
        let mut pixels = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut pixels).unwrap();
        assert_eq!((info.width, info.height), (160, 88));
        let red = (0..22)
            .flat_map(|y| (0..16).map(move |x| (y * 160 + x) * 3))
            .filter(|&i| pixels[i] > pixels[i + 1] + 20)
            .count();
        assert!(red > 20);
        let cursor = (22 * 160 + 8) * 3;
        assert_ne!(&pixels[cursor..cursor + 3], &pixels[cursor + 3..cursor + 6]);
    }
}
