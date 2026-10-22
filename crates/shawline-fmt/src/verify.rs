//! The safety net: every formatting step must leave the sheet's
//! [`Structure`] as it was, routing included, and its syntax diagnostics
//! too.
//!
//! The structure records what connects to what and ignores where things
//! sit, so a step may move text along its row, lengthen a wire or redraw a
//! stroke in its canonical character, but may not change which port, word or
//! arrowhead any wire end meets, a box's style or slipped cells, or the
//! syntax errors. Syntax sits downstream of shape, so its codes are compared
//! beside the structure rather than in it.

use shawline_grid::{Code, Grid};
use shawline_shape::Diagram;
use shawline_shape::structure::{Routing, Structure};

/// Syntax diagnostics by code, sorted.
pub(crate) fn syntax(grid: &Grid, diagram: &Diagram) -> Vec<Code> {
    let mut codes: Vec<Code> =
        shawline_syntax::parse(grid, diagram).1.iter().map(|d| d.code).collect();
    codes.sort_unstable();
    codes
}

/// Whether `diagram`, recognized from `grid`, has structure `want` and the
/// sorted syntax codes `syntax`.
pub(crate) fn holds(grid: &Grid, diagram: &Diagram, want: &Structure, syntax: &[Code]) -> bool {
    want.expect(&Structure::of(grid, diagram), Routing::Kept).is_ok()
        && self::syntax(grid, diagram) == syntax
}
