//! Joins per-cell strokes into long paths, so no cell boundary is ever a
//! place where two antialiased stroke ends meet.
//!
//! Two stroke ends of the same ink at the same point on a cell edge always
//! belong to neighbouring cells whose lines continue each other (a skeleton
//! touches each edge midpoint at most once). Chaining every such pair turns a
//! wire into one path and a closed box border into one closed path, and the
//! pen's round join turns its corners.

use std::collections::HashMap;

use kurbo::{BezPath, PathEl, Point};

use crate::cells::{Ink, Stroke, ends};

/// A run of joined strokes. An open run ends in free ends, drawn round.
#[derive(Clone, Debug)]
pub(crate) struct Chain {
    pub path: BezPath,
    pub ink: Ink,
}

/// A stroke end, as chaining matches them: its point to an eighth of a unit,
/// and its ink.
pub(crate) type Key = (i64, i64, Ink);

pub(crate) fn key(p: Point, ink: Ink) -> Key {
    ((p.x * 8.0).round() as i64, (p.y * 8.0).round() as i64, ink)
}

/// Chains `strokes` in order, so the output is deterministic.
pub(crate) fn merge(strokes: &[Stroke]) -> Vec<Chain> {
    let mut at: HashMap<Key, Vec<usize>> = HashMap::new();
    for (i, s) in strokes.iter().enumerate() {
        let (a, b) = ends(&s.path);
        for (edge, p) in s.on_edge.into_iter().zip([a, b]) {
            if let (true, Some(p)) = (edge, p) {
                at.entry(key(p, s.ink)).or_default().push(i);
            }
        }
    }
    let mut used = vec![false; strokes.len()];
    let mut out = Vec::new();
    for (i, s) in strokes.iter().enumerate() {
        if used[i] {
            continue;
        }
        used[i] = true;
        let mut run = Run {
            path: s.path.clone(),
            on_edge: s.on_edge,
        };
        run.extend(s.ink, strokes, &at, &mut used);
        let (a, b) = ends(&run.path);
        let closed = run.on_edge == [true, true]
            && a.is_some()
            && a.map(|a| key(a, s.ink)) == b.map(|b| key(b, s.ink));
        if closed {
            run.path.close_path();
        } else {
            run.reverse();
            run.extend(s.ink, strokes, &at, &mut used);
            // Run top to bottom, left to right, so the path data reads the
            // way the source does.
            if let (Some(a), Some(b)) = ends(&run.path)
                && (b.y, b.x) < (a.y, a.x)
            {
                run.reverse();
            }
        }
        out.push(Chain {
            path: simplify(&run.path),
            ink: s.ink,
        });
    }
    out
}

/// A chain under construction.
struct Run {
    path: BezPath,
    on_edge: [bool; 2],
}

impl Run {
    fn reverse(&mut self) {
        self.path = self.path.reverse_subpaths();
        self.on_edge.reverse();
    }

    /// Appends strokes for as long as the end lies on a cell edge shared
    /// with exactly one unused stroke.
    fn extend(
        &mut self,
        ink: Ink,
        strokes: &[Stroke],
        at: &HashMap<Key, Vec<usize>>,
        used: &mut [bool],
    ) {
        loop {
            let (_, Some(end)) = ends(&self.path) else {
                return;
            };
            if !self.on_edge[1] {
                return;
            }
            let Some(here) = at.get(&key(end, ink)).filter(|v| v.len() == 2) else {
                return;
            };
            let Some(&j) = here.iter().find(|&&j| !used[j]) else {
                return;
            };
            used[j] = true;
            let mut next = Run {
                path: strokes[j].path.clone(),
                on_edge: strokes[j].on_edge,
            };
            if ends(&next.path).0.map(|s| key(s, ink)) != Some(key(end, ink)) {
                next.reverse();
            }
            for el in &next.path.elements()[1..] {
                self.path.push(*el);
            }
            self.on_edge[1] = next.on_edge[1];
        }
    }
}

/// Drops the points between collinear line segments, so a straight run is a
/// single segment however many cells it spans.
fn simplify(path: &BezPath) -> BezPath {
    let mut out: Vec<PathEl> = Vec::with_capacity(path.elements().len());
    // Start of the last element and the current point.
    let (mut before, mut cur) = (Point::ZERO, Point::ZERO);
    for &el in path.elements() {
        if let (PathEl::LineTo(p), Some(PathEl::LineTo(_))) = (el, out.last()) {
            let (u, v) = (cur - before, p - cur);
            if u.cross(v).abs() <= 1e-6 * u.hypot() * v.hypot() && u.dot(v) > 0.0 {
                *out.last_mut().expect("checked above") = el;
                cur = p;
                continue;
            }
        }
        match el {
            PathEl::MoveTo(p) => (before, cur) = (p, p),
            PathEl::LineTo(p) | PathEl::QuadTo(_, p) | PathEl::CurveTo(_, _, p) => {
                (before, cur) = (cur, p)
            }
            PathEl::ClosePath => {}
        }
        out.push(el);
    }
    BezPath::from_vec(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(x0: f64, x1: f64, on_edge: [bool; 2]) -> Stroke {
        let mut path = BezPath::new();
        path.move_to((x0, 600.0));
        path.line_to((x1, 600.0));
        Stroke {
            path,
            ink: Ink::Wire,
            on_edge,
        }
    }

    #[test]
    fn collinear_runs_become_one_segment() {
        let strokes: Vec<_> = (0..5)
            .map(|i| line(i as f64 * 600.0, (i + 1) as f64 * 600.0, [true, true]))
            .collect();
        let chains = merge(&strokes);
        assert_eq!(chains.len(), 1);
        assert_eq!(chains[0].path.elements().len(), 2, "{:?}", chains[0].path);
    }

    #[test]
    fn reversed_neighbours_join_in_reading_order() {
        let strokes = [
            line(1200.0, 1500.0, [true, false]),
            line(1200.0, 600.0, [true, true]),
            line(300.0, 600.0, [false, true]),
        ];
        let chains = merge(&strokes);
        assert_eq!(chains.len(), 1);
        assert_eq!(
            chains[0].path,
            BezPath::from_svg("M300,600L1500,600").unwrap()
        );
    }

    #[test]
    fn closed_loops_close() {
        let p = |d: &str| BezPath::from_svg(d).unwrap();
        let strokes = [
            Stroke {
                path: p("M600,600L300,600L300,1200"),
                ink: Ink::Border,
                on_edge: [true, true],
            },
            Stroke {
                path: p("M300,1200L300,1800L600,1800"),
                ink: Ink::Border,
                on_edge: [true, true],
            },
            Stroke {
                path: p("M600,1800L900,1800L900,1200"),
                ink: Ink::Border,
                on_edge: [true, true],
            },
            Stroke {
                path: p("M900,1200L900,600L600,600"),
                ink: Ink::Border,
                on_edge: [true, true],
            },
        ];
        let chains = merge(&strokes);
        assert_eq!(chains.len(), 1);
        assert_eq!(chains[0].path.elements().last(), Some(&PathEl::ClosePath));
    }

    #[test]
    fn different_inks_never_join() {
        let mut border = line(600.0, 1200.0, [true, true]);
        border.ink = Ink::Border;
        assert_eq!(merge(&[line(0.0, 600.0, [true, true]), border]).len(), 2);
    }
}
