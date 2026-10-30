//! Removing what the drawing edits add (PLAN P10, "Drawing boxes"): ports,
//! arms and boxes with everything they hold, and redrawing a node box with
//! other port counts or another word.
//!
//! Wires go first, one at a time, each read off the sheet the cut before it
//! left, as `remove_node` does: two cuts on one fan-out leave its tee a
//! corner, then take it. Then the strokes and texts go. The result is
//! checked against the input's structure with exactly those cuts and parts
//! declared, however the remaining wires run.

use std::collections::HashMap;

use shawline_doc::Doc;
use shawline_grid::{Dir, Grid, Pos, Rect};
use shawline_shape::structure::{Across, Mapping, PortPart, Routing, Structure};
use shawline_shape::{Diagram, EndKind, PortKind, TextKind};

use crate::canon::Sheets;
use crate::cut::render;
use crate::drawing::{is_word, items, put, renumber, rows, settle};
use crate::mapped::{self, Read};
use crate::model::{Model, Sink, Term};
use crate::wiring::{Canvas, cut_branch, cut_end, erase_wire, node_rect, read};
use crate::{BoxKind, Edited, Refusal, RefusalKind};

/// The most ports a node box holds on one side, as `place_node` draws.
const MAX_PORTS: u16 = 16;

/// One wire cut, named by what it meets, so it names the same wire on
/// each sheet a cut before it leaves.
#[derive(Clone, Copy, Debug)]
enum Cut {
    /// The branch reaching a sink; a merged output's whole wire with its
    /// last sink.
    Branch(Sink),
    /// The whole wire with this source.
    Wire(Term),
    /// A merged output's stub at this right through-port; the whole wire
    /// when it is the last stub.
    Stub(Pos),
}

impl Cut {
    /// Checks that the cut strands no inline node on `model`.
    fn allowed(self, model: &Model, at: Pos) -> Result<(), Refusal> {
        let (into, from) = match self {
            Cut::Branch(sink) => {
                if let Some(t) = model.inline_after(sink.at).filter(|_| sink.free) {
                    return Err(model.strands(at, t));
                }
                let Some(w) = model.wire_of(sink.term()) else { return Ok(()) };
                (None, model.inline_ends(w).1.filter(|_| model.sink_count(w) <= 1))
            }
            Cut::Wire(term) => match model.wire_of(term) {
                Some(w) => model.inline_ends(w),
                None => (None, None),
            },
            Cut::Stub(_) => (None, None),
        };
        match into.or(from) {
            Some(t) => Err(model.strands(at, t)),
            None => Ok(()),
        }
    }

    /// Makes the cut on `canvas`, a copy of `model`'s sheet.
    fn draw(self, canvas: &mut Canvas, model: &Model) {
        match self {
            Cut::Branch(sink) => {
                let Some(w) = model.wire_of(sink.term()) else { return };
                if model.sink_count(w) <= 1 && model.source_count(w) > 1 {
                    erase_wire(canvas, model, w);
                } else {
                    cut_branch(canvas, model, w, &sink);
                }
            }
            Cut::Wire(term) => {
                if let Some(w) = model.wire_of(term) {
                    erase_wire(canvas, model, w);
                }
            }
            Cut::Stub(port) => {
                let Some(w) = model.wire_of(Term::Port(port, Dir::W)) else { return };
                if model.source_count(w) <= 1 {
                    erase_wire(canvas, model, w);
                    return;
                }
                let end = model.diagram.wires[w]
                    .ends
                    .iter()
                    .find(|e| e.kind == EndKind::Port(port) && e.facing == Dir::W)
                    .expect("a stub ends at its port");
                cut_end(canvas, model, w, end);
            }
        }
    }

    /// Declares the cut in `expected`, the structure of `model`'s sheet.
    fn declare(self, expected: &mut Structure, model: &Model) {
        match self {
            Cut::Branch(sink) => {
                let (Some(w), Some(term)) = (model.wire_of(sink.term()), model.named(sink.term())) else { return };
                if model.sink_count(w) <= 1 && model.source_count(w) > 1 {
                    expected.remove_wire(term);
                } else {
                    expected.disconnect(term);
                }
            }
            Cut::Wire(term) => {
                if let Some(term) = model.named(term).filter(|_| model.wire_of(term).is_some()) {
                    expected.remove_wire(term);
                }
            }
            Cut::Stub(port) => {
                let term = Term::Port(port, Dir::W);
                let (Some(w), Some(named)) = (model.wire_of(term), model.named(term)) else { return };
                if model.source_count(w) <= 1 {
                    expected.remove_wire(named);
                } else if let Some(part) = expected.wires.iter_mut().find(|p| p.terms.contains(&named)) {
                    part.terms.retain(|&t| t != named);
                }
            }
        }
    }
}

/// Makes `cuts` one at a time from canonical sheet `canon`, each on the
/// sheet the one before it left, and declares each in `expected`, the
/// structure of `canon`. Each is judged on the sheet it is made on, so
/// a merge's second stub to go is its last. A cut moves nothing, so boxes,
/// ports and texts keep their indices from sheet to sheet. Gives the sheet
/// the cuts leave.
fn cut_all(canon: &Doc, cuts: &[Cut], expected: &mut Structure, at: Pos) -> Result<Doc, Refusal> {
    let mut doc: Option<Doc> = None;
    for &cut in cuts {
        let current = doc.as_ref().unwrap_or(canon);
        let (g, d) = read(current);
        let m = Model::new(g, d);
        cut.allowed(&m, at)?;
        cut.declare(expected, &m);
        let mut canvas = Canvas::new(g);
        cut.draw(&mut canvas, &m);
        doc = Some(Doc::new(canvas.render()));
    }
    Ok(doc.unwrap_or_else(|| Doc::new(canon.text())))
}

/// The wires lying wholly inside `region`, as `Structure` parts.
fn inside(grid: &Grid, diagram: &Diagram, region: Rect) -> Vec<shawline_shape::structure::WirePart> {
    diagram
        .wires
        .iter()
        .enumerate()
        .filter(|(_, w)| w.cells.iter().all(|&p| region.contains(p)))
        .map(|(w, _)| Structure::wire(grid, diagram, w))
        .collect()
}

/// The branch ending at arrowhead port `p`, for a cut. A cut names it by
/// its term only, so its scope is left out.
fn branch_into(p: &shawline_shape::Port) -> Cut {
    Cut::Branch(Sink { at: p.pos, dir: p.dir, scope: (0, 0), free: false })
}

/// The cuts that free switch `b`'s port `p` from its wires outside the
/// switch: the branch reaching a selector or a left through-port, and a
/// right through-port's stub of its merged output. A definition's ports
/// have no wires outside it.
fn outer_cuts(model: &Model, b: usize, p: &shawline_shape::Port) -> Vec<Cut> {
    if model.kind(b) == BoxKind::Definition || p.kind != PortKind::Arrow {
        return Vec::new();
    }
    match p.side {
        Dir::N | Dir::W => vec![branch_into(p)],
        Dir::E => vec![Cut::Stub(p.pos)],
        Dir::S => Vec::new(),
    }
}

// ---------------------------------------------------------------------------
// Ports.

pub(crate) fn remove_port(doc: &Doc, at: Pos) -> Result<Edited, Refusal> {
    let refuse = |kind, why: &str| Refusal::new(at, kind, why);
    let sheets = Sheets::read(doc, at)?;
    let none = || refuse(RefusalKind::Target, "there is no port here");
    let (gb, k) = shawline_shape::structure::port_index(&sheets.given().1.boxes, at).ok_or_else(none)?;
    let (grid, diagram) = sheets.canon();
    let model = Model::new(grid, diagram);
    let (b, port) = (gb, diagram.boxes[gb].ports[k]);
    let kind = model.kind(b);
    if kind == BoxKind::Node && !model.switch_like(b) {
        return remove_node_port(&sheets, &model, b, k, at);
    }
    let mut cuts = Vec::new();
    // The name to blank: a definition port's.
    let mut name: Option<usize> = None;
    match (kind, port.side) {
        (BoxKind::Definition, Dir::W) => {
            name = crate::sheet::across(grid, &model.texts, port.pos, Dir::E).1;
            if let Some(t) = name {
                cuts.push(Cut::Wire(Term::Text(diagram.texts[t].pos, Dir::W)));
            }
        }
        (BoxKind::Definition, _) => {
            name = crate::sheet::across(grid, &model.texts, port.pos, Dir::E).1;
            cuts.push(branch_into(&port));
        }
        (_, Dir::N) => cuts.extend(outer_cuts(&model, b, &port)),
        (_, Dir::W) => {
            cuts.extend(outer_cuts(&model, b, &port));
            cuts.push(Cut::Wire(Term::Port(port.pos, Dir::W)));
        }
        (_, _) => {
            cuts.push(branch_into(&port));
            cuts.extend(outer_cuts(&model, b, &port));
        }
    }
    let mut expected = Structure::of(grid, diagram);
    let cut = cut_all(sheets.doc(), &cuts, &mut expected, at)?;
    let (g, _) = read(&cut);
    let mut lines = rows(g);
    let line = if port.side.is_horizontal() { '│' } else { '─' };
    put(&mut lines, port.pos, line);
    if let Some(t) = name {
        for p in diagram.texts[t].cells() {
            put(&mut lines, p, ' ');
        }
    }
    // The port goes from its box, and the ports after it move up one.
    expected.boxes[b].ports.remove(k);
    renumber(&mut expected, b, |j| if j > k { j - 1 } else { j });
    let what = "the port can't be removed";
    check(&sheets, render(&lines), at, what, expected, Some)
}

/// Checks a removal's result against `expected`, the canonical input's
/// structure with the removal declared in its own indices, every part
/// carried by place through `cell`.
fn check(
    sheets: &Sheets,
    text: String,
    at: Pos,
    what: &str,
    expected: Structure,
    cell: impl Fn(Pos) -> Option<Pos>,
) -> Result<Edited, Refusal> {
    settle(sheets, text, at, what, Routing::Ignored, |old, new| mapped::map(old, new, cell), |found, map, _| {
        *found = expected.relabel(map);
        Some(())
    })
}

// ---------------------------------------------------------------------------
// Arms.

pub(crate) fn remove_arm(doc: &Doc, at: Pos) -> Result<Edited, Refusal> {
    let refuse = |kind, why: &str| Refusal::new(at, kind, why);
    let sheets = Sheets::read(doc, at)?;
    let none = || refuse(RefusalKind::Target, "no divider runs through this cell");
    let given = sheets.given().1;
    let (b, i) = given
        .boxes
        .iter()
        .enumerate()
        .find_map(|(b, shape)| {
            let r = shape.rect;
            let i = shape.dividers.iter().position(|d| d.row == at.row)?;
            (r.left..=r.right).contains(&at.col).then_some((b, i))
        })
        .ok_or_else(none)?;
    let (grid, diagram) = sheets.canon();
    let model = Model::new(grid, diagram);
    let shape = &diagram.boxes[b];
    let rect = shape.rect;
    let row = shape.dividers[i].row;
    let end = shape.dividers.get(i + 1).map_or(rect.bottom, |d| d.row);
    // The arm's rows below the divider, and the divider's own row.
    let arm = Rect { top: row, left: rect.left + 1, bottom: end - 1, right: rect.right - 1 };
    let ports: Vec<(usize, shawline_shape::Port)> =
        shape.ports.iter().copied().enumerate().filter(|(_, p)| (row + 1..end).contains(&p.pos.row)).collect();
    let cuts: Vec<Cut> = ports.iter().flat_map(|(_, p)| outer_cuts(&model, b, p)).collect();
    let mut expected = Structure::of(grid, diagram);
    for part in inside(grid, diagram, arm) {
        expected.remove_part(&part);
    }
    let cut = cut_all(sheets.doc(), &cuts, &mut expected, at)?;
    let (g, _) = read(&cut);
    let mut lines = rows(g);
    for r in arm.top..=arm.bottom {
        for c in arm.left..=arm.right {
            put(&mut lines, Pos::new(r, c), ' ');
        }
    }
    put(&mut lines, Pos::new(row, rect.left), '│');
    put(&mut lines, Pos::new(row, rect.right), '│');
    for &(_, p) in &ports {
        put(&mut lines, p.pos, '│');
    }
    expected.boxes[b].dividers.remove(i);
    let gone: Vec<usize> = ports.iter().map(|&(k, _)| k).collect();
    for &k in gone.iter().rev() {
        expected.boxes[b].ports.remove(k);
    }
    renumber(&mut expected, b, |j| j - gone.iter().filter(|&&k| k < j).count());
    let what = "the arm can't be removed";
    let kept = move |p: Pos| (!arm.contains(p)).then_some(p);
    check(&sheets, render(&lines), at, what, expected, kept)
}

// ---------------------------------------------------------------------------
// Boxes.

pub(crate) fn remove_box(doc: &Doc, corner: Pos) -> Result<Edited, Refusal> {
    let refuse = |kind, why: &str| Refusal::new(corner, kind, why);
    let sheets = Sheets::read(doc, corner)?;
    let (g, d) = sheets.given();
    let b = Model::new(g, d)
        .box_at(corner)
        .ok_or_else(|| refuse(RefusalKind::Target, "there is no box with its top-left corner here"))?;
    let (grid, diagram) = sheets.canon();
    let model = Model::new(grid, diagram);
    let kind = model.kind(b);
    if kind == BoxKind::Node && !model.switch_like(b) {
        return crate::remove_node(doc, corner);
    }
    let shape = &diagram.boxes[b];
    let rect = shape.rect;
    let cuts: Vec<Cut> = shape.ports.iter().flat_map(|p| outer_cuts(&model, b, p)).collect();
    let mut expected = Structure::of(grid, diagram);
    for part in inside(grid, diagram, rect) {
        expected.remove_part(&part);
    }
    let cut = cut_all(sheets.doc(), &cuts, &mut expected, corner)?;
    let (g, _) = read(&cut);
    let mut lines = rows(g);
    for r in rect.top..=rect.bottom {
        for c in rect.left..=rect.right {
            put(&mut lines, Pos::new(r, c), ' ');
        }
    }
    let what = "the box can't be removed";
    if kind != BoxKind::Definition {
        let kept = move |p: Pos| (!rect.contains(p)).then_some(p);
        return check(&sheets, render(&lines), corner, what, expected, kept);
    }
    // A definition's output names go with it, and its rows close up.
    for p in &shape.ports {
        if p.side == Dir::E
            && let Some(t) = crate::sheet::across(grid, &model.texts, p.pos, Dir::E).1
        {
            for c in diagram.texts[t].cells() {
                put(&mut lines, c, ' ');
            }
        }
    }
    let spans = items(grid, diagram);
    let below = spans.iter().any(|&(first, _)| first > rect.bottom);
    let above = spans.iter().any(|&(_, last)| last < rect.top);
    let (from, to) = match (below, above) {
        (true, _) => (rect.top, rect.bottom + 1),
        (false, true) => (rect.top - 1, rect.bottom),
        (false, false) => (rect.top, rect.bottom),
    };
    let count = to - from + 1;
    lines.drain(from..=to.min(lines.len().saturating_sub(1)));
    let moved = move |p: Pos| match p.row {
        r if r < from => Some(p),
        r if r <= to => None,
        r => Some(Pos::new(r - count, p.col)),
    };
    check(&sheets, render(&lines), corner, what, expected, moved)
}

// ---------------------------------------------------------------------------
// A node box redrawn.

pub(crate) fn set_ports(doc: &Doc, corner: Pos, inputs: u16, outputs: u16) -> Result<Edited, Refusal> {
    let refuse = |kind, why: &str| Refusal::new(corner, kind, why);
    if inputs > MAX_PORTS || outputs > MAX_PORTS {
        return Err(refuse(RefusalKind::Target, "that's more ports than a node box can hold"));
    }
    let sheets = Sheets::read(doc, corner)?;
    let (g, d) = sheets.given();
    let not = || refuse(RefusalKind::Target, "there is no node box with its top-left corner here");
    let b = Model::new(g, d).box_at(corner).ok_or_else(not)?;
    let (grid, diagram) = sheets.canon();
    let model = Model::new(grid, diagram);
    if model.kind(b) != BoxKind::Node || model.switch_like(b) {
        return Err(not());
    }
    let ports = &diagram.boxes[b].ports;
    let side = |s: Dir| -> Vec<Pos> { ports.iter().filter(|p| p.side == s).map(|p| p.pos).collect() };
    let (ins, outs) = (side(Dir::W), side(Dir::E));
    let dropped: Vec<Pos> =
        ins.iter().skip(inputs.into()).chain(outs.iter().skip(outputs.into())).copied().collect();
    let keep = |list: &[Pos], n: u16| list.iter().take(n.into()).enumerate().map(|(k, &p)| (p, k)).collect::<Vec<_>>();
    Rebox { b, word: None, inputs: keep(&ins, inputs), outputs: keep(&outs, outputs), counts: (inputs, outputs), dropped }
        .run(&sheets, &model, corner)
}

/// A node's port removed: the ports after it on that side move up a row,
/// with their wires.
fn remove_node_port(sheets: &Sheets, model: &Model, b: usize, k: usize, at: Pos) -> Result<Edited, Refusal> {
    let ports = &model.diagram.boxes[b].ports;
    let port = ports[k];
    let side = |s: Dir| -> Vec<Pos> { ports.iter().filter(|p| p.side == s).map(|p| p.pos).collect() };
    let (ins, outs) = (side(Dir::W), side(Dir::E));
    let renumbered = |list: &[Pos]| -> Vec<(Pos, usize)> {
        list.iter().filter(|&&p| p != port.pos).enumerate().map(|(k, &p)| (p, k)).collect()
    };
    let (inputs, outputs) = (renumbered(&ins), renumbered(&outs));
    let counts = (inputs.len() as u16, outputs.len() as u16);
    Rebox { b, word: None, inputs, outputs, counts, dropped: vec![port.pos] }.run(sheets, model, at)
}

/// A node box redrawn as `place_node` draws it, its corner kept: each kept
/// port at its new index on its side, with its wire re-routed if it moves,
/// and each dropped port's wire cut.
pub(crate) struct Rebox {
    pub b: usize,
    /// The new word, for a rename.
    pub word: Option<String>,
    /// Each kept input's cell, and its index among the new inputs.
    pub inputs: Vec<(Pos, usize)>,
    pub outputs: Vec<(Pos, usize)>,
    pub counts: (u16, u16),
    pub dropped: Vec<Pos>,
}

impl Rebox {
    pub(crate) fn run(&self, sheets: &Sheets, model: &Model, at: Pos) -> Result<Edited, Refusal> {
        let (grid, diagram) = (model.grid, model.diagram);
        let b = self.b;
        let shape = &diagram.boxes[b];
        let rect = shape.rect;
        let corner = Pos::new(rect.top, rect.left);
        let t = model.word(b).ok_or_else(|| Refusal::new(at, RefusalKind::Target, "this box holds no word"))?;
        let old_word = diagram.texts[t].content.clone();
        let word = self.word.clone().unwrap_or_else(|| old_word.clone());
        let (inputs, outputs) = self.counts;
        let new_rect = node_rect(corner, &word, inputs, outputs);
        let row = |k: usize| corner.row + 1 + k;

        let cuts: Vec<Cut> = self
            .dropped
            .iter()
            .map(|&p| {
                let port = shape.ports.iter().find(|q| q.pos == p).expect("a dropped port is the box's");
                match port.kind {
                    PortKind::Arrow => branch_into(port),
                    PortKind::Tee => Cut::Wire(Term::Port(p, Dir::W)),
                }
            })
            .collect();
        let mut expected = Structure::of(grid, diagram);
        let cut = cut_all(sheets.doc(), &cuts, &mut expected, at)?;
        let (cut_grid, cut_diagram) = read(&cut);

        // Where each kept port goes, and its new index among all ports.
        let mut moves: HashMap<Pos, Pos> = HashMap::new();
        for &(p, k) in &self.inputs {
            moves.insert(p, Pos::new(row(k), new_rect.left));
        }
        for &(p, k) in &self.outputs {
            moves.insert(p, Pos::new(row(k), new_rect.right));
        }
        let mut new_ports: Vec<(Pos, PortPart)> = Vec::new();
        for k in 0..usize::from(inputs.max(outputs)) {
            if k < usize::from(inputs) {
                let meets = if k == 0 { Across::Text(t) } else { Across::Stroke };
                let part = PortPart { side: Dir::W, kind: PortKind::Arrow, dir: Dir::E, meets };
                new_ports.push((Pos::new(row(k), new_rect.left), part));
            }
            if k < usize::from(outputs) {
                let part = PortPart { side: Dir::E, kind: PortKind::Tee, dir: Dir::E, meets: Across::Nothing };
                new_ports.push((Pos::new(row(k), new_rect.right), part));
            }
        }
        // Each kept port's index among the old ports, to its index among
        // the new.
        let index: HashMap<usize, usize> = shape
            .ports
            .iter()
            .enumerate()
            .filter_map(|(k, p)| {
                let to = moves.get(&p.pos)?;
                Some((k, new_ports.iter().position(|(q, _)| q == to)?))
            })
            .collect();
        renumber(&mut expected, b, |k| index.get(&k).copied().unwrap_or(k));
        expected.boxes[b].ports = new_ports.iter().map(|&(_, part)| part).collect();
        if self.word.is_some() {
            expected.texts[t].content.clone_from(&word);
            expected.texts[t].kind = TextKind::Word;
        }

        // The box as `place_node` draws it.
        let width = new_rect.right - new_rect.left;
        let mut cells: Vec<(Pos, char)> = Vec::new();
        for (i, r) in (new_rect.top..=new_rect.bottom).enumerate() {
            let line: String = if r == new_rect.top {
                format!("┌{}┐", "─".repeat(width - 1))
            } else if r == new_rect.bottom {
                format!("└{}┘", "─".repeat(width - 1))
            } else {
                let k = i - 1;
                let left = if k < usize::from(inputs) { '▶' } else { '│' };
                let right = if k < usize::from(outputs) { '├' } else { '│' };
                let inside = if k == 0 { format!(" {word} ") } else { " ".repeat(width - 1) };
                format!("{left}{inside}{right}")
            };
            cells.extend(line.chars().enumerate().map(|(c, ch)| (Pos::new(r, new_rect.left + c), ch)));
        }
        let text = crate::mover::reshape(cut_grid, cut_diagram, b, new_rect, &cells, &moves)
            .map_err(|r| Refusal { at, ..r })?;

        let word_at = Pos::new(corner.row + 1, corner.col + 2);
        let routing = if cuts.is_empty() { Routing::Rerouted } else { Routing::Ignored };
        let what = "the node can't be redrawn so";
        let map = move |old: Read, new: Read| {
            let mut map = Mapping::by_place(
                old.1,
                new.1,
                |i, r| if i == b { new_rect } else { r },
                |i, p| if i == t { word_at } else { p },
            );
            map.wires = mapped::wires_by_terms(old, new, &map);
            map
        };
        settle(sheets, text, at, what, routing, map, |found, map, _| {
            *found = expected.relabel(map);
            Some(())
        })
    }
}

/// Renames node box `b` to `word`, redrawn at `place_node`'s width with its
/// ports kept: the wires at its right border follow it.
pub(crate) fn rename(sheets: &Sheets, model: &Model, b: usize, word: &str, at: Pos) -> Result<Edited, Refusal> {
    if !is_word(word) {
        return Err(Refusal::new(at, RefusalKind::Target, "a node's word is one Shavian word"));
    }
    let ports = &model.diagram.boxes[b].ports;
    let side = |s: Dir| -> Vec<(Pos, usize)> {
        ports.iter().filter(|p| p.side == s).enumerate().map(|(k, p)| (p.pos, k)).collect()
    };
    let (inputs, outputs) = (side(Dir::W), side(Dir::E));
    let counts = (inputs.len() as u16, outputs.len() as u16);
    Rebox { b, word: Some(word.to_owned()), inputs, outputs, counts, dropped: Vec::new() }.run(sheets, model, at)
}
