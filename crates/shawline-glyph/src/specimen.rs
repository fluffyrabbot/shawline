//! The specimen sheet: every glyph in labelled rows, plus a small drawing
//! that shows strokes joining across cells.

use std::fmt::Write;

use crate::letters::{Derivation, derivation, letter_name};
use crate::metrics::{ASCENDER, BASELINE, DESCENDER, X_HEIGHT};
use crate::strokes::edge_midpoint;
use crate::{CELL_HEIGHT, CELL_WIDTH, STROKE_WEIGHT, Skeleton, arrowhead, skeleton};
use kurbo::PathEl;
use shawline_grid::Dir;

/// On-sheet pixels per glyph unit.
const SCALE: f64 = 0.1;
const CELL_W: f64 = CELL_WIDTH * SCALE;
const CELL_H: f64 = CELL_HEIGHT * SCALE;
const MARGIN: f64 = 40.0;
const SHEET_WIDTH: f64 = 1600.0;
const INK: &str = "#1d1d1f";
const GUIDE: &str = "#d8dce6";
const LABEL: &str = "#6b7080";

/// The sample drawing: a definition box with a title gap, ports, a tee and
/// a crossing, so every join between neighbouring cells is on show.
const SAMPLE: &[&str] = &[
    "╭─ ·𐑛𐑳𐑚𐑩𐑤 ────────────╮",
    "│          ┌───┐      │",
    "▶ 𐑯 ──┬────▶ 𐑐 ├──╮   │",
    "│     │    └───┘  │   │",
    "│     ╰───────────┼───▶ 𐑬",
    "│                 ╰───▶ 𐑤",
    "╰─────────────────────╯",
    "",
    "   7 ──────╮",
    "    ┌─ 0 ──▼──────┐",
    "    ├┄┄┄┄┄┄┄┄┄┄┄┄┄┤",
    "    └─────────────┘",
];

/// An SVG sheet showing every glyph: Shavian pairs side by side, then
/// digits and marks, then strokes, then a sample drawing assembled from the
/// strokes to prove the joins are seamless.
pub fn specimen() -> String {
    let mut body = String::new();
    let mut y = MARGIN;

    title(&mut body, y, "Shawline glyph skeletons");
    caption(
        &mut body,
        MARGIN,
        y + 22.0,
        &format!(
            "cell {CELL_WIDTH}×{CELL_HEIGHT} · stroke {STROKE_WEIGHT} · guides: ascender {ASCENDER}, x-height {X_HEIGHT}, baseline {BASELINE}, descender {DESCENDER}"
        ),
    );
    y += 50.0;

    let pairs = |first: u32| (0..10).map(move |i| [shavian(first + i), shavian(first + i + 10)]);
    y = pair_row(&mut body, y, "Shavian · tall and deep", pairs(0x10450));
    y = pair_row(&mut body, y, "Shavian · short", pairs(0x10464));
    y = glyph_row(&mut body, y, "Shavian · compounds", (0x10478..=0x1047F).map(shavian), true, 2.0);

    let marks: Vec<char> = ('0'..='9').chain(['·', '«', '»', '※']).collect();
    y = glyph_row(&mut body, y, "Digits and marks", marks.into_iter(), true, 0.8);
    y = glyph_row(&mut body, y, "Strokes", "─│┌┐└┘├┤┬┴┼╭╮╯╰┄┆▶◀▲▼".chars(), false, 0.2);
    y = glyph_row(&mut body, y, "ASCII fallbacks", "-|+.'~:><^v".chars(), true, 1.0);

    heading(&mut body, y, "Joins");
    y += 16.0;
    let grid: Vec<Vec<char>> = SAMPLE.iter().map(|l| l.chars().collect()).collect();
    for (row, line) in grid.iter().enumerate() {
        for col in 0..line.len() {
            let s = in_context(&grid, row, col);
            place(&mut body, MARGIN + col as f64 * CELL_W, y + row as f64 * CELL_H, &s);
        }
    }
    y += SAMPLE.len() as f64 * CELL_H + MARGIN;

    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{SHEET_WIDTH}" height="{y}" viewBox="0 0 {SHEET_WIDTH} {y}" color="{INK}" font-family="Helvetica, Arial, sans-serif">
<rect width="100%" height="100%" fill="#fbfbfa"/>
{body}</svg>
"##
    )
}

fn shavian(cp: u32) -> char {
    char::from_u32(cp).expect("Shavian code point")
}

/// Ten partner pairs, two cells each, labelled with names and derivation.
fn pair_row(out: &mut String, y: f64, name: &str, pairs: impl Iterator<Item = [char; 2]>) -> f64 {
    heading(out, y, name);
    let top = y + 16.0;
    let pitch = (SHEET_WIDTH - 2.0 * MARGIN) / 10.0;
    for (i, [a, b]) in pairs.enumerate() {
        let x = MARGIN + i as f64 * pitch;
        guides(out, x, top, 2.0 * CELL_W);
        cell(out, x, top, a);
        cell(out, x + CELL_W, top, b);
        caption(out, x, top + CELL_H + 16.0, &format!("{} · {}", name_of(a), name_of(b)));
        caption(out, x, top + CELL_H + 32.0, &format!("{}: {}", name_of(b), how(b)));
        if derivation(a) != Derivation::Drawn {
            caption(out, x, top + CELL_H + 48.0, &format!("{}: {}", name_of(a), how(a)));
        }
    }
    top + CELL_H + 76.0
}

/// One row of single cells. `gap` is the space between cells, in cells.
fn glyph_row(out: &mut String, y: f64, name: &str, chars: impl Iterator<Item = char>, with_guides: bool, gap: f64) -> f64 {
    heading(out, y, name);
    let top = y + 16.0;
    let mut labelled = false;
    for (i, ch) in chars.enumerate() {
        let x = MARGIN + i as f64 * CELL_W * (1.0 + gap);
        if with_guides {
            guides(out, x, top, CELL_W);
        } else {
            let _ = write!(out, r#"<rect x="{x}" y="{top}" width="{CELL_W}" height="{CELL_H}" fill="none" stroke="{GUIDE}"/>"#);
        }
        cell(out, x, top, ch);
        if let Some(n) = letter_name(ch) {
            labelled = true;
            caption(out, x, top + CELL_H + 16.0, n);
            caption(out, x, top + CELL_H + 32.0, &how(ch));
        }
    }
    top + CELL_H + if labelled { 60.0 } else { 30.0 }
}

fn name_of(ch: char) -> &'static str {
    letter_name(ch).unwrap_or("?")
}

fn how(ch: char) -> String {
    match derivation(ch) {
        Derivation::Drawn => "drawn".into(),
        Derivation::Rotated(b) => format!("{} turned 180°", name_of(b)),
        Derivation::FlippedH(b) => format!("{} mirrored ↔", name_of(b)),
        Derivation::FlippedV(b) => format!("{} mirrored ↕", name_of(b)),
    }
}

fn cell(out: &mut String, x: f64, y: f64, ch: char) {
    let s = skeleton(ch).unwrap_or_else(|| panic!("{ch:?} is not in the repertoire"));
    place(out, x, y, &s);
}

fn place(out: &mut String, x: f64, y: f64, s: &Skeleton) {
    if !s.is_empty() {
        let _ = write!(out, r#"<g transform="translate({x} {y}) scale({SCALE})">{}</g>"#, s.to_svg());
    }
    out.push('\n');
}

/// A sample cell as a context-aware renderer would draw it: an arrowhead
/// joins every neighbour that reaches toward it, so borders run through ports.
fn in_context(grid: &[Vec<char>], row: usize, col: usize) -> Skeleton {
    let ch = grid[row][col];
    let Some(points) = arrow_dir(ch) else { return skeleton(ch).expect("repertoire character") };
    let at = |r: Option<usize>, c: Option<usize>| Some(*grid.get(r?)?.get(c?)?);
    let edges: Vec<Dir> = Dir::ALL
        .into_iter()
        .filter(|&d| {
            let n = match d {
                Dir::N => at(row.checked_sub(1), Some(col)),
                Dir::S => at(Some(row + 1), Some(col)),
                Dir::E => at(Some(row), Some(col + 1)),
                Dir::W => at(Some(row), col.checked_sub(1)),
            };
            n.is_some_and(|n| reaches(n, d.opposite()))
        })
        .collect();
    arrowhead(points, &edges)
}

fn arrow_dir(ch: char) -> Option<Dir> {
    Some(match ch {
        '▶' => Dir::E,
        '◀' => Dir::W,
        '▲' => Dir::N,
        '▼' => Dir::S,
        _ => return None,
    })
}

/// True if `ch` draws a line to the midpoint of its `d` edge. Arrowheads count
/// as reaching everywhere, as they do in `shawline-shape`.
fn reaches(ch: char, d: Dir) -> bool {
    let target = edge_midpoint(d);
    arrow_dir(ch).is_some()
        || skeleton(ch).is_some_and(|s| {
            s.strokes.iter().any(|p| {
                p.elements().iter().any(|el| match *el {
                    PathEl::MoveTo(a) | PathEl::LineTo(a) | PathEl::CurveTo(_, _, a) => a == target,
                    _ => false,
                })
            })
        })
}

/// Faint ascender, x-height, baseline and descender lines behind letters.
fn guides(out: &mut String, x: f64, y: f64, width: f64) {
    for g in [ASCENDER, X_HEIGHT, BASELINE, DESCENDER] {
        let gy = y + g * SCALE;
        let _ = write!(out, r#"<line x1="{x}" y1="{gy}" x2="{}" y2="{gy}" stroke="{GUIDE}"/>"#, x + width);
    }
}

fn title(out: &mut String, y: f64, text: &str) {
    let _ = writeln!(out, r#"<text x="{MARGIN}" y="{y}" font-size="22" font-weight="bold" fill="{INK}">{text}</text>"#);
}

fn heading(out: &mut String, y: f64, text: &str) {
    let _ = writeln!(out, r#"<text x="{MARGIN}" y="{y}" font-size="14" font-weight="bold" fill="{LABEL}">{text}</text>"#);
}

fn caption(out: &mut String, x: f64, y: f64, text: &str) {
    let _ = writeln!(out, r#"<text x="{x}" y="{y}" font-size="11" fill="{LABEL}">{text}</text>"#);
}
