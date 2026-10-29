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
mod describe;
mod mapped;
mod model;
mod mover;
mod router;
mod sheet;
mod wiring;

use std::collections::HashSet;
use std::fmt;

use shawline_doc::Doc;
use shawline_grid::{Dir, Grid, Pos, Rect};
use shawline_shape::Diagram;
pub use shawline_shape::structure::Mapping;

use router::{BUDGET, CELL, CLEARANCE, Cell, Start, Terrain};

/// Why an edit was refused. The sheet is unchanged.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refusal {
    /// The cell the refusal is about, for the UI to point at.
    pub at: Pos,
    /// What kind of obstacle it is, so the UI can pick a remedy. The corpus
    /// checks kinds and cells, never wording.
    pub kind: RefusalKind,
    /// A sentence the UI can show as is. It names only gestures that exist,
    /// and says what to try rather than which internal rule refused.
    pub reason: String,
    /// What would let the edit succeed, for the UI to offer. The page acts
    /// on this, never on the kind's name or the reason's words.
    pub remedy: Option<Remedy>,
}

/// What the UI can offer after a refusal (PLAN P10).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Remedy {
    /// [`make_room`] for the refused edit may succeed.
    MakeRoom,
    /// Formatting the sheet first may let the edit succeed.
    Format,
}

impl Remedy {
    /// Its name in JS: `make-room` or `format`.
    pub const fn name(self) -> &'static str {
        match self {
            Remedy::MakeRoom => "make-room",
            Remedy::Format => "format",
        }
    }
}

/// Why there is no room, which decides whether inserting room helps.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Blockage {
    /// Inserting rows or columns would make room: an overlap, a parent too
    /// small, or no free path.
    NoRoom,
    /// The sheet's edge: a box can't move above row 0 or left of column 0.
    Edge,
    /// The spacing rules forbid it whatever room there is: two top-level
    /// items on one row, or text touching a stroke (SPEC §11).
    Spacing,
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
    /// or find no free path. The blockage says whether inserting rows or
    /// columns may help; every blockage is named `blocked` in dumps.
    Blocked(Blockage),
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
            RefusalKind::Blocked(_) => "blocked",
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

impl RefusalKind {
    /// The remedy a refusal of this kind offers unless its edit knows
    /// better: [`Remedy::MakeRoom`] for [`Blockage::NoRoom`].
    pub const fn remedy(self) -> Option<Remedy> {
        match self {
            RefusalKind::Blocked(Blockage::NoRoom) => Some(Remedy::MakeRoom),
            _ => None,
        }
    }
}

impl Refusal {
    /// A refusal with its kind's remedy.
    pub fn new(at: Pos, kind: RefusalKind, reason: impl Into<String>) -> Self {
        Self { at, kind, reason: reason.into(), remedy: kind.remedy() }
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
pub fn move_box(doc: &Doc, corner: Pos, dr: isize, dc: isize) -> Result<Edited, Refusal> {
    let moved = mover::move_box(doc, corner, dr, dc)?;
    Ok(mapped::moved(doc, corner, dr, dc, moved))
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
    /// What it does in plain English: a primitive's short name
    /// ([`Prim::gloss`](shawline_vm::bytecode::Prim::gloss): "add", "sub"),
    /// "empty list" for `𐑯𐑦𐑤`, the port names for a definition ("𐑯 → 𐑬").
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
pub fn connect(doc: &Doc, from: Pos, to: Pos) -> Result<Edited, Refusal> {
    wiring::connect(doc, from, to).map(|d| mapped::kept(doc, d))
}

/// Removes the whole wire that passes through cell `at`, with every branch.
/// Refused when no editable wire passes through `at`.
pub fn remove_wire(doc: &Doc, at: Pos) -> Result<Edited, Refusal> {
    wiring::remove_wire(doc, at).map(|d| mapped::kept(doc, d))
}

/// Removes only the branch of a wire that reaches sink `to`, back to the tee
/// it leaves from. A tee left with two arms becomes a plain stroke. When the
/// wire has no other sink, the whole wire goes, as with [`remove_wire`].
pub fn disconnect(doc: &Doc, to: Pos) -> Result<Edited, Refusal> {
    wiring::disconnect(doc, to).map(|d| mapped::kept(doc, d))
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
    let placed = wiring::place_node(doc, corner, word, inputs, outputs)?;
    let edited = mapped::kept(doc, placed.doc);
    Ok(Placed { doc: edited.doc, map: edited.map, tidied: edited.tidied, ..placed })
}

/// A node [`place_node`] placed.
#[derive(Debug)]
pub struct Placed {
    /// The sheet with the node in it.
    pub doc: Doc,
    /// As [`Edited::map`]: every old box and text maps; the node is new.
    pub map: Mapping,
    /// As [`Edited::tidied`].
    pub tidied: bool,
    /// The node's input arrowheads, top to bottom: sinks for [`connect`].
    pub inputs: Vec<Pos>,
    /// The node's output tees, top to bottom: sources for [`connect`].
    pub outputs: Vec<Pos>,
}

/// Removes the node box whose top-left corner is `corner`. Every wire it
/// sources goes too, and only the branches that reach its inputs
/// ([`disconnect`]).
pub fn remove_node(doc: &Doc, corner: Pos) -> Result<Edited, Refusal> {
    wiring::remove_node(doc, corner).map(|d| mapped::kept(doc, d))
}

/// What the context palette offers, in a fixed order: primitives first in
/// SPEC §9 table order (with `𐑯𐑦𐑤`), then the sheet's definitions in sheet
/// order. With `from`, a wire is being dragged from that source, so only
/// candidates with at least one input are offered.
pub fn palette(doc: &Doc, from: Option<Pos>) -> Vec<Candidate> {
    wiring::palette(doc, from)
}

// ---------------------------------------------------------------------------
// P10 contract: edits as data (PLAN P10, "Oracle first"). Lead-owned.
//
// Every gesture is one `Edit`, so Make room and repairs can hold an edit and
// replay it, and the dumps, the gates and the UI name edits the same way.
// The edits P10 adds are stubs until their wave lands: each refuses as
// `unsupported` at the cell it names.
// ---------------------------------------------------------------------------

/// One checked edit, as a value. Its `Display` is the `.studio` step syntax.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Edit {
    /// [`move_box`].
    Move { corner: Pos, dr: isize, dc: isize },
    /// [`connect`].
    Connect { from: Pos, to: Pos },
    /// [`remove_wire`].
    RemoveWire(Pos),
    /// [`disconnect`].
    Disconnect(Pos),
    /// [`place_node`].
    PlaceNode { corner: Pos, word: String, inputs: u16, outputs: u16 },
    /// [`remove_node`].
    RemoveNode(Pos),
    /// [`rewire`].
    Rewire { from: Pos, to: Pos },
    /// [`place_connected`].
    PlaceConnected { at: Pos, word: String, inputs: u16, outputs: u16, from: Pos },
    /// [`insert`].
    Insert(Insertion),
    /// [`delete`].
    Delete(Insertion),
    /// [`make_room`] for the edit inside.
    MakeRoom(Box<Edit>),
    /// [`set_text`].
    SetText { at: Pos, content: String },
    /// [`place_text`].
    PlaceText { at: Pos, content: String },
    /// [`remove_text`].
    RemoveText(Pos),
    /// [`place_definition`].
    PlaceDefinition { rect: Rect, name: String },
    /// [`place_switch`].
    PlaceSwitch { rect: Rect, guard: Option<String> },
    /// [`add_port`].
    AddPort { at: Pos, name: Option<String> },
    /// [`add_arm`].
    AddArm { at: Pos, guard: Option<String> },
    /// [`place_inline`].
    PlaceInline { at: Pos, word: String },
}

impl fmt::Display for Edit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let some = |f: &mut fmt::Formatter<'_>, text: &Option<String>| match text {
            Some(text) => write!(f, " {text}"),
            None => Ok(()),
        };
        match self {
            Edit::Move { corner, dr, dc } => write!(f, "move {corner} {dr} {dc}"),
            Edit::Connect { from, to } => write!(f, "connect {from} {to}"),
            Edit::RemoveWire(at) => write!(f, "remove_wire {at}"),
            Edit::Disconnect(to) => write!(f, "disconnect {to}"),
            Edit::PlaceNode { corner, word, inputs, outputs } => {
                write!(f, "place_node {corner} {word} {inputs} {outputs}")
            }
            Edit::RemoveNode(corner) => write!(f, "remove_node {corner}"),
            Edit::Rewire { from, to } => write!(f, "rewire {from} {to}"),
            Edit::PlaceConnected { at, word, inputs, outputs, from } => {
                write!(f, "place_connected {at} {word} {inputs} {outputs} {from}")
            }
            Edit::Insert(i) => write!(f, "insert {i}"),
            Edit::Delete(i) => write!(f, "delete {i}"),
            Edit::MakeRoom(edit) => write!(f, "make_room {edit}"),
            Edit::SetText { at, content } => write!(f, "set_text {at} {content}"),
            Edit::PlaceText { at, content } => write!(f, "place_text {at} {content}"),
            Edit::RemoveText(at) => write!(f, "remove_text {at}"),
            Edit::PlaceDefinition { rect, name } => write!(f, "place_definition {rect} {name}"),
            Edit::PlaceSwitch { rect, guard } => {
                write!(f, "place_switch {rect}")?;
                some(f, guard)
            }
            Edit::AddPort { at, name } => {
                write!(f, "add_port {at}")?;
                some(f, name)
            }
            Edit::AddArm { at, guard } => {
                write!(f, "add_arm {at}")?;
                some(f, guard)
            }
            Edit::PlaceInline { at, word } => write!(f, "place_inline {at} {word}"),
        }
    }
}

/// A checked revision and where the old sheet's parts went in it.
#[derive(Debug)]
pub struct Edited {
    pub doc: Doc,
    /// From the boxes, texts and wires of the input's canonical revision
    /// (the input itself when it is canonical) to those of `doc`. A part
    /// the edit removed maps to `None`, and so does a wire it re-routed
    /// until its edit maps wires exactly.
    pub map: Mapping,
    /// The input wasn't canonical, so the edit was made on its formatted
    /// text. The page applies that text first, as its own undo step, so
    /// that `map` starts from what it shows.
    pub tidied: bool,
}

/// Applies `edit` to `doc`.
pub fn apply(doc: &Doc, edit: &Edit) -> Result<Edited, Refusal> {
    match edit {
        Edit::Move { corner, dr, dc } => move_box(doc, *corner, *dr, *dc),
        Edit::Connect { from, to } => connect(doc, *from, *to),
        Edit::RemoveWire(at) => remove_wire(doc, *at),
        Edit::Disconnect(to) => disconnect(doc, *to),
        Edit::PlaceNode { corner, word, inputs, outputs } => place_node(doc, *corner, word, *inputs, *outputs)
            .map(|p| Edited { doc: p.doc, map: p.map, tidied: p.tidied }),
        Edit::RemoveNode(corner) => remove_node(doc, *corner),
        Edit::Rewire { from, to } => rewire(doc, *from, *to),
        Edit::PlaceConnected { at, word, inputs, outputs, from } => {
            place_connected(doc, *at, word, *inputs, *outputs, *from)
        }
        Edit::Insert(i) => insert(doc, *i),
        Edit::Delete(i) => delete(doc, *i),
        Edit::MakeRoom(edit) => make_room(doc, edit).map(|roomed| roomed.edited),
        Edit::SetText { at, content } => set_text(doc, *at, content),
        Edit::PlaceText { at, content } => place_text(doc, *at, content),
        Edit::RemoveText(at) => remove_text(doc, *at),
        Edit::PlaceDefinition { rect, name } => place_definition(doc, *rect, name),
        Edit::PlaceSwitch { rect, guard } => place_switch(doc, *rect, guard.as_deref()),
        Edit::AddPort { at, name } => add_port(doc, *at, name.as_deref()),
        Edit::AddArm { at, guard } => add_arm(doc, *at, guard.as_deref()),
        Edit::PlaceInline { at, word } => place_inline(doc, *at, word),
    }
}

/// Rows or columns.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Axis {
    Row,
    Column,
}

impl Axis {
    /// Its name in dumps and in JS: `row` or `column`.
    pub const fn name(self) -> &'static str {
        match self {
            Axis::Row => "row",
            Axis::Column => "column",
        }
    }
}

/// `count` rows or columns at cell `at`.
///
/// A row is inserted before row `at.row`, or rows `at.row..` deleted,
/// between the top and bottom borders of the top-level box that holds `at`.
/// A column is inserted before column `at.col`, or columns `at.col..`
/// deleted, across the rows of that box only, so other definitions stay
/// where they are.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Insertion {
    pub axis: Axis,
    pub at: Pos,
    pub count: usize,
}

impl fmt::Display for Insertion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {} {}", self.axis.name(), self.at, self.count)
    }
}

/// Inserts blank rows or columns. Each inserted cell continues a stroke
/// that joins across the cut (`─`, `┄` or `│`) and is blank elsewhere. A
/// cut never splits a text: where it would fall in the gap between a text
/// and the stroke it meets, it moves to the stroke's side of the gap. The
/// result is canonical, and its structure is the old one relabelled by the
/// shift ([`Routing::Kept`](shawline_shape::structure::Routing::Kept)).
/// Refused where the top-level box doesn't hold `at`.
pub fn insert(doc: &Doc, insertion: Insertion) -> Result<Edited, Refusal> {
    let _ = doc;
    Err(pending(insertion.at))
}

/// Deletes rows or columns: [`insert`]'s exact inverse. Refused where a
/// cell to delete is neither blank nor a plain stretch of stroke.
pub fn delete(doc: &Doc, deletion: Insertion) -> Result<Edited, Refusal> {
    let _ = doc;
    Err(pending(deletion.at))
}

/// What [`make_room`] did.
#[derive(Debug)]
pub struct Roomed {
    /// The sheet with room made and the edit applied, mapped from `doc`.
    pub edited: Edited,
    /// The rows and columns it inserted, in the order it inserted them.
    pub inserted: Vec<Insertion>,
}

/// Inserts rows or columns until `edit` fits, then applies it, as one
/// checked edit. The search is deterministic and bounded; its order and
/// budget are private. Refused, with the original refusal's cell, when no
/// insertion within the budget lets `edit` succeed.
pub fn make_room(doc: &Doc, edit: &Edit) -> Result<Roomed, Refusal> {
    let _ = doc;
    Err(pending(edit.at()))
}

/// Replaces the content of the text through `at` (any of its cells). A
/// constant grows or shrinks at its left end, so its wire stays put; a
/// name, title, guard or comment at its right end. The text keeps its role:
/// a constant stays a literal, a name a word, a title a `·`-word.
pub fn set_text(doc: &Doc, at: Pos, content: &str) -> Result<Edited, Refusal> {
    let _ = (doc, content);
    Err(pending(at))
}

/// Writes a new text starting at `at`. Refused when it would meet a stroke
/// across spaces: a new constant is stray (`Y012`) until a [`connect`]
/// wires it.
pub fn place_text(doc: &Doc, at: Pos, content: &str) -> Result<Edited, Refusal> {
    let _ = (doc, content);
    Err(pending(at))
}

/// Removes the text through `at`: a stray text or a comment, or a constant
/// with its wire. Refused for names, titles and guards.
pub fn remove_text(doc: &Doc, at: Pos) -> Result<Edited, Refusal> {
    let _ = doc;
    Err(pending(at))
}

/// Draws an empty definition box at `rect`, titled `·name`. Refused when it
/// shares a row with another top-level item.
pub fn place_definition(doc: &Doc, rect: Rect, name: &str) -> Result<Edited, Refusal> {
    let _ = (doc, name);
    Err(pending(Pos::new(rect.top, rect.left)))
}

/// Draws an empty switch box at `rect` inside a definition or an arm, with
/// its first arm's guard in the top border (`None` for a default arm).
pub fn place_switch(doc: &Doc, rect: Rect, guard: Option<&str>) -> Result<Edited, Refusal> {
    let _ = (doc, guard);
    Err(pending(Pos::new(rect.top, rect.left)))
}

/// Adds a port at border cell `at`: a definition's named input or output,
/// or a switch's selector or through-port, which take no name.
pub fn add_port(doc: &Doc, at: Pos, name: Option<&str>) -> Result<Edited, Refusal> {
    let _ = (doc, name);
    Err(pending(at))
}

/// Draws a switch divider from left-border cell `at`, with its guard
/// (`None` for the default arm).
pub fn add_arm(doc: &Doc, at: Pos, guard: Option<&str>) -> Result<Edited, Refusal> {
    let _ = (doc, guard);
    Err(pending(at))
}

/// Splices a 1→1 call of `word` into the straight stretch of wire through
/// `at`, its word starting there.
pub fn place_inline(doc: &Doc, at: Pos, word: &str) -> Result<Edited, Refusal> {
    let _ = (doc, word);
    Err(pending(at))
}

impl Edit {
    /// The cell the edit names first, where a refusal about it points.
    pub fn at(&self) -> Pos {
        match self {
            Edit::Move { corner: at, .. }
            | Edit::Connect { to: at, .. }
            | Edit::RemoveWire(at)
            | Edit::Disconnect(at)
            | Edit::PlaceNode { corner: at, .. }
            | Edit::RemoveNode(at)
            | Edit::Rewire { to: at, .. }
            | Edit::PlaceConnected { at, .. }
            | Edit::SetText { at, .. }
            | Edit::PlaceText { at, .. }
            | Edit::RemoveText(at)
            | Edit::AddPort { at, .. }
            | Edit::AddArm { at, .. }
            | Edit::PlaceInline { at, .. } => *at,
            Edit::Insert(i) | Edit::Delete(i) => i.at,
            Edit::MakeRoom(edit) => edit.at(),
            Edit::PlaceDefinition { rect, .. } | Edit::PlaceSwitch { rect, .. } => {
                Pos::new(rect.top, rect.left)
            }
        }
    }
}

/// Moves the branch reaching sink `to` off its source and onto source
/// `from`, as one edit: [`disconnect`] then [`connect`], refused whole when
/// either is.
pub fn rewire(doc: &Doc, from: Pos, to: Pos) -> Result<Edited, Refusal> {
    let _ = (doc, from);
    Err(pending(to))
}

/// The palette's gesture after a wire from source `from` is dropped on
/// blank cell `at`: places a node for `word` near `at` (its first input on
/// `at`'s row, the corner offset and retried as the page does today) and
/// connects `from` to its first input, as one edit. Refused whole when no
/// placement can be wired.
pub fn place_connected(
    doc: &Doc,
    at: Pos,
    word: &str,
    inputs: u16,
    outputs: u16,
    from: Pos,
) -> Result<Edited, Refusal> {
    let _ = (doc, word, inputs, outputs, from);
    Err(pending(at))
}

/// What a box is, by how recognition reads it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BoxKind {
    /// A top-level box.
    Definition,
    /// A box inside a definition with dividers or a selector on its top
    /// edge.
    Switch,
    /// Any other box inside a definition: a call.
    Node,
}

impl BoxKind {
    /// Its name in JS: `definition`, `switch` or `node`.
    pub const fn name(self) -> &'static str {
        match self {
            BoxKind::Definition => "definition",
            BoxKind::Switch => "switch",
            BoxKind::Node => "node",
        }
    }
}

/// The kind of the box whose top-left corner is `corner`, in the text as
/// given. `None` when no box has its corner there.
pub fn box_kind(doc: &Doc, corner: Pos) -> Option<BoxKind> {
    let (grid, diagram) = (doc.grid().ok()?, doc.diagram()?);
    let b = diagram.boxes.iter().position(|b| (b.rect.top, b.rect.left) == (corner.row, corner.col))?;
    Some(model::Model::new(grid, diagram).kind(b))
}

/// The cells [`disconnect`] would clear for sink `to`, in the text as
/// given, row-major. Empty when it would be refused.
pub fn branch(doc: &Doc, to: Pos) -> Vec<Pos> {
    // The cells that change, read off the result; exact while the given
    // text is canonical.
    let Ok(cut) = disconnect(doc, to) else { return Vec::new() };
    let (Ok(before), Ok(after)) = (doc.grid(), cut.doc.grid()) else { return Vec::new() };
    let mut cells = Vec::new();
    for row in 0..before.height() {
        for col in 0..before.width() {
            let p = Pos::new(row, col);
            let gone = row >= after.height() || col >= after.width() || after.get(p) != before.get(p);
            if gone && before.get(p) != ' ' {
                cells.push(p);
            }
        }
    }
    cells
}

/// What hover, the inspector, the palette and the drawing's hover title say
/// about a word.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Description {
    /// The described cells' first cell.
    pub at: Pos,
    /// How many cells are described.
    pub width: usize,
    /// Markdown: a call word or definition title is described; with
    /// `gloss`, a line follows with the word's reading and the keys that
    /// type it, and a primitive's short name.
    pub text: String,
}

/// The description of the word covering `at`, or `None` when there is
/// nothing to say. Call words and definition titles are described. With
/// `gloss`, every Shavian word (ports, text and comments too) also gets a
/// line with its reading and the keys that type it, and a primitive its
/// short name on that line.
pub fn describe(doc: &Doc, at: Pos, gloss: bool) -> Option<Description> {
    describe::describe(doc, at, gloss)
}

/// The refusal of an edit whose wave hasn't landed.
fn pending(at: Pos) -> Refusal {
    Refusal::new(at, RefusalKind::Unsupported, "Studio can't make this edit yet")
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

/// Where a dragged wire is anchored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Anchor {
    /// Dragged out of a source, towards a sink or blank sheet.
    Source(Pos),
    /// Dragged back out of an unwired sink's arrowhead, towards a source or
    /// blank sheet.
    Sink(Pos),
}

/// A live preview of a dragged wire. Nothing is edited.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Preview {
    /// The whole polyline [`connect`] would draw, ends included: from the
    /// source (or the tee where it joins the source's wire) to the sink, or
    /// to the blank cell the drag is over.
    pub path: Vec<Pos>,
    /// The path starts on the source's existing wire, at a new tee.
    pub joins_wire: bool,
}

/// What [`connect`] would draw for a drag anchored at `anchor` and
/// released at `to`: a free endpoint of the other kind in the same scope,
/// or a blank cell. `None` when nothing fits.
pub fn preview(doc: &Doc, anchor: Anchor, to: Pos) -> Option<Preview> {
    match anchor {
        // Today's path leaves out the ends; wave 1 adds them and sinks.
        Anchor::Source(from) => wiring::preview(doc, from, to).map(|path| Preview { path, joins_wire: false }),
        Anchor::Sink(_) => None,
    }
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
