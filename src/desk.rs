//! The desk screen: what a working day at the desk needs at a glance, under
//! the same header and footer as the dashboard.
//!
//! ```text
//! AGENTS // 01 (270)       │ TODAY // 02 (300)       │ OPS // 03   (230)
//!   CLAUDE  5h % + week    │   NEXT event, later     │   alerts
//!   CODEX   5h % + week    ├─────────────────────────┴──────────────────
//!   tokens · 7 days        │ MEMO // 04  (530, note.md)
//! ```
//!
//! Text that comes from outside (event titles, alerts, the memo) may be in any
//! script, so it's drawn through a `Face` with a Cyrillic fallback rather than
//! straight Helvetica, which would drop the whole string over one glyph.

use chrono::{DateTime, Local, NaiveTime, Utc};
use u8g2_fonts::{fonts, FontRenderer};

use crate::dashboard::{
    draw_footer_with, draw_header, draw_header_meta, draw_registration_marks, draw_section_header,
    draw_text, f_body_bold, f_lg_bold, f_small, f_small_bold, text_width, Align, BODY_H, BODY_TOP,
};
use crate::data::{AgendaItem, AgentUsage, Alert, DashData};
use crate::note::Note;
use crate::note_screen::{draw_markdown, task_counts, Face};
use crate::render::{Canvas, Rect, C};

const MOTTO: &str = "CONTEXT IS FINITE.";

const COL1_W: i32 = 270;
const COL2_W: i32 = 300;
const COL3_W: i32 = 230;
/// Height of the TODAY / OPS strip; the memo spans both columns below it.
const STRIP_H: i32 = 140;
const PAD: i32 = 12;

/// Tokens per day over the last seven local days, oldest first, for each
/// agent that has had a history pushed.
#[derive(Default)]
pub struct TokenWeek {
    pub claude: Option<[u64; 7]>,
    pub codex: Option<[u64; 7]>,
}

pub fn render(
    data: &DashData,
    tokens: &TokenWeek,
    note: &Note,
    unit: &str,
    battery: u8,
    rssi: i32,
) -> anyhow::Result<Vec<u8>> {
    let mut c = Canvas::new(crate::render::W, crate::render::H);
    c.fill(C::White);
    let now = Utc::now();

    let mut header = data.clone();
    header.motto = MOTTO.into();
    draw_registration_marks(&mut c);
    draw_header(&mut c, &header, unit);
    draw_header_meta(&mut c, battery, rssi);

    // Rules, 2px, inside the panel they close: the agents column, the
    // TODAY / OPS split, and the strip's bottom edge over the memo.
    c.fill_rect(Rect::new(COL1_W - 2, BODY_TOP, 2, BODY_H), C::Black);
    c.fill_rect(Rect::new(COL1_W + COL2_W - 2, BODY_TOP, 2, STRIP_H as u32), C::Black);
    c.fill_rect(
        Rect::new(COL1_W, BODY_TOP + STRIP_H - 2, (COL2_W + COL3_W) as u32, 2),
        C::Black,
    );

    let s = &data.status;
    draw_agents(&mut c, data, tokens, s.agents_panel().marker(now).as_deref(), now);
    draw_today(&mut c, &data.agenda, s.agenda.configured, s.agenda.marker(now).as_deref());
    draw_ops(&mut c, &data.alerts, s.alerts.configured, s.ops().marker(now).as_deref());
    draw_memo(&mut c, note);

    draw_footer_with(&mut c, data, &s.desk_degraded(now));
    c.into_png()
}

// ── Faces ───────────────────────────────────────────────────────────────────

/// Helvetica with a Cyrillic fallback of similar size.
fn small() -> Face {
    Face::new(
        vec![FontRenderer::new::<fonts::u8g2_font_helvR08_te>(), FontRenderer::new::<fonts::u8g2_font_6x13_t_cyrillic>()],
        0,
        0,
    )
}
fn small_bold() -> Face {
    Face::new(
        vec![FontRenderer::new::<fonts::u8g2_font_helvB08_te>(), FontRenderer::new::<fonts::u8g2_font_6x13B_t_cyrillic>()],
        0,
        0,
    )
}
fn title_bold() -> Face {
    Face::new(
        vec![FontRenderer::new::<fonts::u8g2_font_helvB12_te>(), FontRenderer::new::<fonts::u8g2_font_7x13_t_cyrillic>()],
        0,
        0,
    )
}

fn f_title() -> FontRenderer {
    FontRenderer::new::<fonts::u8g2_font_helvB12_te>()
}
fn f_hero() -> FontRenderer {
    FontRenderer::new::<fonts::u8g2_font_logisoso32_tn>()
}
fn f_limited() -> FontRenderer {
    FontRenderer::new::<fonts::u8g2_font_logisoso24_tr>()
}

/// `text` cut with `…` until it fits `max_w` in `face`.
fn clip(face: &Face, text: &str, max_w: i32) -> String {
    if face.width(text) <= max_w {
        return text.to_string();
    }
    let mut chars: Vec<char> = text.chars().collect();
    while chars.pop().is_some() {
        let s = format!("{}…", chars.iter().collect::<String>().trim_end());
        if face.width(&s) <= max_w {
            return s;
        }
    }
    "…".into()
}

/// Draw runs of `(text, font, color)` side by side, the last ending at
/// `right` — for right-aligned lines that mix weights.
fn draw_runs_right(c: &mut Canvas, runs: &[(&str, &FontRenderer, C)], right: i32, baseline: i32) {
    let mut x = right;
    for (text, font, color) in runs.iter().rev() {
        draw_text(c, font, text, x, baseline, *color, Align::Right);
        x -= text_width(font, text) as i32;
    }
}

fn draw_runs(c: &mut Canvas, runs: &[(&str, &FontRenderer, C)], left: i32, baseline: i32) {
    let mut x = left;
    for (text, font, color) in runs {
        draw_text(c, font, text, x, baseline, *color, Align::Left);
        x += text_width(font, text) as i32;
    }
}

fn hhmm(t: DateTime<Utc>) -> String {
    t.with_timezone(&Local).format("%H:%M").to_string()
}

/// `MON 09:00` for a reset days away, plain `16:40` for one later today.
fn day_hhmm(t: DateTime<Utc>) -> String {
    let local = t.with_timezone(&Local);
    if local.date_naive() == Local::now().date_naive() {
        local.format("%H:%M").to_string()
    } else {
        local.format("%a %H:%M").to_string().to_uppercase()
    }
}

/// `00:58` for anything within the next 20 hours (unambiguous on a clock
/// face), `MON 09:00` beyond.
fn soon_hhmm(t: DateTime<Utc>) -> String {
    if t - Utc::now() < chrono::Duration::hours(20) {
        hhmm(t)
    } else {
        day_hhmm(t)
    }
}

/// `1H 03M` / `42M`.
fn span(minutes: i64) -> String {
    let m = minutes.max(0);
    if m >= 60 {
        format!("{}H {:02}M", m / 60, m % 60)
    } else {
        format!("{m}M")
    }
}

// ── AGENTS // 01 ────────────────────────────────────────────────────────────

fn draw_agents(c: &mut Canvas, data: &DashData, tokens: &TokenWeek, stale: Option<&str>, now: DateTime<Utc>) {
    let panel = Rect::new(0, BODY_TOP, (COL1_W - 2) as u32, BODY_H);
    let mut y = draw_section_header(c, panel, "AGENTS", "// 01", stale);
    let (x0, x1) = (PAD, panel.right() - PAD);

    let agents: Vec<(&AgentUsage, C)> = [(data.claude.as_ref(), C::Black), (data.codex.as_ref(), C::Blue)]
        .into_iter()
        .filter_map(|(a, ink)| a.map(|a| (a, ink)))
        .collect();
    if agents.is_empty() {
        draw_text(c, &f_small_bold(), "NO AGENT LOGINS", x0, y + 12, C::Black, Align::Left);
        draw_text(c, &f_small(), "SIGN IN AT /agents ON THIS SERVER", x0, y + 26, C::Black, Align::Left);
        y += 40;
    }
    for (a, ink) in agents {
        y = draw_agent(c, a, ink, x0, x1, y, now) + 8;
    }

    draw_tokens(c, tokens, x0, x1, y, panel.bottom() - 6);
}

/// One agent's block; returns the y below its closing rule.
fn draw_agent(c: &mut Canvas, a: &AgentUsage, ink: C, x0: i32, x1: i32, y: i32, now: DateTime<Utc>) -> i32 {
    let (sm, smb) = (f_small(), f_small_bold());

    // Name, and when the 5h window rolls over.
    draw_text(c, &f_title(), &a.name, x0, y + 12, ink, Align::Left);
    match a.session_resets {
        Some(t) => draw_runs_right(c, &[("5H RESETS ", &sm, C::Black), (&hhmm(t), &smb, C::Black)], x1, y + 12),
        None => draw_text(c, &sm, "5H WINDOW IDLE", x1, y + 12, C::Black, Align::Right),
    }
    let mut y = y + 17;

    if a.is_limited() {
        c.fill_rect(Rect::new(x0, y, (x1 - x0) as u32, 36), C::Red);
        draw_text(c, &f_limited(), "LIMITED", x0 + 8, y + 30, C::White, Align::Left);
        let back = a.back_at().map(soon_hhmm).unwrap_or_else(|| "--:--".into());
        draw_text(c, &f_body_bold(), &format!("BACK {back}"), x1 - 8, y + 23, C::White, Align::Right);
        y += 36;
    } else {
        // Hero session %, the bar with its projected tail, and the projection.
        let num = a.session_pct.to_string();
        draw_text(c, &f_hero(), &num, x0, y + 36, C::Black, Align::Left);
        let pct_x = x0 + text_width(&f_hero(), &num) as i32 + 1;
        draw_text(c, &f_lg_bold(), "%", pct_x, y + 36, C::Red, Align::Left);

        let bx = x0 + 78;
        let bar = Rect::new(bx, y + 9, (x1 - bx) as u32, 12);
        let proj = a.session_projection(now);
        draw_bar(c, bar, a.session_pct, proj, ink);
        let proj_txt = proj.map(|p| format!("{p}%")).unwrap_or_else(|| "--".into());
        draw_runs(c, &[("PROJ ", &sm, C::Black), (&proj_txt, &smb, C::Black), (" AT RESET", &sm, C::Black)], bx, y + 34);
        y += 40;
    }
    y += 6;

    // Week: bar with a red tick where even spending would be by now.
    let pace = a.week_pace(now);
    draw_text(c, &smb, "WEEK", x0, y + 9, C::Black, Align::Left);
    draw_text(c, &smb, &format!("{}%", a.week_pct), x1, y + 9, C::Black, Align::Right);
    let bar = Rect::new(x0 + 40, y, (x1 - 32 - x0 - 40) as u32, 10);
    draw_bar(c, bar, a.week_pct, None, ink);
    if let Some(p) = pace {
        let inner = (bar.w - 4) as i32;
        let tx = bar.x + 2 + inner * p as i32 / 100 - 1;
        c.fill_rect(Rect::new(tx, y - 4, 2, 18), C::Red);
    }
    y += 16;

    let resets = a.week_resets.map(day_hhmm).unwrap_or_else(|| "--".into());
    let tail = format!(" · RESETS {resets}");
    match pace {
        Some(p) => {
            let d = a.week_pct as i32 - p as i32;
            let (verdict, color) = match d {
                d if d > 5 => ("OVER PACE", C::Red),
                d if d < -5 => ("UNDER PACE", C::Black),
                _ => ("ON PACE", C::Black),
            };
            draw_runs(c, &[(verdict, &smb, color), (&tail, &sm, C::Black)], x0, y + 9);
        }
        None => draw_text(c, &sm, "WEEK NOT STARTED", x0, y + 9, C::Black, Align::Left),
    }
    y += 17;
    c.hline(x0, y, (x1 - x0) as u32, C::Black);
    y + 1
}

/// 2px-outlined bar filled to `pct` in `ink`, hatched on to `proj` if given.
fn draw_bar(c: &mut Canvas, r: Rect, pct: u8, proj: Option<u8>, ink: C) {
    c.stroke_rect(r, 2, C::Black);
    let inner = Rect::new(r.x + 2, r.y + 2, r.w - 4, r.h - 4);
    let at = |p: u8| inner.w * p.min(100) as u32 / 100;
    let fill = at(pct);
    c.fill_rect(Rect::new(inner.x, inner.y, fill, inner.h), ink);
    if let Some(p) = proj {
        let end = at(p);
        if end > fill {
            c.hatch_135(
                Rect::new(inner.x + fill as i32, inner.y, end - fill, inner.h),
                &[(1, ink), (3, C::White)],
            );
        }
    }
}

/// Stacked daily token bars, Codex under Claude, today hatched because it
/// isn't over yet.
fn draw_tokens(c: &mut Canvas, t: &TokenWeek, x0: i32, x1: i32, y: i32, bottom: i32) {
    let (sm, smb) = (f_small(), f_small_bold());
    draw_text(c, &smb, "TOKENS · 7 DAYS", x0, y + 9, C::Black, Align::Left);
    // Legend, right to left.
    let mut lx = x1;
    for (name, ink, has) in [("CODEX", C::Blue, t.codex.is_some()), ("CLAUDE", C::Black, t.claude.is_some())] {
        if !has {
            continue;
        }
        draw_text(c, &sm, name, lx, y + 9, C::Black, Align::Right);
        lx -= text_width(&sm, name) as i32 + 12;
        c.fill_rect(Rect::new(lx, y + 1, 8, 8), ink);
        lx -= 8;
    }

    let labels_h = 12;
    let top = y + 17;
    let base = bottom - labels_h - 4; // chart's bottom rule
    let chart_h = (base - top).clamp(24, 140);
    let top = base - chart_h;
    c.fill_rect(Rect::new(x0, base, (x1 - x0) as u32, 2), C::Black);

    let claude = t.claude.unwrap_or_default();
    let codex = t.codex.unwrap_or_default();
    let max = (0..7).map(|i| claude[i] + codex[i]).max().unwrap_or(0);
    let gap = 6;
    let bar_w = ((x1 - x0) - gap * 6) as f32 / 7.0;
    let today = Local::now().date_naive();
    let plot_h = chart_h - 4;

    if max == 0 {
        let msg = if t.claude.is_none() && t.codex.is_none() { "NO HISTORY PUSHED" } else { "NO TOKENS THIS WEEK" };
        draw_text(c, &sm, msg, (x0 + x1) / 2, top + chart_h / 2 + 4, C::Black, Align::Center);
    }
    for i in 0..7 {
        let bx = x0 + (i as f32 * (bar_w + gap as f32)).round() as i32;
        let bw = bar_w.round() as u32;
        let is_today = i == 6;
        let h = |v: u64| if max == 0 { 0 } else { (v as f64 / max as f64 * plot_h as f64).round() as i32 };
        let (h_codex, h_claude) = (h(codex[i]), h(claude[i]));
        let mut seg_bottom = base;
        for (seg_h, ink) in [(h_codex, C::Blue), (h_claude, C::Black)] {
            if seg_h <= 0 {
                continue;
            }
            let r = Rect::new(bx, seg_bottom - seg_h, bw, seg_h as u32);
            if is_today {
                c.hatch_135(r, &[(2, ink), (2, C::White)]);
            } else {
                c.fill_rect(r, ink);
            }
            seg_bottom -= seg_h;
        }

        let day = today - chrono::Duration::days(6 - i as i64);
        let label = day.format("%a").to_string().to_uppercase();
        let font = if is_today { &smb } else { &sm };
        draw_text(c, font, &label, bx + bw as i32 / 2, bottom - 2, C::Black, Align::Center);
    }
}

// ── TODAY // 02 ─────────────────────────────────────────────────────────────

/// Timed events still to start today, in order.
fn upcoming(items: &[AgendaItem], now: NaiveTime) -> impl Iterator<Item = &AgendaItem> {
    items
        .iter()
        .filter(move |e| NaiveTime::parse_from_str(&e.time, "%H:%M").is_ok_and(|t| t > now))
}

fn draw_today(c: &mut Canvas, agenda: &[AgendaItem], calendar: bool, stale: Option<&str>) {
    let panel = Rect::new(COL1_W, BODY_TOP, (COL2_W - 2) as u32, (STRIP_H - 2) as u32);
    let mut y = draw_section_header(c, panel, "TODAY", "// 02", stale);
    let (x0, x1) = (panel.x + PAD, panel.right() - PAD);
    let (sm, title) = (small(), title_bold());

    // NEXT: blue time cell, title, and how long until it.
    let box_h = 44;
    c.stroke_rect(Rect::new(x0, y, (x1 - x0) as u32, box_h as u32), 2, C::Black);
    let cell = Rect::new(x0 + 2, y + 2, 58, (box_h - 4) as u32);
    c.fill_rect(cell, C::Blue);
    let cx = cell.x + cell.w as i32 / 2;
    draw_text(c, &f_small_bold(), "NEXT", cx, y + 16, C::White, Align::Center);

    let now = Local::now().time();
    let mut events = upcoming(agenda, now);
    let next = events.next();
    let tx = cell.right() + 8;
    let tw = x1 - 8 - tx;
    let (time, headline, detail) = match next {
        Some(ev) => {
            let start = NaiveTime::parse_from_str(&ev.time, "%H:%M").unwrap_or(now);
            let mut parts = vec![format!("IN {}", span((start - now).num_minutes() + 1))];
            if !ev.duration.is_empty() {
                parts.push(ev.duration.to_uppercase());
            }
            (ev.time.clone(), ev.title.clone(), parts.join(" · "))
        }
        None if !calendar => ("--:--".into(), "NO CALENDAR".into(), "SET ICS_URLS".into()),
        None => ("--:--".into(), "NOTHING ELSE TODAY".into(), String::new()),
    };
    draw_text(c, &f_lg_bold(), &time, cx, y + 35, C::White, Align::Center);
    title.draw(c, &clip(&title, &headline, tw), tx, y + 20, C::Black, false);
    sm.draw(c, &clip(&sm, &detail, tw), tx, y + 34, C::Black, false);
    y += box_h + 4;

    // What follows, one line each; all-day events close the list.
    let row_h = 14;
    let rows = ((panel.bottom() - 4 - y) / row_h).max(0) as usize;
    let all_day = agenda.iter().filter(|e| e.time == "ALL");
    let later: Vec<(&str, &AgendaItem)> = events
        .map(|e| (e.time.as_str(), e))
        .chain(all_day.map(|e| ("ALL", e)))
        .collect();
    let shown = if later.len() > rows { rows.saturating_sub(1) } else { later.len() };
    for (i, (time, ev)) in later.iter().take(shown).enumerate() {
        let base = y + i as i32 * row_h + 10;
        draw_text(c, &f_small_bold(), time, x0, base, C::Black, Align::Left);
        let dur_w = text_width(&f_small(), &ev.duration) as i32;
        draw_text(c, &f_small(), &ev.duration, x1, base, C::Black, Align::Right);
        sm.draw(c, &clip(&sm, &ev.title, x1 - dur_w - 6 - (x0 + 36)), x0 + 36, base, C::Black, false);
    }
    if shown < later.len() {
        let more = format!("+{} MORE TODAY", later.len() - shown);
        draw_text(c, &f_small(), &more, x0, y + shown as i32 * row_h + 10, C::Black, Align::Left);
    }
}

// ── OPS // 03 ───────────────────────────────────────────────────────────────

/// `(target, what)` out of an alert line: the fetcher writes
/// `alertname · target`; anything else splits at its first space.
fn split_alert(message: &str) -> (String, String) {
    if let Some((what, target)) = message.split_once(" · ") {
        return (target.to_string(), what.to_string());
    }
    match message.split_once(' ') {
        Some((target, what)) => (target.to_string(), what.to_string()),
        None => (message.to_string(), String::new()),
    }
}

fn draw_ops(c: &mut Canvas, alerts: &[Alert], configured: bool, stale: Option<&str>) {
    let panel = Rect::new(COL1_W + COL2_W, BODY_TOP, COL3_W as u32, (STRIP_H - 2) as u32);
    let y = draw_section_header(c, panel, "OPS", "// 03", stale);
    let (x0, x1) = (panel.x + PAD, panel.right() - PAD);
    let (sm, smb) = (small(), small_bold());

    if alerts.is_empty() {
        let (head, sub) = if configured { ("ALL CLEAR", "NO FIRING ALERTS") } else { ("NO ALERTS", "ALERTMANAGER_URL NOT SET") };
        draw_text(c, &f_title(), head, x0, y + 16, C::Black, Align::Left);
        draw_text(c, &f_small(), sub, x0, y + 30, C::Black, Align::Left);
        return;
    }

    // One line per alert: level pill, target in bold, what, time.
    let row_h = 15;
    let rows = ((panel.bottom() - 4 - y) / row_h).max(0) as usize;
    let shown = if alerts.len() > rows { rows.saturating_sub(1) } else { alerts.len() };
    for (i, a) in alerts.iter().take(shown).enumerate() {
        let ry = y + i as i32 * row_h;
        let (bg, fg) = match a.level.as_str() {
            "ERR" => (C::Red, C::White),
            "WRN" => (C::Yellow, C::Black),
            _ => (C::Black, C::White),
        };
        c.fill_rect(Rect::new(x0, ry, 30, 12), bg);
        draw_text(c, &f_small_bold(), &a.level, x0 + 15, ry + 10, fg, Align::Center);

        let tx = x0 + 36;
        let right = x1 - text_width(&f_small(), &a.time) as i32 - 6;
        draw_text(c, &f_small(), &a.time, x1, ry + 10, C::Black, Align::Right);
        let (target, what) = split_alert(&a.message);
        let target = clip(&smb, &target, (right - tx) * 3 / 5);
        smb.draw(c, &target, tx, ry + 10, C::Black, false);
        let wx = tx + smb.width(&target) + 5;
        sm.draw(c, &clip(&sm, &what, right - wx), wx, ry + 10, C::Black, false);
    }
    if shown < alerts.len() {
        let more = format!("+{} MORE", alerts.len() - shown);
        draw_text(c, &f_small(), &more, x0, y + shown as i32 * row_h + 10, C::Black, Align::Left);
    }
}

// ── MEMO // 04 ──────────────────────────────────────────────────────────────

/// Largest memo type size on this screen (an index into the memo tiers): the
/// 8x13 face. The memo shares the panel with three others, so it reads as a
/// list rather than a poster, and long notes keep more lines on screen.
const MEMO_LARGEST_TIER: usize = 3;

fn draw_memo(c: &mut Canvas, note: &Note) {
    let panel = Rect::new(COL1_W, BODY_TOP + STRIP_H, (COL2_W + COL3_W) as u32, (BODY_H as i32 - STRIP_H) as u32);
    let (done, total) = task_counts(&note.markdown);
    let label = if total > 0 {
        format!("{done} / {total} DONE")
    } else {
        note.updated_at
            .map(|t| t.with_timezone(&Local).format("EDITED %d %b %H:%M").to_string().to_uppercase())
            .unwrap_or_else(|| "// 04".into())
    };
    let y = draw_section_header(c, panel, "MEMO", &label, None);
    let (x0, x1) = (panel.x + PAD, panel.right() - PAD);

    if note.is_empty() {
        draw_text(c, &f_lg_bold(), "NO MEMO", x0, y + 18, C::Black, Align::Left);
        small_bold().draw(c, "WRITE ONE AT / ON THIS SERVER", x0, y + 34, C::Black, false);
        return;
    }
    let area = Rect::new(x0, y, (x1 - x0) as u32, (panel.bottom() - 6 - y).max(0) as u32);
    draw_markdown(c, &note.markdown, area, MEMO_LARGEST_TIER);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(time: &str, title: &str) -> AgendaItem {
        AgendaItem { time: time.into(), title: title.into(), tag: String::new(), duration: String::new() }
    }

    #[test]
    fn upcoming_skips_started_and_all_day_events() {
        let items = [ev("ALL", "Holiday"), ev("09:00", "Gym"), ev("15:30", "Standup"), ev("17:00", "UPS")];
        let now = NaiveTime::from_hms_opt(14, 27, 0).unwrap();
        let titles: Vec<_> = upcoming(&items, now).map(|e| e.title.as_str()).collect();
        assert_eq!(titles, ["Standup", "UPS"]);
        let late = NaiveTime::from_hms_opt(18, 0, 0).unwrap();
        assert!(upcoming(&items, late).next().is_none());
    }

    #[test]
    fn alert_lines_split_into_target_and_what() {
        assert_eq!(split_alert("TargetDown · node-3:9100"), ("node-3:9100".into(), "TargetDown".into()));
        assert_eq!(split_alert("velero backup.daily failed rc=2"), ("velero".into(), "backup.daily failed rc=2".into()));
    }

    #[test]
    fn spans_read_like_the_design() {
        assert_eq!(span(63), "1H 03M");
        assert_eq!(span(42), "42M");
    }

    #[test]
    fn clipping_keeps_cyrillic() {
        let face = small();
        assert_eq!(clip(&face, "Привет", 500), "Привет");
        let cut = clip(&face, &"Очень длинная тема письма ".repeat(5), 120);
        assert!(cut.ends_with('…') && face.width(&cut) <= 120, "{cut}");
    }

    #[test]
    fn renders_mock_and_empty_states() {
        let tokens = TokenWeek {
            claude: Some([2_800_000, 1_100_000, 400_000, 0, 5_200_000, 6_300_000, 4_100_000]),
            codex: Some([900_000, 1_600_000, 0, 300_000, 600_000, 1_800_000, 700_000]),
        };
        let note = Note { markdown: "- [x] Rotate certs\n- [ ] Renew домен\n\nCall *Anna* re: UPS".into(), updated_at: Some(Utc::now()) };
        let png = render(&DashData::mock(), &tokens, &note, "Desk", 84, -60).unwrap();
        assert!(png.starts_with(b"\x89PNG"));

        let mut limited = DashData::mock();
        if let Some(c) = limited.codex.as_mut() {
            c.limited = true;
        }
        render(&limited, &TokenWeek::default(), &Note::default(), "", 0, 0).unwrap();
        render(&DashData::empty(), &TokenWeek::default(), &Note::default(), "", 0, 0).unwrap();
    }
}
