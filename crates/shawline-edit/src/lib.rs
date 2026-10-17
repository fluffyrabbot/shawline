//! Grid edits for Shawline Studio: every gesture becomes one checked edit to
//! the sheet's text (PLAN P6; the design is in the Shawline Studio doc).
//!
//! # Contract
//!
//! The signatures and the semantics written on them are lead-owned and frozen
//! for the P6 spike. The implementation behind them is agent E's: add modules
//! freely, but don't change what these functions take, return or promise.
//!
//! Every function here takes the sheet as text and returns new text or a
//! [`Refusal`]. The text is the only state (Studio principle 1), so undo,
//! sharing and the CLI need nothing new.
//!
//! An edit is **checked**: before returning `Ok`, the result is recognized
//! again and compared with the input. It must have the same boxes (apart from
//! the one deliberately moved), the same wire connections (which ports each
//! wire joins), the same text adjacency (port names, constants, inline nodes)
//! and the same diagnostic codes. If not, the edit is refused and the sheet is
//! unchanged.

use shawline_grid::Pos;

/// Why an edit was refused. The sheet is unchanged.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refusal {
    /// The cell the refusal is about, for the UI to point at.
    pub at: Pos,
    /// A sentence the UI can show as is, such as "no free path for the wire
    /// from 𐑐; insert a column?".
    pub reason: String,
}

/// Moves the box whose top-left corner is at `corner` by `dr` rows and `dc`
/// columns, with everything inside it.
///
/// - **Wires follow.** Each wire attached to one of the box's ports keeps its
///   far end and its author-drawn segments. Only the stretch that must change
///   is re-routed with [`route`].
/// - **Nothing else moves.** Other boxes, wires and text stay where they are.
///   Studio principle 6: authorship stays with the author.
/// - **The result is canonical.** Formatting it changes nothing, so a moved
///   drawing never needs tidying.
/// - **Refusals:** no box has its corner at `corner`, the box would overlap
///   another box or leave its parent, or a wire can't be re-routed within the
///   search budget.
pub fn move_box(src: &str, corner: Pos, dr: isize, dc: isize) -> Result<String, Refusal> {
    let _ = (src, corner, dr, dc);
    todo!("P6: agent E")
}

/// The cheapest orthogonal path for a wire from cell `from` to cell `to`,
/// through cells that are blank in `src` (both ends included in the path).
///
/// Cost follows the formatter's aesthetics (SPEC §11–§12): each cell costs 1,
/// each bend costs more, each crossing of an existing wire much more, and a
/// cell beside a box border or another wire costs a little extra (clearance).
/// The search is best-first with a fixed budget of expanded cells. `None`
/// when no path exists or the budget runs out, so a caller can't hang.
pub fn route(src: &str, from: Pos, to: Pos) -> Option<Vec<Pos>> {
    let _ = (src, from, to);
    todo!("P6: agent E")
}
