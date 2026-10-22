//! Studio Phase 1's edits (PLAN P7): connect, remove a wire, disconnect a
//! branch, place and remove nodes, and the palette.
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
use crate::check::DIAGNOSTICS;
use crate::model::{BoxKind, Model, NIL, Sink, Source, Term, no_sink, no_source};
use crate::router::{self, BEND, BUDGET, CLEARANCE, Cell, Found, Miss, Start, TEE, Terrain, toward};
use crate::sheet::{glyph, keep_clear};
use crate::{Candidate, Placed, Refusal, RefusalKind};

/// The most ports `place_node` draws on one side.
const MAX_PORTS: u16 = 16;

/// The sheet's characters, edited in place.
struct Canvas {
    rows: Vec<Vec<char>>,
}

impl Canvas {
    fn new(grid: &Grid) -> Self {
        Self { rows: (0..grid.height()).map(|r| grid.row(r).to_vec()).collect() }
    }

    fn get(&self, p: Pos) -> char {
        self.rows.get(p.row).and_then(|r| r.get(p.col)).copied().unwrap_or(' ')
    }

    fn set(&mut self, p: Pos, ch: char) {
        if self.rows.len() <= p.row {
            self.rows.resize(p.row + 1, Vec::new());
        }
        let row = &mut self.rows[p.row];
        if row.len() <= p.col {
            row.resize(p.col + 1, ' ');
        }
        row[p.col] = ch;
    }

    fn render(&self) -> String {
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
fn here(at: Pos) -> impl Fn(Refusal) -> Refusal {
    move |r| Refusal { at, ..r }
}

/// Formats `text` and checks it recognizes to what the edit declares:
/// `expected`, given where each box and text of `old` went. Nothing moves in
/// a wiring edit, so they're found where they were, and a stray move leaves
/// one unfound. Gives the revision the edit returns, already read.
fn finish(
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
            Part::Boxes => "it would change a box",
            Part::Texts => "it would change the text on the sheet",
            Part::Wires => "it would change what another wire connects",
            Part::Codes => DIAGNOSTICS,
        })
    })?;
    Ok(doc)
}

/// The grid and diagram of a text an edit wrote.
fn read(doc: &Doc) -> (&Grid, &Diagram) {
    let grid = doc.grid().expect("edits write only repertoire characters");
    (grid, doc.diagram().expect("a sheet on the grid is recognized"))
}

// ---------------------------------------------------------------------------
// Removing strokes.

/// Erases wire cell `p`, whose own connections are `mask`. A crossing keeps
/// the axis that isn't `mask`'s, if any: another wire's line, or another
/// branch of a wire crossing itself.
fn erase(canvas: &mut Canvas, model: &Model, p: Pos, mask: u8) {
    let crossing = model.diagram.strokes.get(p).filter(|s| s.kind == Kind::Crossing);
    canvas.set(p, crossing.map_or(' ', |s| glyph(s.conn & !mask)));
}

fn erase_wire(canvas: &mut Canvas, model: &Model, w: usize) {
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
fn cut_branch(canvas: &mut Canvas, model: &Model, w: usize, sink: &Sink) {
    let end = model.diagram.wires[w]
        .ends
        .iter()
        .find(|e| e.kind == EndKind::Port(sink.at) && e.facing == sink.dir)
        .expect("the sink's wire ends at it");
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
    let w = through.iter().copied().find(|&w| given.editable(w).is_ok()).ok_or_else(|| {
        Refusal::new(at, RefusalKind::Unsupported, given.editable(first).unwrap_err())
    })?;
    let (grid, diagram) = sheets.canon();
    let model = Model::new(grid, diagram);
    let w = sheets
        .wire(&given, &model, w, at)
        .map_err(|why| Refusal::new(at, RefusalKind::Unreadable, why))?;
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
    let sink = sheets.entity(to).ok_or_else(|| no_sink(to))?;
    let (grid, diagram) = sheets.canon();
    let model = Model::new(grid, diagram);
    let sink = model.sink(sink).map_err(here(to))?;
    let w = model
        .wire_of(sink.term())
        .ok_or_else(|| Refusal::new(to, RefusalKind::Target, "no wire reaches this arrowhead"))?;
    model.editable(w).map_err(|why| Refusal::new(to, RefusalKind::Unsupported, why))?;
    let mut canvas = Canvas::new(grid);
    cut_branch(&mut canvas, &model, w, &sink);
    let mut expected = Structure::of(grid, diagram);
    expected.disconnect(model.named(sink.term()).expect("a sink is a port"));
    finish(canvas.render(), diagram, to, "the wire can't be cut here", |map| expected.relabel(map))
}

pub(crate) fn remove_node(doc: &Doc, corner: Pos) -> Result<Doc, Refusal> {
    let sheets = Sheets::read(doc, corner)?;
    let (g, d) = sheets.given();
    let b = Model::new(g, d).box_at(corner).ok_or_else(|| {
        Refusal::new(corner, RefusalKind::Target, "there is no box with its top-left corner here")
    })?;
    let (grid, diagram) = sheets.canon();
    let model = Model::new(grid, diagram);
    if model.kind(b) != BoxKind::Node {
        let why = "only a node can be deleted; a definition or switch stays";
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
    for w in inputs.iter().filter_map(|s| model.wire_of(s.term())).chain(
        outputs.iter().filter_map(|&t| model.wire_of(t)),
    ) {
        model.editable(w).map_err(|why| Refusal::new(corner, RefusalKind::Unsupported, why))?;
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
            cut_branch(&mut canvas, &m, w, sink);
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
    let what = "the node can't be deleted";
    finish(canvas.render(), diagram, corner, what, |map| expected.relabel(map))
}

// ---------------------------------------------------------------------------
// Placing nodes.

/// The rectangle of a canonical node box for `word` (SPEC §6, as the corpus
/// draws them): one space either side of the word, which sits on the first
/// port row, and one row per port.
fn node_rect(corner: Pos, word: &str, inputs: u16, outputs: u16) -> Rect {
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
        return Err(refuse(RefusalKind::Blocked, "the node would stick out of its definition"));
    };
    match model.kind(holder) {
        BoxKind::Node => {
            return Err(refuse(RefusalKind::Target, "a node can't go inside another node"));
        }
        BoxKind::Switch => {
            let dividers = &diagram.boxes[holder].dividers;
            if dividers.iter().any(|d| (rect.top..=rect.bottom).contains(&d.row)) {
                let why = "the node would cross from one arm of the switch into another";
                return Err(refuse(RefusalKind::Blocked, why));
            }
        }
        BoxKind::Definition => {}
    }
    if diagram.boxes.iter().any(|b| b.rect.intersects(&rect)
        && !b.rect.encloses(&rect)
        && b.rect != diagram.boxes[holder].rect)
    {
        return Err(refuse(RefusalKind::Blocked, "the node would overlap another box"));
    }
    // Clear in the text the caller sees as well as in the canonical one,
    // where formatting may have moved text or wire out of the way.
    let shown = node_rect(corner, word, inputs, outputs);
    for (sheet, r) in [(grid, rect), (sheets.given().0, shown)] {
        for row in r.top..=r.bottom {
            for col in r.left..=r.right {
                if sheet.get(Pos::new(row, col)) != ' ' {
                    let why = "the node would land on something; find a clear spot";
                    return Err(refuse(RefusalKind::Blocked, why));
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
    Ok(Placed { doc, inputs: arrows, outputs: tees })
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
        Wiring { model, source, wire, own, region, blocked, ends, target: sink.and_then(Sink::target) }
    }

    fn joinable(&self, p: Pos, heading: Dir) -> bool {
        let Some(w) = self.wire else { return false };
        if !self.own.contains(&p) || self.ends.contains(&p) {
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
        if self.blocked.contains(&p) || self.own.contains(&p) || self.ends.contains(&p) {
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
    let source = sheets.entity(from).ok_or_else(|| no_source(from))?;
    let sink = sheets.entity(to).ok_or_else(|| no_sink(to))?;
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
    if let Some(w) = model.wire_of(source.term) {
        model.editable(w).map_err(|why| Refusal::new(from, RefusalKind::Unsupported, why))?;
    }
    if sink.target().is_none_or(|t| grid.get(t) != ' ') {
        let why = "the cell before this arrowhead is taken";
        return Err(Refusal::new(to, RefusalKind::Blocked, why));
    }
    let wiring = Wiring::new(&model, &source, Some(&sink));
    let found = search_from_sink(&wiring, &sink).map_err(|miss| {
        let name = &source.name;
        let why = match miss {
            Miss::NoPath => format!("no free path for a wire from {name}; make room first"),
            Miss::GaveUp => {
                format!("the search for a wire from {name} gave up; make room or bring them closer")
            }
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

/// The cells a wire from `from` would take if released at `to`: exactly
/// what [`connect`] would draw when `to` is a free sink, or a route to the
/// blank cell `to` otherwise. `None` when nothing fits.
///
/// The route is found on the canonical sheet and given back as cells of the
/// text as given, by their place in the source's box.
pub(crate) fn preview(doc: &Doc, from: Pos, to: Pos) -> Option<Vec<Pos>> {
    let sheets = Sheets::read(doc, from).ok()?;
    let (grid, diagram) = sheets.canon();
    let model = Model::new(grid, diagram);
    let source = model.source(sheets.entity(from)?).ok()?;
    if let Some(w) = model.wire_of(source.term) {
        model.editable(w).ok()?;
    }
    let back = |cells: &[Pos]| -> Option<Vec<Pos>> {
        cells.iter().map(|&p| sheets.back(source.scope.0, p)).collect()
    };
    if let Some(sink) = sheets.entity(to).and_then(|p| model.sink(p).ok()) {
        if model.wire_of(sink.term()).is_some() || sink.scope != source.scope {
            return None;
        }
        let wiring = Wiring::new(&model, &source, Some(&sink));
        let found = search_from_sink(&wiring, &sink).ok()?;
        let n = found.path.len();
        let last = if wiring.wire.is_some() { n } else { n - 1 };
        return back(&found.path[1..last]);
    }
    if sheets.given().0.get(to) != ' ' {
        return None;
    }
    let to = sheets.place(to)?;
    let wiring = Wiring::new(&model, &source, None);
    if wiring.cell(to) != Cell::Free {
        return None;
    }
    let start = Start { pos: to, heading: None, straight: false };
    let found = router::search(&wiring, start, &mut { BUDGET }).ok()?;
    let n = found.path.len();
    let last = if wiring.wire.is_some() { n } else { n - 1 };
    back(&found.path[..last])
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
    (0..diagram.wires.len())
        .map(|w| {
            model.editable(w).ok()?;
            model.terms[w].iter().find_map(|&term| {
                let cell = match term {
                    Term::Port(p, Dir::W) | Term::Text(p, Dir::W) => p,
                    _ => return None,
                };
                model.source(cell).ok().filter(|s| s.term == term).map(|_| cell)
            })
        })
        .collect()
}
