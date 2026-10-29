//! Gestures made of two edits, applied as one (PLAN P10, "Edits as
//! data"): moving a branch to another source, and placing a node where a
//! wire was dropped and wiring it in. Each is refused whole when either
//! step is, and its map runs through both.

use shawline_doc::Doc;
use shawline_grid::Pos;

use crate::canon::Sheets;
use crate::mapped::compose;
use crate::model::{no_sink, no_source};
use crate::{Edited, Refusal, RefusalKind, connect, disconnect, place_node};

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
