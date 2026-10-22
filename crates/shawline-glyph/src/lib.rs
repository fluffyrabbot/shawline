//! Procedural stroke skeletons for every repertoire character, the single
//! geometric source for both the SVG renderer and Shawline Mono.
//!
//! A glyph is a [`Skeleton`]: centreline paths that are all drawn with one
//! [`pen`], so the whole repertoire shares a single hand. Geometry lives in
//! cell units: one cell is [`CELL_WIDTH`] × [`CELL_HEIGHT`], origin at the
//! cell's top-left corner, y growing downward (as in SVG). A font builder
//! flips y about [`metrics::BASELINE`].

mod gloss;
mod letters;
mod marks;
mod specimen;
mod strokes;

use kurbo::{Affine, BezPath, Cap, Ellipse, Join, PathEl, Point, Rect, Shape, Stroke};
use shawline_grid::{Class, classify};

pub use gloss::{GLOSS_CHARS, GLOSS_HEIGHT, GLOSS_WIDTH, gloss_skeleton};
pub use specimen::specimen;
pub use strokes::arrowhead;

/// Width of one grid cell, in glyph units.
pub const CELL_WIDTH: f64 = 600.0;
/// Height of one grid cell, in glyph units. Rows stack with no gap, so box
/// drawing joins vertically.
pub const CELL_HEIGHT: f64 = 1200.0;
/// The one stroke weight every skeleton is drawn with.
pub const STROKE_WEIGHT: f64 = 72.0;
/// Ink radius of a dot (namer dot, full stop, colon, reference-mark dots).
pub const DOT_RADIUS: f64 = 52.0;

/// Horizontal guide lines, as y offsets from the cell top. They are skeleton
/// guides: centrelines sit on them, and ink reaches half a stroke beyond.
///
/// The x-height band is centred on the cell's horizontal axis, so a 180°
/// rotation about the cell centre carries a tall letter exactly onto the deep
/// position, and short letters stay short. Wires also pass through the axis,
/// so text sits optically centred on them.
pub mod metrics {
    /// Top of tall letters.
    pub const ASCENDER: f64 = 110.0;
    /// Top of the digits.
    pub const FIGURE_HEIGHT: f64 = 280.0;
    /// Top of short letters.
    pub const X_HEIGHT: f64 = 400.0;
    /// The cell's horizontal centre line, where wires run.
    pub const AXIS: f64 = 600.0;
    /// Bottom of short and tall letters.
    pub const BASELINE: f64 = 800.0;
    /// Bottom of deep letters.
    pub const DESCENDER: f64 = 1090.0;
}

/// The pen: [`STROKE_WEIGHT`] with round caps and joins, like a round nib.
/// Fills are outlined with it as it is.
pub fn pen() -> Stroke {
    Stroke::new(STROKE_WEIGHT).with_caps(Cap::Round).with_join(Join::Round)
}

/// The [`pen`] as it draws one stroke centreline: an end that lies on the
/// cell boundary gets a butt cap, every other end stays round.
///
/// A line that runs to an edge then stops exactly there and meets the
/// neighbouring cell's line flush. Overlapping round caps would composite
/// twice under antialiasing and leave a dark bead at every cell boundary.
pub fn pen_for(path: &BezPath) -> Stroke {
    let cap = |p: Option<Point>| if p.is_some_and(on_boundary) { Cap::Butt } else { Cap::Round };
    let (start, end) = ends(path);
    pen().with_start_cap(cap(start)).with_end_cap(cap(end))
}

fn on_boundary(p: Point) -> bool {
    p.x == 0.0 || p.y == 0.0 || p.x == CELL_WIDTH || p.y == CELL_HEIGHT
}

/// First and last point of a single open subpath.
fn ends(path: &BezPath) -> (Option<Point>, Option<Point>) {
    let els = path.elements();
    let start = match els.first() {
        Some(PathEl::MoveTo(p)) => Some(*p),
        _ => None,
    };
    let end = match els.last() {
        Some(PathEl::LineTo(p) | PathEl::QuadTo(_, p) | PathEl::CurveTo(_, _, p)) => Some(*p),
        _ => None,
    };
    (start, end)
}

/// The geometry of one glyph.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Skeleton {
    /// Open centrelines, one subpath each, stroked with [`pen_for`] and never
    /// filled.
    pub strokes: Vec<BezPath>,
    /// Closed centrelines that are filled and also stroked with [`pen`]
    /// (dots and arrowheads), so their corners carry the same round nib.
    pub fills: Vec<BezPath>,
}

impl Skeleton {
    /// This skeleton with every path mapped through `t`.
    pub fn transformed(&self, t: Affine) -> Skeleton {
        let map = |paths: &[BezPath]| paths.iter().map(|p| t * p).collect();
        Skeleton { strokes: map(&self.strokes), fills: map(&self.fills) }
    }

    /// True for the empty skeleton of a space.
    pub fn is_empty(&self) -> bool {
        self.strokes.is_empty() && self.fills.is_empty()
    }

    /// Bounds of the centrelines (not the ink, which is half a stroke wider).
    pub fn bounds(&self) -> Option<Rect> {
        self.strokes
            .iter()
            .chain(&self.fills)
            .map(Shape::bounding_box)
            .reduce(|a, b| a.union(b))
    }

    /// SVG elements drawing this skeleton in cell units, coloured with
    /// `currentColor`. Wrap them in a `<g transform=…>` to place the cell.
    ///
    /// SVG has one cap per path, so a stroke with mixed ends (see
    /// [`pen_for`]) is drawn butt-capped with a disc on its round end.
    pub fn to_svg(&self) -> String {
        let pen = |cap: &str| {
            format!(
                r#"stroke="currentColor" stroke-width="{STROKE_WEIGHT}" stroke-linecap="{cap}" stroke-linejoin="round""#
            )
        };
        let (mut round, mut butt, mut discs) = (Vec::new(), Vec::new(), Vec::new());
        for p in &self.strokes {
            let s = pen_for(p);
            if (s.start_cap, s.end_cap) == (Cap::Round, Cap::Round) {
                round.push(svg_path_data(p));
                continue;
            }
            butt.push(svg_path_data(p));
            let (a, b) = ends(p);
            for (cap, at) in [(s.start_cap, a), (s.end_cap, b)] {
                if let (Cap::Round, Some(at)) = (cap, at) {
                    discs.push(at);
                }
            }
        }
        let mut out = String::new();
        for (paths, cap) in [(round, "round"), (butt, "butt")] {
            if !paths.is_empty() {
                out += &format!(r#"<path d="{}" fill="none" {}/>"#, paths.join(" "), pen(cap));
            }
        }
        for c in discs {
            let r = STROKE_WEIGHT / 2.0;
            out += &format!(r#"<circle cx="{}" cy="{}" r="{r}" fill="currentColor"/>"#, c.x, c.y);
        }
        let pen = pen("round");
        for f in &self.fills {
            out += &format!(r#"<path d="{}" fill="currentColor" {pen}/>"#, svg_path_data(f));
        }
        out
    }
}

/// The skeleton of `ch`, or `None` if `ch` is outside the repertoire. A space
/// has an empty skeleton.
pub fn skeleton(ch: char) -> Option<Skeleton> {
    match classify(ch)? {
        Class::Space => Some(Skeleton::default()),
        Class::Shavian => letters::skeleton(ch),
        Class::Stroke => strokes::skeleton(ch),
        Class::NamerDot | Class::Digit | Class::Mark => marks::skeleton(ch),
    }
}

/// 180° rotation about the cell centre: tall ↔ deep, short stays short.
pub(crate) const ROTATE: Affine = Affine::new([-1.0, 0.0, 0.0, -1.0, CELL_WIDTH, CELL_HEIGHT]);
/// Reflection across the cell's vertical centre line.
pub(crate) const FLIP_H: Affine = Affine::new([-1.0, 0.0, 0.0, 1.0, CELL_WIDTH, 0.0]);
/// Reflection across the cell's horizontal axis: tall ↔ deep, mirrored.
pub(crate) const FLIP_V: Affine = Affine::new([1.0, 0.0, 0.0, -1.0, 0.0, CELL_HEIGHT]);

/// Skeleton from SVG path data (all paths are compile-time literals, so a
/// parse failure is a bug caught by the coverage test).
pub(crate) fn paths(ds: &[&str]) -> Vec<BezPath> {
    ds.iter().map(|d| BezPath::from_svg(d).unwrap_or_else(|e| panic!("bad path {d:?}: {e:?}"))).collect()
}

pub(crate) fn strokes_only(ds: &[&str]) -> Skeleton {
    Skeleton { strokes: paths(ds), fills: vec![] }
}

/// A filled dot whose ink radius is [`DOT_RADIUS`] once the pen goes round it.
pub(crate) fn dot(x: f64, y: f64) -> BezPath {
    ellipse(x, y, DOT_RADIUS - STROKE_WEIGHT / 2.0, DOT_RADIUS - STROKE_WEIGHT / 2.0)
}

pub(crate) fn ellipse(x: f64, y: f64, rx: f64, ry: f64) -> BezPath {
    Ellipse::new(Point::new(x, y), (rx, ry), 0.0).to_path(0.01)
}

/// Path data rounded to a tenth of a unit, which is far below any visible
/// difference and keeps the SVG compact.
fn svg_path_data(p: &BezPath) -> String {
    let n = |v: f64| {
        let r = (v * 10.0).round() / 10.0;
        if r == r.trunc() { format!("{}", r as i64) } else { format!("{r:.1}") }
    };
    let pt = |p: Point| format!("{},{}", n(p.x), n(p.y));
    let mut out = Vec::new();
    for el in p.elements() {
        out.push(match *el {
            PathEl::MoveTo(a) => format!("M{}", pt(a)),
            PathEl::LineTo(a) => format!("L{}", pt(a)),
            PathEl::QuadTo(a, b) => format!("Q{} {}", pt(a), pt(b)),
            PathEl::CurveTo(a, b, c) => format!("C{} {} {}", pt(a), pt(b), pt(c)),
            PathEl::ClosePath => "Z".into(),
        });
    }
    out.join("")
}

#[cfg(test)]
mod tests {
    use super::*;
    use shawline_grid::repertoire::{MARKS, NAMER_DOT, SHAVIAN, STROKES};

    fn repertoire() -> impl Iterator<Item = char> {
        SHAVIAN
            .chain('0'..='9')
            .chain([NAMER_DOT])
            .chain(STROKES.iter().copied())
            .chain(MARKS.iter().copied())
    }

    #[test]
    fn every_repertoire_character_has_a_skeleton() {
        for ch in repertoire() {
            let s = skeleton(ch).unwrap_or_else(|| panic!("{ch:?} has no skeleton"));
            assert!(!s.is_empty(), "{ch:?} is empty");
        }
        assert_eq!(skeleton(' '), Some(Skeleton::default()));
        assert_eq!(skeleton('a'), None);
    }

    #[test]
    fn ink_stays_inside_the_cell_except_at_stroke_ends() {
        // Box drawing deliberately reaches the cell edges; everything else
        // keeps its ink inside the cell.
        let half = STROKE_WEIGHT / 2.0;
        for ch in repertoire().filter(|c| !strokes::is_box_drawing(*c)) {
            let b = skeleton(ch).unwrap().bounds().unwrap().inflate(half, half);
            assert!(
                b.x0 >= 0.0 && b.y0 >= 0.0 && b.x1 <= CELL_WIDTH && b.y1 <= CELL_HEIGHT,
                "{ch:?} ink {b:?} leaves the cell"
            );
        }
    }

    #[test]
    fn strokes_are_single_open_subpaths() {
        // `pen_for` picks caps from a path's two ends, so it needs exactly two.
        for ch in repertoire() {
            for p in skeleton(ch).unwrap().strokes {
                let moves = p.elements().iter().filter(|e| matches!(e, PathEl::MoveTo(_))).count();
                let closed = p.elements().iter().any(|e| matches!(e, PathEl::ClosePath));
                assert!(moves == 1 && !closed, "{ch:?} has a stroke that is not one open subpath");
            }
        }
    }

    #[test]
    fn only_ends_on_the_cell_edge_are_butt() {
        let s = skeleton('┼').unwrap();
        let caps: Vec<_> = s.strokes.iter().map(pen_for).map(|p| (p.start_cap, p.end_cap)).collect();
        assert_eq!(caps, [(Cap::Butt, Cap::Butt), (Cap::Butt, Cap::Round), (Cap::Round, Cap::Butt)]);
        let tot = skeleton('\u{10451}').unwrap();
        assert_eq!((pen_for(&tot.strokes[0]).start_cap, pen_for(&tot.strokes[0]).end_cap), (Cap::Round, Cap::Round));
    }

    #[test]
    fn transforms_are_involutions() {
        for t in [ROTATE, FLIP_H, FLIP_V] {
            assert_eq!(t * t, Affine::IDENTITY);
        }
        assert_eq!(FLIP_H * FLIP_V, ROTATE);
    }
}
