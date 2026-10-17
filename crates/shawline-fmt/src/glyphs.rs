//! SPEC §11 rules 1–2: canonical stroke characters and corner styles.

use std::collections::HashSet;

use shawline_grid::{Dir, Grid, Pos};
use shawline_shape::stroke::{self, E, Kind, N, S, Stroke, W};
use shawline_shape::Diagram;

use crate::canvas::Edit;

/// One edit per box (border and dividers), one per wire, and one per stray
/// stroke, so the safety net can reject them independently.
pub(crate) fn edits(grid: &Grid, diagram: &Diagram) -> Vec<Edit> {
    let glyph = |p: Pos| -> Option<(Pos, char)> {
        let s = diagram.strokes.get(p)?;
        let ch = canonical(s, own_rounding(s))?;
        (ch != grid.get(p)).then_some((p, ch))
    };

    let mut grouped = HashSet::new();
    let mut edits = Vec::new();
    for b in &diagram.boxes {
        let r = b.rect;
        let mut cells: Vec<Pos> = (r.left..=r.right)
            .flat_map(|c| [Pos::new(r.top, c), Pos::new(r.bottom, c)])
            .chain(
                (r.top + 1..r.bottom)
                    .flat_map(|row| [Pos::new(row, r.left), Pos::new(row, r.right)]),
            )
            .collect();
        for d in &b.dividers {
            cells.extend((r.left..=r.right).map(|c| Pos::new(d.row, c)));
        }
        grouped.extend(cells.iter().copied());
        edits.push(cells.into_iter().filter_map(glyph).collect());
    }
    for w in &diagram.wires {
        grouped.extend(w.cells.iter().copied());
        edits.push(w.cells.iter().copied().filter_map(glyph).collect());
    }
    for row in 0..grid.height() {
        for col in 0..grid.row(row).len() {
            let p = Pos::new(row, col);
            if !grouped.contains(&p) {
                edits.extend(glyph(p).map(|e| vec![e]));
            }
        }
    }
    edits.retain(|e: &Edit| !e.is_empty());
    edits
}

/// Whether a corner stays rounded: a canonical corner keeps its own style,
/// and an ASCII one takes the style it draws (`.` and `'` rounded, `+`
/// square). So every box keeps its style, wrong or not: rounding a
/// top-level square box (`Y018`), squaring a nested rounded one (`Y014`) or
/// settling a mixed one (`Y015`) would turn an error into a valid program,
/// and a drawing that isn't a box yet keeps the style it is heading for.
fn own_rounding(s: Stroke) -> bool {
    matches!(s.kind, Kind::Corner { rounded: true })
}

/// The canonical character for a resolved stroke, if one draws exactly the
/// same connections, with corners `rounded` or square.
pub(crate) fn canonical(s: Stroke, rounded: bool) -> Option<char> {
    let pick = |square, round| if rounded { round } else { square };
    let ch = match s.kind {
        Kind::Arrow(Dir::E) => '▶',
        Kind::Arrow(Dir::W) => '◀',
        Kind::Arrow(Dir::N) => '▲',
        Kind::Arrow(Dir::S) => '▼',
        Kind::Crossing => '┼',
        Kind::Line if s.conn == E | W => '─',
        Kind::Line if s.conn == N | S => '│',
        Kind::Dashed if s.conn == E | W => '┄',
        Kind::Dashed if s.conn == N | S => '┆',
        Kind::Corner { .. } if s.conn == E | S => pick('┌', '╭'),
        Kind::Corner { .. } if s.conn == W | S => pick('┐', '╮'),
        Kind::Corner { .. } if s.conn == N | E => pick('└', '╰'),
        Kind::Corner { .. } if s.conn == N | W => pick('┘', '╯'),
        Kind::Tee if s.conn == N | S | E => '├',
        Kind::Tee if s.conn == N | S | W => '┤',
        Kind::Tee if s.conn == E | W | S => '┬',
        Kind::Tee if s.conn == E | W | N => '┴',
        _ => return None,
    };
    // A joint resolved to a dead end has no canonical twin; leave it be.
    let same =
        stroke::fixed(ch).is_some_and(|f| f.conn == s.conn || matches!(s.kind, Kind::Arrow(_)));
    same.then_some(ch)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format;

    fn fmt(src: &str) -> String {
        let grid = Grid::parse(src).unwrap();
        format(&grid, &shawline_shape::recognize(&grid))
    }

    #[test]
    fn ascii_node_becomes_square() {
        let src = ".- ·𐑨𐑛 --------.\n|     +---+    |\n> 𐑩 --> 𐑐 +----> 𐑚\n|     +---+    |\n'--------------'\n";
        let want = "╭─ ·𐑨𐑛 ────────╮\n│     ┌───┐    │\n▶ 𐑩 ──▶ 𐑐 ├────▶ 𐑚\n│     └───┘    │\n╰──────────────╯\n";
        assert_eq!(fmt(src), want);
    }

    #[test]
    fn square_top_level_box_keeps_its_corners() {
        // `Y018`: rounding it would turn an error into a valid program.
        let src = "┌─ ·𐑨𐑛 ──┐\n│ 1 ─────▶ 𐑚\n└────────┘\n";
        assert_eq!(fmt(src), src);
    }

    #[test]
    fn mixed_box_keeps_its_corners() {
        let src = "╭─ ·𐑨𐑛 ──┐\n│ 1 ─────▶ 𐑚\n╰────────╯\n";
        assert_eq!(fmt(src), src);
    }

    #[test]
    fn wire_corners_keep_their_roundedness() {
        let src = "╭─ ·𐑨𐑛 ────╮\n│ 1 ─╮     │\n│    ╰─────▶ 𐑚\n╰──────────╯\n";
        assert_eq!(fmt(src), src);
        let ascii = src.replace('╮', "+").replace('╰', "'");
        assert_eq!(fmt(&ascii), src.replace('╮', "┐"));
    }

    #[test]
    fn unclosed_rounded_box_stays_rounded() {
        // From the formatter audit (/tmp/fmtfuzz/r1.shaw): a right edge
        // missing a cell left the drawing boxless, and its corners squared.
        let src = "╭─ ·𐑛𐑳𐑚𐑩𐑤 ──────╮\n│               │\n▶ 𐑯 ────────────▶ 𐑬\n│\n╰───────────────╯\n";
        assert_eq!(fmt(src), src);
    }

    #[test]
    fn dead_end_joint_is_left_alone() {
        assert_eq!(canonical(Stroke { conn: E, kind: Kind::Line }, false), None);
    }
}
