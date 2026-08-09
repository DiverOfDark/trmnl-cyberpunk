//! Pixel-direct dashboard renderer.
//!
//! We draw straight into an 800×480 indexed-color framebuffer (one byte per
//! pixel = palette index 0..6) and encode the result as a 4-bit indexed PNG.
//! No browser, no antialiasing, no dithering — every pixel is exactly one of
//! the six panel colors, so the panel renders what we drew.

use core::convert::Infallible;

use embedded_graphics::pixelcolor::Rgb888;
use embedded_graphics::prelude::*;
use embedded_graphics::Pixel;

pub const W: u32 = 800;
pub const H: u32 = 480;

// ── Palette ────────────────────────────────────────────────────────────────
//
// Index order MUST match the device firmware's expectation (Spectra 6 panel
// at src/display.cpp:749-756 in the E1002 firmware). The firmware reads the
// per-pixel index directly and looks up its own measured RGB on the panel —
// so the order below is what determines which physical ink each pixel gets.
//
// RGB values are the *measured* on-panel colors (also from the firmware
// table). They're muted compared to vivid sRGB equivalents — the rendered
// PNG looks slightly olive/dim on a normal monitor but accurately previews
// what the panel displays.

#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum C {
    Black  = 0,
    White  = 1,
    Yellow = 2,
    Red    = 3,
    Blue   = 4,
    Green  = 5,
}

impl C {
    pub fn rgb(self) -> Rgb888 {
        match self {
            C::Black  => Rgb888::new(0x02, 0x02, 0x02),
            C::White  => Rgb888::new(0xb3, 0xb6, 0xab),
            C::Yellow => Rgb888::new(0xcd, 0xca, 0x00),
            C::Red    => Rgb888::new(0x75, 0x0a, 0x00),
            C::Blue   => Rgb888::new(0x00, 0x2f, 0x6b),
            C::Green  => Rgb888::new(0x21, 0x45, 0x28),
        }
    }

    pub fn index(self) -> u8 {
        self as u8
    }
}

const PALETTE_ORDER: [C; 6] = [C::Black, C::White, C::Yellow, C::Red, C::Blue, C::Green];

pub fn palette_bytes() -> Vec<u8> {
    PALETTE_ORDER
        .iter()
        .flat_map(|c| {
            let rgb = c.rgb();
            [rgb.r(), rgb.g(), rgb.b()]
        })
        .collect()
}

/// Closest palette match for an arbitrary RGB. Used by the embedded-graphics
/// `DrawTarget` impl so glyph anti-edges (which u8g2-fonts won't emit, but
/// composed primitives might) snap to a real palette color rather than
/// silently writing a transparent pixel.
fn nearest_index(rgb: Rgb888) -> u8 {
    let mut best = 0u8;
    let mut best_d = i32::MAX;
    for c in PALETTE_ORDER {
        let p = c.rgb();
        let dr = rgb.r() as i32 - p.r() as i32;
        let dg = rgb.g() as i32 - p.g() as i32;
        let db = rgb.b() as i32 - p.b() as i32;
        let d = dr * dr + dg * dg + db * db;
        if d < best_d {
            best_d = d;
            best = c.index();
        }
    }
    best
}

// ── Canvas ─────────────────────────────────────────────────────────────────

pub struct Canvas {
    pub buf: Vec<u8>,
    pub w: u32,
    pub h: u32,
}

impl Canvas {
    pub fn new(w: u32, h: u32) -> Self {
        Self { buf: vec![C::White.index(); (w * h) as usize], w, h }
    }

    pub fn fill(&mut self, c: C) {
        self.buf.fill(c.index());
    }

    #[inline]
    pub fn put(&mut self, x: i32, y: i32, c: C) {
        if x < 0 || y < 0 || (x as u32) >= self.w || (y as u32) >= self.h {
            return;
        }
        self.buf[(y as u32 * self.w + x as u32) as usize] = c.index();
    }

    pub fn fill_rect(&mut self, r: Rect, c: C) {
        let x0 = r.x.max(0);
        let y0 = r.y.max(0);
        let x1 = ((r.x + r.w as i32).min(self.w as i32)).max(0);
        let y1 = ((r.y + r.h as i32).min(self.h as i32)).max(0);
        let idx = c.index();
        for y in y0..y1 {
            let row = (y as u32 * self.w) as usize;
            for x in x0..x1 {
                self.buf[row + x as usize] = idx;
            }
        }
    }

    /// Outline rectangle with `stroke`-thick borders inside the bounds.
    pub fn stroke_rect(&mut self, r: Rect, stroke: u32, c: C) {
        let s = stroke as i32;
        // Top + bottom
        self.fill_rect(Rect { x: r.x, y: r.y, w: r.w, h: stroke }, c);
        self.fill_rect(Rect { x: r.x, y: r.y + r.h as i32 - s, w: r.w, h: stroke }, c);
        // Left + right
        self.fill_rect(Rect { x: r.x, y: r.y, w: stroke, h: r.h }, c);
        self.fill_rect(Rect { x: r.x + r.w as i32 - s, y: r.y, w: stroke, h: r.h }, c);
    }

    pub fn hline(&mut self, x: i32, y: i32, w: u32, c: C) {
        self.fill_rect(Rect { x, y, w, h: 1 }, c);
    }

    pub fn vline(&mut self, x: i32, y: i32, h: u32, c: C) {
        self.fill_rect(Rect { x, y, w: 1, h }, c);
    }

    /// One-pixel Bresenham line. The panel is an indexed framebuffer written
    /// pixel-exact — no antialiasing, no dithering — so a 1px stroke stays a
    /// crisp 1px stroke and needs no thickening to survive.
    ///
    /// `dash` draws only every other pixel, which is how a provisional series
    /// distinguishes itself from a settled one without spending a colour.
    pub fn line(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, c: C, dash: bool) {
        let (dx, dy) = ((x1 - x0).abs(), -(y1 - y0).abs());
        let (sx, sy) = (if x0 < x1 { 1 } else { -1 }, if y0 < y1 { 1 } else { -1 });
        let (mut x, mut y, mut err, mut n) = (x0, y0, dx + dy, 0u32);
        loop {
            if !dash || n % 2 == 0 {
                self.put(x, y, c);
            }
            n += 1;
            if x == x1 && y == y1 {
                break;
            }
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                x += sx;
            }
            if e2 <= dx {
                err += dx;
                y += sy;
            }
        }
    }

    /// Filled left-leaning parallelogram: top edge from (x,y) to (x+w,y),
    /// bottom edge offset by `slant` to the left. Used for the TRMNL-01 tab.
    pub fn fill_left_parallelogram(&mut self, x: i32, y: i32, w: u32, h: u32, slant: u32, c: C) {
        let h = h as i32;
        let slant = slant as i32;
        for dy in 0..h {
            let cut = slant * dy / h;
            let row_w = (w as i32 - cut).max(0) as u32;
            self.fill_rect(Rect { x, y: y + dy, w: row_w, h: 1 }, c);
        }
    }

    /// Diagonal hatched fill repeating a 4-stop pattern at 135° angle.
    /// Mirrors the original CSS `repeating-linear-gradient(135deg, ...)`.
    pub fn hatch_135(&mut self, r: Rect, stops: &[(u32, C)]) {
        let total: u32 = stops.iter().map(|(w, _)| *w).sum();
        if total == 0 { return; }
        let x0 = r.x.max(0);
        let y0 = r.y.max(0);
        let x1 = ((r.x + r.w as i32).min(self.w as i32)).max(0);
        let y1 = ((r.y + r.h as i32).min(self.h as i32)).max(0);

        for y in y0..y1 {
            let row = (y as u32 * self.w) as usize;
            for x in x0..x1 {
                // 135° gradient line: position along (x+y) axis.
                let t = ((x - r.x + (y - r.y)).rem_euclid(total as i32)) as u32;
                let mut acc = 0u32;
                let mut idx = stops[0].1.index();
                for (w, c) in stops {
                    acc += *w;
                    if t < acc {
                        idx = c.index();
                        break;
                    }
                }
                self.buf[row + x as usize] = idx;
            }
        }
    }

    /// Encode the canvas as a 4-bit indexed PNG (best zlib compression).
    pub fn into_png(self) -> anyhow::Result<Vec<u8>> {
        let row_bytes = self.w.div_ceil(2) as usize;
        let mut packed = vec![0u8; row_bytes * self.h as usize];
        for y in 0..self.h as usize {
            for x in 0..self.w as usize {
                let v = self.buf[y * self.w as usize + x] & 0x0f;
                let dst = y * row_bytes + x / 2;
                if x & 1 == 0 {
                    packed[dst] |= v << 4;
                } else {
                    packed[dst] |= v;
                }
            }
        }

        let mut out = Vec::with_capacity(32 * 1024);
        {
            let mut encoder = png::Encoder::new(&mut out, self.w, self.h);
            encoder.set_color(png::ColorType::Indexed);
            encoder.set_depth(png::BitDepth::Four);
            encoder.set_palette(palette_bytes());
            encoder.set_compression(png::Compression::Best);
            let mut writer = encoder.write_header()?;
            writer.write_image_data(&packed)?;
        }
        Ok(out)
    }
}

// ── embedded-graphics integration ──────────────────────────────────────────

impl OriginDimensions for Canvas {
    fn size(&self) -> Size {
        Size::new(self.w, self.h)
    }
}

impl DrawTarget for Canvas {
    type Color = Rgb888;
    type Error = Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Rgb888>>,
    {
        for Pixel(p, color) in pixels {
            if p.x < 0 || p.y < 0 || (p.x as u32) >= self.w || (p.y as u32) >= self.h {
                continue;
            }
            self.buf[(p.y as u32 * self.w + p.x as u32) as usize] = nearest_index(color);
        }
        Ok(())
    }
}

// ── Geometry helpers ───────────────────────────────────────────────────────

#[derive(Copy, Clone, Debug)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

impl Rect {
    pub fn new(x: i32, y: i32, w: u32, h: u32) -> Self { Self { x, y, w, h } }
    pub fn right(&self) -> i32 { self.x + self.w as i32 }
    pub fn bottom(&self) -> i32 { self.y + self.h as i32 }
}
