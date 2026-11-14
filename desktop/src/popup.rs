//! Lays out and draws the candidate window: the composition (when the application does not
//! show it), the candidates of the current page, and a short language notice after switching.

use crate::font::{Canvas, Fonts};
use crate::session::View;

pub struct Theme {
    background: u32,
    border: u32,
    text: u32,
    dim: u32,
    highlight: u32,
    highlight_text: u32,
}

impl Theme {
    pub fn new(dark: bool) -> Theme {
        if dark {
            Theme { background: 0xff2b_2d31, border: 0xff5a_5d63, text: 0xffec_edf0, dim: 0xff9a_9ca3, highlight: 0xff35_74f0, highlight_text: 0xffff_ffff }
        } else {
            Theme { background: 0xffff_ffff, border: 0xffb4_b7bd, text: 0xff20_2124, dim: 0xff80_8389, highlight: 0xff2f_6fe4, highlight_text: 0xffff_ffff }
        }
    }
}

/// Where a candidate was drawn, for clicks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hit {
    pub x0: i32,
    pub x1: i32,
    pub y0: i32,
    pub y1: i32,
    /// The candidate's index within the page.
    pub index: usize,
}

pub struct Popup {
    pub canvas: Canvas,
    pub hits: Vec<Hit>,
}

impl Popup {
    pub fn hit(&self, x: i32, y: i32) -> Option<usize> {
        self.hits.iter().find(|h| x >= h.x0 && x < h.x1 && y >= h.y0 && y < h.y1).map(|h| h.index)
    }
}

const PAD: f32 = 6.0;
const GAP: f32 = 14.0;
const MAX_WIDTH: f32 = 1200.0;

/// Draws the window, or returns `None` when there is nothing to show.
pub fn render(fonts: &mut Fonts, theme: &Theme, view: &View, show_preedit: bool, notice: Option<&str>) -> Option<Popup> {
    let line = fonts.line_height();
    let ascent = fonts.ascent();
    let preedit = (show_preedit && !view.preedit.is_empty()).then_some(view.preedit.as_str());
    let mut rows = 0;
    if notice.is_some() {
        rows += 1;
    }
    if preedit.is_some() {
        rows += 1;
    }
    let has_candidates = !view.candidates.is_empty();
    if has_candidates {
        rows += 1;
    }
    if rows == 0 {
        return None;
    }

    // Measure the candidate row first, since it usually sets the width.
    let label = |i: usize| if view.labels { format!("{}", i + 1) } else { String::new() };
    let mut items = Vec::new();
    for (i, candidate) in view.candidates.iter().enumerate() {
        let label = label(i);
        let label_width = if label.is_empty() { 0.0 } else { fonts.measure(&label) + 4.0 };
        items.push((label, label_width, fonts.measure(candidate)));
    }
    let arrows = if view.has_previous_page || view.has_next_page { fonts.measure("‹ ›") + GAP } else { 0.0 };
    let candidates_width: f32 = items.iter().map(|(_, l, c)| l + c + GAP).sum::<f32>() - if items.is_empty() { 0.0 } else { GAP } + arrows;
    let preedit_width = preedit.map_or(0.0, |p| fonts.measure(p));
    let notice_width = notice.map_or(0.0, |n| fonts.measure(n));
    let width = (candidates_width.max(preedit_width).max(notice_width) + 2.0 * PAD).clamp(line * 2.0, MAX_WIDTH).ceil() as usize;
    let height = (rows as f32 * line + 2.0 * PAD).ceil() as usize;

    let mut canvas = Canvas::new(width, height, theme.background);
    let (w, h) = (width as i32, height as i32);
    canvas.fill(0, 0, w, 1, theme.border);
    canvas.fill(0, h - 1, w, 1, theme.border);
    canvas.fill(0, 0, 1, h, theme.border);
    canvas.fill(w - 1, 0, 1, h, theme.border);

    let mut top = PAD;
    let baseline = |top: f32| (top + ascent).round() as i32;
    if let Some(notice) = notice {
        canvas.text(fonts, PAD, baseline(top), notice, theme.text);
        top += line;
    }
    if let Some(preedit) = preedit {
        let end = canvas.text(fonts, PAD, baseline(top), preedit, theme.text);
        canvas.fill(PAD as i32, (top + line - 3.0) as i32, (end - PAD).ceil() as i32, 1, theme.text);
        top += line;
    }
    let mut hits = Vec::new();
    if has_candidates {
        let mut x = PAD;
        let y = baseline(top);
        for (i, ((label, label_width, candidate_width), candidate)) in items.iter().zip(&view.candidates).enumerate() {
            let x0 = x;
            let selected = view.highlight == Some(i);
            let item_width = label_width + candidate_width;
            if x + item_width > width as f32 - PAD {
                break;
            }
            if selected {
                canvas.fill((x - 3.0) as i32, top as i32, (item_width + 6.0).ceil() as i32, line as i32, theme.highlight);
            }
            let (label_color, text_color) = if selected { (theme.highlight_text, theme.highlight_text) } else { (theme.dim, theme.text) };
            if !label.is_empty() {
                canvas.text(fonts, x, y, label, label_color);
                x += label_width;
            }
            canvas.text(fonts, x, y, candidate, text_color);
            x += candidate_width;
            hits.push(Hit { x0: (x0 - GAP / 2.0) as i32, x1: (x + GAP / 2.0) as i32, y0: top as i32, y1: (top + line) as i32, index: i });
            x += GAP;
        }
        if arrows > 0.0 {
            let left = if view.has_previous_page { "‹" } else { " " };
            let right = if view.has_next_page { "›" } else { " " };
            canvas.text(fonts, width as f32 - PAD - arrows + GAP, y, &format!("{left} {right}"), theme.dim);
        }
    }
    Some(Popup { canvas, hits })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lays_out_candidates() {
        let mut fonts = Fonts::discover(&[], 18.0);
        if fonts.is_empty() {
            return;
        }
        let theme = Theme::new(false);
        let view = View { preedit: "nihao".into(), candidates: vec!["你好".into(), "你".into(), "尼".into()], highlight: Some(0), labels: true, ..View::default() };
        let popup = render(&mut fonts, &theme, &view, true, None).unwrap();
        assert_eq!(popup.hits.len(), 3);
        assert!(popup.canvas.height as f32 >= 2.0 * fonts.line_height());
        let first = popup.hits[0];
        assert_eq!(popup.hit((first.x0 + first.x1) / 2, (first.y0 + first.y1) / 2), Some(0));
        assert_eq!(popup.hit(-5, -5), None);
        assert!(render(&mut fonts, &theme, &View::default(), true, None).is_none());
        assert!(render(&mut fonts, &theme, &View::default(), true, Some("日本語")).is_some());
    }

    /// Writes a sample window to $MIN_KEYBOARD_PNG for a visual check:
    /// `MIN_KEYBOARD_PNG=/tmp/popup.png cargo test -p min-keyboard-desktop sample_png`
    #[test]
    fn sample_png() {
        let Some(path) = std::env::var_os("MIN_KEYBOARD_PNG") else { return };
        let mut fonts = Fonts::discover(&[], 20.0);
        let view = View {
            preedit: "nihao".into(),
            candidates: ["你好", "擬好", "日本語", "한국어", "ありがとう", "Hello"].map(String::from).to_vec(),
            highlight: Some(0),
            labels: true,
            has_next_page: true,
            ..View::default()
        };
        let popup = render(&mut fonts, &Theme::new(false), &view, true, Some("简体中文 (拼音)")).unwrap();
        std::fs::write(path, png(&popup.canvas)).unwrap();
    }

    fn png(canvas: &Canvas) -> Vec<u8> {
        fn crc(data: &[u8]) -> u32 {
            let mut crc = !0u32;
            for &b in data {
                crc ^= u32::from(b);
                for _ in 0..8 {
                    crc = if crc & 1 != 0 { (crc >> 1) ^ 0xedb8_8320 } else { crc >> 1 };
                }
            }
            !crc
        }
        fn chunk(out: &mut Vec<u8>, kind: &[u8], data: &[u8]) {
            out.extend_from_slice(&(data.len() as u32).to_be_bytes());
            let mut body = kind.to_vec();
            body.extend_from_slice(data);
            out.extend_from_slice(&body);
            out.extend_from_slice(&crc(&body).to_be_bytes());
        }
        let mut raw = Vec::new();
        for row in canvas.pixels.chunks(canvas.width) {
            raw.push(0);
            for p in row {
                raw.extend_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, *p as u8]);
            }
        }
        // zlib with stored (uncompressed) deflate blocks.
        let mut z = vec![0x78, 0x01];
        let blocks: Vec<&[u8]> = raw.chunks(65535).collect();
        for (i, block) in blocks.iter().enumerate() {
            z.push(u8::from(i + 1 == blocks.len()));
            z.extend_from_slice(&(block.len() as u16).to_le_bytes());
            z.extend_from_slice(&(!(block.len() as u16)).to_le_bytes());
            z.extend_from_slice(block);
        }
        let (mut a, mut b) = (1u32, 0u32);
        for &byte in &raw {
            a = (a + u32::from(byte)) % 65521;
            b = (b + a) % 65521;
        }
        z.extend_from_slice(&((b << 16) | a).to_be_bytes());
        let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
        let mut header = Vec::new();
        header.extend_from_slice(&(canvas.width as u32).to_be_bytes());
        header.extend_from_slice(&(canvas.height as u32).to_be_bytes());
        header.extend_from_slice(&[8, 2, 0, 0, 0]);
        chunk(&mut out, b"IHDR", &header);
        chunk(&mut out, b"IDAT", &z);
        chunk(&mut out, b"IEND", &[]);
        out
    }
}
