//! Layout & content rendering for the dashboard.
//!
//! Pixel coordinates here are the literal sizes from the original HTML/CSS;
//! see `templates/dashboard.html` for the source of truth. Each panel below
//! corresponds to one CSS section and is intentionally laid out 1:1 so a
//! design tweak in one place is easy to mirror in the other.
//!
//! Font choices are u8g2 bitmap fonts (no antialiasing) picked to land close
//! to the Space-Grotesk-like geometric feel of the HTML version:
//!   - body text: helvR / helvB (Helvetica) at the matching pixel sizes
//!   - section headers: helvB10 / helvB12
//!   - hero numbers ($1842, 14°): logisoso variants

use embedded_graphics::geometry::Point;

use u8g2_fonts::types::{FontColor, HorizontalAlignment, VerticalPosition};
use u8g2_fonts::FontRenderer;

use crate::data::*;
use crate::render::{Canvas, Rect, C};

// ── Font picks ─────────────────────────────────────────────────────────────
//
// Each constant is a function to dodge u8g2-fonts' generic API.

fn f_body() -> FontRenderer {
    FontRenderer::new::<u8g2_fonts::fonts::u8g2_font_helvR10_tf>()
}
fn f_body_bold() -> FontRenderer {
    FontRenderer::new::<u8g2_fonts::fonts::u8g2_font_helvB10_tf>()
}
fn f_small() -> FontRenderer {
    FontRenderer::new::<u8g2_fonts::fonts::u8g2_font_helvR08_tf>()
}
fn f_small_bold() -> FontRenderer {
    FontRenderer::new::<u8g2_fonts::fonts::u8g2_font_helvB08_tf>()
}
fn f_lg_bold() -> FontRenderer {
    FontRenderer::new::<u8g2_fonts::fonts::u8g2_font_helvB14_tf>()
}
fn f_xl_bold() -> FontRenderer {
    FontRenderer::new::<u8g2_fonts::fonts::u8g2_font_helvB18_tf>()
}
fn f_huge_bold() -> FontRenderer {
    FontRenderer::new::<u8g2_fonts::fonts::u8g2_font_helvB24_tf>()
}
fn f_temperature() -> FontRenderer {
    // logisoso is a tall futuristic display face; _tn variant has digits + a
    // few punctuation glyphs but no °, so we draw the ° manually.
    FontRenderer::new::<u8g2_fonts::fonts::u8g2_font_logisoso92_tn>()
}

// ── Text drawing helpers ───────────────────────────────────────────────────

#[derive(Copy, Clone)]
pub enum Align { Left, Right, Center }

fn draw_text(
    canvas: &mut Canvas,
    font: &FontRenderer,
    text: &str,
    x: i32,
    baseline_y: i32,
    color: C,
    align: Align,
) {
    let h = match align {
        Align::Left   => HorizontalAlignment::Left,
        Align::Right  => HorizontalAlignment::Right,
        Align::Center => HorizontalAlignment::Center,
    };
    let _ = font.render_aligned(
        text,
        Point::new(x, baseline_y),
        VerticalPosition::Baseline,
        h,
        FontColor::Transparent(color.rgb()),
        canvas,
    );
}

/// Render `text` in `fg` with a 1-pixel `outline` halo around every glyph —
/// crude CSS `text-shadow: 0 1px 0 #000, 1px 0 0 #000, ...;` equivalent.
/// Used for legibility over busy backgrounds like the hatched header.
#[allow(clippy::too_many_arguments)]
fn draw_outlined_text(
    canvas: &mut Canvas,
    font: &FontRenderer,
    text: &str,
    x: i32,
    baseline_y: i32,
    fg: C,
    outline: C,
    align: Align,
) {
    // 2-px halo: every offset in a 5×5 block except the center (the fg
    // glyph itself), painted in `outline` first so the foreground sits on
    // top. Yes, this is 24 redundant text renders — at 50 ms per frame and
    // bitmap glyphs, the cost is negligible.
    for dy in -2..=2 {
        for dx in -2..=2 {
            if dx == 0 && dy == 0 { continue; }
            draw_text(canvas, font, text, x + dx, baseline_y + dy, outline, align);
        }
    }
    draw_text(canvas, font, text, x, baseline_y, fg, align);
}

/// Width of a string in the given font, using the cursor advance (i.e. where
/// the next glyph would start). Cursor advance is what we want for layout —
/// the ink bounding box stops at the last inked pixel and is *narrower* than
/// the advance for digits with right-side bearing, which makes a follow-up
/// glyph land on top of the previous one.
fn text_width(font: &FontRenderer, text: &str) -> u32 {
    font.get_rendered_dimensions(text, Point::zero(), VerticalPosition::Baseline)
        .ok()
        .map(|d| d.advance.x.max(0) as u32)
        .unwrap_or(0)
}

// ── Top-level entry point ──────────────────────────────────────────────────

pub fn render(data: &DashData, battery: u8, rssi: i32) -> anyhow::Result<Vec<u8>> {
    let mut c = Canvas::new(crate::render::W, crate::render::H);
    c.fill(C::White);

    draw_registration_marks(&mut c);
    draw_header(&mut c, data);
    draw_header_meta(&mut c, battery, rssi);
    draw_body(&mut c, data);
    draw_footer(&mut c, data);

    c.into_png()
}

// ── Registration marks (corner brackets) ───────────────────────────────────

fn draw_registration_marks(c: &mut Canvas) {
    // 8×8 L-shaped 1px brackets in each corner of the live area.
    let tl = (4, 4);
    let tr = (crate::render::W as i32 - 4 - 8, 4);
    let bl = (4, crate::render::H as i32 - 22 - 4 - 8);
    let br = (crate::render::W as i32 - 4 - 8, crate::render::H as i32 - 22 - 4 - 8);

    // Top-left: top edge + left edge
    c.hline(tl.0, tl.1, 8, C::Black);
    c.vline(tl.0, tl.1, 8, C::Black);
    // Top-right: top edge + right edge (mirrored)
    c.hline(tr.0, tr.1, 8, C::Black);
    c.vline(tr.0 + 7, tr.1, 8, C::Black);
    // Bottom-left
    c.hline(bl.0, bl.1 + 7, 8, C::Black);
    c.vline(bl.0, bl.1, 8, C::Black);
    // Bottom-right
    c.hline(br.0, br.1 + 7, 8, C::Black);
    c.vline(br.0 + 7, br.1, 8, C::Black);
}

// ── Header (44px tall) ─────────────────────────────────────────────────────

const HDR_H: u32 = 44;

fn draw_header(c: &mut Canvas, data: &DashData) {
    // Bottom 3px black border below the whole header
    c.fill_rect(Rect::new(0, HDR_H as i32 - 3, crate::render::W, 3), C::Black);

    // ── Left tab: black parallelogram with 14px right slant ──
    // Width must clear the slanted right edge so the bold "TRMNL-01" doesn't
    // get clipped: text right-edge ≤ tab_width - slant.
    let unit_baseline = 16;
    let name_baseline = 32;
    let unit_w = text_width(&f_small(), "UNIT");
    let name_w = text_width(&f_huge_bold(), "TRMNL-01");
    let left_pad = 12i32;
    let inner_gap = 8i32;
    let slant = 14u32;
    let left_w = (left_pad as u32) + unit_w + (inner_gap as u32) + name_w + (left_pad as u32) + slant;
    c.fill_left_parallelogram(0, 0, left_w, HDR_H - 3, slant, C::Black);

    draw_text(c, &f_small(), "UNIT", left_pad, unit_baseline, C::White, Align::Left);
    draw_text(
        c,
        &f_huge_bold(),
        "TRMNL-01",
        left_pad + unit_w as i32 + inner_gap,
        name_baseline,
        C::White,
        Align::Left,
    );

    // ── Center: hatched stripe ──
    let right_block_w = 200u32;
    let center = Rect::new(
        left_w as i32,
        0,
        crate::render::W.saturating_sub(left_w + right_block_w),
        HDR_H - 3,
    );
    c.hatch_135(
        center,
        &[(6, C::Red), (2, C::Black), (6, C::Red), (6, C::White)],
    );
    // Motto over the hatch — white text with a 1px black outline, matching
    // the original CSS text-shadow. Drawn by blitting the same string 4
    // times in black at ±1px offsets, then once in white on top.
    let motto = data.motto.as_str();
    draw_outlined_text(
        c,
        &f_lg_bold(),
        motto,
        center.x + center.w as i32 / 2,
        center.y + 28,
        C::White,
        C::Black,
        Align::Center,
    );

    // ── Right: date + meta line ──
    let right_x = crate::render::W as i32 - 12; // right edge minus padding
    draw_text(c, &f_small(), &format!("{} · {}", data.date_dow, data.date),
        right_x, 18, C::Black, Align::Right);
}

pub fn draw_header_meta(c: &mut Canvas, battery: u8, rssi: i32) {
    // Right-aligned BAT % / signal-bar pair on the second header row.
    let right_x = crate::render::W as i32 - 12;
    let baseline = 33;

    let bars = match rssi {
        r if r > -55 => 4,
        r if r > -70 => 3,
        r if r > -80 => 2,
        r if r > -90 => 1,
        _ => 0,
    };

    // Draw signal as four 4×N filled/outlined bars climbing left-to-right —
    // u8g2 Latin-1 fonts don't have ● or █, and ASCII bars look terrible at
    // 8 px, so we draw the indicator as primitives.
    let bar_w = 3u32;
    let bar_gap = 1i32;
    let bar_max_h = 8u32;
    let bar_block_w = (bar_w as i32 + bar_gap) * 4 - bar_gap;
    let bar_x0 = right_x - bar_block_w;
    let bar_baseline = baseline; // bars sit on the same baseline as text
    for i in 0..4 {
        let h = bar_max_h.saturating_sub(0) * (i as u32 + 1) / 4 + 2;
        let bx = bar_x0 + i * (bar_w as i32 + bar_gap);
        let by = bar_baseline - h as i32;
        let r = Rect::new(bx, by, bar_w, h);
        if i < bars {
            c.fill_rect(r, C::Black);
        } else {
            c.stroke_rect(r, 1, C::Black);
        }
    }

    // BAT label sits to the left of the bars.
    let line = format!("BAT {battery}%   SIG");
    draw_text(c, &f_small(), &line, bar_x0 - 6, baseline, C::Black, Align::Right);
}

// ── Body ───────────────────────────────────────────────────────────────────

const FTR_H: u32 = 22;
const BODY_TOP: i32 = HDR_H as i32;
const BODY_H: u32 = 480 - HDR_H - FTR_H; // = 414
const ROW_H: u32 = BODY_H / 2;            // = 207

const COL1_W: u32 = 260;
const COL3_W: u32 = 220;
const COL2_W: u32 = 800 - COL1_W - COL3_W; // = 320

fn col1_rect() -> Rect { Rect::new(0, BODY_TOP, COL1_W, BODY_H) }
fn col2_top()  -> Rect { Rect::new(COL1_W as i32, BODY_TOP, COL2_W, ROW_H) }
fn col2_bot()  -> Rect { Rect::new(COL1_W as i32, BODY_TOP + ROW_H as i32, COL2_W, ROW_H) }
fn col3_top()  -> Rect { Rect::new((COL1_W + COL2_W) as i32, BODY_TOP, COL3_W, ROW_H) }
fn col3_bot()  -> Rect { Rect::new((COL1_W + COL2_W) as i32, BODY_TOP + ROW_H as i32, COL3_W, ROW_H) }

fn draw_body(c: &mut Canvas, data: &DashData) {
    // Inter-panel borders (CSS: p-wx border-right 2px; p-agenda border-bottom
    // 2px; p-budget border-left 2px + border-bottom 2px; p-ops border-left 2px)
    c.vline(COL1_W as i32, BODY_TOP, BODY_H, C::Black);
    c.vline(COL1_W as i32 + 1, BODY_TOP, BODY_H, C::Black); // 2px stroke

    c.hline(COL1_W as i32, BODY_TOP + ROW_H as i32, COL2_W, C::Black);
    c.hline(COL1_W as i32, BODY_TOP + ROW_H as i32 + 1, COL2_W, C::Black);

    let col3_x = (COL1_W + COL2_W) as i32;
    c.vline(col3_x,     BODY_TOP, BODY_H, C::Black);
    c.vline(col3_x + 1, BODY_TOP, BODY_H, C::Black);

    c.hline(col3_x, BODY_TOP + ROW_H as i32,     COL3_W, C::Black);
    c.hline(col3_x, BODY_TOP + ROW_H as i32 + 1, COL3_W, C::Black);

    draw_weather(c, data.weather.as_ref());
    draw_agenda(c, &data.agenda);
    draw_sys(c, &data.hosts, data.cluster.as_ref());
    draw_budget(c, data.budget.as_ref());
    draw_ops(c, &data.tasks, &data.alerts);
}

// ── Section header (the "stamp" with EN tag + // NN seq) ───────────────────

/// Returns the y after the header (where panel content can start).
fn draw_section_header(c: &mut Canvas, panel: Rect, en: &str, seq: &str) -> i32 {
    let pad_x = 12;
    let top   = panel.y + 8;

    // EN tag: black background, white text, ~16×16 px box
    let tag_w = text_width(&f_small_bold(), en) + 10;
    let tag_h = 14u32;
    c.fill_rect(Rect::new(panel.x + pad_x, top, tag_w, tag_h), C::Black);
    draw_text(
        c,
        &f_small_bold(),
        en,
        panel.x + pad_x + 5,
        top + 11,
        C::White,
        Align::Left,
    );

    // Sequence label, right-aligned
    draw_text(
        c,
        &f_small(),
        seq,
        panel.right() - pad_x,
        top + 11,
        C::Black,
        Align::Right,
    );

    // 2px black underline
    let line_y = top + tag_h as i32 + 4;
    c.fill_rect(
        Rect::new(panel.x + pad_x, line_y, panel.w - pad_x as u32 * 2, 2),
        C::Black,
    );

    line_y + 2 + 6 // 6px breathing room before content
}

// ── Weather panel ──────────────────────────────────────────────────────────

fn draw_weather(c: &mut Canvas, w: Option<&WeatherData>) {
    let panel = col1_rect();
    let content_y = draw_section_header(c, panel, "WX", "// 01");

    let Some(w) = w else {
        draw_text(c, &f_lg_bold(), "—", panel.x + 12, content_y + 32, C::Black, Align::Left);
        draw_text(c, &f_small(), "WEATHER NOT SET", panel.x + 12, content_y + 50, C::Black, Align::Left);
        return;
    };

    // Big temperature, with a bit of breathing room from the section header.
    let temp_str = format!("{}", w.temp_c);
    let temp_x = panel.x + 12;
    let temp_baseline = content_y + 100;
    draw_text(c, &f_temperature(), &temp_str, temp_x, temp_baseline, C::Black, Align::Left);

    // Degree ring just past the digits.
    let temp_w = text_width(&f_temperature(), &temp_str) as i32;
    let deg_r = 8i32;
    let deg_gap = 8i32;
    let deg_cx = temp_x + temp_w + deg_gap + deg_r;
    let deg_cy = content_y + 30;
    draw_circle_outline(c, deg_cx, deg_cy, deg_r, 2, C::Red);

    // Side info (condition + HI/LO).
    let side_x = deg_cx + deg_r + 12;
    let side_avail = panel.right() - 12 - side_x;
    if side_avail >= 70 {
        draw_text(c, &f_body_bold(), &w.condition, side_x, content_y + 40, C::Black, Align::Left);
        draw_text(
            c,
            &f_small(),
            &format!("HI {}°  LO {}°", w.hi, w.lo),
            side_x,
            content_y + 56,
            C::Black,
            Align::Left,
        );
    } else {
        draw_text(c, &f_body_bold(), &w.condition, temp_x, content_y + 122, C::Black, Align::Left);
        draw_text(
            c,
            &f_small(),
            &format!("HI {}°  LO {}°", w.hi, w.lo),
            temp_x,
            content_y + 138,
            C::Black,
            Align::Left,
        );
    }

    // Forecast: 4-column grid pinned to bottom of panel.
    let fc_h = 48u32;
    let fc_y = panel.bottom() - fc_h as i32 - 6;
    c.hline(panel.x + 12, fc_y, panel.w - 24, C::Black);

    // Big condition icon centred in the empty band between the temp digits
    // and the forecast grid.
    let icon_band_top = temp_baseline + 16;
    let icon_band_bot = fc_y - 6;
    let icon_cy = (icon_band_top + icon_band_bot) / 2;
    let icon_cx = panel.x + panel.w as i32 / 2;
    let icon_size = (icon_band_bot - icon_band_top - 8).clamp(40, 90);
    draw_weather_icon(c, icon_cx, icon_cy, icon_size, &w.condition, C::Black);

    let cell_w = (panel.w - 24) / 4;
    for (i, day) in w.forecast.iter().take(4).enumerate() {
        let cx = panel.x + 12 + (cell_w * i as u32) as i32 + cell_w as i32 / 2;
        if i < 3 {
            let dx = panel.x + 12 + (cell_w * (i + 1) as u32) as i32;
            for k in 0..(fc_h / 4) {
                c.put(dx, fc_y + (k * 4) as i32 + 2, C::Black);
                c.put(dx, fc_y + (k * 4) as i32 + 3, C::Black);
            }
        }
        draw_text(c, &f_small_bold(), &day.day, cx, fc_y + 12, C::Black, Align::Center);
        draw_text(c, &f_xl_bold(), &format!("{}°", day.hi), cx, fc_y + 32, C::Black, Align::Center);

        // Bottom row: small condition icon + lo temp instead of "° CLOUD".
        let lo_str = format!("{}°", day.lo);
        let lo_w = text_width(&f_small(), &lo_str) as i32;
        let icon_w = 12i32;
        let row_w = lo_w + 4 + icon_w;
        let row_x0 = cx - row_w / 2;
        draw_text(c, &f_small(), &lo_str, row_x0, fc_y + 44, C::Black, Align::Left);
        draw_weather_icon(
            c,
            row_x0 + lo_w + 4 + icon_w / 2,
            fc_y + 40,
            icon_w,
            &day.cond,
            C::Black,
        );
    }
}

// ── Weather icons ──────────────────────────────────────────────────────────
//
// Drawn as primitives at runtime (no bitmap assets to ship). Each icon is
// keyed off a free-form condition string; unknown values fall back to cloud.

fn draw_weather_icon(c: &mut Canvas, cx: i32, cy: i32, size: i32, condition: &str, color: C) {
    let cond = condition.to_uppercase();
    match cond.as_str() {
        "SUN" | "CLEAR" | "SUNNY" | "FAIR" => draw_sun(c, cx, cy, size, color),
        "RAIN" | "RAINY" | "SHOWERS" | "DRIZZLE" => draw_rain(c, cx, cy, size, color),
        "STORM" | "THUNDER" | "THUNDERSTORM" => draw_storm(c, cx, cy, size, color),
        "SNOW" | "SNOWY" | "SLEET" => draw_snow(c, cx, cy, size, color),
        "FOG" | "MIST" | "HAZE" | "SMOKE" => draw_fog(c, cx, cy, size, color),
        // CLOUD / CLOUDS / CLOUDY / OVERCAST / unknown
        _ => draw_cloud(c, cx, cy, size, color),
    }
}

fn fill_disc(c: &mut Canvas, cx: i32, cy: i32, r: i32, color: C) {
    let r2 = r * r;
    for dy in -r..=r {
        for dx in -r..=r {
            if dx * dx + dy * dy <= r2 {
                c.put(cx + dx, cy + dy, color);
            }
        }
    }
}

/// Cloud silhouette: 3 stacked filled discs + flat-bottomed rect.
fn draw_cloud(c: &mut Canvas, cx: i32, cy: i32, size: i32, color: C) {
    let r1 = (size / 4).max(3);
    let r2 = (size / 3).max(4);
    let r3 = (size / 4).max(3);
    let bottom = cy + size / 4;
    fill_disc(c, cx - size / 3, cy + size / 12, r1, color);
    fill_disc(c, cx,             cy - size / 12, r2, color);
    fill_disc(c, cx + size / 3, cy + size / 12, r3, color);
    let bottom_w = (size as u32).saturating_sub(4);
    let bottom_h = (size / 6).max(2) as u32;
    c.fill_rect(
        Rect::new(cx - bottom_w as i32 / 2, bottom - bottom_h as i32, bottom_w, bottom_h),
        color,
    );
}

/// Sun: filled disc + 8 short rays.
fn draw_sun(c: &mut Canvas, cx: i32, cy: i32, size: i32, color: C) {
    let r = (size / 4).max(3);
    fill_disc(c, cx, cy, r, color);
    let inner = r + (size / 12).max(2);
    let outer = r + (size / 4).max(3);
    let dirs: [(i32, i32); 8] = [
        ( 1, 0), (-1, 0), (0,  1), (0, -1),
        ( 1, 1), ( 1,-1), (-1, 1), (-1,-1),
    ];
    for (dx, dy) in dirs {
        // Diagonal rays use a /√2 scale; integer math is close enough.
        let scale_num = if dx.abs() + dy.abs() == 2 { 7 } else { 10 };
        let scale_den = 10;
        let i = inner * scale_num / scale_den;
        let o = outer * scale_num / scale_den;
        let x0 = cx + dx * i;
        let y0 = cy + dy * i;
        let x1 = cx + dx * o;
        let y1 = cy + dy * o;
        // Thin filled rectangle per ray; for diagonals we just stamp little
        // 2-pixel discs along the segment which keeps the shape recognizable
        // without writing a thick-line rasterizer.
        let steps = (o - i).max(1);
        for s in 0..=steps {
            let t = if steps == 0 { 0.0 } else { s as f32 / steps as f32 };
            let x = x0 + ((x1 - x0) as f32 * t) as i32;
            let y = y0 + ((y1 - y0) as f32 * t) as i32;
            fill_disc(c, x, y, 1, color);
        }
    }
}

/// Rain: cloud (raised) + 3 short vertical drops.
fn draw_rain(c: &mut Canvas, cx: i32, cy: i32, size: i32, color: C) {
    draw_cloud(c, cx, cy - size / 4, size, color);
    let drop_y = cy + size / 6;
    let drop_h = (size / 4).max(4) as u32;
    let drop_w = 2u32.max((size / 30) as u32);
    for i in 0..3 {
        let dx = -size / 4 + i * size / 4;
        c.fill_rect(Rect::new(cx + dx - drop_w as i32 / 2, drop_y, drop_w, drop_h), color);
    }
}

/// Storm: cloud + lightning-bolt zigzag.
fn draw_storm(c: &mut Canvas, cx: i32, cy: i32, size: i32, color: C) {
    draw_cloud(c, cx, cy - size / 4, size, color);
    let bx = cx - size / 16;
    let by = cy + size / 12;
    let bw = (size / 14).max(3) as u32;
    let bh1 = (size / 5).max(4) as u32;
    let bh2 = (size / 6).max(3) as u32;
    c.fill_rect(Rect::new(bx, by, bw, bh1), color);
    c.fill_rect(
        Rect::new(bx - size / 12, by + bh1 as i32, (size / 12 + bw as i32) as u32, bw),
        color,
    );
    c.fill_rect(Rect::new(bx - size / 12, by + bh1 as i32, bw, bh2), color);
}

/// Snow: cloud + 3 plus-sign flakes.
fn draw_snow(c: &mut Canvas, cx: i32, cy: i32, size: i32, color: C) {
    draw_cloud(c, cx, cy - size / 4, size, color);
    let y = cy + size / 6;
    let arm = (size / 14).max(3);
    for i in 0..3 {
        let dx = -size / 4 + i * size / 4;
        let x = cx + dx;
        c.fill_rect(Rect::new(x - arm, y, (arm * 2 + 1) as u32, 2), color);
        c.fill_rect(Rect::new(x, y - arm, 2, (arm * 2 + 1) as u32), color);
    }
}

/// Fog: 3 staggered horizontal bars.
fn draw_fog(c: &mut Canvas, cx: i32, cy: i32, size: i32, color: C) {
    let bar_h = 2u32.max((size / 14) as u32);
    let gap = (size / 8).max(3);
    let widths = [size, size - size / 6, size - size / 4];
    for (i, w) in widths.iter().enumerate() {
        let y = cy - gap + (i as i32) * gap;
        let off = ((i as i32) % 2) * (size / 12);
        c.fill_rect(Rect::new(cx - w / 2 + off, y, *w as u32, bar_h), color);
    }
}

fn draw_circle_outline(c: &mut Canvas, cx: i32, cy: i32, r: i32, stroke: i32, color: C) {
    // Midpoint algorithm with a configurable stroke thickness.
    for dy in -r..=r {
        for dx in -r..=r {
            let d2 = dx * dx + dy * dy;
            if d2 <= r * r && d2 >= (r - stroke) * (r - stroke) {
                c.put(cx + dx, cy + dy, color);
            }
        }
    }
}

// ── Agenda panel ───────────────────────────────────────────────────────────

fn draw_agenda(c: &mut Canvas, items: &[AgendaItem]) {
    let panel = col2_top();
    let content_y = draw_section_header(c, panel, "AGENDA", "// 02");

    let pad_x = 12;
    if items.is_empty() {
        draw_text(c, &f_small(), "NO EVENTS TODAY", panel.x + pad_x, content_y + 18, C::Black, Align::Left);
        return;
    }
    let row_h = 32i32;
    for (i, ev) in items.iter().take(4).enumerate() {
        let y = content_y + (i as i32) * row_h;

        // Time (red for first event)
        let time_color = if i == 0 { C::Red } else { C::Black };
        draw_text(c, &f_lg_bold(), &ev.time, panel.x + pad_x, y + 16, time_color, Align::Left);

        // Title
        draw_text(c, &f_body(), &ev.title, panel.x + pad_x + 56, y + 14, C::Black, Align::Left);

        // Tag pill
        let tag_w = 44u32;
        let tag_x = panel.right() - pad_x - tag_w as i32;
        c.fill_rect(Rect::new(tag_x, y + 4, tag_w, 14), C::Black);
        draw_text(
            c,
            &f_small_bold(),
            &ev.tag,
            tag_x + tag_w as i32 / 2,
            y + 14,
            C::White,
            Align::Center,
        );

        // Dashed divider between rows (skip last)
        if i < items.len().min(4).saturating_sub(1) {
            c.dashed_hline(panel.x + pad_x, y + row_h, panel.w - pad_x as u32 * 2, C::Black, 3, 3);
        }
    }
}

// ── SYS panel ──────────────────────────────────────────────────────────────

fn draw_sys(c: &mut Canvas, hosts: &[HostData], cluster: Option<&ClusterMetrics>) {
    let panel = col2_bot();
    let content_y = draw_section_header(c, panel, "SYS", "// 03");

    if hosts.is_empty() && cluster.is_none() {
        draw_text(c, &f_small(), "PROMETHEUS_URL NOT SET", panel.x + 12, content_y + 18, C::Black, Align::Left);
        return;
    }

    // Cluster summary cells. CPU/RAM/DSK come from the dedicated
    // cluster-rollup queries when available (sum-based for CPU/RAM, Ceph
    // for disk); per-host arithmetic mean is the fallback so mock data and
    // pre-cluster-fetch states still produce a reasonable summary.
    // Temperature has no useful cluster metric, so it always comes from
    // the per-host mean.
    let n = hosts.len() as u32;
    let avg = |f: fn(&HostData) -> u32| -> u32 {
        if hosts.is_empty() { 0 } else { hosts.iter().map(f).sum::<u32>() / n.max(1) }
    };
    let cluster_cpu = cluster.map(|c| c.cpu_pct as u32).unwrap_or_else(|| avg(|h| h.cpu as u32));
    let cluster_ram = cluster.map(|c| c.ram_pct as u32).unwrap_or_else(|| avg(|h| h.ram_pct as u32));
    let cluster_dsk = cluster.map(|c| c.disk_pct as u32).unwrap_or_else(|| avg(|h| h.disk_pct as u32));
    let cluster_tmp = avg(|h| h.cpu_temp as u32);
    let cluster_cells: [(&str, u32, &str); 4] = [
        ("CPU", cluster_cpu, "%"),
        ("RAM", cluster_ram, "%"),
        ("DSK", cluster_dsk, "%"),
        ("TMP", cluster_tmp, "°"),
    ];

    let pad_x = 12;
    let cell_w   = (panel.w - pad_x as u32 * 2 - 24) / 4;
    let cell_gap = 8;
    // Cell height bumped to 58 so the helvB24 number has room to sit clear
    // of the helvB10 label above it (the big digits' ascender otherwise
    // crashes into the label baseline).
    let cell_h = 58u32;
    for (i, (lbl, val, unit)) in cluster_cells.iter().enumerate() {
        let cx = panel.x + pad_x + (i as i32) * (cell_w as i32 + cell_gap);
        let cy = content_y;
        let cell = Rect::new(cx, cy, cell_w, cell_h);
        c.stroke_rect(cell, 1, C::Black);

        // Vertical layout inside the cell:
        //   y+12  label baseline (helvB10 cap ≈ 7 → glyph y+5..y+12)
        //   y+38  number baseline (helvB24 cap ≈ 17 → glyph y+21..y+38)
        //   y+47  bar top (height 6, ends y+53 well inside the 58 px cell)
        draw_text(c, &f_small_bold(), lbl, cx + 4, cy + 12, C::Black, Align::Left);
        draw_text(c, &f_huge_bold(), &val.to_string(), cx + 4, cy + 38, C::Black, Align::Left);
        let val_w = text_width(&f_huge_bold(), &val.to_string()) as i32;
        draw_text(c, &f_body(), unit, cx + 6 + val_w, cy + 38, C::Red, Align::Left);

        let bar = Rect::new(cx + 4, cy + 47, cell_w - 8, 6);
        c.stroke_rect(bar, 1, C::Black);
        let fill_w = bar.w.saturating_sub(2) * (*val).min(100) / 100;
        let bar_color = if *val > 70 { C::Red } else { C::Black };
        c.fill_rect(Rect::new(bar.x + 1, bar.y + 1, fill_w, 4), bar_color);
    }

    // Per-host detail rows below the cluster grid.
    let list_y = content_y + cell_h as i32 + 6;
    let row_h = 14i32;
    let max_rows = 6.min(hosts.len());

    // Column anchors: NAME (left), CPU/TMP/RAM/DSK (right-aligned in 4 evenly
    // spaced bands). Right-aligning numbers makes scanning a column
    // ("which node is hottest?") easy.
    let cols_right_edge = panel.right() - pad_x;
    let col_w = 38i32; // ~"100%" + margin
    let cols_left_edge = cols_right_edge - col_w * 4;
    let name_max_w = (cols_left_edge - (panel.x + pad_x) - 4).max(40) as u32;

    for (i, h) in hosts.iter().take(max_rows).enumerate() {
        let y = list_y + (i as i32) * row_h;
        // Name (truncate to fit before metric columns)
        let name = clip_to_width(&f_small_bold(), &h.name.to_uppercase(), name_max_w);
        draw_text(c, &f_small_bold(), &name, panel.x + pad_x, y + 9, C::Black, Align::Left);

        // Each metric right-aligned in its own band; > 70% drawn in red.
        // Column order mirrors the cluster summary row above.
        let metrics = [h.cpu as u32, h.ram_pct as u32, h.disk_pct as u32, h.cpu_temp as u32];
        let units = ["%", "%", "%", "°"];
        for (j, (val, unit)) in metrics.iter().zip(units.iter()).enumerate() {
            let col_right = cols_left_edge + col_w * (j as i32 + 1);
            let color = if *val > 70 { C::Red } else { C::Black };
            draw_text(c, &f_small(), &format!("{val}{unit}"), col_right, y + 9, color, Align::Right);
        }
    }
}

// ── Budget panel ───────────────────────────────────────────────────────────

fn draw_budget(c: &mut Canvas, b: Option<&BudgetData>) {
    let panel = col3_top();
    let content_y = draw_section_header(c, panel, "$", "// 04");

    let Some(b) = b else {
        draw_text(c, &f_lg_bold(), "—", panel.x + 10, content_y + 24, C::Black, Align::Left);
        draw_text(c, &f_small(), "BUDGET NOT SET", panel.x + 10, content_y + 44, C::Black, Align::Left);
        return;
    };

    let pad_x = 10;
    // Hero total
    let total = format!("${}", b.spent);
    draw_text(c, &f_huge_bold(), &total, panel.x + pad_x, content_y + 22, C::Black, Align::Left);
    // Subtext
    let sub = format!("/ ${} · {}", b.cap, b.month_label);
    draw_text(c, &f_small(), &sub, panel.x + pad_x, content_y + 36, C::Black, Align::Left);

    // Overall bar (red fill)
    let bar = Rect::new(panel.x + pad_x, content_y + 42, panel.w - pad_x as u32 * 2, 8);
    c.stroke_rect(bar, 1, C::Black);
    if let Some(fill_w) = (bar.w.saturating_sub(2) * b.spent.min(b.cap)).checked_div(b.cap) {
        c.fill_rect(Rect::new(bar.x + 1, bar.y + 1, fill_w, 6), C::Red);
    }

    // Category rows: label · track · spent
    let cats_y = content_y + 58;
    let row_h = 14i32;
    let label_w = 40u32;
    let value_w = 38u32;
    let track_x = panel.x + pad_x + label_w as i32 + 4;
    let track_w = panel.w - pad_x as u32 * 2 - label_w - value_w - 8;
    for (i, cat) in b.cats.iter().take(5).enumerate() {
        let y = cats_y + (i as i32) * row_h;
        // Label
        draw_text(c, &f_small_bold(), &cat.label, panel.x + pad_x, y + 9, C::Black, Align::Left);

        // Track
        let track = Rect::new(track_x, y + 3, track_w, 6);
        c.stroke_rect(track, 1, C::Black);
        if let Some(pct) = (cat.spent * 100).checked_div(cat.cap).map(|p| p.min(100)) {
            let fill_w = track.w.saturating_sub(2) * pct / 100;
            let color = if pct > 85 { C::Red } else { C::Black };
            c.fill_rect(Rect::new(track.x + 1, track.y + 1, fill_w, 4), color);
        }
        // Value
        draw_text(
            c,
            &f_small(),
            &format!("${}", cat.spent),
            panel.right() - pad_x,
            y + 9,
            C::Black,
            Align::Right,
        );
    }
}

// ── OPS panel (tasks + alerts) ─────────────────────────────────────────────

fn draw_ops(c: &mut Canvas, tasks: &[Task], alerts: &[Alert]) {
    let panel = col3_bot();
    let content_y = draw_section_header(c, panel, "OPS", "// 05");

    let pad_x = 10;
    let task_h = 14i32;
    let max_tasks = 4;
    let task_count = tasks.len().min(max_tasks);
    for (i, t) in tasks.iter().take(max_tasks).enumerate() {
        let y = content_y + (i as i32) * task_h;
        // Checkbox
        let mark = if t.done { "■" } else { "□" };
        draw_text(c, &f_small(), mark, panel.x + pad_x, y + 10, C::Black, Align::Left);
        // Priority
        let pri_color = if t.priority == "HI" { C::Red } else { C::Black };
        draw_text(c, &f_small_bold(), &t.priority, panel.x + pad_x + 14, y + 10, pri_color, Align::Left);
        // Text (truncated visually by clip — we let it render and the right border absorbs overflow)
        let text = clip_to_width(&f_body(), &t.text, panel.w - pad_x as u32 * 2 - 50);
        draw_text(c, &f_body(), &text, panel.x + pad_x + 40, y + 10, C::Black, Align::Left);
    }

    // Alerts area starts just below the rendered tasks (or at the top of
    // content when tasks is empty), separated by a dashed divider only
    // when both halves have content.
    let alert_h = 14i32;
    let alerts_y = if task_count == 0 {
        content_y
    } else {
        let y = content_y + task_count as i32 * task_h + 6;
        c.dashed_hline(panel.x + pad_x, y, panel.w - pad_x as u32 * 2, C::Black, 3, 3);
        y + 4
    };

    // Show as many alerts as fit between alerts_y and the panel bottom
    // (with a small footer pad). Each row is alert_h tall.
    let bottom_pad = 4i32;
    let avail = (panel.bottom() - bottom_pad - alerts_y).max(0);
    let max_alerts = (avail / alert_h).max(0) as usize;

    for (i, a) in alerts.iter().take(max_alerts).enumerate() {
        let y = alerts_y + (i as i32) * alert_h;
        // Level pill
        let (bg, fg) = match a.level.as_str() {
            "ERR" => (C::Red, C::White),
            "WRN" => (C::Yellow, C::Black),
            _     => (C::Black, C::White),
        };
        let pill_w = 30u32;
        c.fill_rect(Rect::new(panel.x + pad_x, y, pill_w, 12), bg);
        draw_text(c, &f_small_bold(), &a.level, panel.x + pad_x + pill_w as i32 / 2, y + 9, fg, Align::Center);
        // Message
        let msg = clip_to_width(&f_small(), &a.message, panel.w - pad_x as u32 * 2 - pill_w - 6);
        draw_text(c, &f_small(), &msg, panel.x + pad_x + pill_w as i32 + 6, y + 9, C::Black, Align::Left);
    }
}

/// Drop characters off the end of `s` until it fits, ellipsizing with "…".
fn clip_to_width(font: &FontRenderer, s: &str, max_w: u32) -> String {
    if text_width(font, s) <= max_w {
        return s.to_string();
    }
    let mut chars: Vec<char> = s.chars().collect();
    while !chars.is_empty() {
        chars.pop();
        let candidate: String = chars.iter().collect::<String>() + "…";
        if text_width(font, &candidate) <= max_w {
            return candidate;
        }
    }
    "…".to_string()
}

// ── Footer (22px tall) ─────────────────────────────────────────────────────

fn draw_footer(c: &mut Canvas, data: &DashData) {
    let y0 = (480 - FTR_H) as i32;
    c.fill_rect(Rect::new(0, y0, crate::render::W, FTR_H), C::Black);

    let baseline = y0 + 15;
    draw_text(
        c,
        &f_small_bold(),
        &format!("SYNC {} → {}", data.last_sync, data.next_sync),
        10,
        baseline,
        C::White,
        Align::Left,
    );
    draw_text(
        c,
        &f_small_bold(),
        "● ONLINE",
        10 + 220,
        baseline,
        C::Red,
        Align::Left,
    );
    draw_text(
        c,
        &f_small_bold(),
        "NEO / TRMNL",
        crate::render::W as i32 - 10,
        baseline,
        C::White,
        Align::Right,
    );
}
