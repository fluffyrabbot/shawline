//! SPEC §11 rule 5: a wire that steps one cell aside and back is straightened.
//!
//! The pattern, for a horizontal wire dipping one row (and its rotations):
//!
//! ```text
//! ─┐   ┌─        ───────
//!  └───┘    →
//! ```

use shawline_grid::{Dir, Grid, Pos};
use shawline_shape::Diagram;
use shawline_shape::stroke::{Kind, Stroke};

use crate::canvas::Edit;

/// One edit per dip.
pub(crate) fn edits(grid: &Grid, diagram: &Diagram) -> Vec<Edit> {
    let mut edits = Vec::new();
    for w in &diagram.wires {
        for &a in &w.cells {
            for (fwd, side) in
                [(Dir::E, Dir::S), (Dir::E, Dir::N), (Dir::S, Dir::E), (Dir::S, Dir::W)]
            {
                edits.extend(dip(grid, diagram, a, fwd, side));
            }
        }
    }
    edits
}

/// The straightening of a dip that leaves the main line at `a`, heading
/// `fwd`, by stepping one cell to `side`.
fn dip(grid: &Grid, diagram: &Diagram, a: Pos, fwd: Dir, side: Dir) -> Option<Edit> {
    let back = fwd.opposite();
    let conn = |p: Pos| diagram.strokes.get(p).map(|s: Stroke| (s.kind, s.conn));
    let corner = |p: Pos, x: Dir, y: Dir| matches!(conn(p), Some((Kind::Corner { .. }, c)) if c == x.bit() | y.bit());
    if !corner(a, back, side) {
        return None;
    }
    let b = a.step(side)?;
    if !corner(b, side.opposite(), fwd) {
        return None;
    }
    let mut dipped = vec![b];
    let mut c = b.step(fwd)?;
    while matches!(conn(c), Some((Kind::Line, m)) if m == fwd.bit() | back.bit()) {
        dipped.push(c);
        c = c.step(fwd)?;
    }
    if !corner(c, back, side.opposite()) {
        return None;
    }
    dipped.push(c);
    let d = c.step(side.opposite())?;
    if !corner(d, side, fwd) {
        return None;
    }

    let line = if fwd.is_horizontal() { '─' } else { '│' };
    let mut edit: Edit = Vec::new();
    let mut p = a;
    while p != d {
        if p != a && grid.get(p) != ' ' {
            return None;
        }
        edit.push((p, line));
        p = p.step(fwd)?;
    }
    edit.push((d, line));
    edit.extend(dipped.into_iter().map(|p| (p, ' ')));
    Some(edit)
}

#[cfg(test)]
mod tests {
    use crate::format;
    use shawline_grid::Grid;

    fn fmt(src: &str) -> String {
        let grid = Grid::parse(src).unwrap();
        format(&grid, &shawline_shape::recognize(&grid))
    }

    #[test]
    fn horizontal_dip() {
        let src = "╭─ ·𐑨𐑛 ──────╮\n▶ 𐑯 ─┐  ┌────▶ 𐑚\n│    └──┘    │\n╰────────────╯\n";
        assert_eq!(fmt(src), "╭─ ·𐑨𐑛 ──────╮\n▶ 𐑯 ─────────▶ 𐑚\n│            │\n╰────────────╯\n");
    }

    #[test]
    fn vertical_jog() {
        let src = "╭─ ·𐑨𐑛 ──────╮\n▶ 𐑯 ─┐       │\n│    └┐      │\n│     │      │\n│    ┌┘      │\n│    │       │\n│    └───────▶ 𐑚\n╰────────────╯\n";
        let want = "╭─ ·𐑨𐑛 ──────╮\n▶ 𐑯 ─┐       │\n│    │       │\n│    │       │\n│    │       │\n│    │       │\n│    └───────▶ 𐑚\n╰────────────╯\n";
        assert_eq!(fmt(src), want);
    }

    #[test]
    fn dip_around_something_stays() {
        let src = "╭─ ·𐑨𐑛 ──────╮\n▶ 𐑯 ─┐ 1┌────▶ 𐑚\n│    └──┘    │\n╰────────────╯\n";
        assert_eq!(fmt(src), src);
    }
}
