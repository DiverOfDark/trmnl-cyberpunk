//! The memo screen: the note's markdown laid out full-width under the same
//! header and footer as the dashboard.
//!
//! Text is set in Inconsolata (short notes, and H1) and the X11 fixed
//! faces (10x20, 9x15, 8x13, 6x13). They're the u8g2 families that ship
//! both a Latin cut (with €) and a Cyrillic cut with identical metrics, so
//! a note mixing
//! "Grüße" and "привет" renders in one consistent face. Typographic
//! punctuation the cuts lack (— … “ ”) is folded to ASCII, and anything
//! else in neither cut is drawn as `?` — u8g2 otherwise refuses the *whole
//! string* over one missing glyph, which is how a single emoji would blank
//! a paragraph.
//!
//! Styling leans on the panel's inks rather than font variants (there are no
//! italic cuts, and only some sizes have bold): emphasis is blue, bold is
//! double-struck, code is reversed out of black.
//!
//! The note is fitted, not scrolled: the largest of five type sizes that
//! holds the whole note wins, and a note too long even for the smallest is
//! cut at the last whole line with a red `MORE IN EDITOR` tag.

use embedded_graphics::geometry::Point;
use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use u8g2_fonts::types::{FontColor, VerticalPosition};
use u8g2_fonts::{fonts, FontRenderer};

use crate::dashboard::{
    draw_footer, draw_header, draw_header_meta, draw_registration_marks, draw_section_header,
    draw_text, f_small_bold, text_width, Align, BODY_H, BODY_TOP,
};
use crate::data::DashData;
use crate::note::Note;
use crate::render::{Canvas, Rect, C};

// ── Faces ───────────────────────────────────────────────────────────────────

/// A primary font plus fallbacks for glyphs it lacks, sharing metrics.
pub(crate) struct Face {
    fonts: Vec<FontRenderer>,
    /// Baseline offset from the top of the line box.
    ascent: i32,
    line_h: i32,
}

impl Face {
    pub(crate) fn new(fonts: Vec<FontRenderer>, ascent: i32, line_h: i32) -> Self {
        Self {
            fonts,
            ascent,
            line_h,
        }
    }

    /// Split `text` into runs that each render in a single font. Characters
    /// neither font carries become `?` in the primary.
    fn runs(&self, text: &str) -> Vec<(usize, String)> {
        let mut runs: Vec<(usize, String)> = Vec::new();
        for ch in text.chars().flat_map(fold_typography) {
            let (idx, ch) = self
                .fonts
                .iter()
                .position(|f| {
                    f.get_rendered_dimensions(ch, Point::zero(), VerticalPosition::Baseline)
                        .is_ok()
                })
                .map_or((0, '?'), |i| (i, ch));
            match runs.last_mut() {
                Some((i, s)) if *i == idx => s.push(ch),
                _ => runs.push((idx, ch.to_string())),
            }
        }
        runs
    }

    pub(crate) fn width(&self, text: &str) -> i32 {
        self.runs(text)
            .iter()
            .map(|(i, s)| text_width(&self.fonts[*i], s) as i32)
            .sum()
    }

    pub(crate) fn draw(&self, c: &mut Canvas, text: &str, x: i32, baseline: i32, color: C, bold: bool) {
        let mut x = x;
        for (i, s) in self.runs(text) {
            let font = &self.fonts[i];
            let passes: &[i32] = if bold { &[0, 1] } else { &[0] };
            for dx in passes {
                let _ = font.render(
                    s.as_str(),
                    Point::new(x + dx, baseline),
                    VerticalPosition::Baseline,
                    FontColor::Transparent(color.rgb()),
                    c,
                );
            }
            x += text_width(font, &s) as i32;
        }
    }
}

/// One type size: body copy plus the H1 face that heads it.
struct Tier {
    body: Face,
    h1: Face,
    /// H1 is drawn double-struck when its face has no real bold cut.
    h1_bold: bool,
}

/// Map punctuation that word processors (and the WYSIWYG editor) insert
/// onto the ASCII the fixed faces carry, instead of letting it fall to `?`.
fn fold_typography(ch: char) -> std::vec::IntoIter<char> {
    let folded: &str = match ch {
        '\u{2013}' | '\u{2014}' | '\u{2212}' => "-",
        '\u{2018}' | '\u{2019}' | '\u{201A}' => "'",
        '\u{201C}' | '\u{201D}' | '\u{201E}' => "\"",
        '\u{2026}' => "...",
        '\u{2022}' => "*",
        '\u{2192}' => "->",
        '\u{2190}' => "<-",
        '\u{00A0}' | '\u{2009}' | '\u{202F}' => " ",
        _ => return vec![ch].into_iter(),
    };
    folded.chars().collect::<Vec<_>>().into_iter()
}

macro_rules! face {
    ($ascent:expr, $line_h:expr; $($font:ident),+) => {
        Face::new(vec![$(FontRenderer::new::<fonts::$font>()),+], $ascent, $line_h)
    };
}

fn tiers() -> [Tier; 5] {
    let x10 = || face!(15, 21; u8g2_font_10x20_te, u8g2_font_10x20_t_cyrillic);
    let x9 = || face!(12, 17; u8g2_font_9x15_te, u8g2_font_9x15_t_cyrillic);
    let x8 = || face!(11, 15; u8g2_font_8x13_te, u8g2_font_8x13_t_cyrillic);
    let x6 = face!(10, 14; u8g2_font_6x13_te, u8g2_font_6x13_t_cyrillic);
    // Inconsolata has no €; borrow it from 10x20 rather than print `?`.
    let inr24 =
        || face!(26, 34; u8g2_font_inr24_mf, u8g2_font_inr24_t_cyrillic, u8g2_font_10x20_te);
    let inr33 = face!(35, 46; u8g2_font_inr33_mf, u8g2_font_inr33_t_cyrillic, u8g2_font_10x20_te);
    [
        // Short notes get display-size type, readable from across the room.
        Tier {
            body: inr24(),
            h1: inr33,
            h1_bold: false,
        },
        Tier {
            body: x10(),
            h1: inr24(),
            h1_bold: false,
        },
        Tier {
            body: x9(),
            h1: x10(),
            h1_bold: true,
        },
        Tier {
            body: x8(),
            h1: x9(),
            h1_bold: true,
        },
        Tier {
            body: x6,
            h1: x8(),
            h1_bold: true,
        },
    ]
}

// ── Markdown → blocks ───────────────────────────────────────────────────────

#[derive(Copy, Clone, Default, PartialEq, Eq, Debug)]
struct Style {
    bold: bool,
    italic: bool,
    strike: bool,
    code: bool,
    link: bool,
}

#[derive(Clone, Debug)]
struct Span {
    text: String,
    style: Style,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
enum Marker {
    Bullet,
    Number(u64),
    Task(bool),
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Kind {
    Para,
    Heading(u8),
    Item(Marker),
    Code,
    Rule,
}

#[derive(Clone, Debug)]
struct Block {
    kind: Kind,
    /// List nesting depth (0 = top level).
    depth: usize,
    /// Which list an item belongs to, so two adjacent lists don't fuse.
    list: usize,
    quote: usize,
    spans: Vec<Span>,
}

fn parse(markdown: &str) -> Vec<Block> {
    let opts = Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS | Options::ENABLE_TABLES;
    let mut blocks = Vec::new();
    let mut cur: Option<Block> = None;
    let mut style = Style::default();
    // Next ordinal per open list; `None` for bullet lists.
    let mut lists: Vec<Option<u64>> = Vec::new();
    let mut list_ids: Vec<usize> = Vec::new();
    let mut next_list_id = 1usize;
    let mut quote = 0usize;
    let mut in_code = false;
    let mut cell = 0usize;

    let flush = |cur: &mut Option<Block>, blocks: &mut Vec<Block>| {
        if let Some(b) = cur.take() {
            if !b.spans.is_empty() || matches!(b.kind, Kind::Item(_) | Kind::Rule) {
                blocks.push(b);
            }
        }
    };
    let depth = |lists: &Vec<Option<u64>>| lists.len().saturating_sub(1);
    let push = |cur: &mut Option<Block>, lists: &Vec<Option<u64>>, quote, span: Span| {
        cur.get_or_insert_with(|| Block {
            kind: Kind::Para,
            depth: if lists.is_empty() { 0 } else { lists.len() },
            list: 0,
            quote,
            spans: Vec::new(),
        })
        .spans
        .push(span);
    };

    for ev in Parser::new_ext(markdown, opts) {
        match ev {
            Event::Start(Tag::Paragraph) => {
                // A loose list item wraps its text in a paragraph: keep
                // writing into the item rather than orphaning the marker.
                let empty_item = matches!(&cur, Some(b) if matches!(b.kind, Kind::Item(_)) && b.spans.is_empty());
                if !empty_item {
                    flush(&mut cur, &mut blocks);
                }
            }
            Event::Start(Tag::Heading { level, .. }) => {
                flush(&mut cur, &mut blocks);
                let n = match level {
                    HeadingLevel::H1 => 1,
                    HeadingLevel::H2 => 2,
                    _ => 3,
                };
                cur = Some(Block {
                    kind: Kind::Heading(n),
                    depth: 0,
                    list: 0,
                    quote,
                    spans: Vec::new(),
                });
            }
            Event::Start(Tag::BlockQuote(_)) => {
                flush(&mut cur, &mut blocks);
                quote += 1;
            }
            Event::End(TagEnd::BlockQuote(_)) => {
                flush(&mut cur, &mut blocks);
                quote = quote.saturating_sub(1);
            }
            Event::Start(Tag::CodeBlock(_)) => {
                flush(&mut cur, &mut blocks);
                in_code = true;
            }
            Event::End(TagEnd::CodeBlock) => in_code = false,
            Event::Start(Tag::List(first)) => {
                flush(&mut cur, &mut blocks);
                lists.push(first);
                list_ids.push(next_list_id);
                next_list_id += 1;
            }
            Event::End(TagEnd::List(_)) => {
                flush(&mut cur, &mut blocks);
                lists.pop();
                list_ids.pop();
            }
            Event::Start(Tag::Item) => {
                flush(&mut cur, &mut blocks);
                let marker = match lists.last_mut() {
                    Some(Some(n)) => {
                        *n += 1;
                        Marker::Number(*n - 1)
                    }
                    _ => Marker::Bullet,
                };
                cur = Some(Block {
                    kind: Kind::Item(marker),
                    depth: depth(&lists),
                    list: list_ids.last().copied().unwrap_or(0),
                    quote,
                    spans: Vec::new(),
                });
            }
            Event::TaskListMarker(done) => {
                if let Some(b) = cur.as_mut() {
                    b.kind = Kind::Item(Marker::Task(done));
                }
            }
            Event::Start(Tag::Emphasis) => style.italic = true,
            Event::End(TagEnd::Emphasis) => style.italic = false,
            Event::Start(Tag::Strong) => style.bold = true,
            Event::End(TagEnd::Strong) => style.bold = false,
            Event::Start(Tag::Strikethrough) => style.strike = true,
            Event::End(TagEnd::Strikethrough) => style.strike = false,
            Event::Start(Tag::Link { .. }) => style.link = true,
            Event::End(TagEnd::Link) => style.link = false,
            // Table rows become one line each, cells separated by a bar; the
            // header row is bold. Proper columns aren't worth it on a memo.
            Event::Start(Tag::TableHead) => {
                flush(&mut cur, &mut blocks);
                style.bold = true;
                cell = 0;
            }
            Event::End(TagEnd::TableHead) => {
                flush(&mut cur, &mut blocks);
                style.bold = false;
            }
            Event::Start(Tag::TableRow) => {
                flush(&mut cur, &mut blocks);
                cell = 0;
            }
            Event::Start(Tag::TableCell) => {
                if cell > 0 {
                    push(
                        &mut cur,
                        &lists,
                        quote,
                        Span {
                            text: " | ".into(),
                            style: Style::default(),
                        },
                    );
                }
                cell += 1;
            }
            Event::End(
                TagEnd::Paragraph | TagEnd::Heading(_) | TagEnd::Item | TagEnd::TableRow,
            ) => flush(&mut cur, &mut blocks),
            Event::Text(t) if in_code => {
                for line in t.trim_end_matches('\n').split('\n') {
                    blocks.push(Block {
                        kind: Kind::Code,
                        depth: if lists.is_empty() { 0 } else { lists.len() },
                        list: 0,
                        quote,
                        spans: vec![Span {
                            text: line.to_string(),
                            style: Style::default(),
                        }],
                    });
                }
            }
            Event::Text(t) => push(
                &mut cur,
                &lists,
                quote,
                Span {
                    text: t.into_string(),
                    style,
                },
            ),
            Event::Code(t) => push(
                &mut cur,
                &lists,
                quote,
                Span {
                    text: t.into_string(),
                    style: Style {
                        code: true,
                        ..style
                    },
                },
            ),
            Event::SoftBreak => push(
                &mut cur,
                &lists,
                quote,
                Span {
                    text: " ".into(),
                    style,
                },
            ),
            Event::HardBreak => push(
                &mut cur,
                &lists,
                quote,
                Span {
                    text: "\n".into(),
                    style,
                },
            ),
            // The WYSIWYG editor saves blank lines as `<br>`; everything
            // else in raw HTML is markup the panel can't do anything with.
            Event::Html(h) | Event::InlineHtml(h) => {
                if h.trim().to_ascii_lowercase().starts_with("<br") {
                    push(
                        &mut cur,
                        &lists,
                        quote,
                        Span {
                            text: "\n".into(),
                            style,
                        },
                    );
                }
            }
            Event::Rule => {
                flush(&mut cur, &mut blocks);
                blocks.push(Block {
                    kind: Kind::Rule,
                    depth: 0,
                    list: 0,
                    quote,
                    spans: Vec::new(),
                });
            }
            _ => {}
        }
    }
    flush(&mut cur, &mut blocks);
    blocks
}

// ── Blocks → positioned draw ops ────────────────────────────────────────────

#[derive(Copy, Clone)]
enum Which {
    Body,
    H1,
}

enum Op {
    Text {
        x: i32,
        baseline: i32,
        text: String,
        face: Which,
        color: C,
        bold: bool,
    },
    Fill {
        rect: Rect,
        color: C,
    },
    Check {
        x: i32,
        y: i32,
        size: u32,
        done: bool,
    },
}

/// Ops plus the bottom edge of the line each belongs to, so a too-long note
/// can be cut cleanly at a line boundary.
struct Layout {
    ops: Vec<(i32, Op)>,
    height: i32,
}

const QUOTE_W: i32 = 14;

fn layout(blocks: &[Block], tier: &Tier, width: i32) -> Layout {
    let body = &tier.body;
    let indent_w = body.width("    ");
    let marker_w = body.width("00. ");
    let mut ops: Vec<(i32, Op)> = Vec::new();
    let mut y = 0i32;
    let mut prev: Option<&Block> = None;

    for b in blocks {
        // Vertical rhythm: list items and code lines stack tightly, other
        // blocks get half a line of air.
        // A nested list belongs to its parent's run, so only a *new* list
        // at the same depth breaks the stack.
        y += match (prev.map(|p| (&p.kind, p.list, p.depth)), &b.kind) {
            (None, _) => 0,
            (Some((Kind::Item(_), list, depth)), Kind::Item(_))
                if list == b.list || depth != b.depth =>
            {
                0
            }
            (Some((Kind::Code, ..)), Kind::Code) => 0,
            (Some(_), Kind::Heading(_)) => body.line_h * 2 / 3,
            _ => body.line_h / 2,
        };
        prev = Some(b);

        let block_top = y;
        let left = b.quote as i32 * QUOTE_W + b.depth as i32 * indent_w;

        let (face, which, bold, color) = match b.kind {
            Kind::Heading(1) => (&tier.h1, Which::H1, tier.h1_bold, C::Black),
            Kind::Heading(2) => (body, Which::Body, true, C::Blue),
            Kind::Heading(_) => (body, Which::Body, true, C::Black),
            _ => (body, Which::Body, false, C::Black),
        };

        if b.kind == Kind::Rule {
            let mid = y + body.line_h / 2;
            let mut x = left;
            while x < width {
                ops.push((
                    mid + 2,
                    Op::Fill {
                        rect: Rect::new(x, mid - 1, 8, 2),
                        color: C::Black,
                    },
                ));
                x += 12;
            }
            y += body.line_h;
        } else {
            let text_left = match b.kind {
                Kind::Item(_) => left + marker_w,
                Kind::Code => left + 6,
                _ => left,
            };
            let avail = (width - text_left - if b.kind == Kind::Code { 6 } else { 0 }).max(1);
            let lines = wrap(&b.spans, face, avail, b.kind == Kind::Code);

            if let Kind::Item(m) = b.kind {
                let top = y;
                let bottom = top + face.line_h;
                match m {
                    Marker::Bullet => {
                        let s = (face.line_h / 4).max(4);
                        let cy = top + face.ascent - face.ascent / 3;
                        ops.push((
                            bottom,
                            Op::Fill {
                                rect: Rect::new(left + 2, cy - s / 2, s as u32, s as u32),
                                color: C::Red,
                            },
                        ));
                    }
                    Marker::Number(n) => ops.push((
                        bottom,
                        Op::Text {
                            x: left,
                            baseline: top + face.ascent,
                            text: format!("{n}."),
                            face: Which::Body,
                            color: C::Red,
                            bold: true,
                        },
                    )),
                    Marker::Task(done) => {
                        let s = (face.ascent - 2).max(8);
                        ops.push((
                            bottom,
                            Op::Check {
                                x: left + 1,
                                y: top + face.ascent - s,
                                size: s as u32,
                                done,
                            },
                        ));
                    }
                }
            }

            for line in lines {
                let top = y;
                let bottom = top + face.line_h;
                let baseline = top + face.ascent;
                if b.kind == Kind::Code {
                    ops.push((
                        bottom,
                        Op::Fill {
                            rect: Rect::new(left, top, (width - left) as u32, face.line_h as u32),
                            color: C::Black,
                        },
                    ));
                }
                for (x, text, st) in line {
                    let x = text_left + x;
                    let w = face.width(&text);
                    let fg = if b.kind == Kind::Code || st.code {
                        C::White
                    } else if st.italic || st.link {
                        C::Blue
                    } else {
                        color
                    };
                    if st.code && b.kind != Kind::Code {
                        ops.push((
                            bottom,
                            Op::Fill {
                                rect: Rect::new(x - 1, top, (w + 2) as u32, face.line_h as u32),
                                color: C::Black,
                            },
                        ));
                    }
                    if st.link {
                        ops.push((
                            bottom,
                            Op::Fill {
                                rect: Rect::new(x, baseline + 2, w as u32, 1),
                                color: C::Blue,
                            },
                        ));
                    }
                    let strike_y = baseline - face.ascent / 3;
                    ops.push((
                        bottom,
                        Op::Text {
                            x,
                            baseline,
                            text,
                            face: which,
                            color: fg,
                            bold: bold || st.bold,
                        },
                    ));
                    if st.strike {
                        ops.push((
                            bottom,
                            Op::Fill {
                                rect: Rect::new(x, strike_y, w as u32, 2),
                                color: C::Red,
                            },
                        ));
                    }
                }
                y = bottom;
            }

            if b.kind == Kind::Heading(1) {
                ops.push((
                    y + 4,
                    Op::Fill {
                        rect: Rect::new(left, y + 1, (width - left) as u32, 3),
                        color: C::Red,
                    },
                ));
                y += 4;
            }
        }

        if b.quote > 0 {
            for q in 0..b.quote as i32 {
                ops.push((
                    y,
                    Op::Fill {
                        rect: Rect::new(q * QUOTE_W, block_top, 4, (y - block_top) as u32),
                        color: C::Blue,
                    },
                ));
            }
        }
    }

    Layout { ops, height: y }
}

type Line = Vec<(i32, String, Style)>;

/// Greedy word wrap of styled spans into lines of `(x, text, style)`. Words
/// wider than a whole line are broken by character. `preserve` keeps runs of
/// spaces (code); otherwise whitespace collapses as in HTML.
fn wrap(spans: &[Span], face: &Face, avail: i32, preserve: bool) -> Vec<Line> {
    let mut lines: Vec<Line> = vec![Vec::new()];
    let mut x = 0i32;

    for span in spans {
        if span.text == "\n" {
            lines.push(Vec::new());
            x = 0;
            continue;
        }
        for word in split_words(&span.text, preserve) {
            let is_space = word.chars().all(char::is_whitespace);
            if is_space && x == 0 && !preserve {
                continue;
            }
            let w = face.width(&word);
            if x + w <= avail || is_space {
                lines.last_mut().unwrap().push((x, word, span.style));
                x += w;
                continue;
            }
            if x > 0 {
                lines.push(Vec::new());
            }
            if w <= avail {
                lines.last_mut().unwrap().push((0, word, span.style));
                x = w;
                continue;
            }
            // Longer than a line on its own (a URL, usually): hard-break it.
            let mut chunk = String::new();
            for ch in word.chars() {
                let mut next = chunk.clone();
                next.push(ch);
                if face.width(&next) > avail && !chunk.is_empty() {
                    lines
                        .last_mut()
                        .unwrap()
                        .push((0, std::mem::take(&mut chunk), span.style));
                    lines.push(Vec::new());
                }
                chunk.push(ch);
            }
            x = face.width(&chunk);
            lines.last_mut().unwrap().push((0, chunk, span.style));
        }
    }
    lines
}

fn split_words(text: &str, preserve: bool) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for ch in text.chars() {
        let ch = if ch.is_whitespace() { ' ' } else { ch };
        let space = ch == ' ';
        match out.last_mut() {
            Some(w) if w.starts_with(' ') == space => {
                if !space || preserve {
                    w.push(ch);
                }
            }
            _ => out.push(ch.to_string()),
        }
    }
    out
}

// ── Entry point ─────────────────────────────────────────────────────────────

pub fn render(data: &DashData, note: &Note, unit: &str, battery: u8, rssi: i32) -> anyhow::Result<Vec<u8>> {
    let mut c = Canvas::new(crate::render::W, crate::render::H);
    c.fill(C::White);

    draw_registration_marks(&mut c);
    draw_header(&mut c, data, unit);
    draw_header_meta(&mut c, battery, rssi);
    draw_memo(&mut c, note);
    draw_footer(&mut c, data);

    c.into_png()
}

fn draw_memo(c: &mut Canvas, note: &Note) {
    let panel = Rect::new(0, BODY_TOP, crate::render::W, BODY_H);
    let edited = note
        .updated_at
        .map(|t| {
            t.with_timezone(&chrono::Local)
                .format("EDITED %d %b %H:%M")
                .to_string()
                .to_uppercase()
        })
        .unwrap_or_else(|| "// 06".into());
    let top = draw_section_header(c, panel, "MEMO", &edited, None);

    let pad = 12;
    let x0 = panel.x + pad;
    let width = panel.w as i32 - pad * 2;
    let avail = panel.bottom() - 8 - top;

    if note.is_empty() {
        draw_text(
            c,
            &crate::dashboard::f_lg_bold(),
            "NO MEMO",
            x0,
            top + 30,
            C::Black,
            Align::Left,
        );
        draw_text(
            c,
            &f_small_bold(),
            "OPEN THIS SERVER IN A BROWSER TO WRITE ONE",
            x0,
            top + 48,
            C::Black,
            Align::Left,
        );
        return;
    }

    draw_markdown(c, &note.markdown, Rect::new(x0, top, width as u32, avail.max(0) as u32), 0);
}

/// `(done, total)` over the note's task-list items — the desk screen's
/// progress count.
pub(crate) fn task_counts(markdown: &str) -> (usize, usize) {
    parse(markdown).iter().fold((0, 0), |(done, total), b| match b.kind {
        Kind::Item(Marker::Task(d)) => (done + d as usize, total + 1),
        _ => (done, total),
    })
}

/// Lay `markdown` out in `area` at the largest type size that holds it, or
/// cut it at the last whole line with a `MORE IN EDITOR` tag. Sizes above
/// `largest` (an index into the tiers, 0 = display size) are skipped — a
/// narrow column wraps every line at display size.
pub(crate) fn draw_markdown(c: &mut Canvas, markdown: &str, area: Rect, largest: usize) {
    let (x0, top, width, avail) = (area.x, area.y, area.w as i32, area.h as i32);
    let blocks = parse(markdown);
    let tiers = tiers();
    let (tier, lay) = tiers[largest.min(tiers.len() - 1)..]
        .iter()
        .map(|t| (t, layout(&blocks, t, width)))
        .find(|(_, l)| l.height <= avail)
        .unwrap_or_else(|| {
            let t = &tiers[tiers.len() - 1];
            (t, layout(&blocks, t, width))
        });

    // Too long even at the smallest size: keep whole lines up to a strip at
    // the bottom, and say there's more rather than silently dropping it.
    let tag = "MORE IN EDITOR";
    let tag_h = 14;
    let limit = if lay.height > avail {
        avail - tag_h - 4
    } else {
        avail
    };

    for (bottom, op) in &lay.ops {
        if *bottom > limit {
            continue;
        }
        match op {
            Op::Text {
                x,
                baseline,
                text,
                face,
                color,
                bold,
            } => {
                let f = match face {
                    Which::Body => &tier.body,
                    Which::H1 => &tier.h1,
                };
                f.draw(c, text, x0 + x, top + baseline, *color, *bold);
            }
            Op::Fill { rect, color } => {
                c.fill_rect(Rect::new(x0 + rect.x, top + rect.y, rect.w, rect.h), *color)
            }
            Op::Check { x, y, size, done } => {
                let r = Rect::new(x0 + x, top + y, *size, *size);
                if *done {
                    c.fill_rect(r, C::Green);
                    // Tick: short down-stroke then long up-stroke, 2px thick.
                    let (s, bx, by) = (*size as i32, r.x, r.y);
                    for d in 0..2 {
                        c.line(
                            bx + s / 5,
                            by + s / 2 + d,
                            bx + s * 2 / 5,
                            by + s * 3 / 4 + d,
                            C::White,
                            false,
                        );
                        c.line(
                            bx + s * 2 / 5,
                            by + s * 3 / 4 + d,
                            bx + s * 4 / 5,
                            by + s / 4 + d,
                            C::White,
                            false,
                        );
                    }
                } else {
                    c.stroke_rect(r, 2, C::Black);
                }
            }
        }
    }

    if lay.height > avail {
        let w = text_width(&f_small_bold(), tag) + 10;
        let x = area.right() - w as i32;
        let y = top + avail - tag_h;
        c.fill_rect(Rect::new(x, y, w, tag_h as u32), C::Red);
        draw_text(
            c,
            &f_small_bold(),
            tag,
            x + 5,
            y + 11,
            C::White,
            Align::Left,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_common_blocks() {
        let md = "# Title\n\nSome **bold** and *it*.\n\n- [ ] todo\n- [x] done\n\n1. one\n2. two\n\n> quoted\n\n```\nfn x() {}\n```\n\n---\n";
        let kinds: Vec<Kind> = parse(md).into_iter().map(|b| b.kind).collect();
        assert_eq!(
            kinds,
            vec![
                Kind::Heading(1),
                Kind::Para,
                Kind::Item(Marker::Task(false)),
                Kind::Item(Marker::Task(true)),
                Kind::Item(Marker::Number(1)),
                Kind::Item(Marker::Number(2)),
                Kind::Para,
                Kind::Code,
                Kind::Rule,
            ]
        );
    }

    #[test]
    fn unknown_glyphs_fall_back_instead_of_blanking() {
        let face = &tiers()[0].body;
        let runs = face.runs("Grüße привет — 5 € 🙂");
        let text: String = runs.iter().map(|(_, s)| s.as_str()).collect();
        assert_eq!(text, "Grüße привет - 5 € ?");
        assert!(
            runs.iter().any(|(i, _)| *i == 1),
            "Cyrillic should use the fallback cut"
        );
    }

    #[test]
    fn long_notes_shrink_then_truncate() {
        let short = parse("hello");
        let long = parse(&"word ".repeat(3000));
        let t = tiers();
        assert!(layout(&short, &t[0], 776).height < 60);
        assert!(layout(&long, &t[t.len() - 1], 776).height > BODY_H as i32);
    }

    #[test]
    fn renders_png() {
        let note = Note {
            markdown: "# Shopping\n\n- [ ] milk\n- [x] **eggs**\n\nCall *mom* about `Sunday`."
                .into(),
            updated_at: Some(chrono::Utc::now()),
        };
        let png = render(&DashData::mock(), &note, "Kitchen", 80, -60).unwrap();
        assert!(png.starts_with(b"\x89PNG"));
    }
}
