//! Each cell's skeleton in grid coordinates, tagged with the ink it is drawn
//! in and whether each stroke end can join a neighbour.

use std::collections::HashMap;

use kurbo::{Affine, BezPath, PathEl, Point};
use shawline_glyph::metrics::AXIS;
use shawline_glyph::{CELL_HEIGHT, CELL_WIDTH, Skeleton, arrowhead, skeleton};
use shawline_grid::{Class, Dir, Grid, Pos, classify};
use shawline_shape::{Diagram, Port, PortKind, TextKind, stroke};

/// Box-drawing characters whose strokes reach the cell edges. The ASCII
/// fallbacks are drawn as text glyphs and never join their neighbours.
const CANONICAL: &str = "─│┌┐└┘├┤┬┴┼╭╮╯╰┄┆▶◀▲▼";

/// What a run of strokes plays in the drawing, which picks its colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Ink {
    Wire,
    Border,
    Text,
    Comment,
}

/// An open centreline in grid coordinates.
#[derive(Clone, Debug)]
pub(crate) struct Stroke {
    pub path: BezPath,
    pub ink: Ink,
    /// Whether the start and the end lie on a cell edge, where a
    /// neighbour's line may continue them.
    pub on_edge: [bool; 2],
}

/// A closed centreline that is filled and outlined with the pen.
#[derive(Clone, Debug)]
pub(crate) struct Fill {
    pub path: BezPath,
    pub ink: Ink,
}

/// Every stroke and fill of the grid, cell by cell in row-major order, with
/// rows `pitch` apart.
pub(crate) fn pieces(grid: &Grid, diagram: &Diagram, pitch: f64) -> (Vec<Stroke>, Vec<Fill>) {
    let inks = inks(grid, diagram);
    let ports: HashMap<Pos, Port> = diagram
        .boxes
        .iter()
        .flat_map(|b| &b.ports)
        .map(|p| (p.pos, *p))
        .collect();
    let (mut strokes, mut fills) = (Vec::new(), Vec::new());
    for (row, row_inks) in inks.iter().enumerate() {
        for (col, &ink) in row_inks.iter().enumerate() {
            let pos = Pos::new(row, col);
            let ch = grid.get(pos);
            let parts = match ports.get(&pos) {
                Some(port) if CANONICAL.contains(ch) => port_parts(grid, *port, ch),
                _ => skeleton(ch).map(|s| vec![(s, ink)]).unwrap_or_default(),
            };
            let at = Affine::translate((col as f64 * CELL_WIDTH, row as f64 * pitch));
            for (s, ink) in parts {
                for path in s.strokes {
                    let (a, b) = ends(&path);
                    let on_edge = [a, b].map(|p| p.is_some_and(on_cell_edge));
                    strokes.push(Stroke {
                        path: at * path,
                        ink,
                        on_edge,
                    });
                }
                fills.extend(s.fills.into_iter().map(|path| Fill {
                    path: at * path,
                    ink,
                }));
            }
        }
    }
    (strokes, fills)
}

/// The ink of every cell: strokes are wires unless they draw a box border or
/// divider, and everything inside a ※ comment is comment.
fn inks(grid: &Grid, diagram: &Diagram) -> Vec<Vec<Ink>> {
    let mut inks: Vec<Vec<Ink>> = (0..grid.height())
        .map(|r| {
            let stroke = |&ch: &char| classify(ch) == Some(Class::Stroke);
            grid.row(r)
                .iter()
                .map(|ch| if stroke(ch) { Ink::Wire } else { Ink::Text })
                .collect()
        })
        .collect();
    let mut set = |p: Pos, ink| {
        if let Some(cell) = inks.get_mut(p.row).and_then(|r| r.get_mut(p.col)) {
            *cell = ink;
        }
    };
    for b in &diagram.boxes {
        let r = b.rect;
        let rows =
            (r.top..=r.bottom).flat_map(|row| [Pos::new(row, r.left), Pos::new(row, r.right)]);
        let cols =
            (r.left..=r.right).flat_map(|col| [Pos::new(r.top, col), Pos::new(r.bottom, col)]);
        let dividers = b
            .dividers
            .iter()
            .flat_map(|d| (r.left..=r.right).map(move |col| Pos::new(d.row, col)));
        for p in rows.chain(cols).chain(dividers) {
            if diagram.strokes.get(p).is_some() {
                set(p, Ink::Border);
            }
        }
    }
    for t in &diagram.texts {
        let ink = match t.kind {
            TextKind::Comment => Ink::Comment,
            TextKind::Literal => Ink::Text,
            TextKind::Word | TextKind::Number => continue,
        };
        for col in t.pos.col..t.pos.col + t.width {
            set(Pos::new(t.pos.row, col), ink);
        }
    }
    inks
}

/// A port splits between inks: the border runs through it in border ink,
/// and the wire's own strokes (and an arrow's head) are wire ink.
fn port_parts(grid: &Grid, port: Port, ch: char) -> Vec<(Skeleton, Ink)> {
    match port.kind {
        PortKind::Arrow => {
            let along_horizontal = !port.side.is_horizontal();
            let reach: Vec<Dir> = Dir::ALL
                .into_iter()
                .filter(|&d| reaches(grid, port.pos, d))
                .collect();
            let (along, across): (Vec<Dir>, Vec<Dir>) = reach
                .into_iter()
                .partition(|d| d.is_horizontal() == along_horizontal);
            let mut border = arrowhead(port.dir, &along);
            border.fills.clear();
            vec![
                (border, Ink::Border),
                (arrowhead(port.dir, &across), Ink::Wire),
            ]
        }
        PortKind::Tee => {
            let Some(s) = skeleton(ch) else { return vec![] };
            let branch = edge_midpoint(port.dir);
            let (wire, border): (Vec<BezPath>, Vec<BezPath>) =
                s.strokes.into_iter().partition(|p| {
                    let (a, b) = ends(p);
                    a == Some(branch) || b == Some(branch)
                });
            vec![
                (
                    Skeleton {
                        strokes: border,
                        fills: s.fills,
                    },
                    Ink::Border,
                ),
                (
                    Skeleton {
                        strokes: wire,
                        fills: vec![],
                    },
                    Ink::Wire,
                ),
            ]
        }
    }
}

/// True if the cell beside `pos` toward `d` draws a line to their shared edge.
/// Arrowheads count as reaching everywhere, since they sit inside borders.
fn reaches(grid: &Grid, pos: Pos, d: Dir) -> bool {
    let Some(q) = pos.step(d) else { return false };
    let ch = grid.get(q);
    CANONICAL.contains(ch)
        && stroke::fixed(ch)
            .is_some_and(|s| matches!(s.kind, stroke::Kind::Arrow(_)) || s.connects(d.opposite()))
}

fn edge_midpoint(d: Dir) -> Point {
    match d {
        Dir::N => Point::new(CELL_WIDTH / 2.0, 0.0),
        Dir::E => Point::new(CELL_WIDTH, AXIS),
        Dir::S => Point::new(CELL_WIDTH / 2.0, CELL_HEIGHT),
        Dir::W => Point::new(0.0, AXIS),
    }
}

/// The test `shawline_glyph::pen_for` uses to give an end a butt cap.
fn on_cell_edge(p: Point) -> bool {
    p.x == 0.0 || p.y == 0.0 || p.x == CELL_WIDTH || p.y == CELL_HEIGHT
}

/// First and last point of a single open subpath.
pub(crate) fn ends(path: &BezPath) -> (Option<Point>, Option<Point>) {
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
