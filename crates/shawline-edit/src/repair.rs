//! Known fixes for diagnostics (PLAN P10, "Repairs"): the code-to-repair
//! table behind [`crate::repairs`].
//!
//! Each diagnostic gets at most one repair. A repair with an edit is
//! offered only when that edit, tried on the sheet, succeeds, clears its
//! diagnostic and adds no error code but those its follow-up resolves: the
//! contract promises that of every repair it offers, and on a sheet broken
//! in several ways an edit the table names may not keep it. Where the table
//! names two edits (`Y003`), the first that keeps it is offered. The trial
//! result also says where a follow-up starts, since a repair may add the
//! arrowhead its follow-up wires.

use std::collections::BTreeMap;

use shawline_doc::Doc;
use shawline_grid::{Code, Diagnostic, Dir, Pos, Severity};
use shawline_shape::{EndKind, PortKind, TextKind};

use crate::drawing::items;
use crate::model::{Model, Term};
use crate::sheet::across;
use crate::{BoxKind, Edit, Edited, Followup, Repair, apply, palette};

/// The name `C005`'s repair gives the entry point, without its namer dot.
const MAIN: &str = "𐑥𐑱𐑯";

pub(crate) fn repairs(doc: &Doc) -> Vec<Repair> {
    let (Ok(grid), Some(diagram)) = (doc.grid(), doc.diagram()) else { return Vec::new() };
    let model = Model::new(grid, diagram);
    let before = doc.diagnostics();
    doc.diagnostics().iter().filter_map(|d| offer(doc, &model, before, d)).collect()
}

/// A repair to try: its label, its edit, and how to find its follow-up's
/// cell on the edit's result.
struct Candidate {
    label: String,
    edit: Option<Edit>,
    then: Then,
}

/// Where a candidate's follow-up starts.
enum Then {
    None,
    /// At this cell of the sheet as given.
    NameField(Pos),
    DrawWire(Pos),
    /// At the first port the edit added that has no wire yet: a node's
    /// new input, or a default arm's first through-port.
    NewArrowhead,
}

fn offer(doc: &Doc, model: &Model, before: &[Diagnostic], d: &Diagnostic) -> Option<Repair> {
    let candidates = candidates(doc, model, d);
    for candidate in candidates {
        let Some(edit) = candidate.edit else {
            let then = match candidate.then {
                Then::NameField(at) => Followup::NameField(at),
                Then::DrawWire(at) => Followup::DrawWire(at),
                Then::None | Then::NewArrowhead => Followup::None,
            };
            return Some(Repair { at: d.at, code: d.code, label: candidate.label, edit: None, then });
        };
        let Ok(edited) = apply(doc, &edit) else { continue };
        let then = match candidate.then {
            Then::None => Followup::None,
            Then::NameField(at) => Followup::NameField(at),
            Then::DrawWire(at) => Followup::DrawWire(at),
            Then::NewArrowhead => match new_arrowhead(doc, &edited) {
                Some(at) => Followup::DrawWire(at),
                None => continue,
            },
        };
        if keeps(before, edited.doc.diagnostics(), d.code, then) {
            return Some(Repair { at: d.at, code: d.code, label: candidate.label, edit: Some(edit), then });
        }
    }
    None
}

/// Whether `after` has fewer `code`s than `before` and no more of any error
/// code, but those `then` resolves.
fn keeps(before: &[Diagnostic], after: &[Diagnostic], code: Code, then: Followup) -> bool {
    let count = |ds: &[Diagnostic]| ds.iter().filter(|d| d.code == code).count();
    if count(after) >= count(before) {
        return false;
    }
    let errors = |ds: &[Diagnostic]| {
        let mut counts: BTreeMap<Code, usize> = BTreeMap::new();
        for d in ds.iter().filter(|d| d.severity() == Severity::Error) {
            *counts.entry(d.code).or_default() += 1;
        }
        counts
    };
    let resolved: &[Code] = match then {
        Followup::DrawWire(_) => &["Y019"],
        Followup::None | Followup::NameField(_) => &[],
    };
    let old = errors(before);
    errors(after).into_iter().all(|(code, n)| n <= old.get(code).copied().unwrap_or(0) || resolved.contains(&code))
}

/// The first unwired arrowhead of a box that `edited` gave more of them
/// than `doc` had: the port a repair added for its follow-up to wire.
/// Ports are row-major, and a box gains them below the ones it has.
fn new_arrowhead(doc: &Doc, edited: &Edited) -> Option<Pos> {
    let had = sinks_per_box(doc.canonical()?);
    let (grid, diagram) = (edited.doc.grid().ok()?, edited.doc.diagram()?);
    let model = Model::new(grid, diagram);
    let mut fresh: Vec<Pos> = Vec::new();
    for (b, shape) in diagram.boxes.iter().enumerate() {
        let before = edited.map.boxes.iter().position(|&m| m == Some(b)).map_or(0, |o| had[o]);
        let sinks = shape.ports.iter().filter_map(|p| model.sink(p.pos).ok()).skip(before);
        fresh.extend(sinks.filter(|s| model.wire_of(s.term()).is_none()).map(|s| s.at));
    }
    fresh.sort();
    fresh.first().copied()
}

/// How many sinks each box of `doc` has on its border.
fn sinks_per_box(doc: &Doc) -> Vec<usize> {
    let (Ok(grid), Some(diagram)) = (doc.grid(), doc.diagram()) else { return Vec::new() };
    let model = Model::new(grid, diagram);
    diagram.boxes.iter().map(|shape| shape.ports.iter().filter(|p| model.sink(p.pos).is_ok()).count()).collect()
}

/// The repairs the table names for `d`, in the order to try them.
fn candidates(doc: &Doc, model: &Model, d: &Diagnostic) -> Vec<Candidate> {
    let diagram = model.diagram;
    let at = d.at;
    let one = |label: String, edit: Option<Edit>, then: Then| vec![Candidate { label, edit, then }];
    let text = model.text_at(at).map(|t| &diagram.texts[t]);
    let corner_box = model.box_at(at);
    match d.code {
        "S012" => one("Move the slipped border cell home".to_owned(), Some(Edit::Fix(at)), Then::None),
        "Y003" | "Y004" => dangling(model, d),
        "Y009" => one("Fix…: name the port".to_owned(), None, Then::NameField(at)),
        "Y013" => one("Fix…: rename the definition: its name is reserved".to_owned(), None, Then::NameField(at)),
        "C004" => {
            one("Fix…: rename the definition: another definition has this name".to_owned(), None, Then::NameField(at))
        }
        "Y012" => {
            let Some(t) = model.text_at(at) else { return Vec::new() };
            if model.constant(t).is_some() {
                one("Fix…: wire the constant".to_owned(), None, Then::DrawWire(at))
            } else {
                one("Remove the stray text".to_owned(), Some(Edit::RemoveText(at)), Then::None)
            }
        }
        "Y019" => one("Fix…: wire the arrowhead".to_owned(), None, Then::DrawWire(at)),
        "Y001" => match text {
            Some(t) if t.title_of.is_some() && t.kind == TextKind::Word && !t.content.contains('·') => {
                let content = format!("·{}", t.content);
                one("Add the namer dot".to_owned(), Some(Edit::SetText { at, content }), Then::None)
            }
            _ => Vec::new(),
        },
        "Y014" | "Y015" | "Y018" => {
            let Some(b) = corner_box else { return Vec::new() };
            let rounded = diagram.boxes[b].parent.is_none();
            let label = match (d.code, rounded) {
                ("Y015", true) => "Make the corners agree: round them all",
                ("Y015", false) => "Make the corners agree: square them all",
                (_, true) => "Round the corners: a definition is a rounded box",
                (_, false) => "Square the corners: a node or switch is a square box",
            };
            one(label.to_owned(), Some(Edit::Restyle { corner: at, rounded }), Then::None)
        }
        "C001" => {
            let Some(t) = text else { return Vec::new() };
            let near: Vec<String> = palette(doc, None)
                .into_iter()
                .map(|c| c.word)
                .filter(|w| distance(w, &t.content) == 1)
                .collect();
            match &near[..] {
                [name] => one(format!("Did you mean {name}?"), Some(Edit::SetText { at, content: name.clone() }), Then::None),
                _ => Vec::new(),
            }
        }
        "C002" => arity(doc, model, at),
        "C005" => {
            let top = items(model.grid, diagram).last().map_or(0, |&(_, last)| last + 2);
            let rect = shawline_grid::Rect { top, left: 0, bottom: top + 4, right: 20 };
            let label = format!("Add ·{MAIN} below the sheet");
            one(label, Some(Edit::PlaceDefinition { rect, name: MAIN.to_owned() }), Then::None)
        }
        "C006" => {
            let Some(b) = diagram.texts.iter().find(|t| t.pos == at).and_then(|t| t.title_of) else { return Vec::new() };
            let Some(port) = diagram.boxes[b].ports.iter().find(|p| p.side == Dir::W && p.kind == PortKind::Arrow) else {
                return Vec::new();
            };
            let label = match input_name(model, port.pos) {
                Some(name) => format!("Remove ·{MAIN}'s input {name}"),
                None => format!("Remove ·{MAIN}'s input"),
            };
            one(label, Some(Edit::RemovePort(port.pos)), Then::None)
        }
        "W001" => {
            let Some((b, _)) = model.port(at) else { return Vec::new() };
            let r = diagram.boxes[b].rect;
            let corner = Pos::new(r.top, r.left);
            let label = "Fix…: add a default arm, then wire its value".to_owned();
            one(label, Some(Edit::AddDefaultArm(corner)), Then::NewArrowhead)
        }
        "W002" => {
            let label = match input_name(model, at) {
                Some(name) => format!("Remove the unused input {name}"),
                None => "Remove the unused input".to_owned(),
            };
            one(label, Some(Edit::RemovePort(at)), Then::None)
        }
        _ => Vec::new(),
    }
}

/// The name after a definition input's arrowhead at `port`.
fn input_name(model: &Model, port: Pos) -> Option<String> {
    let t = across(model.grid, &model.texts, port, Dir::E).1?;
    Some(model.diagram.texts[t].content.clone())
}

/// `Y003` and `Y004`: connect the dangling end to the nearest free
/// arrowhead in its scope, or else remove the wire, with the constant
/// that feeds it if one does.
fn dangling(model: &Model, d: &Diagnostic) -> Vec<Candidate> {
    let diagram = model.diagram;
    let Some(w) = diagram.wires.iter().position(|w| w.ends.iter().any(|e| e.pos == d.at)) else { return Vec::new() };
    let mut out = Vec::new();
    let source = model.terms[w].iter().find_map(|&t| match t {
        Term::Port(p, Dir::W) | Term::Text(p, Dir::W) => Some(p),
        _ => None,
    });
    let end = diagram.wires[w].ends.iter().find(|e| e.pos == d.at && e.kind == EndKind::Open);
    if d.code == "Y003"
        && let (Some(source), Some(end)) = (source, end)
        && let Ok(from) = model.source(source)
    {
        let (_, sinks) = model.endpoints();
        let nearest = sinks
            .iter()
            .filter(|(s, wired)| !wired && s.scope == from.scope)
            .min_by_key(|(s, _)| (s.at.row.abs_diff(end.pos.row) + s.at.col.abs_diff(end.pos.col), s.at));
        if let Some((sink, _)) = nearest {
            let label = "Connect the wire to the nearest free arrowhead".to_owned();
            out.push(Candidate { label, edit: Some(Edit::Connect { from: d.at, to: sink.at }), then: Then::None });
        }
    }
    let constant = source.and_then(|p| model.text_at(p)).filter(|&t| model.constant(t).is_some());
    if let Some(t) = constant {
        let label = "Remove the dangling wire and the constant that feeds it".to_owned();
        out.push(Candidate { label, edit: Some(Edit::RemoveText(diagram.texts[t].pos)), then: Then::None });
    }
    out.push(Candidate { label: "Remove the dangling wire".to_owned(), edit: Some(Edit::RemoveWire(d.at)), then: Then::None });
    out
}

/// `C002`: the node's ports matched to its callee's. A callee that takes
/// more inputs leaves new arrowheads, which the follow-up wires.
fn arity(doc: &Doc, model: &Model, at: Pos) -> Vec<Candidate> {
    let diagram = model.diagram;
    let Some(t) = model.text_at(at) else { return Vec::new() };
    let Some(b) = diagram.texts[t].owner.filter(|&b| model.kind(b) == BoxKind::Node && !model.inline_word(t)) else {
        return Vec::new();
    };
    let word = &diagram.texts[t].content;
    let Some(callee) = palette(doc, None).into_iter().find(|c| &c.word == word) else { return Vec::new() };
    let shape = &diagram.boxes[b];
    let has = shape.ports.iter().filter(|p| p.side == Dir::W).count();
    let r = shape.rect;
    let edit = Some(Edit::SetPorts { corner: Pos::new(r.top, r.left), inputs: callee.inputs, outputs: callee.outputs });
    if usize::from(callee.inputs) > has {
        let label = format!("Fix…: match {word}'s ports, then wire the new input");
        vec![Candidate { label, edit, then: Then::NewArrowhead }]
    } else {
        vec![Candidate { label: format!("Match {word}'s ports"), edit, then: Then::None }]
    }
}

/// The edit distance between two words, in letters: insertions,
/// deletions and substitutions.
fn distance(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, &x) in a.iter().enumerate() {
        let mut diagonal = row[0];
        row[0] = i + 1;
        for (j, &y) in b.iter().enumerate() {
            let above = row[j + 1];
            row[j + 1] = (above + 1).min(row[j] + 1).min(diagonal + usize::from(x != y));
            diagonal = above;
        }
    }
    row[b.len()]
}

#[cfg(test)]
mod tests {
    use super::distance;

    #[test]
    fn distance_counts_letters() {
        assert_eq!(distance("𐑛𐑳𐑚𐑤", "𐑛𐑳𐑚𐑩𐑤"), 1);
        assert_eq!(distance("𐑓𐑵", "𐑐"), 2);
        assert_eq!(distance("𐑐", "𐑚"), 1);
        assert_eq!(distance("𐑯𐑦𐑤", "𐑯𐑦𐑤"), 0);
    }
}
