//! Inline nodes (PLAN P10, "Inline nodes"; SPEC §6): a 1→1 call spliced
//! into a straight stretch of wire as `─▶ word ─`, and spliced out again.
//!
//! Placing one cuts its wire in two: the stretch up to the arrowhead stays
//! the node's input wire and the rest becomes its output wire, so every
//! sink keeps its source. Removing one runs the wire straight on through
//! the cells it held.

use std::collections::{HashMap, HashSet};

use shawline_doc::Doc;
use shawline_grid::{Dir, Pos};
use shawline_shape::structure::{Across, End, Routing, Structure, Term, TextPart, WirePart};
use shawline_shape::TextKind;
use shawline_shape::stroke::{E, W};

use crate::canon::Sheets;
use crate::cut::render;
use crate::drawing::{by_cell, is_word, put, rows, settle};
use crate::model::Model;
use crate::sheet::across;
use crate::wiring::{Canvas, finish};
use crate::{Blockage, Edited, Refusal, RefusalKind};

pub(crate) fn place_inline(doc: &Doc, at: Pos, word: &str) -> Result<Edited, Refusal> {
    let refuse = |kind, why: &str| Refusal::new(at, kind, why);
    if !is_word(word) {
        return Err(refuse(RefusalKind::Target, "an inline node's word is one Shavian word"));
    }
    let sheets = Sheets::read(doc, at)?;
    let none = || refuse(RefusalKind::Target, "an inline node goes on a straight horizontal wire; pick a cell of one");
    let cell = sheets.place(at).ok_or_else(none)?;
    let (grid, diagram) = sheets.canon();
    let model = Model::new(grid, diagram);
    let w = (0..diagram.wires.len())
        .find(|&w| grid.get(cell) == '─' && model.links(w, cell) == E | W)
        .ok_or_else(none)?;
    let n = word.chars().count();
    let row = cell.row;
    // From the line cell before the arrowhead to the output wire's first.
    let stretch = cell.col.checked_sub(3).map(|first| first..=cell.col + n + 1);
    let plain = |col: usize| {
        let p = Pos::new(row, col);
        grid.get(p) == '─' && model.links(w, p) == E | W && !diagram.wires.iter().any(|v| v.crossings.contains(&p))
    };
    let Some(stretch) = stretch.filter(|s| s.clone().all(plain)) else {
        let why = "the straight stretch of wire here is too short for the node; try Make room or a longer stretch";
        return Err(refuse(RefusalKind::Blocked(Blockage::NoRoom), why));
    };
    let mut lines = rows(grid);
    put(&mut lines, Pos::new(row, cell.col - 2), '▶');
    put(&mut lines, Pos::new(row, cell.col - 1), ' ');
    for (k, ch) in word.chars().enumerate() {
        put(&mut lines, Pos::new(row, cell.col + k), ch);
    }
    put(&mut lines, Pos::new(row, cell.col + n), ' ');

    // The wire's ends on either side of the cut, and its tees and crossings.
    let input = Pos::new(row, *stretch.start());
    let cut: HashSet<Pos> = (cell.col - 2..=cell.col + n).map(|col| Pos::new(row, col)).collect();
    let side = sides(&model, w, &cut, input);
    let wire = &diagram.wires[w];
    let old = Structure::wire(grid, diagram, w);
    let ends: Vec<(bool, Term)> = wire
        .ends
        .iter()
        .map(|e| (side.contains(&e.pos), end_term(&model, e)))
        .collect();
    let count = |cells: &[Pos], into: bool| cells.iter().filter(|p| side.contains(p) == into).count();
    let owner = (0..diagram.boxes.len())
        .filter(|&b| diagram.boxes[b].rect.interior_contains(cell))
        .min_by_key(|&b| diagram.boxes[b].rect.area());
    let what = "the inline node can't go here";
    settle(&sheets, render(&lines), at, what, Routing::Kept, by_cell(Some), |expected, map, new| {
        let t = new.1.texts.iter().position(|t| t.pos == cell)?;
        let carry = |term: Term| -> Option<Term> {
            let end = match term.end {
                End::Port(b, k) => End::Port(map.boxes[b]?, k),
                End::Open(Across::Text(i)) => End::Open(Across::Text(map.texts[i]?)),
                End::Arrow(Across::Text(i)) => End::Arrow(Across::Text(map.texts[i]?)),
                other => other,
            };
            Some(Term { facing: term.facing, end })
        };
        // The old wire, as it reads in the new indices.
        let mut was: Vec<Term> = old.terms.iter().map(|&term| carry(term)).collect::<Option<_>>()?;
        was.sort();
        let i = expected
            .wires
            .iter()
            .position(|part| part.terms == was && part.tees == old.tees && part.crossings == old.crossings)?;
        expected.wires.remove(i);
        let part = |into: bool, extra: Term| -> Option<WirePart> {
            let mut terms: Vec<Term> =
                ends.iter().filter(|(s, _)| *s == into).map(|&(_, term)| carry(term)).collect::<Option<_>>()?;
            terms.push(extra);
            terms.sort();
            Some(WirePart { terms, tees: count(&wire.tees, into), crossings: count(&wire.crossings, into) })
        };
        let arrow = Term { facing: Dir::E, end: End::Arrow(Across::Text(t)) };
        let open = Term { facing: Dir::W, end: End::Open(Across::Text(t)) };
        let parts = [part(true, arrow)?, part(false, open)?];
        expected.wires.extend(parts);
        expected.wires.sort();
        let owner = owner.and_then(|b| map.boxes[b]);
        let text = TextPart { kind: TextKind::Word, content: word.to_owned(), owner, title_of: None, divider_of: None };
        (t <= expected.texts.len()).then(|| expected.texts.insert(t, text))
    })
}

/// The cells of wire `w` on `start`'s side once the cells in `cut` are
/// gone, following the wire's own links.
fn sides(model: &Model, w: usize, cut: &HashSet<Pos>, start: Pos) -> HashSet<Pos> {
    let wire = &model.diagram.wires[w];
    let cells: HashMap<Pos, u8> = wire.cells.iter().copied().zip(wire.links.iter().copied()).collect();
    let mut seen = HashSet::from([start]);
    let mut todo = vec![start];
    while let Some(p) = todo.pop() {
        for d in Dir::ALL {
            if cells.get(&p).is_none_or(|m| m & d.bit() == 0) {
                continue;
            }
            if let Some(q) = p.step(d).filter(|q| cells.contains_key(q) && !cut.contains(q))
                && seen.insert(q)
            {
                todo.push(q);
            }
        }
    }
    seen
}

/// The structure term of wire end `e`, as [`Structure::of`] records it.
fn end_term(model: &Model, e: &shawline_shape::WireEnd) -> Term {
    use shawline_shape::EndKind;
    let text = |dir: Dir| across(model.grid, &model.texts, e.pos, dir).1;
    let across_text = |dir: Dir| -> Across {
        if !dir.is_horizontal() {
            return Across::Nothing;
        }
        match text(dir) {
            Some(t) => Across::Text(t),
            None => {
                let (gap, _) = across(model.grid, &model.texts, e.pos, dir);
                let beyond = gap.last().copied().unwrap_or(e.pos).step(dir);
                match beyond.filter(|q| q.col < model.grid.row(q.row).len()) {
                    Some(q) if model.diagram.strokes.get(q).is_some() => Across::Stroke,
                    _ => Across::Nothing,
                }
            }
        }
    };
    let end = match e.kind {
        EndKind::Port(p) => shawline_shape::structure::port_index(&model.diagram.boxes, p)
            .map_or(End::Blocked, |(b, k)| End::Port(b, k)),
        EndKind::Arrow => End::Arrow(across_text(e.facing)),
        EndKind::Open => End::Open(across_text(e.facing)),
        EndKind::Blocked => End::Blocked,
    };
    Term { facing: e.facing, end }
}

/// Splices out the inline node whose word is text `t` of the given sheet,
/// named at given cell `at`: the wire into it runs straight on through the
/// cells it held.
pub(crate) fn splice(sheets: &Sheets, t: usize, at: Pos) -> Result<Doc, Refusal> {
    let (grid, diagram) = sheets.canon();
    let model = Model::new(grid, diagram);
    let text = &diagram.texts[t];
    let (before, _) = across(grid, &model.texts, text.pos, Dir::W);
    let arrow = before.last().copied().unwrap_or(text.pos).step(Dir::W).expect("an inline node has its arrowhead");
    let last = Pos::new(text.pos.row, text.pos.col + text.width - 1);
    let (after, _) = across(grid, &model.texts, last, Dir::E);
    let end = after.last().copied().unwrap_or(last);
    let into = model.wire_of(crate::model::Term::Arrow(arrow));
    let out = model.wire_of(crate::model::Term::Text(text.pos, Dir::W));
    let (Some(into), Some(out)) = (into, out) else {
        let why = "this inline node isn't wired in and out; edit it in the text pane";
        return Err(Refusal::new(at, RefusalKind::Target, why));
    };
    let mut canvas = Canvas::new(grid);
    for col in arrow.col..=end.col {
        canvas.set(Pos::new(arrow.row, col), '─');
    }
    let mut expected = Structure::of(grid, diagram);
    let input = Structure::wire(grid, diagram, into);
    let output = Structure::wire(grid, diagram, out);
    let arrow_term = Term { facing: Dir::E, end: End::Arrow(Across::Text(t)) };
    let open_term = Term { facing: Dir::W, end: End::Open(Across::Text(t)) };
    let mut terms: Vec<Term> =
        input.terms.iter().chain(&output.terms).copied().filter(|&x| x != arrow_term && x != open_term).collect();
    terms.sort();
    expected.remove_part(&input);
    expected.remove_part(&output);
    expected.wires.push(WirePart { terms, tees: input.tees + output.tees, crossings: input.crossings + output.crossings });
    expected.wires.sort();
    let what = "the inline node can't be deleted";
    finish(canvas.render(), diagram, at, what, |map| expected.relabel(map))
}
