//! A TrueType reader and rasteriser for the candidate window: `cmap`, `glyf`/`loca` outlines
//! (simple and composite glyphs), horizontal metrics, and font collections (.ttc). Fonts with
//! CFF outlines are skipped; the font search prefers ones with `glyf` outlines.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

fn u16_at(data: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(data.get(at..at + 2)?.try_into().ok()?))
}

fn i16_at(data: &[u8], at: usize) -> Option<i16> {
    u16_at(data, at).map(|n| n as i16)
}

fn u32_at(data: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(data.get(at..at + 4)?.try_into().ok()?))
}

#[derive(Clone, Copy)]
enum Cmap {
    Format4(usize),
    Format12(usize),
}

pub struct Font {
    data: Vec<u8>,
    glyf: usize,
    glyf_len: usize,
    loca: usize,
    loca_long: bool,
    hmtx: usize,
    num_hmetrics: u16,
    num_glyphs: u16,
    cmap: Cmap,
    pub units_per_em: f32,
    pub ascent: f32,
    pub descent: f32,
}

#[derive(Clone, Copy, Debug)]
struct Point {
    x: f32,
    y: f32,
}

impl Font {
    /// Loads the first face of a file (or collection) that has `glyf` outlines and a Unicode cmap.
    pub fn load(path: &Path) -> Option<Font> {
        let data = std::fs::read(path).ok()?;
        let offsets: Vec<usize> = if data.get(0..4) == Some(b"ttcf") {
            let count = u32_at(&data, 8)? as usize;
            (0..count.min(64)).filter_map(|i| u32_at(&data, 12 + 4 * i).map(|o| o as usize)).collect()
        } else {
            vec![0]
        };
        let face = offsets.into_iter().find_map(|offset| Self::face_tables(&data, offset))?;
        let (glyf, glyf_len, loca, head, hhea, hmtx, maxp, cmap) = face;
        let cmap = Self::pick_cmap(&data, cmap)?;
        Some(Font {
            glyf,
            glyf_len,
            loca,
            loca_long: i16_at(&data, head + 50)? != 0,
            hmtx,
            num_hmetrics: u16_at(&data, hhea + 34)?,
            num_glyphs: u16_at(&data, maxp + 4)?,
            cmap,
            units_per_em: f32::from(u16_at(&data, head + 18)?.max(16)),
            ascent: f32::from(i16_at(&data, hhea + 4)?),
            descent: f32::from(i16_at(&data, hhea + 6)?),
            data,
        })
    }

    #[allow(clippy::type_complexity)]
    fn face_tables(data: &[u8], offset: usize) -> Option<(usize, usize, usize, usize, usize, usize, usize, usize)> {
        let count = u16_at(data, offset + 4)? as usize;
        let mut tables = HashMap::new();
        for i in 0..count {
            let record = offset + 12 + 16 * i;
            let tag = data.get(record..record + 4)?;
            let start = u32_at(data, record + 8)? as usize;
            let len = u32_at(data, record + 12)? as usize;
            if start.checked_add(len)? > data.len() {
                return None;
            }
            tables.insert(tag.to_vec(), (start, len));
        }
        let table = |tag: &[u8]| tables.get(tag).copied();
        let (glyf, glyf_len) = table(b"glyf")?;
        Some((glyf, glyf_len, table(b"loca")?.0, table(b"head")?.0, table(b"hhea")?.0, table(b"hmtx")?.0, table(b"maxp")?.0, table(b"cmap")?.0))
    }

    fn pick_cmap(data: &[u8], cmap: usize) -> Option<Cmap> {
        let count = u16_at(data, cmap + 2)? as usize;
        let mut best: Option<(u8, Cmap)> = None;
        for i in 0..count {
            let record = cmap + 4 + 8 * i;
            let (platform, encoding) = (u16_at(data, record)?, u16_at(data, record + 2)?);
            let sub = cmap + u32_at(data, record + 4)? as usize;
            let format = u16_at(data, sub)?;
            let unicode = platform == 0 || (platform == 3 && (encoding == 1 || encoding == 10));
            let candidate = match format {
                12 if unicode => (2, Cmap::Format12(sub)),
                4 if unicode => (1, Cmap::Format4(sub)),
                _ => continue,
            };
            if best.is_none_or(|(rank, _)| candidate.0 > rank) {
                best = Some(candidate);
            }
        }
        best.map(|(_, cmap)| cmap)
    }

    /// The glyph for a character, or 0 (.notdef) when the font does not have it.
    pub fn glyph_index(&self, c: char) -> u16 {
        let code = c as u32;
        let data = &self.data;
        let found = match self.cmap {
            Cmap::Format12(sub) => (|| {
                let groups = u32_at(data, sub + 12)? as usize;
                let (mut lo, mut hi) = (0, groups);
                while lo < hi {
                    let mid = (lo + hi) / 2;
                    let group = sub + 16 + 12 * mid;
                    let (start, end) = (u32_at(data, group)?, u32_at(data, group + 4)?);
                    if code < start {
                        hi = mid;
                    } else if code > end {
                        lo = mid + 1;
                    } else {
                        return u16::try_from(u32_at(data, group + 8)? + (code - start)).ok();
                    }
                }
                None
            })(),
            Cmap::Format4(sub) => (|| {
                if code > 0xffff {
                    return None;
                }
                let code = code as u16;
                let segments = u16_at(data, sub + 6)? as usize / 2;
                let ends = sub + 14;
                let starts = ends + 2 * segments + 2;
                let deltas = starts + 2 * segments;
                let ranges = deltas + 2 * segments;
                for i in 0..segments {
                    if code > u16_at(data, ends + 2 * i)? {
                        continue;
                    }
                    let start = u16_at(data, starts + 2 * i)?;
                    if code < start {
                        return None;
                    }
                    let delta = u16_at(data, deltas + 2 * i)?;
                    let range = u16_at(data, ranges + 2 * i)? as usize;
                    if range == 0 {
                        return Some(code.wrapping_add(delta));
                    }
                    let at = ranges + 2 * i + range + 2 * (code - start) as usize;
                    let glyph = u16_at(data, at)?;
                    return Some(if glyph == 0 { 0 } else { glyph.wrapping_add(delta) });
                }
                None
            })(),
        };
        found.filter(|&g| g < self.num_glyphs).unwrap_or(0)
    }

    pub fn advance(&self, glyph: u16) -> f32 {
        let index = glyph.min(self.num_hmetrics.saturating_sub(1)) as usize;
        f32::from(u16_at(&self.data, self.hmtx + 4 * index).unwrap_or(0))
    }

    fn glyph_range(&self, glyph: u16) -> Option<(usize, usize)> {
        let g = glyph as usize;
        let (start, end) = if self.loca_long {
            (u32_at(&self.data, self.loca + 4 * g)? as usize, u32_at(&self.data, self.loca + 4 * g + 4)? as usize)
        } else {
            (u16_at(&self.data, self.loca + 2 * g)? as usize * 2, u16_at(&self.data, self.loca + 2 * g + 2)? as usize * 2)
        };
        (start < end && end <= self.glyf_len).then_some((self.glyf + start, self.glyf + end))
    }

    /// The glyph's contours in font units, each a closed list of on- and off-curve points.
    fn outline(&self, glyph: u16, depth: u32, out: &mut Vec<Vec<(Point, bool)>>, transform: [f32; 6]) -> Option<()> {
        let Some((start, _)) = self.glyph_range(glyph) else { return Some(()) };
        let data = &self.data;
        let contours = i16_at(data, start)?;
        let apply = |x: f32, y: f32| Point { x: transform[0] * x + transform[2] * y + transform[4], y: transform[1] * x + transform[3] * y + transform[5] };
        if contours >= 0 {
            let contours = contours as usize;
            let mut ends = Vec::with_capacity(contours);
            for i in 0..contours {
                ends.push(u16_at(data, start + 10 + 2 * i)? as usize);
            }
            let points = ends.last().map_or(0, |&e| e + 1);
            let instructions = u16_at(data, start + 10 + 2 * contours)? as usize;
            let mut at = start + 12 + 2 * contours + instructions;
            let mut flags = Vec::with_capacity(points);
            while flags.len() < points {
                let flag = *data.get(at)?;
                at += 1;
                flags.push(flag);
                if flag & 8 != 0 {
                    let repeat = *data.get(at)?;
                    at += 1;
                    for _ in 0..repeat {
                        flags.push(flag);
                    }
                }
            }
            flags.truncate(points);
            let mut read_coords = |short: u8, same: u8| -> Option<Vec<f32>> {
                let mut values = Vec::with_capacity(points);
                let mut value = 0i32;
                for &flag in &flags {
                    if flag & short != 0 {
                        let delta = i32::from(*data.get(at)?);
                        at += 1;
                        value += if flag & same != 0 { delta } else { -delta };
                    } else if flag & same == 0 {
                        value += i32::from(i16_at(data, at)?);
                        at += 2;
                    }
                    values.push(value as f32);
                }
                Some(values)
            };
            let xs = read_coords(2, 16)?;
            let ys = read_coords(4, 32)?;
            let mut first = 0;
            for end in ends {
                if end < first || end >= points {
                    return None;
                }
                out.push((first..=end).map(|i| (apply(xs[i], ys[i]), flags[i] & 1 != 0)).collect());
                first = end + 1;
            }
            return Some(());
        }
        if depth > 8 {
            return None;
        }
        let mut at = start + 10;
        loop {
            let flags = u16_at(data, at)?;
            let component = u16_at(data, at + 2)?;
            at += 4;
            let (dx, dy) = if flags & 1 != 0 {
                let v = (f32::from(i16_at(data, at)?), f32::from(i16_at(data, at + 2)?));
                at += 4;
                v
            } else {
                let v = (f32::from(*data.get(at)? as i8), f32::from(*data.get(at + 1)? as i8));
                at += 2;
                v
            };
            let (dx, dy) = if flags & 2 != 0 { (dx, dy) } else { (0.0, 0.0) };
            let f2dot14 = |at: usize| i16_at(data, at).map(|v| f32::from(v) / 16384.0);
            let (a, b, c, d) = if flags & 8 != 0 {
                let s = f2dot14(at)?;
                at += 2;
                (s, 0.0, 0.0, s)
            } else if flags & 0x40 != 0 {
                let v = (f2dot14(at)?, 0.0, 0.0, f2dot14(at + 2)?);
                at += 4;
                v
            } else if flags & 0x80 != 0 {
                let v = (f2dot14(at)?, f2dot14(at + 2)?, f2dot14(at + 4)?, f2dot14(at + 6)?);
                at += 8;
                v
            } else {
                (1.0, 0.0, 0.0, 1.0)
            };
            let [ta, tb, tc, td, tx, ty] = transform;
            // The component's transform followed by the parent's.
            let combined = [ta * a + tc * b, tb * a + td * b, ta * c + tc * d, tb * c + td * d, ta * dx + tc * dy + tx, tb * dx + td * dy + ty];
            self.outline(component, depth + 1, out, combined)?;
            if flags & 0x20 == 0 {
                return Some(());
            }
        }
    }

    /// Renders a glyph at `scale` pixels per font unit.
    pub fn rasterize(&self, glyph: u16, scale: f32) -> Bitmap {
        let mut contours = Vec::new();
        let ok = self.outline(glyph, 0, &mut contours, [scale, 0.0, 0.0, -scale, 0.0, 0.0]).is_some();
        if !ok {
            contours.clear();
        }
        let mut lines = Vec::new();
        for contour in &contours {
            flatten(contour, &mut lines);
        }
        let advance = self.advance(glyph) * scale;
        if lines.is_empty() {
            return Bitmap { width: 0, height: 0, left: 0, top: 0, coverage: Vec::new(), advance };
        }
        let (mut min_x, mut min_y, mut max_x, mut max_y) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for (a, _) in &lines {
            min_x = min_x.min(a.x);
            max_x = max_x.max(a.x);
            min_y = min_y.min(a.y);
            max_y = max_y.max(a.y);
        }
        let left = min_x.floor() as i32 - 1;
        let top = min_y.floor() as i32 - 1;
        let width = (max_x.ceil() as i32 - left + 2).clamp(1, 2048) as usize;
        let height = (max_y.ceil() as i32 - top + 2).clamp(1, 2048) as usize;
        let mut raster = Raster::new(width, height);
        let shift = |p: Point| Point { x: p.x - left as f32, y: p.y - top as f32 };
        for (a, b) in &lines {
            raster.line(shift(*a), shift(*b));
        }
        // `top` is in y-down pixels relative to the baseline, so the glyph rises above it by -top.
        Bitmap { width, height, left, top: -top, coverage: raster.coverage(), advance }
    }
}

/// Turns a TrueType contour (quadratic, with implied on-curve midpoints) into line segments.
fn flatten(contour: &[(Point, bool)], lines: &mut Vec<(Point, Point)>) {
    let n = contour.len();
    if n < 2 {
        return;
    }
    let mid = |a: Point, b: Point| Point { x: (a.x + b.x) / 2.0, y: (a.y + b.y) / 2.0 };
    // Start at an on-curve point, or at the midpoint of two off-curve points.
    let start_index = contour.iter().position(|&(_, on)| on);
    let (start, first) = match start_index {
        Some(i) => (contour[i].0, i),
        None => (mid(contour[0].0, contour[1].0), 0),
    };
    let mut current = start;
    let mut control: Option<Point> = None;
    let order = (1..=n).map(|k| contour[(first + k) % n]);
    let quad = |from: Point, ctrl: Point, to: Point, lines: &mut Vec<(Point, Point)>| {
        let length = (from.x - ctrl.x).hypot(from.y - ctrl.y) + (ctrl.x - to.x).hypot(ctrl.y - to.y);
        let steps = (length / 2.0).ceil().clamp(1.0, 32.0) as usize;
        let mut previous = from;
        for step in 1..=steps {
            let t = step as f32 / steps as f32;
            let u = 1.0 - t;
            let p = Point { x: u * u * from.x + 2.0 * u * t * ctrl.x + t * t * to.x, y: u * u * from.y + 2.0 * u * t * ctrl.y + t * t * to.y };
            lines.push((previous, p));
            previous = p;
        }
    };
    for (point, on) in order {
        match (on, control) {
            (true, None) => {
                lines.push((current, point));
                current = point;
            }
            (true, Some(ctrl)) => {
                quad(current, ctrl, point, lines);
                current = point;
                control = None;
            }
            (false, None) => control = Some(point),
            (false, Some(ctrl)) => {
                let m = mid(ctrl, point);
                quad(current, ctrl, m, lines);
                current = m;
                control = Some(point);
            }
        }
    }
    match control {
        Some(ctrl) => quad(current, ctrl, start, lines),
        None if current.x != start.x || current.y != start.y => lines.push((current, start)),
        None => {}
    }
}

/// Signed-area accumulation rasteriser: each line adds its coverage to the cells it crosses,
/// and a running sum over the buffer gives the coverage of every pixel.
struct Raster {
    width: usize,
    height: usize,
    acc: Vec<f32>,
}

impl Raster {
    fn new(width: usize, height: usize) -> Raster {
        Raster { width, height, acc: vec![0.0; width * height + 4] }
    }

    fn add(&mut self, index: isize, value: f32) {
        if let Some(cell) = usize::try_from(index).ok().and_then(|i| self.acc.get_mut(i)) {
            *cell += value;
        }
    }

    fn line(&mut self, p0: Point, p1: Point) {
        if (p0.y - p1.y).abs() <= f32::EPSILON {
            return;
        }
        let (dir, p0, p1) = if p0.y < p1.y { (1.0, p0, p1) } else { (-1.0, p1, p0) };
        let dxdy = (p1.x - p0.x) / (p1.y - p0.y);
        let mut x = p0.x;
        let y_start = p0.y.max(0.0) as usize;
        if p0.y < 0.0 {
            x -= p0.y * dxdy;
        }
        let y_end = self.height.min(p1.y.ceil().max(0.0) as usize);
        for y in y_start..y_end {
            let row = (y * self.width) as isize;
            let dy = ((y + 1) as f32).min(p1.y) - (y as f32).max(p0.y);
            let x_next = x + dxdy * dy;
            let d = dy * dir;
            let (x0, x1) = if x < x_next { (x, x_next) } else { (x_next, x) };
            let x0_floor = x0.floor();
            let x0i = x0_floor as isize;
            let x1_ceil = x1.ceil();
            let x1i = x1_ceil as isize;
            if x1i <= x0i + 1 {
                let xmf = 0.5 * (x + x_next) - x0_floor;
                self.add(row + x0i, d - d * xmf);
                self.add(row + x0i + 1, d * xmf);
            } else {
                let s = (x1 - x0).recip();
                let x0f = x0 - x0_floor;
                let a0 = 0.5 * s * (1.0 - x0f) * (1.0 - x0f);
                let x1f = x1 - x1_ceil + 1.0;
                let am = 0.5 * s * x1f * x1f;
                self.add(row + x0i, d * a0);
                if x1i == x0i + 2 {
                    self.add(row + x0i + 1, d * (1.0 - a0 - am));
                } else {
                    let a1 = s * (1.5 - x0f);
                    self.add(row + x0i + 1, d * (a1 - a0));
                    for xi in x0i + 2..x1i - 1 {
                        self.add(row + xi, d * s);
                    }
                    let a2 = a1 + (x1i - x0i - 3) as f32 * s;
                    self.add(row + x1i - 1, d * (1.0 - a2 - am));
                }
                self.add(row + x1i, d * am);
            }
            x = x_next;
        }
    }

    fn coverage(&self) -> Vec<u8> {
        let mut sum = 0.0f32;
        self.acc[..self.width * self.height]
            .iter()
            .map(|&a| {
                sum += a;
                (sum.abs().min(1.0) * 255.0 + 0.5) as u8
            })
            .collect()
    }
}

/// A rendered glyph: coverage from 0 to 255, placed `left` pixels right of the pen and `top`
/// pixels above the baseline.
pub struct Bitmap {
    pub width: usize,
    pub height: usize,
    pub left: i32,
    pub top: i32,
    pub coverage: Vec<u8>,
    pub advance: f32,
}

/// Fonts tried in order for each character, at one pixel size.
pub struct Fonts {
    fonts: Vec<Font>,
    size: f32,
    cache: HashMap<char, Rc<Bitmap>>,
}

impl Fonts {
    pub fn new(fonts: Vec<Font>, size: f32) -> Fonts {
        Fonts { fonts, size, cache: HashMap::new() }
    }

    /// Loads the configured fonts, then Latin, Chinese, Japanese and Korean fonts from fontconfig,
    /// then well-known paths.
    pub fn discover(configured: &[PathBuf], size: f32) -> Fonts {
        let mut paths: Vec<PathBuf> = configured.to_vec();
        for pattern in ["sans:fontformat=TrueType", ":lang=zh-cn:fontformat=TrueType", ":lang=zh-tw:fontformat=TrueType", ":lang=ja:fontformat=TrueType", ":lang=ko:fontformat=TrueType"] {
            paths.extend(fontconfig(pattern));
        }
        for known in [
            "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
            "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
            "/usr/share/fonts/truetype/droid/DroidSansFallbackFull.ttf",
            "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
            "/usr/share/fonts/truetype/wqy/wqy-zenhei.ttc",
            "/usr/share/fonts/truetype/arphic/uming.ttc",
            "/usr/share/fonts/truetype/unfonts-core/UnDotum.ttf",
            "/usr/share/fonts/truetype/nanum/NanumGothic.ttf",
            "/usr/share/fonts/opentype/ipafont-gothic/ipag.ttf",
            "/usr/share/fonts/truetype/fonts-japanese-gothic.ttf",
            "/usr/share/fonts/TTF/DejaVuSans.ttf",
            "/usr/share/fonts/droid/DroidSansFallbackFull.ttf",
            "/usr/share/fonts/wenquanyi/wqy-microhei/wqy-microhei.ttc",
        ] {
            paths.push(PathBuf::from(known));
        }
        let mut seen = std::collections::HashSet::new();
        let fonts: Vec<Font> = paths.into_iter().filter(|p| seen.insert(p.clone())).filter_map(|p| Font::load(&p)).take(8).collect();
        if fonts.is_empty() {
            eprintln!("min-keyboard: no TrueType font found; set fonts = /path/to/font.ttf under [ime] in the config file");
        }
        Fonts::new(fonts, size)
    }

    pub fn is_empty(&self) -> bool {
        self.fonts.is_empty()
    }

    pub fn ascent(&self) -> f32 {
        self.fonts.first().map_or(self.size * 0.8, |f| (f.ascent / f.units_per_em * self.size).ceil())
    }

    pub fn line_height(&self) -> f32 {
        let descent = self.fonts.first().map_or(self.size * 0.2, |f| (-f.descent / f.units_per_em * self.size).ceil());
        (self.ascent() + descent).max(self.size * 1.2).ceil()
    }

    pub fn glyph(&mut self, c: char) -> Rc<Bitmap> {
        if let Some(bitmap) = self.cache.get(&c) {
            return bitmap.clone();
        }
        let found = self.fonts.iter().find_map(|f| Some((f, f.glyph_index(c))).filter(|(_, g)| *g != 0));
        let bitmap = match (found, self.fonts.first()) {
            (Some((font, glyph)), _) => font.rasterize(glyph, self.size / font.units_per_em),
            (None, Some(font)) => font.rasterize(0, self.size / font.units_per_em),
            (None, None) => Bitmap { width: 0, height: 0, left: 0, top: 0, coverage: Vec::new(), advance: self.size * 0.5 },
        };
        let bitmap = Rc::new(bitmap);
        if self.cache.len() > 4096 {
            self.cache.clear();
        }
        self.cache.insert(c, bitmap.clone());
        bitmap
    }

    pub fn measure(&mut self, text: &str) -> f32 {
        text.chars().map(|c| self.glyph(c).advance).sum()
    }
}

fn fontconfig(pattern: &str) -> Vec<PathBuf> {
    let output = std::process::Command::new("fc-list").args([pattern, "file"]).output();
    let Ok(output) = output else { return Vec::new() };
    let mut files: Vec<PathBuf> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.trim().strip_suffix(':').map(str::trim).map(PathBuf::from))
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("ttf") || e.eq_ignore_ascii_case("ttc")))
        .collect();
    // Prefer regular weights: their file names usually say so, or say nothing.
    files.sort_by_key(|p| {
        let name = p.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
        let heavy = ["bold", "black", "light", "thin", "italic", "oblique", "mono", "serif", "condensed"].iter().any(|w| name.contains(w));
        (heavy, !name.contains("regular"), name)
    });
    files.truncate(2);
    files
}

/// A 32-bit pixel buffer in 0xAARRGGBB, which both X11 (32 bits per pixel, depth 24) and
/// Wayland (ARGB8888) take as it is on little-endian machines.
pub struct Canvas {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<u32>,
}

impl Canvas {
    pub fn new(width: usize, height: usize, color: u32) -> Canvas {
        Canvas { width, height, pixels: vec![color; width * height] }
    }

    pub fn fill(&mut self, x: i32, y: i32, w: i32, h: i32, color: u32) {
        let (x0, y0) = (x.max(0) as usize, y.max(0) as usize);
        let x1 = ((x + w).max(0) as usize).min(self.width);
        let y1 = ((y + h).max(0) as usize).min(self.height);
        for row in y0..y1 {
            for px in &mut self.pixels[row * self.width + x0.min(x1)..row * self.width + x1] {
                *px = color;
            }
        }
    }

    fn blend(&mut self, x: i32, y: i32, color: u32, alpha: u8) {
        if x < 0 || y < 0 || x as usize >= self.width || y as usize >= self.height || alpha == 0 {
            return;
        }
        let px = &mut self.pixels[y as usize * self.width + x as usize];
        let a = u32::from(alpha);
        let mix = |shift: u32| {
            let fg = (color >> shift) & 0xff;
            let bg = (*px >> shift) & 0xff;
            ((fg * a + bg * (255 - a) + 127) / 255) << shift
        };
        *px = 0xff00_0000 | mix(16) | mix(8) | mix(0);
    }

    /// Draws `text` with its baseline at `y`, returning the pen position after it.
    pub fn text(&mut self, fonts: &mut Fonts, x: f32, y: i32, text: &str, color: u32) -> f32 {
        let mut pen = x;
        for c in text.chars() {
            let glyph = fonts.glyph(c);
            let gx = pen.round() as i32 + glyph.left;
            let gy = y - glyph.top;
            for row in 0..glyph.height {
                for col in 0..glyph.width {
                    self.blend(gx + col as i32, gy + row as i32, color, glyph.coverage[row * glyph.width + col]);
                }
            }
            pen += glyph.advance;
        }
        pen
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn some_font() -> Option<Font> {
        let fonts = Fonts::discover(&[], 20.0);
        fonts.fonts.into_iter().next()
    }

    #[test]
    fn rasterizes_a_square() {
        let mut raster = Raster::new(10, 10);
        let p = |x, y| Point { x, y };
        let square = [p(2.0, 2.0), p(8.0, 2.0), p(8.0, 8.0), p(2.0, 8.0)];
        for i in 0..4 {
            raster.line(square[i], square[(i + 1) % 4]);
        }
        let coverage = raster.coverage();
        assert_eq!(coverage[5 * 10 + 5], 255);
        assert_eq!(coverage[5 * 10 + 1], 0);
        assert_eq!(coverage[0], 0);
        assert_eq!(coverage.iter().filter(|&&c| c == 255).count(), 36);
    }

    #[test]
    fn flattens_off_curve_contours() {
        let p = |x, y| Point { x, y };
        let mut lines = Vec::new();
        flatten(&[(p(0.0, 0.0), false), (p(10.0, 0.0), false), (p(10.0, 10.0), false), (p(0.0, 10.0), false)], &mut lines);
        let first = lines.first().unwrap().0;
        let last = lines.last().unwrap().1;
        assert!((first.x - last.x).abs() < 1e-4 && (first.y - last.y).abs() < 1e-4, "contour closes");
    }

    /// Runs when the machine has a TrueType font (CI images and desktops normally do).
    #[test]
    fn renders_installed_font() {
        let Some(font) = some_font() else { return };
        let glyph = font.glyph_index('A');
        assert_ne!(glyph, 0);
        let bitmap = font.rasterize(glyph, 24.0 / font.units_per_em);
        assert!(bitmap.width > 5 && bitmap.height > 10);
        assert!(bitmap.top > 10, "A stands on the baseline");
        assert!(bitmap.coverage.iter().any(|&c| c == 255));
        let mut fonts = Fonts::new(vec![font], 24.0);
        let mut canvas = Canvas::new(100, 40, 0xffff_ffff);
        let end = canvas.text(&mut fonts, 4.0, 30, "Ab", 0xff00_0000);
        assert!(end > 20.0);
        assert!(canvas.pixels.iter().any(|&p| p == 0xff00_0000));
    }
}
