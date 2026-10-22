//! Grid to SVG, drawing each cell from its `shawline-glyph` skeleton.
//!
//! The output needs no fonts: every letter, digit, mark and stroke is a path
//! drawn with the glyph pen. Lines are never drawn cell by cell. Strokes that
//! continue across a cell edge are chained into one path first (see
//! `chain`), because two antialiased stroke ends meeting at a boundary leave a
//! faint seam however exactly they abut.
//!
//! Colour follows meaning, quietly: box borders, wires, text and comments
//! each get one ink from the [`Theme`].
//!
//! The gloss layer (PLAN P9) is drawn only on request ([`render_with`]): a
//! strip under every row holding each word's reading, or a primitive's short
//! name, in the comment ink.

mod cells;
mod chain;
mod gloss;
mod strip;

use std::fmt::{self, Write};

use kurbo::{BezPath, PathEl, Point};
use shawline_glyph::{CELL_HEIGHT, CELL_WIDTH, STROKE_WEIGHT};
use shawline_grid::Grid;

use cells::{Fill, Ink};
use chain::Chain;

pub use gloss::{Gloss, Options};

/// Output pixels per glyph unit: a cell is 15 × 30 px.
pub const SCALE: f64 = 0.025;
/// Blank space around the drawing on every side, in glyph units (one cell
/// width, half a cell height).
pub const MARGIN: f64 = 600.0;
/// Height of the gloss strip under each row, in glyph units.
const STRIP: f64 = CELL_HEIGHT / 2.0;

/// An sRGB colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl fmt::Display for Rgb {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{:02x}{:02x}{:02x}", self.0, self.1, self.2)
    }
}

/// The palette a program is drawn in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Theme {
    pub background: Rgb,
    /// Words, numbers, literals and titles.
    pub text: Rgb,
    /// Box borders, switch dividers, and the border running through a port.
    pub border: Rgb,
    /// Wires and their arrowheads.
    pub wire: Rgb,
    /// `※` comments.
    pub comment: Rgb,
}

impl Theme {
    /// Ink on warm paper.
    pub const LIGHT: Theme = Theme {
        background: Rgb(0xfa, 0xf9, 0xf6),
        text: Rgb(0x1b, 0x1d, 0x22),
        border: Rgb(0x4a, 0x4f, 0x59),
        wire: Rgb(0x2a, 0x5b, 0x8f),
        comment: Rgb(0x73, 0x78, 0x82),
    };

    /// Chalk on slate.
    pub const DARK: Theme = Theme {
        background: Rgb(0x17, 0x19, 0x1e),
        text: Rgb(0xe8, 0xe6, 0xe1),
        border: Rgb(0xa9, 0xae, 0xb8),
        wire: Rgb(0x86, 0xb0, 0xdf),
        comment: Rgb(0x75, 0x7b, 0x86),
    };

    fn ink(&self, ink: Ink) -> Rgb {
        match ink {
            Ink::Wire => self.wire,
            Ink::Border => self.border,
            Ink::Text => self.text,
            Ink::Comment => self.comment,
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        Theme::LIGHT
    }
}

/// A standalone SVG of the whole grid, [`SCALE`] pixels per glyph unit with
/// [`MARGIN`] around it. The source text rides along in `<desc>`. Plain: no
/// gloss layer.
pub fn render(grid: &Grid, theme: Theme) -> String {
    render_with(grid, Options { theme, gloss: false })
}

/// [`render`] with options. With `gloss`, a strip opens under every row, so
/// rows sit [`row_pitch`] apart, and each shown [`Gloss`] is drawn in it.
/// Without it, the output is exactly [`render`]'s.
pub fn render_with(grid: &Grid, options: Options) -> String {
    let theme = options.theme;
    let pitch = row_pitch(options.gloss);
    let diagram = shawline_shape::recognize(grid);
    let (strokes, fills) = cells::pieces(grid, &diagram, pitch);
    let chains = chain::merge(&strokes);

    let (w, h) = (
        grid.width() as f64 * CELL_WIDTH + 2.0 * MARGIN,
        grid.height() as f64 * pitch + 2.0 * MARGIN,
    );
    let mut out = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{}" height="{}" viewBox="{} {} {} {}" role="img">"#,
        num(w * SCALE),
        num(h * SCALE),
        num(-MARGIN),
        num(-MARGIN),
        num(w),
        num(h),
    );
    out.push('\n');
    let _ = writeln!(out, "<desc>{}</desc>", escape(&source(grid)));
    let _ = writeln!(
        out,
        r#"<rect x="{}" y="{}" width="{}" height="{}" fill="{}"/>"#,
        num(-MARGIN),
        num(-MARGIN),
        num(w),
        num(h),
        theme.background
    );
    let _ = writeln!(
        out,
        r#"<g fill="none" stroke-width="{STROKE_WEIGHT}" stroke-linecap="round" stroke-linejoin="round">"#
    );
    // Heads go over every line so a border visibly runs under its port's
    // arrowhead, and wire ends hide beneath them. Text sits on top.
    for ink in [Ink::Wire, Ink::Border] {
        lines(&mut out, &chains, ink, theme.ink(ink));
    }
    for ink in [Ink::Wire, Ink::Border] {
        solids(&mut out, &fills, ink, theme.ink(ink));
    }
    for ink in [Ink::Text, Ink::Comment] {
        lines(&mut out, &chains, ink, theme.ink(ink));
        solids(&mut out, &fills, ink, theme.ink(ink));
    }
    out.push_str("</g>\n</svg>\n");
    out
}

/// Every Shavian word on the sheet in row-major order, with its reading, a
/// primitive's short name, and where the gloss strip shows it.
pub fn glosses(grid: &Grid) -> Vec<Gloss> {
    strip::glosses(grid)
}

/// The distance from one row's top to the next, in glyph units:
/// [`CELL_HEIGHT`] plain, plus the strip with the gloss layer. Hit-testing a
/// drawing divides by this.
pub fn row_pitch(gloss: bool) -> f64 {
    if gloss { CELL_HEIGHT + STRIP } else { CELL_HEIGHT }
}

/// All chains of one ink as a single element, so it is rasterized as one
/// coverage mask and overlaps (a tee's branch on its through line) never
/// double up. Every end left after chaining is free, so every cap is round.
fn lines(out: &mut String, chains: &[Chain], ink: Ink, color: Rgb) {
    let d: String = chains
        .iter()
        .filter(|c| c.ink == ink)
        .map(|c| path_data(&c.path))
        .collect();
    if !d.is_empty() {
        let _ = writeln!(out, r#"<path d="{d}" stroke="{color}"/>"#);
    }
}

/// Arrowheads and dots of one ink: filled, and outlined with the pen so their
/// corners carry the same round nib.
fn solids(out: &mut String, fills: &[Fill], ink: Ink, color: Rgb) {
    let d: String = fills
        .iter()
        .filter(|f| f.ink == ink)
        .map(|f| path_data(&f.path))
        .collect();
    if !d.is_empty() {
        let _ = writeln!(out, r#"<path d="{d}" fill="{color}" stroke="{color}"/>"#);
    }
}

/// Path data rounded to a tenth of a unit, far below any visible difference.
fn path_data(p: &BezPath) -> String {
    let pt = |p: Point| format!("{},{}", num(p.x), num(p.y));
    let mut out = String::new();
    for el in p.elements() {
        match *el {
            PathEl::MoveTo(a) => out += &format!("M{}", pt(a)),
            PathEl::LineTo(a) => out += &format!("L{}", pt(a)),
            PathEl::QuadTo(a, b) => out += &format!("Q{} {}", pt(a), pt(b)),
            PathEl::CurveTo(a, b, c) => out += &format!("C{} {} {}", pt(a), pt(b), pt(c)),
            PathEl::ClosePath => out.push('Z'),
        }
    }
    out
}

fn num(v: f64) -> String {
    let r = (v * 10.0).round() / 10.0;
    if r == r.trunc() {
        format!("{}", r as i64)
    } else {
        format!("{r:.1}")
    }
}

fn source(grid: &Grid) -> String {
    (0..grid.height())
        .map(|r| grid.row(r).iter().collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests;
