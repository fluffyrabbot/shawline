//! Gestures made of two edits, applied as one (PLAN P10, "Edits as
//! data"): moving a branch to another source, and placing a node where a
//! wire was dropped and wiring it in. Each is refused whole when either
//! step is, and its map runs through both.

use shawline_doc::Doc;
use shawline_grid::Pos;

use shawline_grid::Dir;
use shawline_shape::TextKind;

use crate::canon::Sheets;
use crate::cut::render;
use crate::mapped::{self, compose};
use crate::model::{Model, NIL, no_sink, no_source};
use crate::retext::token;
use crate::wiring::node_rect;
use crate::{Blockage, Edited, NewSource, Refusal, RefusalKind, connect, disconnect, place_node};

pub(crate) fn rewire(doc: &Doc, from: Pos, to: Pos) -> Result<Edited, Refusal> {
    let sheets = Sheets::read(doc, to)?;
    let source = sheets.entity(from).ok_or_else(|| no_source(from))?;
    let sink = sheets.entity(to).ok_or_else(|| no_sink(to))?;
    let cut = disconnect(doc, to)?;
    // Nothing moves when a branch is cut, so the canonical cells still
    // name the source and the sink.
    let back = |at: Pos| if at == sink { to } else if at == source { from } else { at };
    let wired = connect(&cut.doc, source, sink).map_err(|r| Refusal { at: back(r.at), ..r })?;
    Ok(Edited { doc: wired.doc, map: compose(&cut.map, &wired.map), tidied: cut.tidied })
}

pub(crate) fn place_connected(
    doc: &Doc,
    at: Pos,
    word: &str,
    inputs: u16,
    outputs: u16,
    from: Pos,
) -> Result<Edited, Refusal> {
    if inputs == 0 {
        let why = format!("{word} has no input to wire the source into; pick one that has");
        return Err(Refusal::new(at, RefusalKind::Target, why));
    }
    let sheets = Sheets::read(doc, at)?;
    let source = sheets.entity(from).ok_or_else(|| no_source(from))?;
    // The first input lands on the drop row, with the box's top border
    // above it. A drop a row or two off the source's row tries level with
    // the source first, so the new wire runs straight instead of jogging.
    let rows = if from.row != at.row && from.row.abs_diff(at.row) <= 2 { vec![from.row, at.row] } else { vec![at.row] };
    let mut unwired: Option<Refusal> = None;
    let mut unplaced: Option<Refusal> = None;
    for row in rows {
        let corner = Pos::new(row.saturating_sub(1), at.col);
        match place_node(doc, corner, word, inputs, outputs) {
            Ok(placed) => match connect(&placed.doc, source, placed.inputs[0]) {
                Ok(wired) => {
                    let map = compose(&placed.map, &wired.map);
                    return Ok(Edited { doc: wired.doc, map, tidied: placed.tidied });
                }
                Err(refusal) => {
                    unwired.get_or_insert(refusal);
                }
            },
            // The drop row is tried last; its refusal is the one to show.
            Err(refusal) => unplaced = Some(refusal),
        }
    }
    match unwired {
        Some(r) => {
            let reason = format!("nothing placed: the new node couldn't be wired in ({})", r.reason);
            Err(Refusal { at, reason, ..r })
        }
        None => Err(unplaced.expect("every row was tried and refused")),
    }
}

/// How many strokes a fed source's wire runs before the arrowhead, in the
/// order tried: the corpus's `21 ───▶` first, then shorter, then longer.
const LEADS: [usize; 6] = [3, 2, 1, 4, 5, 6];

pub(crate) fn feed(doc: &Doc, to: Pos, source: &NewSource) -> Result<Edited, Refusal> {
    let sheets = Sheets::read(doc, to)?;
    let sink = sheets.entity(to).ok_or_else(|| no_sink(to))?;
    let (grid, diagram) = sheets.canon();
    let model = Model::new(grid, diagram);
    let found = model.sink(sink).map_err(|r| Refusal { at: to, ..r })?;
    if model.wire_of(found.term()).is_some() {
        let why = "this arrowhead already has a wire; disconnect it first";
        return Err(Refusal::new(to, RefusalKind::Target, why));
    }
    if found.dir != Dir::E {
        let why = "a new source goes left of an arrowhead pointing right; drag a wire to this one instead";
        return Err(Refusal::new(to, RefusalKind::Target, why));
    }
    if let NewSource::Constant(content) = source
        && !is_constant(content)
    {
        return Err(Refusal::new(to, RefusalKind::Target, "a constant is a number, a «text» or 𐑯𐑦𐑤"));
    }
    if let NewSource::Node { word, outputs: 0, .. } = source {
        let why = format!("{word} has no output to feed the arrowhead; pick one that has");
        return Err(Refusal::new(to, RefusalKind::Target, why));
    }
    let canon = sheets.doc();
    let mut unwired: Option<Refusal> = None;
    for lead in LEADS {
        // The source's own end: the constant's last cell, one space before
        // the wire, or the node's tee, where the wire starts.
        let placed = match source {
            NewSource::Constant(content) => {
                sink.col.checked_sub(lead + 2).and_then(|end| constant(canon, sink, end, content))
            }
            NewSource::Node { word, inputs, outputs } => sink
                .col
                .checked_sub(lead + 1)
                .and_then(|end| node(canon, sink, end, word, *inputs, *outputs)),
        };
        // The sheet with the source unwired is only ever connected.
        let Some((placed, from)) = placed else { continue };
        match connect(&placed.doc, from, sink) {
            Ok(wired) => {
                let map = compose(&placed.map, &wired.map);
                return Ok(Edited { doc: wired.doc, map, tidied: sheets.tidied() });
            }
            Err(refusal) => {
                unwired.get_or_insert(refusal);
            }
        }
    }
    Err(match unwired {
        Some(r) => {
            let reason = format!("nothing placed: the new source couldn't be wired in ({})", r.reason);
            Refusal { at: to, reason, ..r }
        }
        None => {
            let why = "there's no room left of this arrowhead for a new source; try Make room or move what's there";
            Refusal::new(to, RefusalKind::Blocked(Blockage::NoRoom), why)
        }
    })
}

/// Whether `content` written alone is a constant: a number, a `«»` text
/// or `𐑯𐑦𐑤`.
fn is_constant(content: &str) -> bool {
    token(content).is_some_and(|t| {
        matches!(t.kind, TextKind::Number | TextKind::Literal) || (t.kind == TextKind::Word && t.content == NIL)
    })
}

/// Canonical `canon` with `content` ending at column `end` of the sink's
/// row, and its first cell: the sheet a connect then wires. `None` where
/// the cells from one before the text up to the arrowhead aren't blank and
/// inside one box.
fn constant(canon: &Doc, sink: Pos, end: usize, content: &str) -> Option<(Edited, Pos)> {
    let (grid, diagram) = mapped::read(canon);
    let width = content.chars().count();
    let start = (end + 1).checked_sub(width)?;
    let row = sink.row;
    let before = Pos::new(row, start.checked_sub(1)?);
    let last = Pos::new(row, sink.col - 1);
    if !(before.col..sink.col).all(|c| grid.get(Pos::new(row, c)) == ' ') {
        return None;
    }
    if !diagram.boxes.iter().any(|b| b.rect.interior_contains(before) && b.rect.interior_contains(last)) {
        return None;
    }
    let mut rows: Vec<Vec<char>> = (0..grid.height()).map(|r| grid.row(r).to_vec()).collect();
    for (k, ch) in content.chars().enumerate() {
        let line = &mut rows[row];
        if line.len() <= start + k {
            line.resize(start + k + 1, ' ');
        }
        line[start + k] = ch;
    }
    let drawn = Doc::new(render(&rows));
    let new = drawn.diagram()?;
    if new.texts.len() != diagram.texts.len() + 1 || new.boxes.len() != diagram.boxes.len() {
        return None;
    }
    let map = mapped::map(mapped::read(canon), mapped::read(&drawn), Some);
    Some((Edited { doc: drawn, map, tidied: false }, Pos::new(row, start)))
}

/// Canonical `canon` with a node placed so that its first output tee is at
/// column `end` of the sink's row, and that tee.
fn node(canon: &Doc, sink: Pos, end: usize, word: &str, inputs: u16, outputs: u16) -> Option<(Edited, Pos)> {
    let width = node_rect(Pos::new(0, 0), word, inputs, outputs).right;
    let corner = Pos::new(sink.row.checked_sub(1)?, end.checked_sub(width)?);
    let placed = place_node(canon, corner, word, inputs, outputs).ok()?;
    let tee = *placed.outputs.first()?;
    Some((Edited { doc: placed.doc, map: placed.map, tidied: false }, tee))
}
