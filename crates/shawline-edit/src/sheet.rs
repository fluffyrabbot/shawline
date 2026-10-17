//! Facts about a recognized sheet that every edit needs: which cells are
//! box borders, where text sits, which blank cells a new wire must keep
//! clear of, and the canonical glyph for a set of connections.

use std::collections::{HashMap, HashSet};

use shawline_grid::{Dir, Grid, Pos, Rect};
use shawline_shape::stroke::{E, N, S, W};
use shawline_shape::{Diagram, EndKind};

/// Every cell of `r`'s border.
pub(crate) fn perimeter(r: Rect) -> impl Iterator<Item = Pos> {
    let sides =
        (r.top..=r.bottom).flat_map(move |row| [Pos::new(row, r.left), Pos::new(row, r.right)]);
    let ends =
        (r.left + 1..r.right).flat_map(move |col| [Pos::new(r.top, col), Pos::new(r.bottom, col)]);
    sides.chain(ends)
}

/// Border cells of every box, title gaps included, plus divider rows and
/// slipped cells: strokes that are never wire.
pub(crate) fn borders(diagram: &Diagram) -> HashSet<Pos> {
    let mut cells = HashSet::new();
    for b in &diagram.boxes {
        cells.extend(perimeter(b.rect));
        cells.extend(b.slips.iter().map(|s| s.at));
        for d in &b.dividers {
            cells.extend((b.rect.left..=b.rect.right).map(|col| Pos::new(d.row, col)));
        }
    }
    cells
}

/// The index of the text covering each cell.
pub(crate) fn text_cells(diagram: &Diagram) -> HashMap<Pos, usize> {
    let mut at = HashMap::new();
    for (i, t) in diagram.texts.iter().enumerate() {
        for col in t.pos.col..t.pos.col + t.width {
            at.insert(Pos::new(t.pos.row, col), i);
        }
    }
    at
}

/// The blank cells between `from` and the first non-blank cell in
/// horizontal direction `dir`, and that cell's text index if it is text
/// (SPEC §6: text meets wires across spaces on one row).
pub(crate) fn across(
    grid: &Grid,
    texts: &HashMap<Pos, usize>,
    from: Pos,
    dir: Dir,
) -> (Vec<Pos>, Option<usize>) {
    let mut gap = Vec::new();
    let len = grid.row(from.row).len();
    let mut p = from;
    while let Some(q) = p.step(dir).filter(|q| q.col < len) {
        if grid.get(q) != ' ' {
            return (gap, texts.get(&q).copied());
        }
        gap.push(q);
        p = q;
    }
    (gap, None)
}

/// Blank cells a new wire must keep clear of, each with the cell it
/// belongs to (its anchor), so a caller moving that anchor can move the
/// reservation with it:
///
/// - the cell each wire end faces, where a new stroke would block or join it;
/// - the cell outside each box port, where a wire meets (or may meet) it;
/// - the spaces between an arrowhead or open end and the text it meets.
///
/// Ends for which `skip` is true are left out.
pub(crate) fn keep_clear(
    grid: &Grid,
    diagram: &Diagram,
    skip: impl Fn(Pos, Dir) -> bool,
) -> Vec<(Pos, Pos)> {
    let texts = text_cells(diagram);
    let blank = |p: Pos| grid.get(p) == ' ';
    let mut out = Vec::new();
    let reach = |anchor: Pos, dir: Dir, out: &mut Vec<(Pos, Pos)>| {
        if dir.is_horizontal() {
            let (gap, text) = across(grid, &texts, anchor, dir);
            if text.is_some() {
                out.extend(gap.into_iter().map(|p| (anchor, p)));
            }
        }
    };
    for e in diagram.wires.iter().flat_map(|w| &w.ends) {
        if skip(e.pos, e.facing) {
            continue;
        }
        if let Some(f) = e.pos.step(e.facing).filter(|&f| blank(f)) {
            out.push((e.pos, f));
        }
        if matches!(e.kind, EndKind::Open | EndKind::Arrow) {
            reach(e.pos, e.facing, &mut out);
        }
    }
    for b in &diagram.boxes {
        for p in &b.ports {
            if let Some(f) = p.pos.step(p.side).filter(|&f| blank(f)) {
                out.push((p.pos, f));
            }
            if p.kind == shawline_shape::PortKind::Arrow {
                reach(p.pos, p.dir, &mut out);
            }
        }
    }
    out
}

/// The canonical character drawing exactly connections `mask`. Corners are
/// square, as the corpus draws wires. A lone direction draws its line.
pub(crate) fn glyph(mask: u8) -> char {
    match mask {
        0 => ' ',
        m if m == E | W || m == E || m == W => '─',
        m if m == N | S || m == N || m == S => '│',
        m if m == E | S => '┌',
        m if m == W | S => '┐',
        m if m == N | E => '└',
        m if m == N | W => '┘',
        m if m == N | S | E => '├',
        m if m == N | S | W => '┤',
        m if m == E | W | S => '┬',
        m if m == E | W | N => '┴',
        _ => '┼',
    }
}

/// True if `mask` turns a corner.
pub(crate) fn is_corner(mask: u8) -> bool {
    mask.count_ones() == 2 && mask != E | W && mask != N | S
}
