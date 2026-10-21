//! Box recognition: closed rectangles of strokes, and near boxes whose
//! border has cells slipped one cell off their line (SPEC §3).

use std::collections::BTreeSet;

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

/// The rectangles [`near`] might accept: at least three corners in place,
/// with nothing between them that a border can't hold. Any three corners
/// include both top corners or both bottom ones. A corner can't sit inside a
/// border, so the second corner of a pair is the first such stroke past the
/// first along its row, and the third is the first below or above either.
///
/// Each corner starts at most one walk each way, and walks from corners of
/// one shape going one way stop by the next such corner, so they never
/// overlap: finding candidates is linear in the grid. Pairing every
/// combination of corners instead grows with the fourth power of a grid
/// dense with them.
fn near_candidates(grid: &Grid, strokes: &StrokeMap) -> BTreeSet<RectKey> {
    // The first cell past `p` going `d` that a border along `d` can't hold,
    // if it is a corner with connections `conn`.
    let stop = |p: Pos, d: Dir, conn: u8| -> Option<Pos> {
        let mut q = p.step(d)?;
        while q.row < grid.height() && q.col < grid.width() {
            if strokes.get(q).is_some_and(|s| border_cell(Some(s), d).is_none()) {
                return corner(strokes, q, conn).map(|_| q);
            }
            q = q.step(d)?;
        }
        None
    };

    let mut found = BTreeSet::new();
    for row in 0..grid.height() {
        for col in 0..grid.row(row).len() {
            let p = Pos::new(row, col);
            // Both top corners in place; the bottom from either.
            if corner(strokes, p, E | S).is_some()
                && let Some(tr) = stop(p, Dir::E, W | S)
            {
                let below = [stop(p, Dir::S, N | E), stop(tr, Dir::S, N | W)];
                found.extend(below.into_iter().flatten().map(|b| (row, col, b.row, tr.col)));
            }
            // Both bottom corners in place; the top from either.
            if corner(strokes, p, N | E).is_some()
                && let Some(br) = stop(p, Dir::E, N | W)
            {
                let above = [stop(p, Dir::N, E | S), stop(br, Dir::N, W | S)];
                found.extend(above.into_iter().flatten().map(|t| (t.row, col, row, br.col)));
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
    if grid.get(home) != ' ' || strokes.is_masked(home) {
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

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use shawline_grid::{Grid, Pos, Rect};

    use super::{E, N, RectKey, S, StrokeMap, W, corner, key, near, near_candidates, trace};

    /// Every rectangle with three corners in place, by taking every
    /// combination of them: what `near_candidates` prunes.
    fn every_combination(grid: &Grid, strokes: &StrokeMap) -> BTreeSet<RectKey> {
        let mut at: BTreeMap<u8, Vec<Pos>> = BTreeMap::new();
        for row in 0..grid.height() {
            for col in 0..grid.row(row).len() {
                for shape in [E | S, W | S, N | W, N | E] {
                    if corner(strokes, Pos::new(row, col), shape).is_some() {
                        at.entry(shape).or_default().push(Pos::new(row, col));
                    }
                }
            }
        }
        let all = |shape| at.get(&shape).cloned().unwrap_or_default();
        let mut found = BTreeSet::new();
        for tl in all(E | S) {
            for tr in all(W | S).into_iter().filter(|q| q.row == tl.row && q.col > tl.col) {
                for b in all(N | E).into_iter().filter(|q| q.col == tl.col && q.row > tl.row) {
                    found.insert((tl.row, tl.col, b.row, tr.col));
                }
                for b in all(N | W).into_iter().filter(|q| q.col == tr.col && q.row > tl.row) {
                    found.insert((tl.row, tl.col, b.row, tr.col));
                }
            }
        }
        for bl in all(N | E) {
            for br in all(N | W).into_iter().filter(|q| q.row == bl.row && q.col > bl.col) {
                for t in all(E | S).into_iter().filter(|q| q.col == bl.col && q.row < bl.row) {
                    found.insert((t.row, bl.col, bl.row, br.col));
                }
                for t in all(W | S).into_iter().filter(|q| q.col == br.col && q.row < bl.row) {
                    found.insert((t.row, bl.col, bl.row, br.col));
                }
            }
        }
        found
    }

    /// The near boxes `near` accepts among `candidates`.
    fn accepted(grid: &Grid, strokes: &StrokeMap, candidates: &BTreeSet<RectKey>) -> Vec<RectKey> {
        let exact: BTreeSet<RectKey> = (0..grid.height())
            .flat_map(|row| (0..grid.row(row).len()).map(move |col| Pos::new(row, col)))
            .filter_map(|p| trace(grid, strokes, p))
            .map(|b| key(b.rect))
            .collect();
        candidates
            .iter()
            .filter(|k| !exact.contains(k))
            .filter_map(|&(top, left, bottom, right)| {
                near(grid, strokes, Rect { top, left, bottom, right }).map(|b| key(b.rect))
            })
            .collect()
    }

    /// SplitMix64, so the grids are the same on every run.
    struct Rng(u64);

    impl Rng {
        fn below(&mut self, n: usize) -> usize {
            self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
            let mut z = self.0;
            z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
            ((z ^ (z >> 31)) % n as u64) as usize
        }
    }

    /// A few overlapping boxes, some border cells slipped one cell out, and
    /// some stray strokes.
    fn jumble(rng: &mut Rng) -> String {
        const ROWS: usize = 9;
        const COLS: usize = 14;
        let mut cells = [[' '; COLS]; ROWS];
        for _ in 0..1 + rng.below(4) {
            let (top, left) = (rng.below(ROWS - 2), rng.below(COLS - 2));
            let bottom = top + 2 + rng.below(ROWS - top - 2);
            let right = left + 2 + rng.below(COLS - left - 2);
            let round = rng.below(2) == 0;
            let (tl, tr, br, bl) = if round { ('╭', '╮', '╯', '╰') } else { ('┌', '┐', '┘', '└') };
            cells[top][left + 1..right].fill('─');
            cells[bottom][left + 1..right].fill('─');
            for row in &mut cells[top + 1..bottom] {
                row[left] = '│';
                row[right] = '│';
            }
            cells[top][left] = tl;
            cells[top][right] = tr;
            cells[bottom][right] = br;
            cells[bottom][left] = bl;
        }
        for _ in 0..1 + rng.below(3) {
            let (row, col) = (1 + rng.below(ROWS - 2), 1 + rng.below(COLS - 2));
            let ch = cells[row][col];
            if ch != ' ' {
                let (r, c) = [(row - 1, col), (row + 1, col), (row, col - 1), (row, col + 1)][rng.below(4)];
                cells[row][col] = ' ';
                cells[r][c] = ch;
            }
        }
        for _ in 0..rng.below(3) {
            let strays = ['┌', '┐', '└', '┘', '─', '│', '┬', '▶', '+'];
            cells[rng.below(ROWS)][rng.below(COLS)] = strays[rng.below(strays.len())];
        }
        cells.iter().map(|row| row.iter().collect::<String>() + "\n").collect()
    }

    #[test]
    fn pruned_candidates_lose_no_near_box() {
        let mut rng = Rng(7);
        let (mut near_boxes, mut pruned_away) = (0, 0);
        for _ in 0..4000 {
            let src = jumble(&mut rng);
            let grid = Grid::parse(&src).unwrap();
            let strokes = StrokeMap::new(&grid);
            let pruned = near_candidates(&grid, &strokes);
            let all = every_combination(&grid, &strokes);
            assert!(pruned.is_subset(&all));
            let want = accepted(&grid, &strokes, &all);
            assert_eq!(accepted(&grid, &strokes, &pruned), want, "\n{src}");
            near_boxes += want.len();
            pruned_away += all.len() - pruned.len();
        }
        // So the jumbles exercise both sides of the pruning.
        assert!(near_boxes > 300 && pruned_away > 3000, "{near_boxes} {pruned_away}");
    }

    /// Rows of `┌┐┌┐…` over rows of `└┘└┘…` hold a quarter of a million
    /// three-corner rectangles at this size, and a grid twice as wide and
    /// tall holds sixteen times as many.
    #[test]
    fn corner_dense_grid_has_few_candidates() {
        let n = 64;
        let src: String = (0..n)
            .map(|row| if row % 2 == 0 { "┌┐" } else { "└┘" }.repeat(n / 2) + "\n")
            .collect();
        let grid = Grid::parse(&src).unwrap();
        let strokes = StrokeMap::new(&grid);
        assert!(near_candidates(&grid, &strokes).len() <= n * n);
    }
}
