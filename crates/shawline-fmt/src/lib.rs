//! The canonical formatter and aesthetic lints (SPEC §11–12).
//!
//! The formatter keeps the author's placement: every box and wire stays
//! where it was drawn, and only local style is normalized. It works on the
//! [`Grid`] and its recognized [`Diagram`], in phases. Each phase proposes
//! independent edits; an edit survives only if re-recognizing the result
//! leaves the program's structure (what connects to what) unchanged, so
//! formatting can never change meaning.
//!
//! Rule 6 (edge snapping) is not applied. Recognition only finds exact
//! rectangles, so a ragged edge is not a box yet, and snapping it would
//! change the structure the safety net protects.

mod canvas;
mod glyphs;
mod jitter;
mod layout;
mod lint;
mod spacing;
mod verify;

use shawline_grid::{Diagnostic, Grid};
use shawline_shape::Diagram;

use canvas::{Canvas, Edit, reparse};
use verify::Fingerprint;

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

/// Formats a sheet into canonical text (SPEC §11).
///
/// `diagram` must be `shawline_shape::recognize(grid)`. The result is
/// idempotent and recognizes to the same structure as the input; any edit
/// that would change the structure is dropped.
pub fn format(grid: &Grid, diagram: &Diagram) -> String {
    let base = Fingerprint::of(grid, diagram);
    let mut canvas = Canvas::from_grid(grid);
    for phase in PHASES {
        let (g, d) = canvas.recognize();
        let edits = phase(&g, &d);
        apply(&mut canvas, &edits, &base);
    }

    let rows = layout::trimmed(&canvas);
    let flat = canvas::render_rows(rows.clone());
    let (_, d) = reparse(&flat);
    let spaced = layout::spaced(&rows, &d);
    for out in [spaced, flat] {
        let (g, d) = reparse(&out);
        if Fingerprint::of(&g, &d) == base {
            return out;
        }
    }
    // Unreachable while every phase is checked, but the input is always safe.
    Canvas::from_grid(grid).render()
}

/// Reports the aesthetic lints `L001`–`L006` (SPEC §12), sorted by position.
pub fn lint(grid: &Grid, diagram: &Diagram) -> Vec<Diagnostic> {
    lint::lint(grid, diagram)
}

/// Applies `edits`, keeping only those that leave the structure as `base`.
/// All at once is the common case; edit by edit is the fallback.
fn apply(canvas: &mut Canvas, edits: &[Edit], base: &Fingerprint) {
    if edits.is_empty() {
        return;
    }
    let before = canvas.clone();
    for e in edits {
        canvas.write(e);
    }
    if unchanged(canvas, base) {
        return;
    }
    *canvas = before;
    for e in edits {
        let undo: Edit = e.iter().map(|&(p, _)| (p, canvas.get(p))).collect();
        canvas.write(e);
        if !unchanged(canvas, base) {
            canvas.write(&undo);
        }
    }
}

fn unchanged(canvas: &Canvas, base: &Fingerprint) -> bool {
    let (g, d) = canvas.recognize();
    Fingerprint::of(&g, &d) == *base
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fmt(src: &str) -> String {
        let grid = Grid::parse(src).unwrap();
        format(&grid, &shawline_shape::recognize(&grid))
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
}
