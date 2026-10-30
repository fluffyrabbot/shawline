//! Studio Phase 1's edits (PLAN P7): connect, remove a wire, disconnect a
//! branch, place and remove nodes, and the palette. Since P10 wave 2 they
//! reach merged outputs ([`crate::merge`]) and inline nodes, whose wires
//! stay while the node does.
//!
//! Each edit draws on a copy of the canonical sheet, formats the result and
//! recognizes it again. What it recognizes to must equal the input's
//! [`Structure`] with exactly the change the edit declares, or the edit is
//! refused and the sheet stays as it was.
//!
//! The caller's cells name the text as given, which may not be canonical,
//! so each target is resolved there and carried over ([`Sheets`]).
//! Refusals point at the caller's cells.

use std::collections::HashSet;

use shawline_doc::Doc;
use shawline_grid::{Dir, Grid, Pos, Rect};
use shawline_shape::stroke::Kind;
use shawline_shape::structure::{
    Across, BoxPart, Mapping, Part, PortPart, Routing, Structure, TextPart,
};
use shawline_shape::{BoxStyle, Diagram, EndKind, PortKind, TextKind};
use shawline_vm::bytecode::Prim;

use crate::canon::{Sheets, tidy};
use crate::BoxKind;
use crate::model::{Model, NIL, Sink, Source, Term, no_sink, no_source};
use crate::router::{self, BEND, BUDGET, CLEARANCE, Cell, Found, Miss, Start, TEE, Terrain, toward};
use crate::sheet::{glyph, keep_clear};
use crate::{Blockage, Candidate, Placed, Refusal, RefusalKind};

/// The most ports `place_node` draws on one side.
const MAX_PORTS: u16 = 16;

/// The sheet's characters, edited in place.
pub(crate) struct Canvas {
    pub rows: Vec<Vec<char>>,
}

impl Canvas {
    pub(crate) fn new(grid: &Grid) -> Self {
        Self { rows: (0..grid.height()).map(|r| grid.row(r).to_vec()).collect() }
    }

    pub(crate) fn get(&self, p: Pos) -> char {
        self.rows.get(p.row).and_then(|r| r.get(p.col)).copied().unwrap_or(' ')
    }

    pub(crate) fn set(&mut self, p: Pos, ch: char) {
        if self.rows.len() <= p.row {
            self.rows.resize(p.row + 1, Vec::new());
        }
        let row = &mut self.rows[p.row];
        if row.len() <= p.col {
            row.resize(p.col + 1, ' ');
        }
        row[p.col] = ch;
    }

    pub(crate) fn render(&self) -> String {
        let mut out = String::new();
        for row in &self.rows {
            let line: String = row.iter().collect();
            out.push_str(line.trim_end());
            out.push('\n');
        }
        out
    }
}

/// A refusal about the canonical sheet, pointed at the caller's cell `at`.
pub(crate) fn here(at: Pos) -> impl Fn(Refusal) -> Refusal {
    move |r| Refusal { at, ..r }
}

/// Formats `text` and checks it recognizes to what the edit declares:
/// `expected`, given where each box and text of `old` went. Nothing moves in
/// a wiring edit, so they're found where they were, and a stray move leaves
/// one unfound. Gives the revision the edit returns, already read.
pub(crate) fn finish(
    text: String,
    old: &Diagram,
    at: Pos,
    what: &str,
    expected: impl FnOnce(&Mapping) -> Structure,
) -> Result<Doc, Refusal> {
    let refuse = |why: &str| Refusal::new(at, RefusalKind::Changed, format!("{what}: {why}"));
    let doc = tidy(Doc::new(text)).map_err(refuse)?;
    let (grid, diagram) = read(&doc);
    let map = Mapping::by_place(old, diagram, |_, r| r, |_, p| p);
    let found = Structure::of(grid, diagram);
    expected(&map).expect(&found, Routing::Ignored).map_err(|part| {
        refuse(match part {
            Part::Boxes => "it would disturb a box; try another route or spot",
            Part::Texts => "it would disturb a text; try another route or spot",
            Part::Wires => "it would change what another wire connects; try another route or spot",
            Part::Codes => "it would change what the program means; try another route or spot",
        })
    })?;
    Ok(doc)
}

/// The grid and diagram of a text an edit wrote.
pub(crate) fn read(doc: &Doc) -> (&Grid, &Diagram) {
    let grid = doc.grid().expect("edits write only repertoire characters");
    (grid, doc.diagram().expect("a sheet on the grid is recognized"))
}

// ---------------------------------------------------------------------------
// Removing strokes.

/// Erases wire cell `p`, whose own connections are `mask`. A crossing keeps
/// the axis that isn't `mask`'s, if any: another wire's line, or another
/// branch of a wire crossing itself.
pub(crate) fn erase(canvas: &mut Canvas, model: &Model, p: Pos, mask: u8) {
    let crossing = model.diagram.strokes.get(p).filter(|s| s.kind == Kind::Crossing);
    canvas.set(p, crossing.map_or(' ', |s| glyph(s.conn & !mask)));
}

pub(crate) fn erase_wire(canvas: &mut Canvas, model: &Model, w: usize) {
    let wire = &model.diagram.wires[w];
    for (&p, &mask) in wire.cells.iter().zip(&wire.links) {
        erase(canvas, model, p, mask);
    }
}

/// The run of a wire from one end back to the tee it leaves from.
struct Branch {
    /// From the end inward, each with its own connections.
    cells: Vec<(Pos, u8)>,
    /// The tee, and the direction from it into the branch. `None` when the
    /// walk reached the wire's other end: the wire is just this branch.
    tee: Option<(Pos, Dir)>,
}

/// Walks wire `w` from its end at `start`, which faces `facing`.
fn branch(model: &Model, w: usize, start: Pos, facing: Dir) -> Branch {
    let d = model.diagram;
    let cells: HashSet<Pos> = d.wires[w].cells.iter().copied().collect();
    let (mut p, mut from) = (start, facing);
    let mut out = Vec::new();
    // A wire is a tree, so the walk ends; the bound only guards a bad one.
    for _ in 0..=cells.len() {
        let Some(s) = d.strokes.get(p) else { break };
        if s.kind == Kind::Tee {
            return Branch { cells: out, tee: Some((p, from)) };
        }
        let mask =
            if s.kind == Kind::Crossing { from.bit() | from.opposite().bit() } else { s.conn };
        out.push((p, mask));
        let onward: Vec<Dir> = Dir::ALL
            .into_iter()
            .filter(|&dir| dir != from && mask & dir.bit() != 0)
            .filter(|&dir| {
                p.step(dir).is_some_and(|q| {
                    cells.contains(&q)
                        && !model.borders.contains(&q)
                        && d.strokes.get(q).is_some_and(|s| s.connects(dir.opposite()))
                })
            })
            .collect();
        match onward[..] {
            [dir] => {
                p = p.step(dir).expect("joined cells exist");
                from = dir.opposite();
            }
            _ => break,
        }
    }
    Branch { cells: out, tee: None }
}

/// Removes the branch of wire `w` that reaches `sink`, or the whole wire
/// when it has no other branch.
pub(crate) fn cut_branch(canvas: &mut Canvas, model: &Model, w: usize, sink: &Sink) {
    let end = model.diagram.wires[w]
        .ends
        .iter()
        .find(|e| match e.kind {
            EndKind::Port(p) => p == sink.at && e.facing == sink.dir && !sink.free,
            EndKind::Arrow => e.pos == sink.at && sink.free,
            _ => false,
        })
        .expect("the sink's wire ends at it");
    cut_end(canvas, model, w, end);
}

/// Removes the run of wire `w` from its end `end` back to the tee it
/// leaves from, or the whole wire when the run reaches its other end.
pub(crate) fn cut_end(canvas: &mut Canvas, model: &Model, w: usize, end: &shawline_shape::WireEnd) {
    let found = branch(model, w, end.pos, end.facing);
    match found.tee {
        Some((tee, into)) => {
            for &(p, mask) in &found.cells {
                erase(canvas, model, p, mask);
            }
            let mask = model.diagram.strokes.get(tee).map_or(0, |s| s.conn) & !into.bit();
            canvas.set(tee, glyph(mask));
        }
        None => erase_wire(canvas, model, w),
    }
}

pub(crate) fn remove_wire(doc: &Doc, at: Pos) -> Result<Doc, Refusal> {
    let sheets = Sheets::read(doc, at)?;
    let (g, d) = sheets.given();
    let given = Model::new(g, d);
    let through: Vec<usize> =
        (0..d.wires.len()).filter(|&w| d.wires[w].cells.contains(&at)).collect();
    let Some(&first) = through.first() else {
        return Err(Refusal::new(at, RefusalKind::Target, "no wire passes through this cell"));
    };
    // A wire into or out of an inline node stays while the node does.
    let strands = |w: usize| {
        let (into, from) = given.inline_ends(w);
        into.or(from)
    };
    let w = through.iter().copied().find(|&w| strands(w).is_none()).ok_or_else(|| {
        given.strands(at, strands(first).expect("every wire here meets an inline node"))
    })?;
    let (grid, diagram) = sheets.canon();
    let model = Model::new(grid, diagram);
    let w = sheets.wire(&given, &model, w, at).map_err(|why| Refusal {
        remedy: Some(crate::Remedy::Format),
        ..Refusal::new(at, RefusalKind::Unreadable, why)
    })?;
    let mut canvas = Canvas::new(grid);
    erase_wire(&mut canvas, &model, w);
    let mut expected = Structure::of(grid, diagram);
    match model.terms[w].iter().find_map(|&t| model.named(t)) {
        Some(term) => expected.remove_wire(term),
        // A wire joining only loose ends shares its terms with any
        // look-alike, so it's named whole, and just one goes.
        None => expected.remove_part(&Structure::wire(grid, diagram, w)),
    }
    finish(canvas.render(), diagram, at, "the wire can't be removed", |map| expected.relabel(map))
}

pub(crate) fn disconnect(doc: &Doc, to: Pos) -> Result<Doc, Refusal> {
    let sheets = Sheets::read(doc, to)?;
    let sink = sheets.endpoint(to).ok_or_else(|| no_sink(to))?;
    let (grid, diagram) = sheets.canon();
    let model = Model::new(grid, diagram);
    let sink = model.sink(sink).map_err(here(to))?;
    let w = model
        .wire_of(sink.term())
        .ok_or_else(|| Refusal::new(to, RefusalKind::Target, "no wire reaches this arrowhead"))?;
    let mut canvas = Canvas::new(grid);
    let mut expected = Structure::of(grid, diagram);
    let term = model.named(sink.term()).expect("a sink is a port or an inline node's input");
    cut_sink(&mut canvas, &model, &mut expected, w, &sink, term).map_err(here(to))?;
    finish(canvas.render(), diagram, to, "the wire can't be cut here", |map| expected.relabel(map))
}

/// Cuts the branch of wire `w` reaching `sink` (named `term` in
/// `expected`), and declares it. Refused where an inline node would lose
/// its input or its last output branch. A merged output's wire goes whole
/// with its last sink, stubs and all.
pub(crate) fn cut_sink(
    canvas: &mut Canvas,
    model: &Model,
    expected: &mut Structure,
    w: usize,
    sink: &Sink,
    term: shawline_shape::structure::Term,
) -> Result<(), Refusal> {
    if let Some(t) = model.inline_after(sink.at).filter(|_| sink.free) {
        return Err(model.strands(sink.at, t));
    }
    let last = model.sink_count(w) <= 1;
    if last && let Some(t) = model.inline_ends(w).1 {
        return Err(model.strands(sink.at, t));
    }
    if last && model.source_count(w) > 1 {
        erase_wire(canvas, model, w);
        expected.remove_wire(term);
    } else {
        cut_branch(canvas, model, w, sink);
        expected.disconnect(term);
    }
    Ok(())
}

pub(crate) fn remove_node(doc: &Doc, corner: Pos) -> Result<Doc, Refusal> {
    let sheets = Sheets::read(doc, corner)?;
    let (g, d) = sheets.given();
    let given = Model::new(g, d);
    if let Some(t) = given.text_at(corner).filter(|&t| d.texts[t].pos == corner && given.inline_word(t)) {
        return crate::inline::splice(&sheets, t, corner);
    }
    let b = given.box_at(corner).ok_or_else(|| {
        Refusal::new(corner, RefusalKind::Target, "there is no box with its top-left corner here")
    })?;
    let (grid, diagram) = sheets.canon();
    let model = Model::new(grid, diagram);
    if model.kind(b) != BoxKind::Node {
        let why = "only a node can be deleted this way; a definition or switch is removed as a whole box";
        return Err(Refusal::new(corner, RefusalKind::Target, why));
    }
    let shape = &diagram.boxes[b];
    let inputs: Vec<Sink> = shape.ports.iter().filter_map(|p| model.sink(p.pos).ok()).collect();
    let outputs: Vec<Term> = shape
        .ports
        .iter()
        .filter(|p| p.kind == PortKind::Tee)
        .map(|p| Term::Port(p.pos, Dir::W))
        .collect();
    // An inline node keeps its wires: a node whose input is an inline
    // node's last branch, or whose output feeds one, stays.
    let fed: Vec<usize> = inputs.iter().filter_map(|s| model.wire_of(s.term())).collect();
    for &w in &fed {
        let here = fed.iter().filter(|&&v| v == w).count();
        if model.sink_count(w) <= here
            && let Some(t) = model.inline_ends(w).1
        {
            return Err(model.strands(corner, t));
        }
    }
    for w in outputs.iter().filter_map(|&t| model.wire_of(t)) {
        if let Some(t) = model.inline_ends(w).0 {
            return Err(model.strands(corner, t));
        }
    }

    // One wire at a time, reading the sheet again after each: two inputs
    // fed by one fan-out leave the tee a corner, then take it with them.
    // `None` until the first cut: the canonical sheet as read.
    let mut cut: Option<Doc> = None;
    for sink in &inputs {
        let (g, d) = cut.as_ref().map_or((grid, diagram), read);
        let m = Model::new(g, d);
        if let Some(w) = m.wire_of(sink.term()) {
            let mut canvas = Canvas::new(g);
            // A merged output goes whole with its last sink.
            let mine = inputs.iter().filter(|s| m.wire_of(s.term()) == Some(w)).count();
            if m.sink_count(w) <= mine && m.source_count(w) > 1 {
                erase_wire(&mut canvas, &m, w);
            } else {
                cut_branch(&mut canvas, &m, w, sink);
            }
            cut = Some(Doc::new(canvas.render()));
        }
    }
    for &term in &outputs {
        let (g, d) = cut.as_ref().map_or((grid, diagram), read);
        let m = Model::new(g, d);
        if let Some(w) = m.wire_of(term) {
            let mut canvas = Canvas::new(g);
            erase_wire(&mut canvas, &m, w);
            cut = Some(Doc::new(canvas.render()));
        }
    }
    let mut canvas = Canvas::new(cut.as_ref().map_or(grid, |doc| read(doc).0));
    let r = shape.rect;
    for row in r.top..=r.bottom {
        for col in r.left..=r.right {
            canvas.set(Pos::new(row, col), ' ');
        }
    }
    // Its outputs' wires go. The node and its word are left out of the
    // mapping, which takes its inputs off their wires.
    let mut expected = Structure::of(grid, diagram);
    for term in outputs.iter().filter_map(|&t| model.named(t)) {
        expected.remove_wire(term);
    }
    for &w in &fed {
        let here = fed.iter().filter(|&&v| v == w).count();
        if model.sink_count(w) <= here && model.source_count(w) > 1 {
            let source = model.terms[w].iter().find(|t| matches!(t, Term::Port(_, Dir::W)));
            if let Some(term) = source.and_then(|&t| model.named(t)) {
                expected.remove_wire(term);
            }
        }
    }
    let what = "the node can't be deleted";
    finish(canvas.render(), diagram, corner, what, |map| expected.relabel(map))
}

// ---------------------------------------------------------------------------
// Placing nodes.

/// The rectangle of a canonical node box for `word` (SPEC §6, as the corpus
/// draws them): one space either side of the word, which sits on the first
/// port row, and one row per port.
pub(crate) fn node_rect(corner: Pos, word: &str, inputs: u16, outputs: u16) -> Rect {
    let rows = inputs.max(outputs).max(1) as usize;
    Rect {
        top: corner.row,
        left: corner.col,
        bottom: corner.row + rows + 1,
        right: corner.col + word.chars().count() + 3,
    }
}

pub(crate) fn place_node(
    doc: &Doc,
    corner: Pos,
    word: &str,
    inputs: u16,
    outputs: u16,
) -> Result<Placed, Refusal> {
    let refuse = |kind, why: &str| Refusal::new(corner, kind, why);
    if word.is_empty() || !word.chars().all(|c| matches!(c, '\u{10450}'..='\u{1047F}')) {
        return Err(refuse(RefusalKind::Unsupported, "a node's word is written in Shavian letters"));
    }
    if inputs > MAX_PORTS || outputs > MAX_PORTS {
        return Err(refuse(RefusalKind::Unsupported, "that's more ports than a node box can hold"));
    }
    let sheets = Sheets::read(doc, corner)?;
    // The corner keeps its place in the box around it, so the node lands
    // where the caller pointed even when formatting shifts the rows.
    let Some(at) = sheets.place(corner) else {
        return Err(refuse(RefusalKind::Target, "a node must sit wholly inside a definition"));
    };
    let (grid, diagram) = sheets.canon();
    let model = Model::new(grid, diagram);
    let rect = node_rect(at, word, inputs, outputs);

    // The innermost box whose interior holds the whole node.
    let holder = (0..diagram.boxes.len())
        .filter(|&i| {
            let r = diagram.boxes[i].rect;
            r.top < rect.top && r.left < rect.left && rect.bottom < r.bottom && rect.right < r.right
        })
        .min_by_key(|&i| diagram.boxes[i].rect.area());
    // The corner is in a box, so the node reaches past a definition's
    // border.
    let Some(holder) = holder else {
        let why = "the node would stick out of its definition; try Make room or a spot further in";
        return Err(refuse(RefusalKind::Blocked(Blockage::NoRoom), why));
    };
    match model.kind(holder) {
        BoxKind::Node => {
            return Err(refuse(RefusalKind::Target, "a node can't go inside another node"));
        }
        BoxKind::Switch => {
            let dividers = &diagram.boxes[holder].dividers;
            if dividers.iter().any(|d| (rect.top..=rect.bottom).contains(&d.row)) {
                let why = "the node would cross from one arm of the switch into another; try Make room";
                return Err(refuse(RefusalKind::Blocked(Blockage::NoRoom), why));
            }
        }
        BoxKind::Definition => {}
    }
    if diagram.boxes.iter().any(|b| b.rect.intersects(&rect)
        && !b.rect.encloses(&rect)
        && b.rect != diagram.boxes[holder].rect)
    {
        let why = "the node would overlap another box; find a clear spot";
        return Err(refuse(RefusalKind::Blocked(Blockage::NoRoom), why));
    }
    // Clear in the text the caller sees as well as in the canonical one,
    // where formatting may have moved text or wire out of the way.
    let shown = node_rect(corner, word, inputs, outputs);
    for (sheet, r) in [(grid, rect), (sheets.given().0, shown)] {
        for row in r.top..=r.bottom {
            for col in r.left..=r.right {
                if sheet.get(Pos::new(row, col)) != ' ' {
                    let why = "the node would land on something; find a clear spot";
                    return Err(refuse(RefusalKind::Blocked(Blockage::NoRoom), why));
                }
            }
        }
    }

    let mut canvas = Canvas::new(grid);
    let width = rect.right - rect.left;
    for (i, row) in (rect.top..=rect.bottom).enumerate() {
        let line: String = if row == rect.top {
            format!("┌{}┐", "─".repeat(width - 1))
        } else if row == rect.bottom {
            format!("└{}┘", "─".repeat(width - 1))
        } else {
            let port = i - 1;
            let left = if port < inputs as usize { '▶' } else { '│' };
            let right = if port < outputs as usize { '├' } else { '│' };
            let inside =
                if port == 0 { format!(" {word} ") } else { " ".repeat(width - 1) };
            format!("{left}{inside}{right}")
        };
        for (k, ch) in line.chars().enumerate() {
            canvas.set(Pos::new(row, rect.left + k), ch);
        }
    }

    // The check below holds the box to exactly these cells, so they name
    // its ports in the result too.
    let row = |k: usize| rect.top + 1 + k;
    let arrows: Vec<Pos> = (0..inputs as usize).map(|k| Pos::new(row(k), rect.left)).collect();
    let tees: Vec<Pos> = (0..outputs as usize).map(|k| Pos::new(row(k), rect.right)).collect();
    // The new box and its word take their places in row-major order.
    let corner_of = |r: Rect| (r.top, r.left);
    let index = diagram.boxes.iter().filter(|b| corner_of(b.rect) < corner_of(rect)).count();
    let word_at = Pos::new(rect.top + 1, rect.left + 2);
    let text = diagram.texts.iter().filter(|t| t.pos < word_at).count();
    let mut ports = Vec::new();
    for k in 0..usize::from(inputs.max(outputs)) {
        if k < usize::from(inputs) {
            // The first input points at the word, the others across the box.
            let meets = if k == 0 { Across::Text(text) } else { Across::Stroke };
            ports.push(PortPart { side: Dir::W, kind: PortKind::Arrow, dir: Dir::E, meets });
        }
        if k < usize::from(outputs) {
            let meets = Across::Nothing;
            ports.push(PortPart { side: Dir::E, kind: PortKind::Tee, dir: Dir::E, meets });
        }
    }
    let named = TextPart {
        kind: TextKind::Word,
        content: word.to_owned(),
        owner: Some(index),
        title_of: None,
        divider_of: None,
    };
    let old = Structure::of(grid, diagram);
    let expected = |map: &Mapping| {
        let mut expected = old.relabel(map);
        let node = BoxPart {
            style: BoxStyle::Square,
            titled: false,
            slips: 0,
            parent: map.boxes[holder],
            ports,
            dividers: Vec::new(),
        };
        // A stray move leaves fewer, and the comparison fails on that.
        expected.boxes.insert(index.min(expected.boxes.len()), node);
        expected.texts.insert(text.min(expected.texts.len()), named);
        expected
    };
    let doc = finish(canvas.render(), diagram, corner, "the node can't go there", expected)?;
    // `crate::place_node` maps it.
    let map = crate::Mapping { boxes: Vec::new(), texts: Vec::new(), wires: Vec::new() };
    Ok(Placed { doc, map, tidied: false, inputs: arrows, outputs: tees })
}

// ---------------------------------------------------------------------------
// Connecting.

/// The router's view while drawing a wire from a sink back to its source.
struct Wiring<'a> {
    model: &'a Model<'a>,
    source: &'a Source,
    /// The source's wire, when it has one: the route joins it with a tee.
    wire: Option<usize>,
    own: HashSet<Pos>,
    region: Rect,
    /// Cells no route may take.
    blocked: HashSet<Pos>,
    /// Every wire's end cells: never crossed or joined.
    ends: HashSet<Pos>,
    /// The line cell before each inline node's arrowhead: never crossed or
    /// joined, so the straight stretch through the node stays straight.
    stretch: HashSet<Pos>,
    /// The cell before the sink's arrowhead, if the route starts at a sink.
    target: Option<Pos>,
}

impl<'a> Wiring<'a> {
    fn new(model: &'a Model<'a>, source: &'a Source, sink: Option<&Sink>) -> Self {
        let d = model.diagram;
        let wire = model.wire_of(source.term);
        let own: HashSet<Pos> =
            wire.map(|w| d.wires[w].cells.iter().copied().collect()).unwrap_or_default();
        let region = model.region(source.scope);
        let mut blocked = model.borders.clone();
        let ancestors: HashSet<usize> =
            std::iter::successors(Some(source.scope.0), |&b| d.boxes[b].parent).collect();
        for (i, b) in d.boxes.iter().enumerate() {
            let r = b.rect;
            if ancestors.contains(&i) || !r.intersects(&region) {
                continue;
            }
            blocked.extend(
                (r.top..=r.bottom).flat_map(|row| (r.left..=r.right).map(move |c| Pos::new(row, c))),
            );
        }
        let sink_at = sink.map(|s| s.at);
        for (anchor, cell) in keep_clear(model.grid, d, |_, _| false) {
            if Some(anchor) != sink_at && anchor != source.goal {
                blocked.insert(cell);
            }
        }
        // Room for every other text source's wire to start later.
        for t in model.text_sources() {
            if Some(t) == source.text {
                continue;
            }
            let text = &d.texts[t];
            for k in 0..2 {
                let p = Pos::new(text.pos.row, text.pos.col + text.width + k);
                if model.grid.get(p) == ' ' {
                    blocked.insert(p);
                }
            }
        }
        let ends = d.wires.iter().flat_map(|w| w.ends.iter().map(|e| e.pos)).collect();
        let stretch = d
            .wires
            .iter()
            .flat_map(|w| &w.ends)
            .filter(|e| e.kind == EndKind::Arrow && model.inline_after(e.pos).is_some())
            .filter_map(|e| e.pos.step(Dir::W))
            .collect();
        let target = sink.and_then(Sink::target);
        Wiring { model, source, wire, own, region, blocked, ends, stretch, target }
    }

    fn joinable(&self, p: Pos, heading: Dir) -> bool {
        let Some(w) = self.wire else { return false };
        if !self.own.contains(&p) || self.ends.contains(&p) || self.stretch.contains(&p) {
            return false;
        }
        let Some(s) = self.model.diagram.strokes.get(p) else { return false };
        let back = heading.opposite().bit();
        let mask = self.model.links(w, p);
        matches!(s.kind, Kind::Line | Kind::Corner { .. })
            && mask & back == 0
            && (mask | back).count_ones() == 3
    }
}

impl Terrain for Wiring<'_> {
    fn cell(&self, p: Pos) -> Cell {
        let r = self.region;
        if p.row < r.top || p.row > r.bottom || p.col < r.left || p.col > r.right {
            return Cell::Blocked;
        }
        if self.blocked.contains(&p)
            || self.own.contains(&p)
            || self.ends.contains(&p)
            || self.stretch.contains(&p)
        {
            return Cell::Blocked;
        }
        let first = self.wire.is_none() && p == self.source.first();
        match self.model.grid.get(p) {
            ' ' => Cell::Free,
            '─' if !first => Cell::Line(true),
            '│' if !first => Cell::Line(false),
            _ => Cell::Blocked,
        }
    }

    fn extra(&self, p: Pos) -> u32 {
        let near = Dir::ALL
            .into_iter()
            .filter_map(|d| p.step(d))
            .filter(|q| !self.own.contains(q) && self.model.diagram.strokes.get(*q).is_some())
            .count();
        near as u32 * CLEARANCE
    }

    fn goal(&self, p: Pos, heading: Dir) -> Option<u32> {
        match self.wire {
            None => (p == self.source.goal && heading == Dir::W).then_some(0),
            Some(_) => self.joinable(p, heading).then_some(TEE),
        }
    }

    fn bend_extra(&self, p: Pos) -> u32 {
        // A bend right by an arrowhead or a text reads badly (lint L002).
        if Some(p) == self.target || (self.wire.is_none() && p == self.source.first()) {
            BEND
        } else {
            0
        }
    }
}

/// Draws a route found from a sink back to its source (or the source's
/// wire) onto `canvas`. `path[0]` is the sink's arrowhead, already drawn.
fn draw(canvas: &mut Canvas, wiring: &Wiring, path: &[Pos]) {
    let n = path.len();
    for k in 1..n - 1 {
        let p = path[k];
        let mask = toward(p, path[k + 1]).bit() | toward(p, path[k - 1]).bit();
        let ch = if canvas.get(p) == ' ' { glyph(mask) } else { '┼' };
        canvas.set(p, ch);
    }
    if let Some(w) = wiring.wire {
        let goal = path[n - 1];
        let mask = wiring.model.links(w, goal) | toward(goal, path[n - 2]).bit();
        canvas.set(goal, glyph(mask));
    }
}

fn search_from_sink(wiring: &Wiring, sink: &Sink) -> Result<Found, Miss> {
    let start = Start { pos: sink.at, heading: Some(sink.dir.opposite()), straight: true };
    let found = router::search(wiring, start, &mut { BUDGET })?;
    if found.path.len() < 3 { Err(Miss::NoPath) } else { Ok(found) }
}

pub(crate) fn connect(doc: &Doc, from: Pos, to: Pos) -> Result<Doc, Refusal> {
    let sheets = Sheets::read(doc, to)?;
    if sheets.entity(from).is_none()
        && let Some(extended) = extend(&sheets, from, to)
    {
        return extended;
    }
    let source = sheets.entity(from).ok_or_else(|| no_source(from))?;
    let sink = sheets.endpoint(to).ok_or_else(|| no_sink(to))?;
    let (grid, diagram) = sheets.canon();
    let model = Model::new(grid, diagram);
    let source = model.source(source).map_err(here(from))?;
    let sink = model.sink(sink).map_err(here(to))?;
    if model.wire_of(sink.term()).is_some() {
        let why = "this arrowhead already has a wire; disconnect it first";
        return Err(Refusal::new(to, RefusalKind::Target, why));
    }
    if source.scope != sink.scope {
        return Err(Refusal::new(
            to,
            RefusalKind::Target,
            format!("{} can't reach this arrowhead: they're in different boxes or arms", source.name),
        ));
    }
    if !source.merged.is_empty() && model.wire_of(source.term).is_none() {
        // The merge first, as its own wire with no sink yet; then the wire
        // from it, as for any wired source.
        let merged = crate::merge::draw(grid, &model, &source).map_err(here(from))?;
        return connect(&merged, source.goal, sink.at).map_err(here(to));
    }
    if sink.target().is_none_or(|t| grid.get(t) != ' ') {
        let why = "the cell before this arrowhead is taken; move what's there or try Make room";
        return Err(Refusal::new(to, RefusalKind::Blocked(Blockage::NoRoom), why));
    }
    let wiring = Wiring::new(&model, &source, Some(&sink));
    let found = search_from_sink(&wiring, &sink).map_err(|miss| {
        let name = &source.name;
        let why = match miss {
            Miss::NoPath => format!("no free path for a wire from {name}; try Make room or move them closer"),
            Miss::GaveUp => format!("the search for a wire from {name} gave up; move them closer"),
        };
        Refusal::new(to, miss.kind(), why)
    })?;
    let mut canvas = Canvas::new(grid);
    draw(&mut canvas, &wiring, &found.path);

    let mut expected = Structure::of(grid, diagram);
    let named = |t| model.named(t).expect("sources and sinks are ports or texts");
    expected.connect(named(source.term), named(sink.term()));
    finish(canvas.render(), diagram, to, "the wire can't go there", |map| expected.relabel(map))
}

/// The router's view while extending a dangling end to a sink: [`Wiring`]'s
/// ground, ending at the dangling end, entered from the cell it faces.
struct Extension<'a> {
    wiring: Wiring<'a>,
    end: Pos,
    facing: Dir,
}

impl Terrain for Extension<'_> {
    fn cell(&self, p: Pos) -> Cell {
        // The cell the end faces is kept clear for it, and it is the way in.
        if Some(p) == self.end.step(self.facing) && self.wiring.model.grid.get(p) == ' ' {
            return Cell::Free;
        }
        self.wiring.cell(p)
    }

    fn extra(&self, p: Pos) -> u32 {
        self.wiring.extra(p)
    }

    fn goal(&self, p: Pos, heading: Dir) -> Option<u32> {
        (p == self.end && heading == self.facing.opposite()).then_some(0)
    }

    fn bend_extra(&self, p: Pos) -> u32 {
        self.wiring.bend_extra(p)
    }
}

/// [`connect`] from a dangling end at given cell `from` (`Y003`): the wire
/// runs on from that end to sink `to`, so the end is no longer loose.
/// `None` when `from` is no dangling end, for `connect` to refuse it as it
/// refuses any other cell that isn't a source.
fn extend(sheets: &Sheets, from: Pos, to: Pos) -> Option<Result<Doc, Refusal>> {
    let (g, d) = sheets.given();
    let dangling = |d: &Diagram, g: &Grid, p: Pos| {
        let model = Model::new(g, d);
        d.wires.iter().enumerate().find_map(|(w, wire)| {
            let e = wire.ends.iter().find(|e| e.pos == p && e.kind == EndKind::Open)?;
            let loose = model.terms[w].contains(&Term::Loose(e.pos, e.facing, false));
            loose.then_some((w, *e))
        })
    };
    dangling(d, g, from)?;
    let (grid, diagram) = sheets.canon();
    let end_at = sheets.place(from).unwrap_or(from);
    let refuse = |kind, why: &str| Refusal::new(to, kind, why);
    let Some((w, end)) = dangling(diagram, grid, end_at) else {
        return Some(Err(refuse(RefusalKind::Unreadable, "format the sheet first, then connect this end")));
    };
    let model = Model::new(grid, diagram);
    let source = model.terms[w].iter().find_map(|&t| match t {
        Term::Port(p, Dir::W) | Term::Text(p, Dir::W) => model.source(p).ok().filter(|s| s.term == t),
        _ => None,
    });
    let Some(source) = source else {
        return Some(Err(refuse(RefusalKind::Target, "this wire has no source to connect from")));
    };
    let result = (|| {
        let sink = sheets.endpoint(to).ok_or_else(|| no_sink(to))?;
        let sink = model.sink(sink).map_err(here(to))?;
        if model.wire_of(sink.term()).is_some() {
            return Err(refuse(RefusalKind::Target, "this arrowhead already has a wire; disconnect it first"));
        }
        if source.scope != sink.scope {
            return Err(refuse(RefusalKind::Target, "this wire can't reach that arrowhead: they're in different boxes or arms"));
        }
        let wiring = Wiring::new(&model, &source, Some(&sink));
        let terrain = Extension { wiring, end: end.pos, facing: end.facing };
        let start = Start { pos: sink.at, heading: Some(sink.dir.opposite()), straight: true };
        let found = router::search(&terrain, start, &mut { BUDGET }).map_err(|miss| {
            Refusal::new(to, miss.kind(), "no free path from this end to the arrowhead; try Make room")
        })?;
        let mut canvas = Canvas::new(grid);
        draw(&mut canvas, &terrain.wiring, &found.path);
        let mut expected = Structure::of(grid, diagram);
        let loose = Structure::wire(grid, diagram, w)
            .terms
            .into_iter()
            .find(|t| t.facing == end.facing && matches!(t.end, shawline_shape::structure::End::Open(_)));
        let named = model.named(source.term).expect("a source is a port or a text");
        let sink_term = model.named(sink.term()).expect("a sink is a port or an inline node's input");
        if let (Some(part), Some(loose)) = (expected.wires.iter_mut().find(|p| p.terms.contains(&named)), loose)
            && let Some(k) = part.terms.iter().position(|t| *t == loose)
        {
            part.terms[k] = sink_term;
            part.terms.sort();
        }
        expected.wires.sort();
        finish(canvas.render(), diagram, to, "the wire can't go there", |map| expected.relabel(map))
    })();
    Some(result)
}

/// Routes a new branch from `sink` back to `source`'s wire, joining it
/// with a tee, and draws it on `canvas`, a copy of `model`'s sheet. `sink`
/// need not be one [`Model::sink`] names: a switch's new right
/// through-port, say, whose stub leaves eastward to join a merge. Nothing
/// is checked; the caller checks the sheet it makes.
pub(crate) fn join(canvas: &mut Canvas, model: &Model, source: &Source, sink: &Sink) -> Result<(), Refusal> {
    if model.wire_of(source.term).is_none() {
        let why = format!("{} has no wire to join", source.name);
        return Err(Refusal::new(sink.at, RefusalKind::Target, why));
    }
    let wiring = Wiring::new(model, source, Some(sink));
    let found = search_from_sink(&wiring, sink).map_err(|miss| {
        let why = format!("no free path to {}; try Make room", source.name);
        Refusal::new(sink.at, miss.kind(), why)
    })?;
    draw(canvas, &wiring, &found.path);
    Ok(())
}

/// Where a dragged wire is released: a free sink, or a blank cell.
enum Release {
    Sink(Sink),
    Cell(Pos),
}

/// The router's view while drawing a wire from a sink to a blank cell,
/// with no source yet: [`Wiring`]'s ground, ending at `to`.
struct Loose<'a> {
    wiring: Wiring<'a>,
    to: Pos,
}

impl Terrain for Loose<'_> {
    fn cell(&self, p: Pos) -> Cell {
        self.wiring.cell(p)
    }

    fn extra(&self, p: Pos) -> u32 {
        self.wiring.extra(p)
    }

    fn goal(&self, p: Pos, _heading: Dir) -> Option<u32> {
        (p == self.to).then_some(0)
    }

    fn bend_extra(&self, p: Pos) -> u32 {
        self.wiring.bend_extra(p)
    }
}

/// What [`connect`] would draw from `source` to `release`, on the canonical
/// sheet: the polyline from the source's own cell (a port, or a text's last
/// cell) or the tee joining its wire, to the sink or the blank cell.
fn polyline(model: &Model, source: &Source, release: &Release) -> Option<crate::Preview> {
    let found = match release {
        Release::Sink(sink) => {
            if model.wire_of(sink.term()).is_some() || sink.scope != source.scope {
                return None;
            }
            let wiring = Wiring::new(model, source, Some(sink));
            (search_from_sink(&wiring, sink).ok()?, wiring.wire.is_some())
        }
        Release::Cell(to) => {
            let wiring = Wiring::new(model, source, None);
            if wiring.cell(*to) != Cell::Free {
                return None;
            }
            let start = Start { pos: *to, heading: None, straight: false };
            (router::search(&wiring, start, &mut { BUDGET }).ok()?, wiring.wire.is_some())
        }
    };
    let (found, joins_wire) = found;
    let mut path: Vec<Pos> = found.path.into_iter().rev().collect();
    let own = match source.term {
        Term::Text(p, _) => {
            let width = source.text.map_or(1, |t| model.diagram.texts[t].width);
            Some(Pos::new(p.row, p.col + width - 1))
        }
        Term::Port(p, _) => Some(p),
        _ => None,
    };
    if !joins_wire && let Some(own) = own.filter(|&own| path.first() != Some(&own)) {
        path.insert(0, own);
    }
    Some(crate::Preview { path, joins_wire })
}

/// The cells, as given, of a wire dragged from `anchor` and released at
/// `to` (see [`crate::preview`]).
pub(crate) fn preview(doc: &Doc, anchor: crate::Anchor, to: Pos) -> Option<crate::Preview> {
    let at = match anchor {
        crate::Anchor::Source(p) | crate::Anchor::Sink(p) => p,
    };
    let sheets = Sheets::read(doc, at).ok()?;
    let (grid, diagram) = sheets.canon();
    let model = Model::new(grid, diagram);
    let released = |scope: (usize, usize)| -> Option<Release> {
        if let Some(sink) = sheets.endpoint(to).and_then(|p| model.sink(p).ok()) {
            return Some(Release::Sink(sink));
        }
        if sheets.given().0.get(to) != ' ' {
            return None;
        }
        let cell = sheets.place(to)?;
        model.region(scope).contains(cell).then_some(Release::Cell(cell))
    };
    let (source, release) = match anchor {
        crate::Anchor::Source(from) => {
            let source = model.source(sheets.entity(from)?).ok()?;
            let release = released(source.scope)?;
            (source, release)
        }
        crate::Anchor::Sink(sink) => {
            let sink = model.sink(sheets.endpoint(sink)?).ok()?;
            if model.wire_of(sink.term()).is_some() {
                return None;
            }
            match source_under(&sheets, &model, to) {
                Some(source) => (source, Release::Sink(sink)),
                None => {
                    if sheets.given().0.get(to) != ' ' {
                        return None;
                    }
                    let to = sheets.place(to)?;
                    if !model.region(sink.scope).contains(to) {
                        return None;
                    }
                    // No source yet: a route from the sink to the cell.
                    let stand_in = Source {
                        term: Term::Loose(to, Dir::W, false),
                        scope: sink.scope,
                        goal: to,
                        text: None,
                        name: String::new(),
                        merged: Vec::new(),
                    };
                    let terrain = Loose { wiring: Wiring::new(&model, &stand_in, Some(&sink)), to };
                    let start = Start { pos: sink.at, heading: Some(sink.dir.opposite()), straight: true };
                    let found = router::search(&terrain, start, &mut { BUDGET }).ok()?;
                    let path: Vec<Pos> = found.path.into_iter().rev().collect();
                    let back: Option<Vec<Pos>> = path.iter().map(|&p| sheets.back(sink.scope.0, p)).collect();
                    return Some(crate::Preview { path: back?, joins_wire: false });
                }
            }
        }
    };
    let found = polyline(&model, &source, &release)?;
    let path: Option<Vec<Pos>> = found.path.iter().map(|&p| sheets.back(source.scope.0, p)).collect();
    Some(crate::Preview { path: path?, ..found })
}

/// The source a drag anchored on a sink would connect to when released at
/// given cell `to`: the source there, or the source of the wire through
/// it.
fn source_under(sheets: &Sheets, model: &Model, to: Pos) -> Option<Source> {
    if let Some(source) = sheets.entity(to).and_then(|p| model.source(p).ok()) {
        return Some(source);
    }
    let cell = sheets.place(to)?;
    let w = (0..model.diagram.wires.len()).find(|&w| model.diagram.wires[w].cells.contains(&cell))?;
    model.source(model.source_cell(w)?).ok()
}

// ---------------------------------------------------------------------------
// The palette.

pub(crate) fn palette(doc: &Doc, from: Option<Pos>) -> Vec<Candidate> {
    // SPEC §9's table, row by row: each letter, then its mirror.
    let mut order: Vec<Prim> = Vec::new();
    for p in Prim::ALL {
        if !order.contains(&p) {
            order.push(p);
            order.push(p.mirror());
        }
    }
    let mut out: Vec<Candidate> = order
        .into_iter()
        .map(|p| {
            let (inputs, outputs) = p.arity();
            Candidate {
                word: p.letter().to_string(),
                inputs: inputs.into(),
                outputs: outputs.into(),
                gloss: p.gloss().to_owned(),
                mirror: Some(p.mirror().letter().to_string()),
            }
        })
        .collect();
    out.push(Candidate {
        word: NIL.to_owned(),
        inputs: 0,
        outputs: 1,
        gloss: "empty list".to_owned(),
        mirror: None,
    });
    if let Some(sheet) = doc.sheet() {
        let mut seen = HashSet::new();
        for def in &sheet.definitions {
            // The entry point runs the program; nothing calls it.
            if def.name.text == "𐑥𐑱𐑯" || !seen.insert(def.name.text.clone()) {
                continue;
            }
            let names = |ports: &[shawline_syntax::ast::PortDecl]| -> String {
                ports.iter().map(|p| p.name.text.as_str()).collect::<Vec<_>>().join(" ")
            };
            let gloss = format!("{} → {}", names(&def.inputs), names(&def.outputs));
            out.push(Candidate {
                word: def.name.text.clone(),
                inputs: def.inputs.len() as u16,
                outputs: def.outputs.len() as u16,
                gloss: gloss.trim().to_owned(),
                mirror: None,
            });
        }
    }
    if from.is_some() {
        out.retain(|c| c.inputs > 0);
    }
    out
}

pub(crate) fn wire_sources(doc: &Doc) -> Vec<Option<Pos>> {
    let (Ok(grid), Some(diagram)) = (doc.grid(), doc.diagram()) else { return Vec::new() };
    let model = Model::new(grid, diagram);
    (0..diagram.wires.len()).map(|w| model.source_cell(w)).collect()
}
