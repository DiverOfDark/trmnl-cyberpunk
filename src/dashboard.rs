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

    draw_weather(c, &data.weather);
    draw_agenda(c, &data.agenda);
    draw_sys(c, &data.hosts);
    draw_budget(c, &data.budget);
    draw_ops(c, &data.tasks, &data.alerts);
}

// ── Section header (the "stamp" with EN tag + // NN seq) ───────────────────

/// Returns the y after the header (where panel content can start).
fn draw_section_header(c: &mut Canvas, panel: Rect, en: &str, seq: &str) -> i32 {
    let pad_x = 12;
    let top   = panel.y + 8;

    // EN tag: black background, white text, ~16×16 px box
    let tag_w = (text_width(&f_small_bold(), en) + 10) as u32;
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

fn draw_weather(c: &mut Canvas, w: &WeatherData) {
    let panel = col1_rect();
    let content_y = draw_section_header(c, panel, "WX", "// 01");

    // Big temperature.
    let temp_str = format!("{}", w.temp_c);
    let temp_x = panel.x + 12;
    let temp_baseline = content_y + 88;
    draw_text(c, &f_temperature(), &temp_str, temp_x, temp_baseline, C::Black, Align::Left);

    // Degree symbol — small ring just past the digits' right edge.
    let temp_w = text_width(&f_temperature(), &temp_str) as i32;
    let deg_r = 8i32;
    let deg_gap = 8i32;
    let deg_cx = temp_x + temp_w + deg_gap + deg_r;
    let deg_cy = content_y + 18;
    draw_circle_outline(c, deg_cx, deg_cy, deg_r, 2, C::Red);

    // Side info (condition + HI/LO). Land it past the degree ring with a
    // safety margin so 2-digit temps still leave room. If the panel is too
    // narrow, drop the side block to a second column below the digits.
    let side_x = deg_cx + deg_r + 12;
    let side_avail = panel.right() - 12 - side_x;
    // 70 px just fits "HI 99°  LO -9°" in helvR08 — go to the stacked
    // fallback only on truly cramped temperatures (3-digit °F etc.).
    if side_avail >= 70 {
        draw_text(c, &f_body_bold(), &w.condition, side_x, content_y + 28, C::Black, Align::Left);
        draw_text(
            c,
            &f_small(),
            &format!("HI {}°  LO {}°", w.hi, w.lo),
            side_x,
            content_y + 44,
            C::Black,
            Align::Left,
        );
    } else {
        // Fallback: stack under the temp if the row is too tight (e.g. 3-digit °F).
        draw_text(c, &f_body_bold(), &w.condition, temp_x, content_y + 110, C::Black, Align::Left);
        draw_text(
            c,
            &f_small(),
            &format!("HI {}°  LO {}°", w.hi, w.lo),
            temp_x,
            content_y + 126,
            C::Black,
            Align::Left,
        );
    }

    // Forecast: 4-column grid pinned to bottom of panel.
    let fc_h = 48u32;
    let fc_y = panel.bottom() - fc_h as i32 - 6;
    c.hline(panel.x + 12, fc_y, panel.w - 24, C::Black);
    let cell_w = (panel.w - 24) / 4;
    for (i, day) in w.forecast.iter().take(4).enumerate() {
        let cx = panel.x + 12 + (cell_w * i as u32) as i32 + cell_w as i32 / 2;
        // dashed divider on right (skip last)
        if i < 3 {
            let dx = panel.x + 12 + (cell_w * (i + 1) as u32) as i32;
            for k in 0..(fc_h / 4) {
                c.put(dx, fc_y + (k * 4) as i32 + 2, C::Black);
                c.put(dx, fc_y + (k * 4) as i32 + 3, C::Black);
            }
        }
        draw_text(c, &f_small_bold(), &day.day, cx, fc_y + 12, C::Black, Align::Center);
        draw_text(c, &f_xl_bold(),    &format!("{}°", day.hi), cx, fc_y + 32, C::Black, Align::Center);
        draw_text(c, &f_small(),      &format!("{}° {}", day.lo, day.cond), cx, fc_y + 44, C::Black, Align::Center);
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

fn draw_sys(c: &mut Canvas, hosts: &[HostData]) {
    let panel = col2_bot();
    let content_y = draw_section_header(c, panel, "SYS", "// 03");

    let host = hosts.first().cloned().unwrap_or(HostData {
        name: "??".into(), cpu: 0, cpu_temp: 0, ram_pct: 0, disk_pct: 0,
        uptime_days: 0, load: [0.0, 0.0, 0.0],
    });

    let pad_x = 12;
    let metrics: [(&str, u32, &str); 4] = [
        ("CPU", host.cpu  as u32,      "%"),
        ("TMP", host.cpu_temp as u32,  "°"),
        ("RAM", host.ram_pct as u32,   "%"),
        ("DSK", host.disk_pct as u32,  "%"),
    ];
    let cell_w  = (panel.w - pad_x as u32 * 2 - 24) / 4;
    let cell_gap = 8;
    for (i, (lbl, val, unit)) in metrics.iter().enumerate() {
        let cx = panel.x + pad_x + (i as i32) * (cell_w as i32 + cell_gap);
        let cy = content_y;
        let cell = Rect::new(cx, cy, cell_w, 50);
        c.stroke_rect(cell, 1, C::Black);

        draw_text(c, &f_small_bold(), lbl, cx + 4, cy + 11, C::Black, Align::Left);

        // Value + small red unit
        draw_text(c, &f_huge_bold(), &val.to_string(), cx + 4, cy + 32, C::Black, Align::Left);
        let val_w = text_width(&f_huge_bold(), &val.to_string()) as i32;
        draw_text(c, &f_body(), unit, cx + 6 + val_w, cy + 32, C::Red, Align::Left);

        // Inset progress bar
        let bar = Rect::new(cx + 4, cy + 38, cell_w - 8, 6);
        c.stroke_rect(bar, 1, C::Black);
        let fill_w = bar.w.saturating_sub(2) * (*val).min(100) / 100;
        let bar_color = if *val > 70 { C::Red } else { C::Black };
        c.fill_rect(Rect::new(bar.x + 1, bar.y + 1, fill_w, 4), bar_color);
    }

    // Meta line: HOST / UPTIME / LOAD
    let meta_y = content_y + 64;
    draw_text(c, &f_small(),
        &format!("HOST {}", host.name.to_uppercase()),
        panel.x + pad_x, meta_y, C::Black, Align::Left);
    draw_text(c, &f_small(),
        &format!("UPTIME {}d", host.uptime_days),
        panel.x + panel.w as i32 / 2, meta_y, C::Black, Align::Center);
    draw_text(c, &f_small(),
        &format!("LOAD {:.2} / {:.2} / {:.2}", host.load[0], host.load[1], host.load[2]),
        panel.right() - pad_x, meta_y, C::Black, Align::Right);
}

// ── Budget panel ───────────────────────────────────────────────────────────

fn draw_budget(c: &mut Canvas, b: &BudgetData) {
    let panel = col3_top();
    let content_y = draw_section_header(c, panel, "$", "// 04");

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
    if b.cap > 0 {
        let fill_w = bar.w.saturating_sub(2) * b.spent.min(b.cap) / b.cap;
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
        if cat.cap > 0 {
            let pct = (cat.spent * 100 / cat.cap).min(100);
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
    for (i, t) in tasks.iter().take(4).enumerate() {
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

    // Dashed top border for alerts area
    let alerts_y = content_y + task_h * 4 + 6;
    c.dashed_hline(panel.x + pad_x, alerts_y, panel.w - pad_x as u32 * 2, C::Black, 3, 3);

    let alert_h = 14i32;
    for (i, a) in alerts.iter().take(2).enumerate() {
        let y = alerts_y + 4 + (i as i32) * alert_h;
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
