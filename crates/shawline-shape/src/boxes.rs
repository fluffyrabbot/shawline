//! Box recognition: closed rectangles of strokes.

use shawline_grid::{Diagnostic, Dir, Grid, Pos, Rect};

use crate::stroke::{E, Kind, N, S, Stroke, StrokeMap, W};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoxStyle {
    Square,
    Rounded,
    /// Some corners round, some square: always a diagnostic downstream.
    Mixed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PortKind {
    /// An arrowhead in the border: a wire ends here.
    Arrow,
    /// A tee in the border: a wire starts here.
    Tee,
}

/// A point on a box border where a wire meets it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Port {
    pub pos: Pos,
    /// Which side of the box the port is on.
    pub side: Dir,
    pub kind: PortKind,
    /// For arrows, where the head points; for tees, where the branch leaves.
    pub dir: Dir,
}

impl Port {
    /// True if the wire flows into the box here.
    pub fn is_inward(&self) -> bool {
        self.dir == self.side.opposite()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BoxShape {
    pub rect: Rect,
    pub style: BoxStyle,
    /// Columns `start..end` of the title gap in the top border.
    pub title_gap: Option<(usize, usize)>,
    /// Ports in row-major order.
    pub ports: Vec<Port>,
    /// Interior rows holding a dashed divider (`├┄┄┤`).
    pub dividers: Vec<usize>,
    /// The innermost box enclosing this one.
    pub parent: Option<usize>,
}

pub(crate) fn find(grid: &Grid, strokes: &StrokeMap) -> Vec<BoxShape> {
    let mut boxes: Vec<BoxShape> = (0..grid.height())
        .flat_map(|row| (0..grid.row(row).len()).map(move |col| Pos::new(row, col)))
        .filter_map(|p| trace(grid, strokes, p))
        .collect();
    let rects: Vec<Rect> = boxes.iter().map(|b| b.rect).collect();
    for (i, b) in boxes.iter_mut().enumerate() {
        b.parent = (0..rects.len())
            .filter(|&j| rects[j].encloses(&rects[i]))
            .min_by_key(|&j| rects[j].area());
    }
    boxes
}

/// Boxes must nest or stay apart; anything else is ambiguous.
pub(crate) fn overlaps(boxes: &[BoxShape]) -> Vec<Diagnostic> {
    let mut found = Vec::new();
    for (i, a) in boxes.iter().enumerate() {
        for b in &boxes[i + 1..] {
            let (ra, rb) = (a.rect, b.rect);
            if ra.intersects(&rb) && !ra.encloses(&rb) && !rb.encloses(&ra) {
                let at = Pos::new(rb.top, rb.left);
                found.push(Diagnostic::new(at, format!("box {rb} overlaps box {ra}")));
            }
        }
    }
    found
}

/// The roundness of the corner at `p`, if it is a corner with exactly `conn`.
fn corner(strokes: &StrokeMap, p: Pos, conn: u8) -> Option<bool> {
    match strokes.get(p)? {
        Stroke { conn: c, kind: Kind::Corner { rounded } } if c == conn => Some(rounded),
        _ => None,
    }
}

/// How a cell sits in a border running along `axis`: `None` if it can't,
/// `Some(None)` as plain border, `Some(Some(..))` as a port.
fn border_cell(s: Option<Stroke>, axis: Dir) -> Option<Option<(PortKind, Dir)>> {
    let s = s?;
    match s.kind {
        Kind::Arrow(points) => Some(Some((PortKind::Arrow, points))),
        _ if !s.runs(axis) => None,
        Kind::Tee => {
            let off = Dir::ALL
                .into_iter()
                .find(|&d| d.is_horizontal() != axis.is_horizontal() && s.connects(d))?;
            Some(Some((PortKind::Tee, off)))
        }
        _ => Some(None),
    }
}

fn add_port(ports: &mut Vec<Port>, pos: Pos, side: Dir, cell: Option<(PortKind, Dir)>) {
    if let Some((kind, dir)) = cell {
        ports.push(Port { pos, side, kind, dir });
    }
}

/// Walks clockwise from a candidate top-left corner, returning the box if the
/// border closes.
fn trace(grid: &Grid, strokes: &StrokeMap, tl: Pos) -> Option<BoxShape> {
    let tl_round = corner(strokes, tl, E | S)?;
    let mut ports = Vec::new();

    // Top edge. One run of non-stroke cells is allowed: the title gap.
    let mut title_gap = None;
    let mut col = tl.col + 1;
    let (right, tr_round) = loop {
        if col >= grid.width() {
            return None;
        }
        let p = Pos::new(tl.row, col);
        if let Some(rounded) = corner(strokes, p, W | S) {
            break (col, rounded);
        }
        match strokes.get(p) {
            None if title_gap.is_none() && col > tl.col + 1 => {
                let start = col;
                while col < grid.width() && strokes.get(Pos::new(tl.row, col)).is_none() {
                    col += 1;
                }
                title_gap = Some((start, col));
                continue;
            }
            s => add_port(&mut ports, p, Dir::N, border_cell(s, Dir::E)?),
        }
        col += 1;
    };

    // Right edge.
    let mut row = tl.row + 1;
    let (bottom, br_round) = loop {
        if row >= grid.height() {
            return None;
        }
        let p = Pos::new(row, right);
        if let Some(rounded) = corner(strokes, p, N | W) {
            break (row, rounded);
        }
        add_port(&mut ports, p, Dir::E, border_cell(strokes.get(p), Dir::S)?);
        row += 1;
    };

    // Bottom edge, then the left edge back up to the start.
    for col in (tl.col + 1..right).rev() {
        let p = Pos::new(bottom, col);
        add_port(&mut ports, p, Dir::S, border_cell(strokes.get(p), Dir::E)?);
    }
    let bl_round = corner(strokes, Pos::new(bottom, tl.col), N | E)?;
    for row in (tl.row + 1..bottom).rev() {
        let p = Pos::new(row, tl.col);
        add_port(&mut ports, p, Dir::W, border_cell(strokes.get(p), Dir::S)?);
    }

    let at = |r, c| strokes.get(Pos::new(r, c));
    let dividers: Vec<usize> = (tl.row + 1..bottom)
        .filter(|&r| {
            at(r, tl.col).is_some_and(|s| s.kind == Kind::Tee && s.connects(Dir::E))
                && at(r, right).is_some_and(|s| s.kind == Kind::Tee && s.connects(Dir::W))
                && (tl.col + 1..right)
                    .all(|c| at(r, c).is_some_and(|s| s.kind == Kind::Dashed && s.runs(Dir::E)))
        })
        .collect();
    // A divider's end tees belong to the divider, not to a wire.
    ports.retain(|p| !(p.side.is_horizontal() && dividers.contains(&p.pos.row)));
    ports.sort_by_key(|p| p.pos);

    let corners = [tl_round, tr_round, br_round, bl_round];
    let style = match corners.iter().filter(|&&r| r).count() {
        4 => BoxStyle::Rounded,
        0 => BoxStyle::Square,
        _ => BoxStyle::Mixed,
    };
    Some(BoxShape {
        rect: Rect { top: tl.row, left: tl.col, bottom, right },
        style,
        title_gap,
        ports,
        dividers,
        parent: None,
    })
}
