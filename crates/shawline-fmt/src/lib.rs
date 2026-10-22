//! The canonical formatter and aesthetic lints (SPEC §11–12).
//!
//! The formatter keeps the author's placement: every box and wire stays
//! where it was drawn, and only local style is normalized. It works on the
//! [`Grid`] and its recognized [`Diagram`], in phases. Each phase proposes
//! independent edits; an edit survives only if re-recognizing the result
//! leaves the program's structure (what connects to what) unchanged, so
//! formatting can never change meaning.
//!
//! Rule 6: [`format`] never moves a near box's slipped border cell, because
//! moving one turns an error into a valid program. [`fix`] does that repair
//! first, under the same safety net, and then formats.

mod canvas;
mod glyphs;
mod jitter;
mod layout;
mod lint;
mod spacing;
mod verify;

use shawline_grid::{Code, Diagnostic, Grid};
use shawline_shape::Diagram;
use shawline_shape::structure::Structure;

use canvas::{Canvas, Edit, reparse};

/// The limits behind the lints of SPEC §12, in one place so they can be
/// tuned against the corpus.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Thresholds {
    /// `L003`: most bends a wire may make between its source and a sink.
    pub max_bends: usize,
    /// `L004`: most wire crossings in one definition.
    pub max_crossings: usize,
    /// `L005`: most non-space cells in a definition's interior, in percent.
    pub max_ink_percent: usize,
    /// `L006`: most difference between a node word's left and right padding.
    /// One cell is unavoidable when the spare width is odd.
    pub max_padding_skew: usize,
}

/// The lint thresholds in force.
pub const THRESHOLDS: Thresholds =
    Thresholds { max_bends: 4, max_crossings: 2, max_ink_percent: 60, max_padding_skew: 1 };

/// A formatting phase: proposes edits against the canvas as it stands.
type Phase = fn(&Grid, &Diagram) -> Vec<Edit>;

/// SPEC §11 rules 1–5. Straightening runs before spacing so that spacing
/// sees the final wires. Rule 7 is applied while rendering.
const PHASES: [Phase; 4] = [glyphs::edits, jitter::edits, spacing::titles, spacing::ports];

/// Most full passes [`format`] makes looking for a fixpoint. One fix can
/// make room for another, but each pass only tidies, so a few suffice.
const MAX_PASSES: usize = 8;

/// Formats a sheet into canonical text (SPEC §11).
///
/// `diagram` must be `shawline_shape::recognize(grid)`. The result is
/// idempotent and recognizes to the same structure as the input; any edit
/// that would change the structure is dropped.
pub fn format(grid: &Grid, diagram: &Diagram) -> String {
    let mut out = format_once(grid, diagram);
    for _ in 1..MAX_PASSES {
        let (g, d) = reparse(&out);
        let next = format_once(&g, &d);
        if next == out {
            break;
        }
        out = next;
    }
    out
}

/// Formats a sheet like [`format`], first moving each near box's slipped
/// border cells into place (SPEC §3, §11 rule 6: `shawline fmt --fix`).
///
/// A box is repaired only if it then recognizes as the same box with no
/// slips and nothing else about the sheet changes.
pub fn fix(grid: &Grid, diagram: &Diagram) -> String {
    let (g, d) = repair(grid, diagram).recognize();
    format(&g, &d)
}

/// Reports the aesthetic lints `L001`–`L006` (SPEC §12), sorted by position.
pub fn lint(grid: &Grid, diagram: &Diagram) -> Vec<Diagnostic> {
    lint::lint(grid, diagram)
}

/// One pass of every phase, then trailing spaces and blank lines.
fn format_once(grid: &Grid, diagram: &Diagram) -> String {
    let base = Structure::of(grid, diagram);
    let syntax = verify::syntax(grid, diagram);
    let mut canvas = Canvas::from_grid(grid);
    for phase in PHASES {
        let (g, d) = canvas.recognize();
        let edits = phase(&g, &d);
        apply(&mut canvas, &edits, &base, &syntax);
    }

    let rows = layout::trimmed(&canvas);
    let flat = canvas::render_rows(rows.clone());
    let (_, d) = reparse(&flat);
    let same = |out: &str| {
        let (g, d) = reparse(out);
        verify::holds(&g, &d, &base, &syntax)
    };
    // Each blank-line change stands alone, so an inserted blank that would
    // split a wire doesn't block the rest. Drops go first, as one batch
    // when they can: they never bring strokes together, since one blank
    // row always stays between two items.
    let ok = |ops: &[layout::Op]| same(&layout::render(&rows, ops));
    let (drops, inserts): (Vec<_>, Vec<_>) =
        layout::ops(&rows, &d).into_iter().partition(|op| matches!(op, layout::Op::Drop(_)));
    let kept = keep_safe(Vec::new(), &drops, ok);
    let kept = keep_safe(kept, &inserts, ok);
    let out = layout::render(&rows, &kept);
    if same(&out) {
        return out;
    }
    // Unreachable while every step is checked, but the input is always safe.
    Canvas::from_grid(grid).render()
}

/// `kept` plus those of `items` that `ok` accepts on top of it: all of them
/// at once when possible, otherwise each in turn, greedily.
fn keep_safe<T: Copy>(mut kept: Vec<T>, items: &[T], ok: impl Fn(&[T]) -> bool) -> Vec<T> {
    if items.is_empty() {
        return kept;
    }
    let all: Vec<T> = kept.iter().chain(items).copied().collect();
    if ok(&all) {
        return all;
    }
    for &item in items {
        kept.push(item);
        if !ok(&kept) {
            kept.pop();
        }
    }
    kept
}

/// Applies `edits`, keeping only those that leave the structure as `base`
/// and the syntax codes as `syntax`. All at once is the common case; edit
/// by edit is the fallback.
fn apply(canvas: &mut Canvas, edits: &[Edit], base: &Structure, syntax: &[Code]) {
    if edits.is_empty() {
        return;
    }
    let before = canvas.clone();
    for e in edits {
        canvas.write(e);
    }
    if unchanged(canvas, base, syntax) {
        return;
    }
    *canvas = before;
    for e in edits {
        let undo: Edit = e.iter().map(|&(p, _)| (p, canvas.get(p))).collect();
        canvas.write(e);
        if !unchanged(canvas, base, syntax) {
            canvas.write(&undo);
        }
    }
}

fn unchanged(canvas: &Canvas, base: &Structure, syntax: &[Code]) -> bool {
    let (g, d) = canvas.recognize();
    verify::holds(&g, &d, base, syntax)
}

/// Moves each near box's slipped cells home, one box at a time, keeping a
/// move only if that box then has no slips and nothing else changed.
fn repair(grid: &Grid, diagram: &Diagram) -> Canvas {
    let mut canvas = Canvas::from_grid(grid);
    let mut want = Structure::of(grid, diagram);
    let syntax = verify::syntax(grid, diagram);
    for (i, b) in diagram.boxes.iter().enumerate() {
        if b.slips.is_empty() {
            continue;
        }
        let undo: Edit =
            b.slips.iter().flat_map(|s| [s.at, s.home]).map(|p| (p, canvas.get(p))).collect();
        let edit: Edit =
            b.slips.iter().flat_map(|s| [(s.at, ' '), (s.home, canvas.get(s.at))]).collect();
        canvas.write(&edit);
        let (g, d) = canvas.recognize();
        let whole = d.boxes.get(i).is_some_and(|nb| nb.rect == b.rect && nb.slips.is_empty());
        let mut target = want.clone();
        target.boxes[i].slips = 0;
        if whole && verify::holds(&g, &d, &target, &syntax) {
            want = target;
        } else {
            canvas.write(&undo);
        }
    }
    canvas
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fmt(src: &str) -> String {
        let grid = Grid::parse(src).unwrap();
        format(&grid, &shawline_shape::recognize(&grid))
    }

    fn fixed(src: &str) -> String {
        let grid = Grid::parse(src).unwrap();
        fix(&grid, &shawline_shape::recognize(&grid))
    }

    #[test]
    fn safety_net_rejects_meaning_changes() {
        // Straightening the dip would put a line under the dangling wire
        // above it, turning its open end into a blocked one.
        let src = "╭─ ·𐑨𐑛 ───────╮\n│     │       │\n▶ 𐑯 ─┐ ┌──────▶ 𐑚\n│    └─┘      │\n╰─────────────╯\n";
        assert_eq!(fmt(src), src);
        let clear = src.replace("│     │       │", "│             │");
        assert!(fmt(&clear).contains("▶ 𐑯 ──────────▶ 𐑚"), "{}", fmt(&clear));
    }

    #[test]
    fn empty_sheet() {
        assert_eq!(fmt(""), "");
        assert_eq!(fmt("\n\n"), "");
    }

    const WHOLE: &str = "╭─ ·𐑨𐑛 ──╮\n│ 1 ─────▶ 𐑚\n│        │\n╰────────╯\n";

    #[test]
    fn fix_moves_slipped_cells_and_format_does_not() {
        let edge = WHOLE.replace("│        │", "│         │");
        let corner = WHOLE.replace("╰────────╯", "╰────────\n         ╯");
        for src in [&edge, &corner] {
            assert_eq!(fmt(src), *src, "format leaves slips alone");
            assert_eq!(fixed(src), WHOLE, "{src}");
        }
        let ascii = edge.replace("│\n╰", "|\n╰");
        assert_eq!(fmt(&ascii), edge, "redrawn in place");
        assert_eq!(fixed(&ascii), WHOLE);
    }

    #[test]
    fn fix_leaves_cells_two_out() {
        let far = WHOLE.replace("│        │", "│          │");
        assert_eq!(fixed(&far), fmt(&far));
        assert!(fixed(&far).contains("│          │"));
    }

    #[test]
    fn fix_formats_too() {
        let rough = WHOLE.replace("│        │", "│         │").replace("│ 1 ─", "│ 1  ");
        assert_eq!(fixed(&rough), WHOLE);
    }
}
