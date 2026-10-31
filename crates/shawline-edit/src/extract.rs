//! Extracting a definition (PLAN P10, "Extracting a definition"): the
//! pick moves into a new definition one blank line below the sheet, and a
//! call node for it takes the pick's top-left corner.
//!
//! The new definition is a copy of the pick at its own relative placement,
//! with the wires among the picked parts copied as drawn. Its ports are
//! wired with [`crate::connect`], one branch at a time, each checked. At
//! the call, a lone node box is redrawn for the new word and ports, and its
//! wires are re-routed as a move re-routes them. Any other pick is cleared,
//! the call placed, and the call's wires connected afresh, since there is
//! no one box for its wires to follow. A lone inline node or constant is
//! first given way to in its own row: its call is drawn over it, ports on
//! that row, and its wires kept. That is the only way past an inline node
//! it feeds, whose arrowhead can't be left without its wire. Otherwise the
//! call goes in the pick's corner when it fits there, and then in the
//! nearest clear spot of the pick's scope, since a constant or an inline
//! node is narrower than a call. The whole is checked: canonical, with the
//! input's codes, and every part found again.

use std::collections::{HashMap, HashSet};

use shawline_doc::Doc;
use shawline_grid::{Dir, Pos, Rect};
use shawline_shape::Diagram;
use shawline_shape::stroke::Kind;
use shawline_shape::structure::Mapping;

use crate::canon::{Sheets, tidy};
use crate::clearing::Rebox;
use crate::cut::{codes, render};
use crate::drawing::{is_word, items, put, rows, rule, write};
use crate::mapped::{self, Read};
use crate::model::{Model, Scope, Sink, Term};
use crate::sheet::{across, glyph};
use crate::wiring::{Canvas, cut_branch, erase_wire, node_rect, read};
use crate::{BoxKind, Blockage, Edited, Extracted, PORT_NAMES, Refusal, RefusalKind, connect, place_node};

/// Wire cells between the input names and the pick, and between the pick
/// and the outputs: room for a fan-out's tee and its bends.
const LEAD: usize = 4;

/// The most spots a cleared pick's call is tried in, its corner included.
const CALL_SPOTS: usize = 12;

pub(crate) fn extract(doc: &Doc, picks: &[Pos], name: &str) -> Result<Extracted, Refusal> {
    let at = picks.first().copied().unwrap_or(Pos::new(0, 0));
    let refuse = |kind, why: &str| Refusal::new(at, kind, why);
    if picks.is_empty() {
        return Err(refuse(RefusalKind::Target, "pick the nodes, switches or constants to move into the definition"));
    }
    if !is_word(name) || name.chars().count() < 2 {
        return Err(refuse(RefusalKind::Target, "a new definition's name is two or more Shavian letters"));
    }
    let sheets = Sheets::read(doc, at)?;
    let (grid, diagram) = sheets.canon();
    let model = Model::new(grid, diagram);
    let title = format!("·{name}");
    if diagram.texts.iter().any(|t| t.content == title && t.title_of.is_some_and(|b| diagram.boxes[b].parent.is_none())) {
        return Err(refuse(RefusalKind::Target, "a definition has this name already; pick another"));
    }
    let pick = Pick::read(&sheets, &model, picks)?;
    let flows = Flows::of(&model, &pick).map_err(|why| refuse(RefusalKind::Target, why))?;
    let names = Names::of(&model, &flows).ok_or_else(|| refuse(RefusalKind::Target, "that's more ports than a definition can name"))?;

    // The call first, where the pick was; then the definition below it all.
    let finish = |called: Edited, call: Pos| -> Result<Extracted, Refusal> {
        let defined = Definition::lay_out(&model, &pick, &flows, &names, name, mapped::read(&called.doc))
            .map_err(|why| refuse(RefusalKind::Blocked(Blockage::NoRoom), why))?;
        let done = defined.draw(&called.doc, &model, &pick, &flows, &names, at)?;

        let changed = |why: &str| refuse(RefusalKind::Changed, &format!("the definition can't be made: {why}"));
        if done.formatted() != Some(done.text()) {
            return Err(changed("the sheet wouldn't stay tidy"));
        }
        if codes(&done) != codes(sheets.doc()) {
            return Err(changed("it would change what the program means"));
        }
        let map = defined.map(sheets.canon(), &called, mapped::read(&done), &pick);
        if !map.is_total() {
            return Err(changed("a part wouldn't keep its place"));
        }
        let edited = Edited { doc: done, map, tidied: sheets.tidied() };
        Ok(Extracted { edited, definition: Pos::new(defined.top, 0), call })
    };
    let lone = match pick.boxes[..] {
        [b] if pick.texts.is_empty() && model.kind(b) == BoxKind::Node => Some(b),
        _ => None,
    };
    if let Some(b) = lone {
        return finish(rebox(&sheets, &model, b, name, &flows, at)?, sheets.corner(b));
    }
    // In the pick's row, then each spot in turn. A refusal is the first
    // one tried, whose room is the one `landing` tells Make room of.
    let mut refused = None;
    if let Some(row) = InRow::of(&model, &pick, name, &flows) {
        match row.draw(&sheets, &model, name, at).and_then(|called| finish(called, row.corner)) {
            Ok(done) => return Ok(done),
            Err(r) => refused = Some(r),
        }
    }
    let cleared = clear(&sheets, &model, &pick, &flows, at)?;
    for corner in call_spots(&cleared, &model, &pick, name, &flows) {
        match call(&sheets, &cleared, name, &flows, corner, at).and_then(|called| finish(called, corner)) {
            Ok(done) => return Ok(done),
            Err(r) => {
                refused.get_or_insert(r);
            }
        }
    }
    Err(refused.unwrap_or_else(|| refuse(RefusalKind::Blocked(Blockage::NoRoom), "there's no room for the call")))
}

/// Where [`extract`] on canonical `doc` would put the call, for
/// [`crate::make_room`]: the box it goes in, its rectangle, and the
/// rectangle of the pick's own cells.
pub(crate) fn landing(doc: &Doc, picks: &[Pos], name: &str) -> Option<(usize, Rect, Rect)> {
    let sheets = Sheets::read(doc, *picks.first()?).ok()?;
    let (grid, diagram) = sheets.canon();
    let model = Model::new(grid, diagram);
    let pick = Pick::read(&sheets, &model, picks).ok()?;
    let flows = Flows::of(&model, &pick).ok()?;
    let lone = match pick.boxes[..] {
        [b] if pick.texts.is_empty() && model.kind(b) == BoxKind::Node => Some(b),
        _ => None,
    };
    let in_row = lone.is_none().then(|| InRow::of(&model, &pick, name, &flows)).flatten();
    let corner = match (lone, &in_row) {
        (Some(b), _) => sheets.corner(b),
        (None, Some(row)) => row.corner,
        (None, None) => pick.corner(&model),
    };
    let call = node_rect(corner, name, flows.inputs.len() as u16, flows.outputs.len() as u16);
    let cells = pick.cells(diagram);
    let mut span = Rect {
        top: cells.iter().map(|p| p.row).min()?,
        left: cells.iter().map(|p| p.col).min()?,
        bottom: cells.iter().map(|p| p.row).max()?,
        right: cells.iter().map(|p| p.col).max()?,
    };
    // Room for a call in the row goes inside its output wire, which a
    // column there stretches; a column between a text and its wire would
    // part them.
    if let Some(row) = in_row {
        span.right = span.right.max(row.wire);
    }
    Some((pick.scope.0, call, span))
}

// ---------------------------------------------------------------------------
// The pick.

/// What the cells of a pick name, on the canonical sheet.
struct Pick {
    scope: Scope,
    /// Picked node boxes and switches.
    boxes: Vec<usize>,
    /// Picked constants and inline nodes' words.
    texts: Vec<usize>,
    /// Each picked inline node's word, with its arrowhead.
    inline: Vec<(usize, Pos)>,
}

impl Pick {
    fn read(sheets: &Sheets, model: &Model, picks: &[Pos]) -> Result<Self, Refusal> {
        let (g, d) = sheets.given();
        let given = Model::new(g, d);
        let diagram = model.diagram;
        let mut pick = Pick { scope: (0, 0), boxes: Vec::new(), texts: Vec::new(), inline: Vec::new() };
        let mut scopes: Vec<Scope> = Vec::new();
        for &p in picks {
            let none = || {
                let why = "pick a node box or switch by its top-left corner, or an inline node or constant by its first cell";
                Refusal::new(p, RefusalKind::Target, why)
            };
            if let Some(b) = given.box_at(p) {
                let picked = match model.kind(b) {
                    BoxKind::Switch => true,
                    BoxKind::Node => model.word(b).is_some(),
                    BoxKind::Definition => false,
                };
                let parent = diagram.boxes[b].parent.filter(|_| picked).ok_or_else(none)?;
                scopes.push((parent, model.arm(parent, diagram.boxes[b].rect.top)));
                if !pick.boxes.contains(&b) {
                    pick.boxes.push(b);
                }
                continue;
            }
            let t = given.text_at(p).filter(|&t| d.texts[t].pos == p).ok_or_else(none)?;
            let text = &diagram.texts[t];
            if let Some(scope) = model.constant(t) {
                scopes.push(scope);
            } else if model.inline_word(t) {
                let owner = text.owner.ok_or_else(none)?;
                scopes.push((owner, model.arm(owner, text.pos.row)));
                let (gap, _) = across(model.grid, &model.texts, text.pos, Dir::W);
                let arrow = gap.last().copied().unwrap_or(text.pos).step(Dir::W).ok_or_else(none)?;
                if !pick.inline.iter().any(|&(u, _)| u == t) {
                    pick.inline.push((t, arrow));
                }
            } else {
                return Err(none());
            }
            if !pick.texts.contains(&t) {
                pick.texts.push(t);
            }
        }
        scopes.sort_unstable();
        scopes.dedup();
        if let [scope] = scopes[..] {
            pick.scope = scope;
        } else {
            let why = "the picked parts lie in different definitions or arms; pick them from one";
            return Err(Refusal::new(picks[0], RefusalKind::Target, why));
        }
        pick.boxes.sort_unstable();
        pick.texts.sort_unstable();
        Ok(pick)
    }

    fn rects<'a>(&'a self, diagram: &'a Diagram) -> impl Iterator<Item = Rect> + 'a {
        self.boxes.iter().map(|&b| diagram.boxes[b].rect)
    }

    /// Whether cell `p` lies in a picked box.
    fn boxed(&self, diagram: &Diagram, p: Pos) -> bool {
        self.rects(diagram).any(|r| r.contains(p))
    }

    /// Every box the pick moves: the picked ones and the boxes in them.
    fn moved_boxes(&self, diagram: &Diagram) -> Vec<usize> {
        (0..diagram.boxes.len())
            .filter(|&b| {
                let rect = diagram.boxes[b].rect;
                self.rects(diagram).any(|r| r == rect || r.encloses(&rect))
            })
            .collect()
    }

    /// Every text the pick moves: those in picked boxes, and the picked ones.
    fn moved_texts(&self, diagram: &Diagram) -> Vec<usize> {
        (0..diagram.texts.len())
            .filter(|&t| self.texts.contains(&t) || self.boxed(diagram, diagram.texts[t].pos))
            .collect()
    }

    /// The cells copied as they are: the picked boxes whole, the texts,
    /// and each inline node from its arrowhead to its word's last cell.
    fn cells(&self, diagram: &Diagram) -> Vec<Pos> {
        let mut cells: Vec<Pos> = self
            .rects(diagram)
            .flat_map(|r| (r.top..=r.bottom).flat_map(move |row| (r.left..=r.right).map(move |col| Pos::new(row, col))))
            .collect();
        for &t in &self.texts {
            cells.extend(diagram.texts[t].cells());
        }
        for &(t, arrow) in &self.inline {
            cells.extend((arrow.col..diagram.texts[t].pos.col).map(|col| Pos::new(arrow.row, col)));
        }
        cells.sort_unstable();
        cells.dedup();
        cells
    }

    /// The top-left corner of the pick's cells.
    fn corner(&self, model: &Model) -> Pos {
        let cells = self.cells(model.diagram);
        let row = cells.iter().map(|p| p.row).min().unwrap_or(0);
        let col = cells.iter().map(|p| p.col).min().unwrap_or(0);
        Pos::new(row, col)
    }

    /// Whether a wire's `term` is the pick's: a port of a picked box, a
    /// picked text's start, or a picked inline node's arrowhead.
    fn holds(&self, model: &Model, term: Term) -> bool {
        match term {
            Term::Port(p, _) => model.port(p).is_some_and(|(b, _)| self.boxes.contains(&b)),
            Term::Text(p, Dir::W) => model.text_at(p).is_some_and(|t| self.texts.contains(&t)),
            Term::Arrow(p) => self.inline.iter().any(|&(_, a)| a == p),
            _ => false,
        }
    }
}

// ---------------------------------------------------------------------------
// The wires across the pick's edge.

/// One wire across the pick's edge.
struct Flow {
    /// The cell naming its source, as [`connect`] takes it.
    source: Pos,
    /// Its sinks in the pick, row-major.
    inside: Vec<Pos>,
    /// Its sinks outside the pick, row-major.
    outside: Vec<Pos>,
}

/// The wires the pick's ports come from, each kind ordered row-major by
/// the wire's first cell.
struct Flows {
    /// One per source outside the pick.
    inputs: Vec<Flow>,
    /// One per source inside the pick whose wire leaves it.
    outputs: Vec<Flow>,
    /// Wires with every end in the pick and outside its boxes: copied.
    internal: Vec<usize>,
}

impl Flows {
    fn of(model: &Model, pick: &Pick) -> Result<Self, &'static str> {
        let diagram = model.diagram;
        let is_source = |t: &Term| matches!(t, Term::Port(_, Dir::W) | Term::Text(_, Dir::W));
        let cell = |t: &Term| match *t {
            Term::Port(p, _) | Term::Text(p, _) | Term::Arrow(p) | Term::Loose(p, _, _) => p,
        };
        let mut inputs: Vec<(Pos, Flow)> = Vec::new();
        let mut outputs: Vec<(Pos, Flow)> = Vec::new();
        let mut internal = Vec::new();
        for (w, wire) in diagram.wires.iter().enumerate() {
            // A switch's own wires go with it.
            if wire.cells.iter().all(|&p| pick.boxed(diagram, p)) {
                continue;
            }
            let terms = &model.terms[w];
            if !terms.iter().any(|&t| pick.holds(model, t)) {
                continue;
            }
            if terms.iter().any(|t| matches!(t, Term::Loose(..) | Term::Text(_, Dir::E))) {
                return Err("a wire with a loose end meets the pick; mend it first");
            }
            let source_inside = terms.iter().filter(|t| is_source(t)).any(|&t| pick.holds(model, t));
            let sinks = terms.iter().filter(|t| !is_source(t));
            let mut inside: Vec<Pos> = sinks.clone().filter(|&&t| pick.holds(model, t)).map(cell).collect();
            let mut outside: Vec<Pos> = sinks.filter(|&&t| !pick.holds(model, t)).map(cell).collect();
            inside.sort_unstable();
            outside.sort_unstable();
            let source = model.source_cell(w).ok_or("a wire into the pick has no source")?;
            let flow = Flow { source, inside, outside };
            match (source_inside, flow.outside.is_empty()) {
                (true, true) => internal.push(w),
                (true, false) => outputs.push((wire.cells[0], flow)),
                (false, _) => inputs.push((wire.cells[0], flow)),
            }
        }
        inputs.sort_by_key(|(first, _)| *first);
        outputs.sort_by_key(|(first, _)| *first);
        Ok(Flows {
            inputs: inputs.into_iter().map(|(_, f)| f).collect(),
            outputs: outputs.into_iter().map(|(_, f)| f).collect(),
            internal,
        })
    }
}

/// The new definition's port names.
struct Names {
    inputs: Vec<String>,
    outputs: Vec<String>,
}

impl Names {
    /// An input from a definition input takes its name; every other port
    /// takes the first of [`PORT_NAMES`] not yet taken, inputs first.
    fn of(model: &Model, flows: &Flows) -> Option<Self> {
        let named: Vec<Option<String>> = flows
            .inputs
            .iter()
            .map(|f| {
                let t = model.text_at(f.source)?;
                model.input_named(t).map(|_| model.diagram.texts[t].content.clone())
            })
            .collect();
        let mut taken: HashSet<String> = named.iter().flatten().cloned().collect();
        let mut free = PORT_NAMES.iter().map(char::to_string);
        let mut next = |taken: &mut HashSet<String>| -> Option<String> {
            let name = free.by_ref().find(|n| !taken.contains(n))?;
            taken.insert(name.clone());
            Some(name)
        };
        let mut inputs = Vec::new();
        for name in named {
            inputs.push(match name {
                Some(name) => name,
                None => next(&mut taken)?,
            });
        }
        let outputs = (0..flows.outputs.len()).map(|_| next(&mut taken)).collect::<Option<Vec<_>>>()?;
        Some(Names { inputs, outputs })
    }
}

// ---------------------------------------------------------------------------
// The call.

/// A lone node box redrawn as the call: each input keeps the wire of the
/// first port its source fed, each output its tee's wire, and the other
/// ports go with their branches.
fn rebox(sheets: &Sheets, model: &Model, b: usize, name: &str, flows: &Flows, at: Pos) -> Result<Edited, Refusal> {
    let inputs: Vec<(Pos, usize)> = flows.inputs.iter().enumerate().map(|(k, f)| (f.inside[0], k)).collect();
    let outputs: Vec<(Pos, usize)> = flows.outputs.iter().enumerate().map(|(k, f)| (f.source, k)).collect();
    let kept: HashSet<Pos> = inputs.iter().chain(&outputs).map(|&(p, _)| p).collect();
    let dropped = model.diagram.boxes[b].ports.iter().map(|p| p.pos).filter(|p| !kept.contains(p)).collect();
    let counts = (inputs.len() as u16, outputs.len() as u16);
    Rebox { b, word: Some(name.to_owned()), inputs, outputs, counts, dropped }
        .run(sheets, model, at)
        .map_err(|r| Refusal { at, ..r })
}

/// A lone inline node or constant's call drawn over it in its row: the
/// inline node's arrowhead becomes the call's input, and its output wire,
/// straight along the row where the call would cover it, runs on from the
/// call's tee.
struct InRow {
    /// The call's top-left corner, a row above the pick.
    corner: Pos,
    /// The call's right border's column.
    right: usize,
    /// The pick's row, and the column of its output wire's first cell
    /// on it past the pick.
    row: usize,
    wire: usize,
    /// The picked text.
    text: usize,
    /// Whether the pick is an inline node, which has an input.
    inline: bool,
}

impl InRow {
    /// Where the call would go, for a pick of one inline node or constant
    /// whose output wire leaves along its row; None for any other pick.
    fn of(model: &Model, pick: &Pick, name: &str, flows: &Flows) -> Option<Self> {
        let ([t], [], [output]) = (&pick.texts[..], &pick.boxes[..], &flows.outputs[..]) else { return None };
        if flows.inputs.len() > 1 || !flows.internal.is_empty() {
            return None;
        }
        let text = &model.diagram.texts[*t];
        let start = pick.corner(model);
        let row = text.pos.row;
        let corner = Pos::new(row.checked_sub(1)?, start.col);
        let right = start.col + name.chars().count() + 3;
        let w = model.source(output.source).ok().and_then(|s| model.wire_of(s.term))?;
        let end = text.pos.col + text.width;
        let wire = model.diagram.wires[w].cells.iter().filter(|p| p.row == row && p.col >= end).map(|p| p.col).min()?;
        Some(InRow { corner, right, row, wire, text: *t, inline: !pick.inline.is_empty() })
    }

    /// The call drawn, or refused for want of room: blank sheet above and
    /// below it, and in its row only the pick, blank cells and the output
    /// wire's straight line, which goes on past its tee.
    fn draw(&self, sheets: &Sheets, model: &Model, name: &str, at: Pos) -> Result<Edited, Refusal> {
        let grid = model.grid;
        let (top, left, right, row) = (self.corner.row, self.corner.col, self.right, self.row);
        let blank = |p: Pos| grid.get(p) == ' ';
        let fits = (left..=right).all(|col| blank(Pos::new(top, col)) && blank(Pos::new(row + 1, col)))
            && (self.wire..=right + 1).all(|col| grid.get(Pos::new(row, col)) == '─')
            && (left..self.wire).all(|col| {
                let p = Pos::new(row, col);
                blank(p) || model.text_at(p) == Some(self.text) || (col == left && self.inline)
            });
        if !fits {
            let why = "the call has no room in this row; try Make room";
            return Err(Refusal::new(at, RefusalKind::Blocked(Blockage::NoRoom), why));
        }
        let mut lines = rows(grid);
        let width = right - left + 1;
        write(&mut lines, self.corner, &rule('┌', '─', None, '┐', width));
        write(&mut lines, Pos::new(row + 1, left), &rule('└', '─', None, '┘', width));
        for col in left..=right {
            put(&mut lines, Pos::new(row, col), ' ');
        }
        put(&mut lines, Pos::new(row, left), if self.inline { '▶' } else { '│' });
        write(&mut lines, Pos::new(row, left + 2), name);
        put(&mut lines, Pos::new(row, right), '├');
        for col in right + 1..self.wire {
            put(&mut lines, Pos::new(row, col), '─');
        }
        let changed = |why: &str| Refusal::new(at, RefusalKind::Changed, format!("the call can't be drawn here: {why}"));
        let drawn = tidy(Doc::new(render(&lines))).map_err(changed)?;
        let map = mapped::map(sheets.canon(), mapped::read(&drawn), Some);
        Ok(Edited { doc: drawn, map, tidied: false })
    }
}

/// Any other pick, cleared for its call: its wires across the edge cut,
/// and its parts and the wires among them erased.
fn clear(sheets: &Sheets, model: &Model, pick: &Pick, flows: &Flows, at: Pos) -> Result<Doc, Refusal> {
    let diagram = model.diagram;
    let mut current = Doc::new(sheets.doc().text());
    // One cut at a time, each read off the sheet the one before it left.
    // Outputs go first: an inline node with its input cut is a stray word,
    // which feeds no wire to find.
    for flow in &flows.outputs {
        let (g, d) = read(&current);
        let m = Model::new(g, d);
        let Some(w) = m.source(flow.source).ok().and_then(|s| m.wire_of(s.term)) else { continue };
        let mut canvas = Canvas::new(g);
        erase_wire(&mut canvas, &m, w);
        // An inline node's arrowhead is its wire's end, and stays for the
        // call's wire to reach, with the line cell before it: a bare
        // arrowhead is a slip, which formatting would join to whatever
        // is left of it on the row.
        for &p in flow.outside.iter().filter(|&&p| model.port(p).is_none()) {
            canvas.set(p, model.grid.get(p));
            if let Some(before) = p.step(Dir::W) {
                canvas.set(before, model.grid.get(before));
            }
        }
        current = Doc::new(canvas.render());
    }
    for &sink in flows.inputs.iter().flat_map(|f| &f.inside) {
        let (g, d) = read(&current);
        let m = Model::new(g, d);
        let Some(sink) = sink_at(&m, sink) else { continue };
        let Some(w) = m.wire_of(sink.term()) else { continue };
        let mut canvas = Canvas::new(g);
        cut_branch(&mut canvas, &m, w, &sink);
        current = Doc::new(canvas.render());
    }
    let (g, d) = read(&current);
    let m = Model::new(g, d);
    let mut canvas = Canvas::new(g);
    for &w in &flows.internal {
        if let Some(v) = d.wires.iter().position(|x| x.cells == diagram.wires[w].cells) {
            erase_wire(&mut canvas, &m, v);
        }
    }
    for p in pick.cells(diagram) {
        canvas.set(p, ' ');
    }
    let changed = |why: &str| Refusal::new(at, RefusalKind::Changed, format!("the pick can't be cleared: {why}"));
    tidy(Doc::new(canvas.render())).map_err(changed)
}

/// Where a cleared pick's call is tried, best first, at most
/// [`CALL_SPOTS`]: the pick's corner, then the spots in its scope with
/// clear sheet all round the call, nearest first to where the call's first
/// port would be on the pick's top row, so its wires run as the pick's did.
fn call_spots(cleared: &Doc, model: &Model, pick: &Pick, name: &str, flows: &Flows) -> Vec<Pos> {
    let corner = pick.corner(model);
    let (grid, _) = read(cleared);
    let (inputs, outputs) = (flows.inputs.len() as u16, flows.outputs.len() as u16);
    let scope = &model.diagram.boxes[pick.scope.0];
    let inner = scope.rect;
    let clear = |at: Pos| {
        let r = node_rect(at, name, inputs, outputs);
        let rows = r.top..=r.bottom;
        r.top > inner.top
            && r.left > inner.left + 1
            && r.bottom < inner.bottom
            && r.right + 1 < inner.right
            && model.arm(pick.scope.0, r.top) == pick.scope.1
            && model.arm(pick.scope.0, r.bottom) == pick.scope.1
            && !model.diagram.boxes.iter().any(|b| b.rect != inner && inner.encloses(&b.rect) && b.rect.contains(at))
            && rows.into_iter().all(|row| (r.left - 1..=r.right + 1).all(|col| grid.get(Pos::new(row, col)) == ' '))
    };
    let aim = Pos::new(corner.row.saturating_sub(1), corner.col);
    let mut spots: Vec<Pos> = (inner.top + 1..inner.bottom)
        .flat_map(|row| (inner.left + 2..inner.right).map(move |col| Pos::new(row, col)))
        .filter(|&p| p != corner && clear(p))
        .collect();
    spots.sort_by_key(|p| (2 * p.row.abs_diff(aim.row) + p.col.abs_diff(aim.col), *p));
    spots.truncate(CALL_SPOTS - 1);
    spots.insert(0, corner);
    spots
}

/// The call placed at `corner` on the cleared sheet, and wired from each
/// input's source and to each output's sinks.
fn call(sheets: &Sheets, cleared: &Doc, name: &str, flows: &Flows, corner: Pos, at: Pos) -> Result<Edited, Refusal> {
    let here = |r: Refusal| Refusal { at, ..r };
    let (inputs, outputs) = (flows.inputs.len() as u16, flows.outputs.len() as u16);
    let placed = place_node(cleared, corner, name, inputs, outputs).map_err(here)?;
    let mut current = placed.doc;
    for (flow, &to) in flows.inputs.iter().zip(&placed.inputs) {
        current = connect(&current, flow.source, to).map_err(here)?.doc;
    }
    for (flow, &from) in flows.outputs.iter().zip(&placed.outputs) {
        for &to in &flow.outside {
            current = connect(&current, from, to).map_err(here)?.doc;
        }
    }
    let map = mapped::map(sheets.canon(), mapped::read(&current), Some);
    Ok(Edited { doc: current, map, tidied: false })
}

/// The sink at `p`: an arrowhead on a box border, or an inline node's.
fn sink_at(model: &Model, p: Pos) -> Option<Sink> {
    model.sink(p).ok()
}

// ---------------------------------------------------------------------------
// The definition.

/// Where the new definition goes and how the pick sits in it.
struct Definition {
    /// Its title, `·name`.
    title: String,
    /// Its top border's row; it starts at column 0.
    top: usize,
    bottom: usize,
    /// Its right border's column.
    right: usize,
    /// The pick's top-left corner, and where that goes.
    from: Pos,
    to: Pos,
    /// Each input's and output's row.
    inputs: Vec<usize>,
    outputs: Vec<usize>,
}

impl Definition {
    /// Lays the definition out below everything on `base`, the sheet with
    /// the call. Inputs sit on the left in order, each on its first sink's
    /// row when the order allows, and outputs likewise on the right.
    fn lay_out(model: &Model, pick: &Pick, flows: &Flows, names: &Names, name: &str, base: Read) -> Result<Self, &'static str> {
        let diagram = model.diagram;
        let mut cells = pick.cells(diagram);
        for &w in &flows.internal {
            cells.extend(diagram.wires[w].cells.iter().copied());
        }
        let span = Rect {
            top: cells.iter().map(|p| p.row).min().ok_or("nothing is picked")?,
            left: cells.iter().map(|p| p.col).min().ok_or("nothing is picked")?,
            bottom: cells.iter().map(|p| p.row).max().ok_or("nothing is picked")?,
            right: cells.iter().map(|p| p.col).max().ok_or("nothing is picked")?,
        };
        let top = items(base.0, base.1).last().map_or(0, |&(_, last)| last + 2);
        let widest = names.inputs.iter().map(|n| n.chars().count()).max();
        let left = widest.map_or(3, |w| 3 + w + LEAD);
        let to = Pos::new(top + 2, left);
        let place = |p: Pos| Pos::new(p.row - span.top + to.row, p.col - span.left + to.col);
        // `╭─ ·name ─╮`: the title and five cells around it.
        let right = (left + (span.right - span.left) + 1 + LEAD).max(name.chars().count() + 6);
        let inline_at = |p: Pos| pick.inline.iter().any(|&(_, a)| a == p);
        let inline_word = |p: Pos| model.text_at(p).is_some_and(|t| pick.inline.iter().any(|&(u, _)| u == t));
        // Each row as wanted, or the next free one below the port above.
        let rows = |wants: Vec<(usize, bool)>| -> Result<Vec<usize>, &'static str> {
            let mut next = top + 2;
            let mut rows = Vec::new();
            for (want, fixed) in wants {
                let row = want.max(next);
                if fixed && row != want {
                    return Err("an inline node's port can't keep its row; extract it with fewer parts");
                }
                rows.push(row);
                next = row + 1;
            }
            Ok(rows)
        };
        let inputs = rows(
            flows
                .inputs
                .iter()
                .map(|f| {
                    let inline = f.inside.iter().copied().find(|&p| inline_at(p));
                    let first = inline.or(f.inside.first().copied()).map_or(top + 2, |p| place(p).row);
                    (first, inline.is_some())
                })
                .collect(),
        )?;
        let outputs = rows(flows.outputs.iter().map(|f| (place(f.source).row, inline_word(f.source))).collect())?;
        let bottom = [span.bottom - span.top + to.row, inputs.last().copied().unwrap_or(0), outputs.last().copied().unwrap_or(0)]
            .into_iter()
            .max()
            .unwrap_or(0)
            + 2;
        let title = format!("·{name}");
        Ok(Definition { title, top, bottom, right, from: Pos::new(span.top, span.left), to, inputs, outputs })
    }

    /// Where canonical cell `p` of the pick goes.
    fn place(&self, p: Pos) -> Pos {
        Pos::new(p.row - self.from.row + self.to.row, p.col - self.from.col + self.to.col)
    }

    fn place_rect(&self, r: Rect) -> Rect {
        let (a, b) = (self.place(Pos::new(r.top, r.left)), self.place(Pos::new(r.bottom, r.right)));
        Rect { top: a.row, left: a.col, bottom: b.row, right: b.col }
    }

    /// Draws the definition on `base` and wires its ports.
    fn draw(&self, base: &Doc, model: &Model, pick: &Pick, flows: &Flows, names: &Names, at: Pos) -> Result<Doc, Refusal> {
        let no_room = |why: &str| Refusal::new(at, RefusalKind::Blocked(Blockage::NoRoom), why);
        let (grid, diagram) = (model.grid, model.diagram);
        let (top, bottom, right) = (self.top, self.bottom, self.right);
        let mut lines = rows(read(base).0);
        write(&mut lines, Pos::new(top, 0), &rule('╭', '─', Some(&self.title), '╮', right + 1));
        for row in top + 1..bottom {
            put(&mut lines, Pos::new(row, 0), '│');
            put(&mut lines, Pos::new(row, right), '│');
        }
        write(&mut lines, Pos::new(bottom, 0), &rule('╰', '─', None, '╯', right + 1));
        for (name, &row) in names.inputs.iter().zip(&self.inputs) {
            put(&mut lines, Pos::new(row, 0), '▶');
            write(&mut lines, Pos::new(row, 2), name);
        }
        for (name, &row) in names.outputs.iter().zip(&self.outputs) {
            put(&mut lines, Pos::new(row, right), '▶');
            write(&mut lines, Pos::new(row, right + 2), name);
        }
        for p in pick.cells(diagram) {
            put(&mut lines, self.place(p), grid.get(p));
        }
        for &w in &flows.internal {
            let wire = &diagram.wires[w];
            for (&p, &links) in wire.cells.iter().zip(&wire.links) {
                if pick.boxed(diagram, p) {
                    continue;
                }
                // A crossing with a wire that stays behind keeps only this one's line.
                let crossing = diagram.strokes.get(p).is_some_and(|s| s.kind == Kind::Crossing);
                put(&mut lines, self.place(p), if crossing { glyph(links) } else { grid.get(p) });
            }
        }
        // An inline node's wires across the edge run straight, so it stays one.
        let blank = |lines: &Vec<Vec<char>>, p: Pos| lines.get(p.row).and_then(|r| r.get(p.col)).is_none_or(|&c| c == ' ');
        let straight = |lines: &mut Vec<Vec<char>>, row: usize, from: usize, to: usize| -> Result<(), Refusal> {
            if from > to || !(from..=to).all(|col| blank(lines, Pos::new(row, col))) {
                return Err(no_room("an inline node's wire can't run straight to its port; extract it with fewer parts"));
            }
            (from..=to).for_each(|col| put(lines, Pos::new(row, col), '─'));
            Ok(())
        };
        let inline_word = |p: Pos| model.text_at(p).filter(|&t| pick.inline.iter().any(|&(u, _)| u == t));
        let mut straights: HashSet<Pos> = HashSet::new();
        for ((flow, name), &row) in flows.inputs.iter().zip(&names.inputs).zip(&self.inputs) {
            if let Some(&(_, arrow)) = pick.inline.iter().find(|&&(_, a)| flow.inside.contains(&a)) {
                straight(&mut lines, row, 3 + name.chars().count(), self.place(arrow).col - 1)?;
                straights.insert(arrow);
            }
        }
        for (flow, &row) in flows.outputs.iter().zip(&self.outputs) {
            if let Some(t) = inline_word(flow.source) {
                let word = &diagram.texts[t];
                straight(&mut lines, row, self.place(word.pos).col + word.width + 1, right - 1)?;
                straights.insert(flow.source);
            }
        }
        let drawn = Doc::new(render(&lines));
        if drawn.formatted() != Some(drawn.text()) {
            let why = "the new definition wouldn't be tidy as drawn; extract fewer parts";
            return Err(Refusal::new(at, RefusalKind::Changed, why));
        }
        let wired = |r: Refusal| Refusal {
            at,
            reason: format!("the new definition's wires can't be drawn: {}", r.reason),
            remedy: None,
            ..r
        };
        let mut current = drawn;
        for (flow, &row) in flows.inputs.iter().zip(&self.inputs) {
            let from = Pos::new(row, 2);
            for &sink in flow.inside.iter().filter(|p| !straights.contains(p)) {
                current = connect(&current, from, self.place(sink)).map_err(wired)?.doc;
            }
        }
        for (flow, &row) in flows.outputs.iter().zip(&self.outputs) {
            let from = self.place(flow.source);
            if !straights.contains(&flow.source) {
                current = connect(&current, from, Pos::new(row, right)).map_err(wired)?.doc;
            }
            for &sink in &flow.inside {
                current = connect(&current, from, self.place(sink)).map_err(wired)?.doc;
            }
        }
        Ok(current)
    }

    /// The map from the canonical input to `done`: every part that stays
    /// where `called` put it, and the pick's parts at their copies.
    fn map(&self, before: Read, called: &Edited, done: Read, pick: &Pick) -> Mapping {
        let (old, mid, new) = (before.1, mapped::read(&called.doc).1, done.1);
        let boxes: HashMap<Rect, usize> = new.boxes.iter().enumerate().map(|(i, b)| (b.rect, i)).collect();
        let texts: HashMap<Pos, usize> = new.texts.iter().enumerate().map(|(i, t)| (t.pos, i)).collect();
        let moved_boxes = pick.moved_boxes(old);
        let moved_texts = pick.moved_texts(old);
        let mut map = Mapping {
            boxes: (0..old.boxes.len())
                .map(|i| {
                    let rect = if moved_boxes.contains(&i) {
                        self.place_rect(old.boxes[i].rect)
                    } else {
                        mid.boxes[called.map.boxes.get(i).copied().flatten()?].rect
                    };
                    boxes.get(&rect).copied()
                })
                .collect(),
            texts: (0..old.texts.len())
                .map(|i| {
                    let pos = if moved_texts.contains(&i) {
                        self.place(old.texts[i].pos)
                    } else {
                        mid.texts[called.map.texts.get(i).copied().flatten()?].pos
                    };
                    texts.get(&pos).copied()
                })
                .collect(),
            wires: Vec::new(),
        };
        map.wires = mapped::wires_by_terms(before, done, &map);
        map
    }
}
