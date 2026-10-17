//! Box drawing, arrowheads and their ASCII fallbacks.
//!
//! Every canonical line runs through the cell centre to the exact midpoint of
//! the cell edge it connects to, so a neighbour's line continues it with no
//! seam. The ASCII fallbacks are drawn as the plain ASCII characters they are.

use kurbo::{Arc, BezPath, Point, Vec2};
use shawline_grid::Dir;
use std::f64::consts::FRAC_PI_2;

use crate::metrics::{AXIS, BASELINE, X_HEIGHT};
use crate::{CELL_HEIGHT, CELL_WIDTH, DOT_RADIUS, FLIP_H, STROKE_WEIGHT, Skeleton, dot, strokes_only};

const CX: f64 = CELL_WIDTH / 2.0;
const CY: f64 = AXIS;

/// Radius of `╭ ╮ ╯ ╰`. Half the cell width: the arc meets the side edge
/// tangentially at its midpoint, then runs straight to the top or bottom.
const CORNER_RADIUS: f64 = 300.0;
/// Half-length of each dash's centreline in `┄ ┆`. Dashes repeat every 200
/// units and sit centred on the cell edges, so they tile in both phases.
const DASH_HALF: f64 = 30.0;
const DASH_PITCH: f64 = 200.0;
/// Visible clearance either side of the through-line in `┼`.
const CROSSING_CLEARANCE: f64 = 54.0;
/// Arrowhead: length along the wire and half-width across it.
const ARROW_LENGTH: f64 = 260.0;
const ARROW_HALF_WIDTH: f64 = 150.0;

/// True for the canonical characters whose strokes reach the cell edges.
#[cfg(test)]
pub(crate) fn is_box_drawing(ch: char) -> bool {
    "─│┌┐└┘├┤┬┴┼╭╮╯╰┄┆▶◀▲▼".contains(ch)
}

pub(crate) fn skeleton(ch: char) -> Option<Skeleton> {
    let h = format!("M0,{CY} L{CELL_WIDTH},{CY}");
    let v = format!("M{CX},0 L{CX},{CELL_HEIGHT}");
    let n = format!("M{CX},0 L{CX},{CY}");
    let s = format!("M{CX},{CY} L{CX},{CELL_HEIGHT}");
    let e = format!("M{CX},{CY} L{CELL_WIDTH},{CY}");
    let w = format!("M{CX},{CY} L0,{CY}");
    Some(match ch {
        '─' => strokes_only(&[&h]),
        '│' => strokes_only(&[&v]),
        // Square corners are one path, so the pen's round join turns them.
        '┌' => strokes_only(&[&format!("M{CELL_WIDTH},{CY} L{CX},{CY} L{CX},{CELL_HEIGHT}")]),
        '┐' => strokes_only(&[&format!("M0,{CY} L{CX},{CY} L{CX},{CELL_HEIGHT}")]),
        '└' => strokes_only(&[&format!("M{CX},0 L{CX},{CY} L{CELL_WIDTH},{CY}")]),
        '┘' => strokes_only(&[&format!("M{CX},0 L{CX},{CY} L0,{CY}")]),
        '├' => strokes_only(&[&v, &e]),
        '┤' => strokes_only(&[&v, &w]),
        '┬' => strokes_only(&[&h, &s]),
        '┴' => strokes_only(&[&h, &n]),
        '┼' => crossing(),
        '╭' => rounded(1.0, 1.0),
        '╮' => rounded(-1.0, 1.0),
        '╰' => rounded(1.0, -1.0),
        '╯' => rounded(-1.0, -1.0),
        '┄' => dashed(|t| Point::new(t, CY), CELL_WIDTH),
        '┆' => dashed(|t| Point::new(CX, t), CELL_HEIGHT),
        '▶' => arrowhead(Dir::E, &[Dir::W]),
        '◀' => arrowhead(Dir::W, &[Dir::E]),
        '▲' => arrowhead(Dir::N, &[Dir::S]),
        '▼' => arrowhead(Dir::S, &[Dir::N]),
        _ => ascii(ch)?,
    })
}

/// `┼`: the horizontal wire passes over; the vertical one breaks around it,
/// so the cell can never be mistaken for a four-way junction.
fn crossing() -> Skeleton {
    let gap = STROKE_WEIGHT + CROSSING_CLEARANCE;
    strokes_only(&[
        &format!("M0,{CY} L{CELL_WIDTH},{CY}"),
        &format!("M{CX},0 L{CX},{}", CY - gap),
        &format!("M{CX},{} L{CX},{CELL_HEIGHT}", CY + gap),
    ])
}

/// A rounded corner opening toward (`dx`, `dy`): `╭` is (1, 1), east and south.
/// The path runs from the horizontal edge midpoint, round a true circular
/// arc, to the vertical edge midpoint.
fn rounded(dx: f64, dy: f64) -> Skeleton {
    let r = CORNER_RADIUS;
    let side = Point::new(CX + dx * CX, CY);
    let end = Point::new(CX, if dy > 0.0 { CELL_HEIGHT } else { 0.0 });
    let centre = Point::new(CX + dx * r, CY + dy * r);
    let arc_start = Point::new(CX + dx * r, CY);
    let start_angle = (arc_start - centre).atan2();
    // Sweep a quarter turn toward the vertical line through the cell centre.
    let sweep = if dx * dy > 0.0 { -FRAC_PI_2 } else { FRAC_PI_2 };
    let mut p = BezPath::new();
    p.move_to(side);
    if arc_start != side {
        p.line_to(arc_start);
    }
    let arc = Arc::new(centre, (r, r), start_angle, sweep, 0.0);
    arc.to_cubic_beziers(0.01, |a, b, c| p.curve_to(a, b, c));
    p.line_to(end);
    Skeleton { strokes: vec![p], fills: vec![] }
}

fn dashed(at: impl Fn(f64) -> Point, length: f64) -> Skeleton {
    let count = (length / DASH_PITCH).round() as usize;
    let strokes = (0..=count)
        .map(|i| {
            let mid = i as f64 * DASH_PITCH;
            let mut p = BezPath::new();
            p.move_to(at((mid - DASH_HALF).max(0.0)));
            p.line_to(at((mid + DASH_HALF).min(length)));
            p
        })
        .collect();
    Skeleton { strokes, fills: vec![] }
}

/// An arrowhead pointing `points`, with a line from the cell centre to the
/// midpoint of each edge in `edges`.
///
/// The head is solid and centred in the cell. On its own (`skeleton('▶')`)
/// only the incoming wire reaches an edge. Every port arrowhead also sits in
/// a box border, and a renderer that knows the context passes the border's
/// edges too, so the border runs on through the head instead of breaking.
pub fn arrowhead(points: Dir, edges: &[Dir]) -> Skeleton {
    let c = Point::new(CX, CY);
    let d = unit(points);
    let across = Vec2::new(-d.y, d.x) * ARROW_HALF_WIDTH;
    let tip = c + d * (ARROW_LENGTH / 2.0);
    let base = c - d * (ARROW_LENGTH / 2.0);
    let mut head = BezPath::new();
    head.move_to(base + across);
    head.line_to(tip);
    head.line_to(base - across);
    head.close_path();
    let strokes = edges
        .iter()
        .map(|&e| {
            let mut p = BezPath::new();
            p.move_to(edge_midpoint(e));
            p.line_to(c);
            p
        })
        .collect();
    Skeleton { strokes, fills: vec![head] }
}

fn unit(d: Dir) -> Vec2 {
    match d {
        Dir::N => Vec2::new(0.0, -1.0),
        Dir::E => Vec2::new(1.0, 0.0),
        Dir::S => Vec2::new(0.0, 1.0),
        Dir::W => Vec2::new(-1.0, 0.0),
    }
}

/// Where a line leaving the cell centre toward `d` meets the cell edge.
pub(crate) fn edge_midpoint(d: Dir) -> Point {
    match d {
        Dir::N => Point::new(CX, 0.0),
        Dir::E => Point::new(CELL_WIDTH, CY),
        Dir::S => Point::new(CX, CELL_HEIGHT),
        Dir::W => Point::new(0.0, CY),
    }
}

/// The ASCII fallbacks, as ordinary text glyphs in the same hand.
fn ascii(ch: char) -> Option<Skeleton> {
    // Dots sit so their ink lines up with stroke ink on the guide lines.
    let low = BASELINE + STROKE_WEIGHT / 2.0 - DOT_RADIUS;
    let high = X_HEIGHT - STROKE_WEIGHT / 2.0 + DOT_RADIUS;
    Some(match ch {
        '-' => strokes_only(&["M170,600 L430,600"]),
        '|' => strokes_only(&["M300,110 L300,1090"]),
        '+' => strokes_only(&["M160,600 L440,600", "M300,460 L300,740"]),
        '.' => Skeleton { strokes: vec![], fills: vec![dot(CX, low)] },
        '\'' => strokes_only(&["M300,110 L300,320"]),
        '~' => strokes_only(&["M160,630 C200,555 255,555 300,600 C345,645 400,645 440,570"]),
        ':' => Skeleton { strokes: vec![], fills: vec![dot(CX, high), dot(CX, low)] },
        '>' => strokes_only(&["M190,440 L420,600 L190,760"]),
        '<' => strokes_only(&["M190,440 L420,600 L190,760"]).transformed(FLIP_H),
        '^' => strokes_only(&["M180,370 L300,170 L420,370"]),
        // The same form as 𐑫 wool: a plain v at x-height.
        'v' => strokes_only(&["M180,400 L300,800 L420,400"]),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use kurbo::{ParamCurve, PathEl, Shape};

    /// Which edges each box-drawing character must reach: N E S W.
    fn edges(ch: char) -> [bool; 4] {
        let has = |set: &str| set.contains(ch);
        [
            has("│└┘├┤┴┼╯╰┆▼"),
            has("─┌└├┬┴┼╭╰┄◀"),
            has("│┌┐├┤┬┼╭╮┆▲"),
            has("─┐┘┤┬┴┼╮╯┄▶"),
        ]
    }

    fn endpoints(s: &Skeleton) -> Vec<Point> {
        let mut out = Vec::new();
        for p in &s.strokes {
            for el in p.elements() {
                match *el {
                    PathEl::MoveTo(a) | PathEl::LineTo(a) | PathEl::QuadTo(_, a) | PathEl::CurveTo(_, _, a) => {
                        out.push(a)
                    }
                    PathEl::ClosePath => {}
                }
            }
        }
        out
    }

    #[test]
    fn box_drawing_meets_edges_exactly_at_their_midpoints() {
        let mids = [
            Point::new(CX, 0.0),
            Point::new(CELL_WIDTH, CY),
            Point::new(CX, CELL_HEIGHT),
            Point::new(0.0, CY),
        ];
        for ch in "─│┌┐└┘├┤┬┴┼╭╮╯╰┄┆▶◀▲▼".chars() {
            let s = skeleton(ch).unwrap();
            let pts = endpoints(&s);
            for p in &pts {
                let on_edge = p.x == 0.0 || p.x == CELL_WIDTH || p.y == 0.0 || p.y == CELL_HEIGHT;
                assert!(!on_edge || mids.contains(p), "{ch:?} touches an edge off-centre at {p:?}");
            }
            for (mid, want) in mids.iter().zip(edges(ch)) {
                assert_eq!(pts.contains(mid), want, "{ch:?} at {mid:?}");
            }
            let b = s.bounds().unwrap();
            assert!(b.x0 >= 0.0 && b.y0 >= 0.0 && b.x1 <= CELL_WIDTH && b.y1 <= CELL_HEIGHT, "{ch:?} {b:?}");
        }
    }

    #[test]
    fn rounded_corners_are_circular_arcs() {
        for ch in "╭╮╯╰".chars() {
            let p = &skeleton(ch).unwrap().strokes[0];
            let b = p.bounding_box();
            assert!((b.width() - CORNER_RADIUS).abs() < 1e-9 && (b.height() - CY).abs() < 1e-9, "{ch:?} {b:?}");
            // Sample the curve: every point on the arc part is CORNER_RADIUS
            // from the arc centre.
            let centre = Point::new(
                if b.x0 < CX { CX - CORNER_RADIUS } else { CX + CORNER_RADIUS },
                if b.y0 < CY { CY - CORNER_RADIUS } else { CY + CORNER_RADIUS },
            );
            for seg in p.segments() {
                if let kurbo::PathSeg::Cubic(c) = seg {
                    for i in 0..=8 {
                        let d = (c.eval(i as f64 / 8.0) - centre).hypot();
                        assert!((d - CORNER_RADIUS).abs() < 0.1, "{ch:?} off-circle by {}", d - CORNER_RADIUS);
                    }
                }
            }
        }
    }

    #[test]
    fn crossing_leaves_a_gap_in_one_line() {
        let s = skeleton('┼').unwrap();
        assert_eq!(s.strokes.len(), 3);
        assert!(s.fills.is_empty(), "no junction blob");
        let half = STROKE_WEIGHT / 2.0;
        for v in &s.strokes[1..] {
            let b = v.bounding_box().inflate(half, half);
            assert!(b.y1 < CY - half || b.y0 > CY + half, "vertical ink touches the horizontal");
        }
    }
}
