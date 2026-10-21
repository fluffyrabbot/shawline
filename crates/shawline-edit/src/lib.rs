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

mod canon;
mod check;
mod model;
mod mover;
mod router;
mod sheet;
mod wiring;

use std::collections::HashSet;

use shawline_grid::{Dir, Grid, Pos};
use shawline_shape::{Diagram, recognize};

use router::{BUDGET, CELL, CLEARANCE, Cell, Start, Terrain};

/// Why an edit was refused. The sheet is unchanged.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refusal {
    /// The cell the refusal is about, for the UI to point at.
    pub at: Pos,
    /// A sentence the UI can show as is, such as "no free path for the wire
    /// from 𐑐; insert a column?".
    pub reason: String,
}

impl Refusal {
    pub(crate) fn new(at: Pos, reason: impl Into<String>) -> Self {
        Self { at, reason: reason.into() }
    }
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
    mover::move_box(src, corner, dr, dc)
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
    let grid = Grid::parse(src).ok()?;
    let diagram = recognize(&grid);
    let sheet = Plain::new(&grid, &diagram, from, to);
    if sheet.cell(from) != Cell::Free || sheet.cell(to) != Cell::Free {
        return None;
    }
    if from == to {
        return Some(vec![from]);
    }
    let start = Start { pos: from, heading: None, straight: false };
    router::search(&sheet, start, &mut { BUDGET }).ok().map(|found| found.path)
}

// ---------------------------------------------------------------------------
// P7 contract: Studio Phase 1. Lead-owned and frozen; agent W implements.
//
// Endpoints are named by cell. A *source* is a node's output tee, a
// definition input's arrowhead, a constant's first cell, or an arm's left
// through-port. A *sink* is an arrowhead: a node input, a definition output,
// a switch selector or through-port, an arm's right through-port. Wires that
// a switch's merged outputs feed, and wires touching an inline node, aren't
// editable in Phase 1: edits to them are refused with that reason.
// ---------------------------------------------------------------------------

/// A callee the context palette offers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    /// The word to write in the node box.
    pub word: String,
    pub inputs: u16,
    pub outputs: u16,
    /// What it does in plain English: the SPEC §9 meaning for a primitive
    /// ("add"), the port names for a definition ("𐑯 → 𐑬").
    pub gloss: String,
    /// A primitive's mirrored partner, as its word.
    pub mirror: Option<String>,
}

/// Connects source `from` to sink `to`.
///
/// When the source has no wire yet, a new one is routed. When it has one, the
/// new branch joins that wire with a tee. The sheet's edge set becomes the
/// old one plus this pair, and nothing else changes. The result is canonical.
/// Refused when `to` already has a wire, the cells aren't a source and a sink,
/// or no route fits within the router's budget.
pub fn connect(src: &str, from: Pos, to: Pos) -> Result<String, Refusal> {
    wiring::connect(src, from, to)
}

/// Removes the whole wire that passes through cell `at`, with every branch.
/// Refused when no editable wire passes through `at`.
pub fn remove_wire(src: &str, at: Pos) -> Result<String, Refusal> {
    wiring::remove_wire(src, at)
}

/// Removes only the branch of a wire that reaches sink `to`, back to the tee
/// it leaves from. A tee left with two arms becomes a plain stroke. When the
/// wire has no other sink, the whole wire goes, as with [`remove_wire`].
pub fn disconnect(src: &str, to: Pos) -> Result<String, Refusal> {
    wiring::disconnect(src, to)
}

/// Places a canonical node box for `word` with its top-left corner at
/// `corner`: `inputs` arrowheads on its left border, `outputs` tees on its
/// right, and no wires. Refused when the box would overlap anything, or not
/// lie wholly inside one definition (or one arm of a switch).
pub fn place_node(
    src: &str,
    corner: Pos,
    word: &str,
    inputs: u16,
    outputs: u16,
) -> Result<String, Refusal> {
    wiring::place_node(src, corner, word, inputs, outputs)
}

/// Removes the node box whose top-left corner is `corner`. Every wire it
/// sources goes too, and only the branches that reach its inputs
/// ([`disconnect`]).
pub fn remove_node(src: &str, corner: Pos) -> Result<String, Refusal> {
    wiring::remove_node(src, corner)
}

/// What the context palette offers, in a fixed order: primitives first in
/// SPEC §9 table order (with `𐑯𐑦𐑤`), then the sheet's definitions in sheet
/// order. With `from`, a wire is being dragged from that source, so only
/// candidates with at least one input are offered.
pub fn palette(src: &str, from: Option<Pos>) -> Vec<Candidate> {
    wiring::palette(src, from)
}

// ---------------------------------------------------------------------------
// Studio helpers beyond the P7 contract, for the playground's drag gesture.
// They read the sheet exactly as the contract's edits do.

/// A cell where a wire can start or end, for the UI to hit-test and
/// highlight.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Endpoint {
    /// The cell naming it, as [`connect`] takes it.
    pub at: Pos,
    /// Sources and sinks connect only within one scope: equal keys.
    pub scope: (usize, usize),
    /// A wire already meets it. A wired sink takes no other; a wired
    /// source fans out.
    pub wired: bool,
    /// How many cells from `at` eastward name it too: a constant's whole
    /// text, or an input's arrowhead through its name.
    pub span: usize,
}

/// Every source and every sink of `src`, each in row-major order. Empty
/// when the text has grid errors.
pub fn endpoints(src: &str) -> (Vec<Endpoint>, Vec<Endpoint>) {
    let Ok(grid) = Grid::parse(src) else { return (Vec::new(), Vec::new()) };
    let diagram = recognize(&grid);
    let model = model::Model::new(&grid, &diagram);
    let (sources, sinks) = model.endpoints();
    let sources = sources
        .into_iter()
        .map(|(at, s, wired)| {
            let span = s.goal.col.saturating_sub(at.col).max(1);
            Endpoint { at, scope: s.scope, wired, span }
        })
        .collect();
    let sinks = sinks
        .into_iter()
        .map(|(s, wired)| Endpoint { at: s.at, scope: s.scope, wired, span: 1 })
        .collect();
    (sources, sinks)
}

/// The cells a wire dragged from source `from` would take if released at
/// `to`: what [`connect`] would draw when `to` is a free sink in reach, or
/// a route to the blank cell `to`. `None` when nothing fits. For a live
/// preview; nothing is edited.
pub fn preview(src: &str, from: Pos, to: Pos) -> Option<Vec<Pos>> {
    wiring::preview(src, from, to)
}

/// For each wire of the recognized diagram (`Diagram::wires` order), the
/// cell naming its source as [`connect`] takes it, when the wire is
/// editable: so a drag may start on a wire to fan it out.
pub fn wire_sources(src: &str) -> Vec<Option<Pos>> {
    wiring::wire_sources(src)
}

/// A sheet as [`route`] sees it: blank cells are free except where a new
/// stroke would touch an existing wire end, port or text gap, straight
/// wire lines may be crossed, and box borders (title gaps included) wall
/// regions off.
struct Plain<'a> {
    grid: &'a Grid,
    diagram: &'a Diagram,
    walls: HashSet<Pos>,
    /// The ends may sit where a wire meets a port.
    ends: [Pos; 2],
}

impl<'a> Plain<'a> {
    fn new(grid: &'a Grid, diagram: &'a Diagram, from: Pos, to: Pos) -> Self {
        let mut walls = sheet::borders(diagram);
        walls.extend(sheet::keep_clear(grid, diagram, |_, _| false).into_iter().map(|(_, p)| p));
        Self { grid, diagram, walls, ends: [from, to] }
    }

    fn stroke(&self, p: Pos) -> bool {
        self.diagram.strokes.get(p).is_some()
    }
}

impl Terrain for Plain<'_> {
    fn cell(&self, p: Pos) -> Cell {
        if p.row >= self.grid.height()
            || p.col >= self.grid.width()
            || (self.walls.contains(&p) && !self.ends.contains(&p))
        {
            return Cell::Blocked;
        }
        match self.grid.get(p) {
            ' ' => Cell::Free,
            '─' | '-' => Cell::Line(true),
            '│' | '|' => Cell::Line(false),
            _ => Cell::Blocked,
        }
    }

    fn extra(&self, p: Pos) -> u32 {
        let near =
            Dir::ALL.into_iter().filter_map(|d| p.step(d)).filter(|&q| self.stroke(q)).count();
        near as u32 * CLEARANCE
    }

    fn goal(&self, p: Pos, _heading: Dir) -> Option<u32> {
        (p == self.ends[1]).then(|| CELL + self.extra(p))
    }
}

#[cfg(test)]
mod tests;
