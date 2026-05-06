use ab_glyph::{Font, FontArc, PxScale, ScaleFont};
use image::{ImageBuffer, Rgb, RgbImage};

use crate::data::DashData;

// ── e-ink Spectra 6 palette ──────────────────────────────────────────────────
const BLACK:  Rgb<u8> = Rgb([0,   0,   0]);
const WHITE:  Rgb<u8> = Rgb([255, 255, 255]);
const RED:    Rgb<u8> = Rgb([193,  18,  31]);
const YELLOW: Rgb<u8> = Rgb([212, 162,   0]); // WRN alerts

// Canvas
const W: u32 = 800;
const H: u32 = 480;

// Layout constants
const HEADER_H: i32 = 44;
const FOOTER_H: i32 = 22;
const BODY_Y: i32 = HEADER_H;
const FOOTER_Y: i32 = H as i32 - FOOTER_H; // 458
const BODY_H: i32 = FOOTER_Y - BODY_Y;     // 414

// Column boundaries
const C1W: i32 = 260; // weather (left, spans both rows)
const C3W: i32 = 220; // right panels
const C2W: i32 = W as i32 - C1W - C3W - 4; // center (~316), -4 for 2×2px borders
const C1X: i32 = 0;
const C2X: i32 = C1W + 2;          // 262
const C3X: i32 = W as i32 - C3W;  // 580

// Row boundaries within body
const R1H: i32 = BODY_H / 2;       // 207
const R2H: i32 = BODY_H - R1H - 2; // 205
const R1Y: i32 = BODY_Y;           // 44
const R2Y: i32 = R1Y + R1H + 2;    // 253

pub struct Fonts {
    pub bold: FontArc,
    pub regular: FontArc,
    pub mono: FontArc,
}

impl Fonts {
    pub fn load(dir: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let bold    = FontArc::try_from_vec(std::fs::read(format!("{dir}/SpaceGrotesk-Bold.ttf"))?)?;
        let regular = FontArc::try_from_vec(std::fs::read(format!("{dir}/SpaceGrotesk-Regular.ttf"))?)?;
        let mono    = FontArc::try_from_vec(std::fs::read(format!("{dir}/JetBrainsMono-Regular.ttf"))?)?;
        Ok(Self { bold, regular, mono })
    }
}

fn truncate(s: &str, max_chars: usize) -> String {
    let mut chars = s.chars();
    let collected: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{collected}…")
    } else {
        collected
    }
}

// ── Low-level drawing ────────────────────────────────────────────────────────

fn fill(img: &mut RgbImage, x: i32, y: i32, w: i32, h: i32, c: Rgb<u8>) {
    let x0 = x.max(0) as u32;
    let y0 = y.max(0) as u32;
    let x1 = (x + w).min(W as i32) as u32;
    let y1 = (y + h).min(H as i32) as u32;
    for py in y0..y1 {
        for px in x0..x1 {
            img.put_pixel(px, py, c);
        }
    }
}

fn hline(img: &mut RgbImage, x: i32, y: i32, w: i32, c: Rgb<u8>) {
    fill(img, x, y, w, 1, c);
}

fn vline(img: &mut RgbImage, x: i32, y: i32, h: i32, c: Rgb<u8>) {
    fill(img, x, y, 1, h, c);
}

fn outline(img: &mut RgbImage, x: i32, y: i32, w: i32, h: i32, c: Rgb<u8>, t: i32) {
    fill(img, x, y, w, t, c);
    fill(img, x, y + h - t, w, t, c);
    fill(img, x, y, t, h, c);
    fill(img, x + w - t, y, t, h, c);
}

fn progress_bar(img: &mut RgbImage, x: i32, y: i32, w: i32, h: i32, pct: f32, fc: Rgb<u8>) {
    outline(img, x, y, w, h, BLACK, 1);
    let inner = ((pct.clamp(0.0, 100.0) / 100.0) * (w - 2) as f32) as i32;
    if inner > 0 {
        fill(img, x + 1, y + 1, inner, h - 2, fc);
    }
}

// Text helpers ─────────────────────────────────────────────────────────────

fn glyph_width(font: &FontArc, ch: char, size: f32) -> f32 {
    let scale = PxScale::from(size);
    let sf = font.as_scaled(scale);
    sf.h_advance(sf.glyph_id(ch))
}

fn str_width(font: &FontArc, text: &str, size: f32) -> f32 {
    text.chars().map(|c| glyph_width(font, c, size)).sum()
}

fn draw_glyph(img: &mut RgbImage, font: &FontArc, ch: char, cx: f32, baseline: f32, size: f32, color: Rgb<u8>) {
    let scale = PxScale::from(size);
    let sf = font.as_scaled(scale);
    let gid = sf.glyph_id(ch);
    let g = gid.with_scale_and_position(scale, ab_glyph::point(cx, baseline));
    if let Some(og) = font.outline_glyph(g) {
        let b = og.px_bounds();
        og.draw(|px, py, cov| {
            if cov > 0.35 {
                let gx = b.min.x as i32 + px as i32;
                let gy = b.min.y as i32 + py as i32;
                if gx >= 0 && gy >= 0 && (gx as u32) < W && (gy as u32) < H {
                    img.put_pixel(gx as u32, gy as u32, color);
                }
            }
        });
    }
}

fn draw_str(img: &mut RgbImage, font: &FontArc, text: &str, x: i32, y: i32, size: f32, color: Rgb<u8>) {
    let scale = PxScale::from(size);
    let sf = font.as_scaled(scale);
    let baseline = y as f32 + sf.ascent();
    let mut cx = x as f32;
    for ch in text.chars() {
        draw_glyph(img, font, ch, cx, baseline, size, color);
        cx += glyph_width(font, ch, size);
    }
}

fn draw_str_right(img: &mut RgbImage, font: &FontArc, text: &str, right_x: i32, y: i32, size: f32, color: Rgb<u8>) {
    let w = str_width(font, text, size) as i32;
    draw_str(img, font, text, right_x - w, y, size, color);
}

#[allow(clippy::too_many_arguments)]
fn draw_str_centered(img: &mut RgbImage, font: &FontArc, text: &str, x: i32, y: i32, w: i32, size: f32, color: Rgb<u8>) {
    let tw = str_width(font, text, size) as i32;
    draw_str(img, font, text, x + (w - tw) / 2, y, size, color);
}

// Draws text with a 1-pixel black outline (for legibility on striped bg)
#[allow(clippy::too_many_arguments)]
fn draw_str_outlined(img: &mut RgbImage, font: &FontArc, text: &str, x: i32, y: i32, size: f32, fg: Rgb<u8>, bg: Rgb<u8>) {
    for dx in -1i32..=1 {
        for dy in -1i32..=1 {
            if dx != 0 || dy != 0 {
                draw_str(img, font, text, x + dx, y + dy, size, bg);
            }
        }
    }
    draw_str(img, font, text, x, y, size, fg);
}

// ── Stamp header ─────────────────────────────────────────────────────────────
// Returns y after header (where content starts).
fn stamp_header(img: &mut RgbImage, fonts: &Fonts, x: i32, y: i32, w: i32, label: &str, code: &str) -> i32 {
    let pill = format!(" {} ", label);
    let pw = str_width(&fonts.bold, &pill, 9.0) as i32;
    fill(img, x, y + 1, pw, 12, BLACK);
    draw_str(img, &fonts.bold, &pill, x, y + 1, 9.0, WHITE);

    let code_str = format!("// {}", code);
    let cw = str_width(&fonts.mono, &code_str, 8.0) as i32;
    // dim gray approximation: draw BLACK at half coverage — just use black, it's fine on white
    draw_str(img, &fonts.mono, &code_str, x + w - cw, y + 1, 8.0, BLACK);

    hline(img, x, y + 15, w, BLACK);
    hline(img, x, y + 16, w, BLACK);
    y + 20
}

// ── Corner registration marks ────────────────────────────────────────────────
fn draw_corners(img: &mut RgbImage) {
    let marks = [(0i32, 0i32, 1, 1), (W as i32 - 9, 0, -1, 1), (0, H as i32 - 9, 1, -1), (W as i32 - 9, H as i32 - 9, -1, -1)];
    for (cx, cy, sx, sy) in marks {
        for i in 0..9 {
            img.put_pixel((cx + sx * i).clamp(0, W as i32 - 1) as u32, cy.clamp(0, H as i32 - 1) as u32, BLACK);
            img.put_pixel(cx.clamp(0, W as i32 - 1) as u32, (cy + sy * i).clamp(0, H as i32 - 1) as u32, BLACK);
        }
    }
}

// ── Header ───────────────────────────────────────────────────────────────────
fn draw_header(img: &mut RgbImage, data: &DashData, fonts: &Fonts) {
    // Left black panel (0..200)
    let left_w = 195;
    fill(img, 0, 0, left_w, HEADER_H, BLACK);
    draw_str(img, &fonts.regular, "UNIT", 10, 6, 9.0, Rgb([180, 180, 180]));
    draw_str(img, &fonts.bold, &data.device, 50, 10, 18.0, WHITE);
    // Diagonal cut: draw white triangles at right edge
    for i in 0..14i32 {
        fill(img, left_w - 14 + i, i * HEADER_H / 14, 14 - i, 1, BLACK);
        fill(img, left_w - 14 + i, i * HEADER_H / 14, 14 - i, HEADER_H / 14 + 1, BLACK);
    }

    // Center stripe panel (200..590)
    let stripe_x = left_w;
    let stripe_w = 590 - left_w;
    for py in 0..HEADER_H {
        for px in stripe_x..(stripe_x + stripe_w) {
            let d = ((px + py) % 20 + 20) % 20;
            let c = if d < 6 { RED } else if d < 8 { BLACK } else if d < 14 { RED } else { WHITE };
            img.put_pixel(px as u32, py as u32, c);
        }
    }
    // Motto text centered with outline
    let motto_tw = str_width(&fonts.bold, &data.motto, 11.0) as i32;
    let motto_x = stripe_x + (stripe_w - motto_tw) / 2;
    draw_str_outlined(img, &fonts.bold, &data.motto, motto_x, 14, 11.0, WHITE, BLACK);

    // Right panel (590..800)
    let right_x = 590;
    let date_str = format!("{} · {}", data.date_dow, &data.date_display);
    draw_str_right(img, &fonts.regular, &date_str, W as i32 - 8, 6, 9.5, BLACK);

    let bat_str = format!("BAT {}%", data.battery_pct);
    let sig_str = format!("SIG {}{}", "●".repeat(data.signal_bars as usize), "○".repeat((4 - data.signal_bars) as usize));
    draw_str(img, &fonts.mono, &bat_str, right_x + 6, 20, 9.0, BLACK);
    draw_str_right(img, &fonts.mono, &sig_str, W as i32 - 8, 20, 9.0, BLACK);

    // Header bottom border (3px)
    fill(img, 0, HEADER_H - 3, W as i32, 3, BLACK);
}

// ── Body grid borders ────────────────────────────────────────────────────────
fn draw_grid(img: &mut RgbImage) {
    // Left column right border
    vline(img, C1X + C1W,     BODY_Y, BODY_H, BLACK);
    vline(img, C1X + C1W + 1, BODY_Y, BODY_H, BLACK);
    // Right column left border
    vline(img, C3X - 2, BODY_Y, BODY_H, BLACK);
    vline(img, C3X - 1, BODY_Y, BODY_H, BLACK);
    // Center row divider
    hline(img, C2X, R1Y + R1H,     C2W + 4, BLACK);
    hline(img, C2X, R1Y + R1H + 1, C2W + 4, BLACK);
    // Right col row divider
    hline(img, C3X, R1Y + R1H,     C3W, BLACK);
    hline(img, C3X, R1Y + R1H + 1, C3W, BLACK);
}

// ── Weather panel (left col, spans both rows) ─────────────────────────────────
fn draw_weather(img: &mut RgbImage, data: &DashData, fonts: &Fonts) {
    let x = C1X;
    let y = R1Y;
    let w = C1W;
    let h = BODY_H;
    let pad = 10;

    let cy = stamp_header(img, fonts, x + pad, y + 8, w - pad * 2, "WX", "01");

    // Big temperature
    let temp_str = format!("{}", data.weather.temp_c);
    let temp_size = 88.0;
    let temp_w = str_width(&fonts.bold, &temp_str, temp_size) as i32;
    draw_str(img, &fonts.bold, &temp_str, x + pad, cy, temp_size, BLACK);
    // Degree symbol in RED, offset
    draw_str(img, &fonts.bold, "°", x + pad + temp_w, cy, 40.0, RED);

    // Condition + hi/lo to the right of temperature
    let cond_x = x + pad + temp_w + 48;
    let sf = fonts.bold.as_scaled(PxScale::from(temp_size));
    let temp_bottom = cy + sf.ascent() as i32;
    draw_str(img, &fonts.bold, &data.weather.condition, cond_x, cy + 6, 11.0, BLACK);
    let hilo = format!("↑{}° ↓{}°", data.weather.hi, data.weather.lo);
    draw_str(img, &fonts.regular, &hilo, cond_x, cy + 22, 9.5, BLACK);

    // 4-day forecast grid at the bottom of the weather panel
    let forecast_h = 60;
    let forecast_y = y + h - forecast_h - 4;
    hline(img, x, forecast_y, w, BLACK);

    let cell_w = w / 4;
    for (i, f) in data.weather.forecast.iter().enumerate() {
        let fx = x + i as i32 * cell_w;
        if i > 0 {
            vline(img, fx, forecast_y, forecast_h, Rgb([180, 180, 180]));
        }
        // Day label
        draw_str_centered(img, &fonts.bold, &f.day, fx, forecast_y + 4, cell_w, 9.0, BLACK);
        // Hi temp — larger
        let hi_str = format!("{}°", f.hi);
        draw_str_centered(img, &fonts.bold, &hi_str, fx, forecast_y + 18, cell_w, 17.0, BLACK);
        // Lo + condition
        let lo_cond = format!("{}° {}", f.lo, &f.cond[..f.cond.len().min(3)]);
        draw_str_centered(img, &fonts.regular, &lo_cond, fx, forecast_y + 40, cell_w, 8.0, BLACK);
    }

    // Cluster node strip (between big temp and forecast)
    let _ = temp_bottom; // used for reference
}

// ── Agenda panel (center top) ─────────────────────────────────────────────────
fn draw_agenda(img: &mut RgbImage, data: &DashData, fonts: &Fonts) {
    let x = C2X;
    let y = R1Y;
    let w = C2W;
    let h = R1H;
    let pad = 10;

    let cy = stamp_header(img, fonts, x + pad, y + 6, w - pad, "AGENDA", "02");

    for (i, item) in data.agenda.iter().enumerate() {
        let row_y = cy + i as i32 * 42;
        if row_y + 42 > y + h - 4 { break; }

        // Time — large, accent on first
        let time_color = if i == 0 { RED } else { BLACK };
        draw_str(img, &fonts.bold, &item.time, x + pad, row_y, 14.0, time_color);

        // Title
        draw_str(img, &fonts.bold, &item.title, x + pad, row_y + 17, 10.5, BLACK);

        // Tag pill
        let tag_w = str_width(&fonts.bold, &item.tag, 8.0) as i32 + 8;
        let tag_x = x + w - tag_w - 4;
        fill(img, tag_x, row_y, tag_w, 12, BLACK);
        draw_str(img, &fonts.bold, &item.tag, tag_x + 4, row_y + 1, 8.0, WHITE);

        // Dashed separator
        if i < data.agenda.len() - 1 {
            let sep_y = row_y + 36;
            let mut dx = x + pad;
            while dx < x + w - 4 {
                hline(img, dx, sep_y, 4, BLACK);
                dx += 8;
            }
        }
    }
}

// ── System panel (center bottom) ─────────────────────────────────────────────
fn draw_system(img: &mut RgbImage, data: &DashData, fonts: &Fonts) {
    let x = C2X;
    let y = R2Y;
    let w = C2W;
    let pad = 8;

    let cy = stamp_header(img, fonts, x + pad, y + 6, w - pad, "SYS", "03");

    let stats = [
        ("CPU", data.host.cpu as f32, format!("{}%", data.host.cpu)),
        ("TMP", data.host.cpu_temp as f32 * 100.0 / 100.0, format!("{}°", data.host.cpu_temp)),
        ("RAM", data.host.ram_pct as f32, format!("{}%", data.host.ram_pct)),
        ("DSK", data.host.disk_pct as f32, format!("{}%", data.host.disk_pct)),
    ];

    let box_w = (w - pad * 2 - 3 * 6) / 4; // 4 boxes, 3 gaps of 6px
    for (i, (label, pct, val)) in stats.iter().enumerate() {
        let bx = x + pad + i as i32 * (box_w + 6);
        let by = cy;

        outline(img, bx, by, box_w, 52, BLACK, 1);
        draw_str(img, &fonts.bold, label, bx + 4, by + 4, 8.0, BLACK);

        let val_color = if *pct > 70.0 { RED } else { BLACK };
        draw_str(img, &fonts.bold, val, bx + 4, by + 15, 19.0, val_color);

        let bar_pct = pct.min(100.0);
        let bar_color = if *pct > 70.0 { RED } else { BLACK };
        progress_bar(img, bx + 4, by + 43, box_w - 8, 6, bar_pct, bar_color);
    }

    // Host info line
    let info_y = cy + 60;
    let info = format!(
        "HOST {}   UP {}d   LOAD {} / {} / {}",
        data.host.name,
        data.host.uptime_days,
        data.host.load[0],
        data.host.load[1],
        data.host.load[2],
    );
    draw_str(img, &fonts.mono, &info, x + pad, info_y, 8.5, BLACK);
}

// ── Budget panel (right top) ─────────────────────────────────────────────────
fn draw_budget(img: &mut RgbImage, data: &DashData, fonts: &Fonts) {
    let x = C3X;
    let y = R1Y;
    let w = C3W;
    let pad = 8;

    let cy = stamp_header(img, fonts, x + pad, y + 6, w - pad - 4, "$", "04");

    let spent_str = format!("${}", data.budget.spent);
    draw_str(img, &fonts.bold, &spent_str, x + pad, cy, 24.0, BLACK);

    let cap_str = format!("/ ${} · {}", data.budget.cap, data.budget.month_label);
    draw_str(img, &fonts.regular, &cap_str, x + pad, cy + 28, 8.5, BLACK);

    let pct = data.budget.spent as f32 / data.budget.cap as f32 * 100.0;
    let bar_color = if pct > 85.0 { RED } else { BLACK };
    progress_bar(img, x + pad, cy + 40, w - pad * 2 - 4, 8, pct, bar_color);

    // Category breakdown
    let mut row_y = cy + 55;
    for cat in &data.budget.cats {
        if row_y + 14 > y + R1H - 4 { break; }
        let cat_pct = cat.spent as f32 / cat.cap as f32 * 100.0;
        let cat_color = if cat_pct > 85.0 { RED } else { BLACK };

        draw_str(img, &fonts.bold, &cat.label, x + pad, row_y, 8.0, BLACK);

        let val_str = format!("${}", cat.spent);
        draw_str_right(img, &fonts.mono, &val_str, x + w - 6, row_y, 8.0, BLACK);

        // Mini bar
        let bar_x = x + pad + 38;
        let bar_w = w - pad - 38 - 32;
        progress_bar(img, bar_x, row_y + 2, bar_w, 5, cat_pct, cat_color);

        // Dashed separator
        let sep_y = row_y + 13;
        let mut dx = x + pad;
        while dx < x + w - 6 {
            hline(img, dx, sep_y, 3, Rgb([180, 180, 180]));
            dx += 6;
        }

        row_y += 16;
    }
}

// ── OPS panel (right bottom — tasks + alerts) ────────────────────────────────
fn draw_ops(img: &mut RgbImage, data: &DashData, fonts: &Fonts) {
    let x = C3X;
    let y = R2Y;
    let w = C3W;
    let h = R2H;
    let pad = 8;

    let cy = stamp_header(img, fonts, x + pad, y + 6, w - pad - 4, "OPS", "05");

    // Tasks
    let mut ty = cy;
    for task in data.tasks.iter().take(4) {
        if ty + 14 > y + h - 30 { break; }

        // Checkbox
        outline(img, x + pad, ty + 1, 9, 9, BLACK, 1);
        if task.done {
            fill(img, x + pad + 2, ty + 3, 5, 5, BLACK);
        }

        // Priority badge
        let prio_color = if task.priority == "HI" { RED } else { BLACK };
        draw_str(img, &fonts.bold, &task.priority, x + pad + 13, ty, 8.0, prio_color);

        // Task text (truncated at char boundary)
        let text = truncate(&task.text, 18);
        let td_color = if task.done { Rgb([160, 160, 160]) } else { BLACK };
        draw_str(img, &fonts.regular, &text, x + pad + 35, ty, 8.5, td_color);

        ty += 16;
    }

    // Alert separator
    let sep_y = y + h - 34;
    let mut dx = x + pad;
    while dx < x + w - 6 {
        hline(img, dx, sep_y, 4, BLACK);
        dx += 8;
    }

    // Alerts (last 2)
    let mut ay = sep_y + 4;
    for alert in data.alerts.iter().take(2) {
        if ay + 12 > y + h - 2 { break; }

        let (bg, fg) = match alert.level.as_str() {
            "ERR" => (RED,    WHITE),
            "WRN" => (YELLOW, BLACK),
            _     => (WHITE,  BLACK),
        };
        let lbl = format!(" {} ", &alert.level);
        let lw = str_width(&fonts.bold, &lbl, 7.5) as i32;
        fill(img, x + pad, ay, lw, 11, bg);
        if bg == WHITE { outline(img, x + pad, ay, lw, 11, BLACK, 1); }
        draw_str(img, &fonts.bold, &lbl, x + pad, ay + 1, 7.5, fg);

        let max_chars = 22;
        let msg = if alert.message.len() > max_chars {
            format!("{}…", &alert.message[..max_chars])
        } else {
            alert.message.clone()
        };
        draw_str(img, &fonts.mono, &msg, x + pad + lw + 4, ay + 1, 7.5, BLACK);

        ay += 14;
    }
}

// ── Footer ───────────────────────────────────────────────────────────────────
fn draw_footer(img: &mut RgbImage, data: &DashData, fonts: &Fonts) {
    fill(img, 0, FOOTER_Y, W as i32, FOOTER_H, BLACK);

    let left = format!("SYNC {}→{}", data.last_sync, data.next_sync);
    draw_str(img, &fonts.mono, &left, 10, FOOTER_Y + 5, 9.0, WHITE);

    draw_str(img, &fonts.bold, "● ONLINE", 160, FOOTER_Y + 5, 9.0, RED);

    let right = "neo / TRMNL · e1002";
    draw_str_right(img, &fonts.mono, right, W as i32 - 8, FOOTER_Y + 5, 9.0, WHITE);
}

// ── Public entry point ────────────────────────────────────────────────────────

pub fn render_bmp(data: &DashData, fonts: &Fonts) -> Vec<u8> {
    let mut img: RgbImage = ImageBuffer::new(W, H);

    // White base
    fill(&mut img, 0, 0, W as i32, H as i32, WHITE);

    draw_corners(&mut img);
    draw_header(&mut img, data, fonts);
    draw_grid(&mut img);
    draw_weather(&mut img, data, fonts);
    draw_agenda(&mut img, data, fonts);
    draw_system(&mut img, data, fonts);
    draw_budget(&mut img, data, fonts);
    draw_ops(&mut img, data, fonts);
    draw_footer(&mut img, data, fonts);

    let dyn_img = image::DynamicImage::ImageRgb8(img);
    let mut buf = std::io::Cursor::new(Vec::new());
    dyn_img.write_to(&mut buf, image::ImageFormat::Bmp).expect("BMP encode");
    buf.into_inner()
}

pub fn render_png(data: &DashData, fonts: &Fonts) -> Vec<u8> {
    let mut img: RgbImage = ImageBuffer::new(W, H);
    fill(&mut img, 0, 0, W as i32, H as i32, WHITE);
    draw_corners(&mut img);
    draw_header(&mut img, data, fonts);
    draw_grid(&mut img);
    draw_weather(&mut img, data, fonts);
    draw_agenda(&mut img, data, fonts);
    draw_system(&mut img, data, fonts);
    draw_budget(&mut img, data, fonts);
    draw_ops(&mut img, data, fonts);
    draw_footer(&mut img, data, fonts);

    let dyn_img = image::DynamicImage::ImageRgb8(img);
    let mut buf = std::io::Cursor::new(Vec::new());
    dyn_img.write_to(&mut buf, image::ImageFormat::Png).expect("PNG encode");
    buf.into_inner()
}
