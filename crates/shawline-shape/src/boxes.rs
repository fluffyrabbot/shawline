//! Box recognition: closed rectangles of strokes, and near boxes whose
//! border has cells slipped one cell off their line (SPEC §3).

use std::collections::{BTreeMap, BTreeSet};

use shawline_grid::{Diagnostic, Dir, Grid, Pos, Rect};

use crate::stroke::{self, E, Kind, N, S, Stroke, StrokeMap, W};

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
    /// Dashed dividers between switch arms, top to bottom.
    pub dividers: Vec<Divider>,
    /// The innermost box enclosing this one.
    pub parent: Option<usize>,
    /// Border cells drawn one cell off their line, in order of `at`. Empty
    /// for a box whose border closes exactly; each one is an `S012`.
    pub slips: Vec<Slip>,
}

/// A near box's border cell that sits one cell off its line (SPEC §3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Slip {
    /// Where the cell is drawn.
    pub at: Pos,
    /// Where it belongs on the border. Always blank.
    pub home: Pos,
}

/// A dashed rule across a box (`├┄┄┄┤`, or `├┄ 1 ┄┤` with a guard gap).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Divider {
    pub row: usize,
    /// Columns `start..end` of the guard gap.
    pub gap: Option<(usize, usize)>,
}

type RectKey = (usize, usize, usize, usize);

fn key(r: Rect) -> RectKey {
    (r.top, r.left, r.bottom, r.right)
}

pub(crate) fn find(grid: &Grid, strokes: &StrokeMap) -> Vec<BoxShape> {
    let mut boxes: Vec<BoxShape> = (0..grid.height())
        .flat_map(|row| (0..grid.row(row).len()).map(move |col| Pos::new(row, col)))
        .filter_map(|p| trace(grid, strokes, p))
        .collect();
    // A border that closes exactly is never reread as a near box, so a
    // blank in a closed top edge stays a title gap.
    let exact: BTreeSet<RectKey> = boxes.iter().map(|b| key(b.rect)).collect();
    for (top, left, bottom, right) in near_candidates(grid, strokes) {
        if !exact.contains(&(top, left, bottom, right)) {
            boxes.extend(near(grid, strokes, Rect { top, left, bottom, right }));
        }
    }
    boxes.sort_by_key(|b| key(b.rect));

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
                found.push(Diagnostic::new(at, "S001", format!("box {rb} overlaps box {ra}")));
            }
        }
    }
    found
}

/// One `S012` per slipped border cell, at the cell (SPEC §14).
pub(crate) fn slips(boxes: &[BoxShape]) -> Vec<Diagnostic> {
    boxes
        .iter()
        .flat_map(|b| &b.slips)
        .map(|s| {
            // A direction, not a coordinate: positions in messages would be
            // zero-based while the CLI prints one-based locations.
            let way = match (s.home.row.cmp(&s.at.row), s.home.col.cmp(&s.at.col)) {
                (std::cmp::Ordering::Less, _) => "up",
                (std::cmp::Ordering::Greater, _) => "down",
                (_, std::cmp::Ordering::Less) => "left",
                _ => "right",
            };
            let message = format!(
                "border cell is one cell out of line; it belongs one cell {way} (`shawline fmt --fix` moves it)"
            );
            Diagnostic::new(s.at, "S012", message)
        })
        .collect()
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

    let rect = Rect { top: tl.row, left: tl.col, bottom, right };
    let corners = [tl_round, tr_round, br_round, bl_round];
    Some(finish(strokes, rect, title_gap, ports, corners, Vec::new()))
}

/// Completes a box whose border has been read: dividers, port order, style.
fn finish(
    strokes: &StrokeMap,
    rect: Rect,
    title_gap: Option<(usize, usize)>,
    mut ports: Vec<Port>,
    corners: [bool; 4],
    mut slips: Vec<Slip>,
) -> BoxShape {
    let dividers: Vec<Divider> = (rect.top + 1..rect.bottom)
        .filter_map(|row| divider(strokes, row, rect.left, rect.right))
        .collect();
    // A divider's end tees belong to the divider, not to a wire.
    ports.retain(|p| !(p.side.is_horizontal() && dividers.iter().any(|d| d.row == p.pos.row)));
    ports.sort_by_key(|p| p.pos);
    slips.sort_by_key(|s| s.at);

    let style = match corners.iter().filter(|&&r| r).count() {
        4 => BoxStyle::Rounded,
        0 => BoxStyle::Square,
        _ => BoxStyle::Mixed,
    };
    BoxShape { rect, style, title_gap, ports, dividers, parent: None, slips }
}

/// Every rectangle with at least three corners in place. Any three corners
/// include both top corners or both bottom ones, so a pair on one row plus a
/// corner in either of its columns finds them all.
fn near_candidates(grid: &Grid, strokes: &StrokeMap) -> BTreeSet<RectKey> {
    const SHAPES: [u8; 4] = [E | S, W | S, N | W, N | E];
    // Corner rows by (shape, column), and corner columns by (shape, row).
    let mut rows: BTreeMap<(u8, usize), Vec<usize>> = BTreeMap::new();
    let mut cols: BTreeMap<(u8, usize), Vec<usize>> = BTreeMap::new();
    for row in 0..grid.height() {
        for col in 0..grid.row(row).len() {
            let p = Pos::new(row, col);
            for shape in SHAPES.into_iter().filter(|&c| corner(strokes, p, c).is_some()) {
                rows.entry((shape, col)).or_default().push(row);
                cols.entry((shape, row)).or_default().push(col);
            }
        }
    }
    let at = |map: &BTreeMap<(u8, usize), Vec<usize>>, shape: u8, i: usize| -> Vec<usize> {
        map.get(&(shape, i)).cloned().unwrap_or_default()
    };

    let mut found = BTreeSet::new();
    for row in 0..grid.height() {
        // Both top corners on `row`; the bottom from either bottom corner.
        for left in at(&cols, E | S, row) {
            for right in at(&cols, W | S, row).into_iter().filter(|&r| r > left) {
                let below = at(&rows, N | E, left).into_iter().chain(at(&rows, N | W, right));
                found.extend(below.filter(|&b| b > row).map(|b| (row, left, b, right)));
            }
        }
        // Both bottom corners on `row`; the top from either top corner.
        for left in at(&cols, N | E, row) {
            for right in at(&cols, N | W, row).into_iter().filter(|&r| r > left) {
                let above = at(&rows, E | S, left).into_iter().chain(at(&rows, W | S, right));
                found.extend(above.filter(|&t| t < row).map(|t| (t, left, row, right)));
            }
        }
    }
    found
}

/// The near box on `rect`: at least three corners in place, every other
/// border cell in place or slipped, and at least one slipped.
fn near(grid: &Grid, strokes: &StrokeMap, rect: Rect) -> Option<BoxShape> {
    let Rect { top, left, bottom, right } = rect;
    let mut slips = Vec::new();
    let mut ports = Vec::new();

    // Clockwise from top-left: each corner, and the two ways out it can slip.
    let corners = [
        (Pos::new(top, left), E | S, [Dir::N, Dir::W]),
        (Pos::new(top, right), W | S, [Dir::N, Dir::E]),
        (Pos::new(bottom, right), N | W, [Dir::S, Dir::E]),
        (Pos::new(bottom, left), N | E, [Dir::S, Dir::W]),
    ];
    let mut round = [false; 4];
    for (i, (home, conn, outs)) in corners.into_iter().enumerate() {
        round[i] = match corner(strokes, home, conn) {
            Some(rounded) => rounded,
            None if !slips.is_empty() => return None,
            None => {
                let fits = |ch| corner_glyph(ch, conn);
                let (at, rounded) = slipped(grid, strokes, home, &outs, fits)?;
                slips.push(Slip { at, home });
                rounded
            }
        };
    }

    // The top edge may hold one title gap: a run of cells that are neither
    // border strokes nor the homes of slipped cells.
    let mut title_gap: Option<(usize, usize)> = None;
    for col in left + 1..right {
        let home = Pos::new(top, col);
        match edge(grid, strokes, home, Dir::E)? {
            Edge::Border(cell) => add_port(&mut ports, home, Dir::N, cell),
            Edge::Slip(at) => slips.push(Slip { at, home }),
            Edge::Empty => match &mut title_gap {
                Some((_, end)) if *end == col => *end += 1,
                None if col > left + 1 => title_gap = Some((col, col + 1)),
                _ => return None,
            },
        }
    }
    let rows = || top + 1..bottom;
    let sides: [(Dir, Dir, Vec<Pos>); 3] = [
        (Dir::E, Dir::S, rows().map(|row| Pos::new(row, right)).collect()),
        (Dir::S, Dir::E, (left + 1..right).map(|col| Pos::new(bottom, col)).collect()),
        (Dir::W, Dir::S, rows().map(|row| Pos::new(row, left)).collect()),
    ];
    for (side, axis, cells) in sides {
        for home in cells {
            match edge(grid, strokes, home, axis)? {
                Edge::Border(cell) => add_port(&mut ports, home, side, cell),
                Edge::Slip(at) => slips.push(Slip { at, home }),
                Edge::Empty => return None,
            }
        }
    }
    if slips.is_empty() {
        return None;
    }
    Some(finish(strokes, rect, title_gap, ports, round, slips))
}

/// How a near box's border cell at a given home is drawn.
enum Edge {
    /// In place, maybe as a port.
    Border(Option<(PortKind, Dir)>),
    /// Blank, with its cell slipped to the given position.
    Slip(Pos),
    /// Not a stroke, and nothing slipped from here.
    Empty,
}

/// Reads the border cell at `home`, on an edge running along `axis`.
/// `None` if a stroke there can't be part of the border.
fn edge(grid: &Grid, strokes: &StrokeMap, home: Pos, axis: Dir) -> Option<Edge> {
    let s = strokes.get(home);
    if s.is_some() {
        return border_cell(s, axis).map(Edge::Border);
    }
    let (line, across) = if axis.is_horizontal() {
        (E | W, [Dir::N, Dir::S])
    } else {
        (N | S, [Dir::E, Dir::W])
    };
    let fits = |ch| (stroke::fixed(ch) == Some(Stroke { conn: line, kind: Kind::Line })).then_some(());
    Some(match slipped(grid, strokes, home, &across, fits) {
        Some((at, ())) => Edge::Slip(at),
        None => Edge::Empty,
    })
}

/// The single cell beside blank `home`, in one of `dirs`, whose character
/// `fits` and which joins no other stroke. Two candidates are ambiguous.
fn slipped<T>(
    grid: &Grid,
    strokes: &StrokeMap,
    home: Pos,
    dirs: &[Dir],
    fits: impl Fn(char) -> Option<T>,
) -> Option<(Pos, T)> {
    if grid.get(home) != ' ' {
        return None;
    }
    let mut found = dirs
        .iter()
        .filter_map(|&d| home.step(d))
        .filter(|&q| joins_nothing(strokes, q))
        .filter_map(|q| fits(grid.get(q)).map(|t| (q, t)));
    let first = found.next()?;
    found.next().is_none().then_some(first)
}

/// True if `p` is a stroke that no neighbouring stroke joins.
fn joins_nothing(strokes: &StrokeMap, p: Pos) -> bool {
    let Some(s) = strokes.get(p) else { return false };
    Dir::ALL.into_iter().all(|d| {
        let back = p.step(d).and_then(|q| strokes.get(q)).is_some_and(|t| t.connects(d.opposite()));
        !(s.connects(d) && back)
    })
}

/// Whether `ch` can draw the corner with connections `conn`, and if so
/// whether it is rounded. An ASCII joint counts by the corners it draws.
fn corner_glyph(ch: char, conn: u8) -> Option<bool> {
    match (ch, stroke::fixed(ch)) {
        (_, Some(Stroke { conn: c, kind: Kind::Corner { rounded } })) if c == conn => Some(rounded),
        ('+', _) => Some(false),
        ('.', _) if conn & S != 0 => Some(true),
        ('\'', _) if conn & N != 0 => Some(true),
        _ => None,
    }
}

/// The divider on `row` between border columns `left` and `right`, if any:
/// inward tees at both ends, dashes between, and at most one gap that has
/// dashes on both sides.
fn divider(strokes: &StrokeMap, row: usize, left: usize, right: usize) -> Option<Divider> {
    let at = |c| strokes.get(Pos::new(row, c));
    let tee = |c, d| at(c).is_some_and(|s: Stroke| s.kind == Kind::Tee && s.connects(d));
    if !tee(left, Dir::E) || !tee(right, Dir::W) {
        return None;
    }
    let dashed = |c| at(c).is_some_and(|s: Stroke| s.kind == Kind::Dashed && s.runs(Dir::E));
    let interior = left + 1..right;
    let gap_start = interior.clone().find(|&c| !dashed(c));
    let gap = match gap_start {
        None => None,
        Some(start) => {
            let end = (start..right).find(|&c| at(c).is_some()).unwrap_or(right);
            let clean = start > left + 1
                && end < right
                && (start..end).all(|c| at(c).is_none())
                && (end..right).all(dashed);
            if !clean {
                return None;
            }
            Some((start, end))
        }
    };
    Some(Divider { row, gap })
}
