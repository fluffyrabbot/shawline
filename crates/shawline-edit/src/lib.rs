//! Grid edits for Shawline Studio: every gesture becomes one checked edit to
//! the sheet's text (PLAN P6; the design is in the Shawline Studio doc).
//!
//! # Contract
//!
//! The signatures and the semantics written on them are lead-owned (P6, P7,
//! and P8's revisions and refusal kinds). The implementation behind them is
//! the edit agent's: add modules freely, but don't change what these
//! functions take, return or promise.
//!
//! Every function here takes one revision of the sheet as a [`Doc`] and
//! returns a new revision or a [`Refusal`]. The text is the only state
//! (Studio principle 1), so undo, sharing and the CLI need nothing new.
//! An edit returns the revision it checked, so whatever reads it next
//! shares that reading (PLAN P8).
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
use std::fmt;

use shawline_doc::Doc;
use shawline_grid::{Dir, Grid, Pos};
use shawline_shape::Diagram;

use router::{BUDGET, CELL, CLEARANCE, Cell, Start, Terrain};

/// Why an edit was refused. The sheet is unchanged.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refusal {
    /// The cell the refusal is about, for the UI to point at.
    pub at: Pos,
    /// What kind of obstacle it is, so the UI can pick a remedy. The corpus
    /// checks kinds and cells, never wording.
    pub kind: RefusalKind,
    /// A sentence the UI can show as is, such as "no free path for the wire
    /// from 𐑐; insert a column?".
    pub reason: String,
}

/// What stopped an edit (PLAN P8).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RefusalKind {
    /// The sheet can't be read for editing: a grid error, or a text the
    /// formatter can't tidy.
    Unreadable,
    /// The cell named isn't what the edit needs: no box corner, no source,
    /// no sink, no editable wire, or a sink that already has a wire.
    Target,
    /// Studio can't make this edit yet: a switch's merged outputs, a wire
    /// touching an inline node, a word that isn't Shavian.
    Unsupported,
    /// There is no room: the result would overlap a box, leave its parent,
    /// or find no free path. Inserting rows or columns may help.
    Blocked,
    /// The router's search budget ran out before it found a path.
    GaveUp,
    /// The result would change something the edit promises to keep: the
    /// connections, the diagnostics, or its own canonical form.
    Changed,
}

impl RefusalKind {
    /// Its name in dumps and in JS: `unreadable`, `target`, `unsupported`,
    /// `blocked`, `gave-up` or `changed`.
    pub const fn name(self) -> &'static str {
        match self {
            RefusalKind::Unreadable => "unreadable",
            RefusalKind::Target => "target",
            RefusalKind::Unsupported => "unsupported",
            RefusalKind::Blocked => "blocked",
            RefusalKind::GaveUp => "gave-up",
            RefusalKind::Changed => "changed",
        }
    }
}

impl fmt::Display for RefusalKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

impl Refusal {
    pub(crate) fn new(at: Pos, kind: RefusalKind, reason: impl Into<String>) -> Self {
        Self { at, kind, reason: reason.into() }
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
pub fn move_box(doc: &Doc, corner: Pos, dr: isize, dc: isize) -> Result<Doc, Refusal> {
    mover::move_box(doc, corner, dr, dc)
}

/// The cheapest orthogonal path for a wire from cell `from` to cell `to`,
/// through cells that are blank in `doc` (both ends included in the path).
///
/// Cost follows the formatter's aesthetics (SPEC §11–§12): each cell costs 1,
/// each bend costs more, each crossing of an existing wire much more, and a
/// cell beside a box border or another wire costs a little extra (clearance).
/// The search is best-first with a fixed budget of expanded cells. `None`
/// when no path exists or the budget runs out, so a caller can't hang.
pub fn route(doc: &Doc, from: Pos, to: Pos) -> Option<Vec<Pos>> {
    let (grid, diagram) = (doc.grid().ok()?, doc.diagram()?);
    let sheet = Plain::new(grid, diagram, from, to);
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
pub fn connect(doc: &Doc, from: Pos, to: Pos) -> Result<Doc, Refusal> {
    wiring::connect(doc, from, to)
}

/// Removes the whole wire that passes through cell `at`, with every branch.
/// Refused when no editable wire passes through `at`.
pub fn remove_wire(doc: &Doc, at: Pos) -> Result<Doc, Refusal> {
    wiring::remove_wire(doc, at)
}

/// Removes only the branch of a wire that reaches sink `to`, back to the tee
/// it leaves from. A tee left with two arms becomes a plain stroke. When the
/// wire has no other sink, the whole wire goes, as with [`remove_wire`].
pub fn disconnect(doc: &Doc, to: Pos) -> Result<Doc, Refusal> {
    wiring::disconnect(doc, to)
}

/// Places a canonical node box for `word` with its top-left corner at
/// `corner`: `inputs` arrowheads on its left border, `outputs` tees on its
/// right, and no wires. Refused when the box would overlap anything, or not
/// lie wholly inside one definition (or one arm of a switch).
///
/// The result names the new node's ports in its own cells, so a compound
/// gesture (place, then connect) can name them without guessing.
pub fn place_node(
    doc: &Doc,
    corner: Pos,
    word: &str,
    inputs: u16,
    outputs: u16,
) -> Result<Placed, Refusal> {
    wiring::place_node(doc, corner, word, inputs, outputs)
}

/// A node [`place_node`] placed.
#[derive(Debug)]
pub struct Placed {
    /// The sheet with the node in it.
    pub doc: Doc,
    /// The node's input arrowheads, top to bottom: sinks for [`connect`].
    pub inputs: Vec<Pos>,
    /// The node's output tees, top to bottom: sources for [`connect`].
    pub outputs: Vec<Pos>,
}

/// Removes the node box whose top-left corner is `corner`. Every wire it
/// sources goes too, and only the branches that reach its inputs
/// ([`disconnect`]).
pub fn remove_node(doc: &Doc, corner: Pos) -> Result<Doc, Refusal> {
    wiring::remove_node(doc, corner)
}

/// What the context palette offers, in a fixed order: primitives first in
/// SPEC §9 table order (with `𐑯𐑦𐑤`), then the sheet's definitions in sheet
/// order. With `from`, a wire is being dragged from that source, so only
/// candidates with at least one input are offered.
pub fn palette(doc: &Doc, from: Option<Pos>) -> Vec<Candidate> {
    wiring::palette(doc, from)
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

/// Every source and every sink of `doc`, each in row-major order. Empty
/// when the text has grid errors.
pub fn endpoints(doc: &Doc) -> (Vec<Endpoint>, Vec<Endpoint>) {
    let (Ok(grid), Some(diagram)) = (doc.grid(), doc.diagram()) else {
        return (Vec::new(), Vec::new());
    };
    let model = model::Model::new(grid, diagram);
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
pub fn preview(doc: &Doc, from: Pos, to: Pos) -> Option<Vec<Pos>> {
    wiring::preview(doc, from, to)
}

/// For each wire of the recognized diagram (`Diagram::wires` order), the
/// cell naming its source as [`connect`] takes it, when the wire is
/// editable: so a drag may start on a wire to fan it out.
pub fn wire_sources(doc: &Doc) -> Vec<Option<Pos>> {
    wiring::wire_sources(doc)
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
