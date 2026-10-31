//! The Studio edit sequences the gates walk, and the `.studio` dump that
//! records how each step ends (PLAN P8 and P10, "Oracle first").
//!
//! Per valid program, all from the canonical text:
//!
//! - **rewire:** remove every wire the mouse can edit, then connect each
//!   original (source, sink) pair again, sinks in row-major order;
//! - **rebuild,** per node box whose wires are all editable: remove it,
//!   place it again at its corner, and reconnect its wires, incoming
//!   branches first;
//! - **move,** per node box: one cell up, down, left and right, each from
//!   the original text;
//! - **room:** each move refused as blocked, retried with `make_room`;
//! - **insert,** per definition: a row before its bottom border, a column at
//!   its right border, and a row and a column through its middle, each
//!   inserted and deleted again;
//! - **retext,** per number constant: +1, then ×10, then set back;
//! - **build:** the whole program from an empty sheet;
//! - **extract,** per node box: extract it on its own into a new definition
//!   named [`EXTRACTED`].
//!
//! A sequence stops at its first refusal. The dump has one line per step:
//! the step, then `ok` with a hash of the resulting text, or `refused` with
//! the refusal's kind and the cell it points at. Wording is never recorded,
//! as with diagnostics. The hash makes a refactor that should change nothing prove
//! it: any different text, even an equally good route, shows as a diff.

use std::fmt::Write;

use shawline_doc::Doc;
use shawline_edit::{Axis, Edit, Insertion, RefusalKind, apply};
use shawline_grid::{Dir, Grid, Pos};
use shawline_shape::{PortKind, TextKind, recognize};
use shawline_syntax::ast::{Endpoint, Graph, NodeKind, Sheet};

/// One wire, named by cells as the edit contract names endpoints.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Link {
    pub from: Pos,
    /// Sink cells, row-major.
    pub to: Vec<Pos>,
    /// Editable in Phase 1: no switch output and no inline node involved.
    pub editable: bool,
    /// An index into `Diagram::wires`.
    pub wire: usize,
}

/// Every wire of `sheet`, in every definition and every switch arm.
pub fn links(sheet: &Sheet) -> Vec<Link> {
    let mut out = Vec::new();
    for def in &sheet.definitions {
        let params: Vec<Pos> = def.inputs.iter().map(|p| p.arrow).collect();
        let results: Vec<Pos> = def.outputs.iter().map(|p| p.arrow).collect();
        scope(&def.body, &params, &results, &mut out);
    }
    out
}

fn scope(graph: &Graph, params: &[Pos], results: &[Pos], out: &mut Vec<Link>) {
    for node in &graph.nodes {
        if let NodeKind::Switch(switch) = &node.kind {
            for arm in &switch.arms {
                scope(&arm.body, &arm.params, &arm.results, out);
            }
        }
    }
    for edge in &graph.edges {
        let (from, mut editable) = match edge.from {
            Endpoint::Param(i) => (params[i as usize], true),
            Endpoint::Output(n, k) => {
                let node = &graph.nodes[n];
                match &node.kind {
                    NodeKind::Const(_) => (Pos::new(node.rect.top, node.rect.left), true),
                    NodeKind::Call(_) => (node.outputs[k as usize], true),
                    _ => (node.outputs[k as usize], false),
                }
            }
            other => panic!("a wire can't start at {other:?}"),
        };
        let mut to = Vec::new();
        for sink in &edge.to {
            match *sink {
                Endpoint::Result(i) => to.push(results[i as usize]),
                Endpoint::Input(n, k) => {
                    let node = &graph.nodes[n];
                    editable &= !matches!(node.kind, NodeKind::Inline(_));
                    to.push(node.inputs[k as usize]);
                }
                other => panic!("a wire can't end at {other:?}"),
            }
        }
        to.sort();
        out.push(Link { from, to, editable, wire: edge.wire });
    }
}

/// A boxed call: its word, top-left corner, input cells and output cells.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Call {
    pub word: String,
    pub corner: Pos,
    pub inputs: Vec<Pos>,
    pub outputs: Vec<Pos>,
}

/// Every boxed call in `sheet`, arms included, in sheet order.
pub fn calls(sheet: &Sheet) -> Vec<Call> {
    let mut out = Vec::new();
    for def in &sheet.definitions {
        collect_calls(&def.body, &mut out);
    }
    out
}

fn collect_calls(graph: &Graph, out: &mut Vec<Call>) {
    for node in &graph.nodes {
        match &node.kind {
            NodeKind::Call(name) => out.push(Call {
                word: name.text.clone(),
                corner: Pos::new(node.rect.top, node.rect.left),
                inputs: node.inputs.clone(),
                outputs: node.outputs.clone(),
            }),
            NodeKind::Switch(switch) => {
                for arm in &switch.arms {
                    collect_calls(&arm.body, out);
                }
            }
            _ => {}
        }
    }
}

/// Top-left corners of node boxes as recognition sees them: square boxes
/// inside a definition that aren't switches (no dividers, no selector on
/// the top edge).
pub fn node_corners(src: &str) -> Vec<Pos> {
    let diagram = recognize(&Grid::parse(src).expect("a valid program"));
    diagram
        .boxes
        .iter()
        .filter(|b| b.parent.is_some() && b.dividers.is_empty())
        .filter(|b| !b.ports.iter().any(|p| p.side == Dir::N && p.kind == PortKind::Arrow))
        .map(|b| Pos::new(b.rect.top, b.rect.left))
        .collect()
}

/// The rewire sequence for `src`, or `None` when it has no editable wire.
/// Wires never move when others are removed, so each removal's cell stays
/// valid.
pub fn rewire(src: &str, sheet: &Sheet) -> Option<Vec<Edit>> {
    let editable: Vec<Link> = links(sheet).into_iter().filter(|l| l.editable).collect();
    if editable.is_empty() {
        return None;
    }
    let diagram = recognize(&Grid::parse(src).expect("a valid program"));
    let mut steps: Vec<Edit> =
        editable.iter().map(|l| Edit::RemoveWire(diagram.wires[l.wire].cells[0])).collect();
    let mut connects: Vec<(Pos, Pos)> =
        editable.iter().flat_map(|l| l.to.iter().map(|&to| (l.from, to))).collect();
    connects.sort_by_key(|&(_, to)| to);
    steps.extend(connects.into_iter().map(|(from, to)| Edit::Connect { from, to }));
    Some(steps)
}

/// The rebuild sequence for each node box whose wires are all editable.
pub fn rebuilds(sheet: &Sheet) -> Vec<(Call, Vec<Edit>)> {
    let all = links(sheet);
    let mut out = Vec::new();
    for call in calls(sheet) {
        let touching: Vec<&Link> = all
            .iter()
            .filter(|l| call.outputs.contains(&l.from) || l.to.iter().any(|t| call.inputs.contains(t)))
            .collect();
        if touching.iter().any(|l| !l.editable) {
            continue;
        }
        let mut steps = vec![
            Edit::RemoveNode(call.corner),
            Edit::PlaceNode {
                corner: call.corner,
                word: call.word.clone(),
                inputs: call.inputs.len() as u16,
                outputs: call.outputs.len() as u16,
            },
        ];
        // Incoming branches first, then everything the node feeds.
        for l in &touching {
            for &to in l.to.iter().filter(|t| call.inputs.contains(t)) {
                steps.push(Edit::Connect { from: l.from, to });
            }
        }
        for l in touching.iter().filter(|l| call.outputs.contains(&l.from)) {
            steps.extend(l.to.iter().map(|&to| Edit::Connect { from: l.from, to }));
        }
        out.push((call, steps));
    }
    out
}

/// One move per node box and direction: up, down, left, right.
pub fn moves(src: &str) -> Vec<Edit> {
    node_corners(src)
        .into_iter()
        .flat_map(|corner| {
            [(-1, 0), (1, 0), (0, -1), (0, 1)].map(|(dr, dc)| Edit::Move { corner, dr, dc })
        })
        .collect()
}

/// Each move [`moves`] lists that is refused as blocked, retried with
/// `make_room`.
pub fn rooms(src: &str) -> Vec<Edit> {
    let doc = Doc::new(src);
    moves(src)
        .into_iter()
        .filter(|m| apply(&doc, m).is_err_and(|r| matches!(r.kind, RefusalKind::Blocked(_))))
        .map(|m| Edit::MakeRoom(Box::new(m)))
        .collect()
}

/// Per definition, by its corner: a row before its bottom border, a column
/// at its right border, and a row and a column through its middle, each
/// inserted and deleted again.
pub fn insertions(sheet: &Sheet) -> Vec<(Pos, Vec<[Edit; 2]>)> {
    sheet
        .definitions
        .iter()
        .map(|def| {
            let r = def.rect;
            let (middle_row, middle_col) = ((r.top + r.bottom) / 2, (r.left + r.right) / 2);
            let pairs = [
                (Axis::Row, Pos::new(r.bottom, r.left)),
                (Axis::Column, Pos::new(r.top, r.right)),
                (Axis::Row, Pos::new(middle_row, r.left)),
                (Axis::Column, Pos::new(r.top, middle_col)),
            ]
            .map(|(axis, at)| {
                let i = Insertion { axis, at, count: 1 };
                [Edit::Insert(i), Edit::Delete(i)]
            });
            (Pos::new(r.top, r.left), pairs.into())
        })
        .collect()
}

/// Per number constant: +1, then ×10, then set back. Each step names the
/// text where the step before left it: a constant grows and shrinks at its
/// left end.
pub fn retexts(src: &str) -> Vec<Vec<Edit>> {
    let diagram = recognize(&Grid::parse(src).expect("a valid program"));
    let doc = Doc::new(src);
    let mut constants: Vec<Pos> = Vec::new();
    for def in &doc.sheet().expect("a valid program").definitions {
        collect_constants(&def.body, &mut constants);
    }
    constants.sort();
    let mut out = Vec::new();
    for at in constants {
        let Some(text) = diagram.texts.iter().find(|t| t.pos == at && t.kind == TextKind::Number) else {
            continue;
        };
        let Ok(n) = text.content.parse::<i64>() else { continue };
        let Some(up) = n.checked_add(1) else { continue };
        let Some(wide) = up.checked_mul(10) else { continue };
        let end = at.col + text.width - 1;
        let mut now = text.content.clone();
        let mut steps = Vec::new();
        for content in [up.to_string(), wide.to_string(), text.content.clone()] {
            let start = Pos::new(at.row, end + 1 - now.chars().count());
            now.clone_from(&content);
            steps.push(Edit::SetText { at: start, content });
        }
        out.push(steps);
    }
    out
}

fn collect_constants(graph: &Graph, out: &mut Vec<Pos>) {
    for node in &graph.nodes {
        match &node.kind {
            NodeKind::Const(_) => out.push(Pos::new(node.rect.top, node.rect.left)),
            NodeKind::Switch(switch) => {
                for arm in &switch.arms {
                    collect_constants(&arm.body, out);
                }
            }
            _ => {}
        }
    }
}

/// The steps that rebuild `src` from an empty sheet (PLAN P10, gate
/// `studio_build`), each phase in row-major order:
///
/// 1. top-level comments;
/// 2. each definition, then its ports;
/// 3. the boxes inside definitions: a node, or a switch followed by its
///    selector and through-ports and a divider per arm after the first;
/// 4. constants and comments inside boxes;
/// 5. every connection: first each inline chain's, from its first source
///    straight through to its sink, followed at once by the chain's inline
///    nodes, each placed on that wire; then the rest, each group with sinks
///    in row-major order. Placing a chain's nodes before the next chain
///    connects leaves its source's wire ending at the first node, so a
///    second chain from that source tees off ahead of the nodes rather than
///    beside the sinks, and its own stretch is straight for its nodes. A
///    chain whose first nodes are already placed connects from the last of
///    them. A branch that fans out beside a chain tees off the chain's wire
///    in the same way.
pub fn build(src: &str, sheet: &Sheet) -> Vec<Edit> {
    let grid = Grid::parse(src).expect("a valid program");
    let diagram = recognize(&grid);
    let raw = |t: &shawline_shape::Text| -> String {
        (0..t.width).map(|k| grid.get(Pos::new(t.pos.row, t.pos.col + k))).collect()
    };
    let mut steps = Vec::new();

    // 1. Top-level comments.
    let mut texts: Vec<&shawline_shape::Text> = diagram.texts.iter().collect();
    texts.sort_by_key(|t| t.pos);
    for t in texts.iter().filter(|t| t.kind == TextKind::Comment && t.owner.is_none()) {
        steps.push(Edit::PlaceText { at: t.pos, content: raw(t) });
    }

    // 2. Definitions and their ports.
    for def in &sheet.definitions {
        steps.push(Edit::PlaceDefinition { rect: def.rect, name: def.name.text.clone() });
        let mut ports: Vec<(Pos, String)> =
            def.inputs.iter().chain(&def.outputs).map(|p| (p.arrow, p.name.text.clone())).collect();
        ports.sort();
        steps.extend(ports.into_iter().map(|(at, name)| Edit::AddPort { at, name: Some(name) }));
    }

    // 3. Boxes inside definitions.
    let calls = calls(sheet);
    let mut inner: Vec<usize> = (0..diagram.boxes.len()).filter(|&b| diagram.boxes[b].parent.is_some()).collect();
    inner.sort_by_key(|&b| (diagram.boxes[b].rect.top, diagram.boxes[b].rect.left));
    for b in inner {
        let shape = &diagram.boxes[b];
        let corner = Pos::new(shape.rect.top, shape.rect.left);
        let switch = !shape.dividers.is_empty()
            || shape.ports.iter().any(|p| p.side == Dir::N && p.kind == PortKind::Arrow);
        if !switch {
            if let Some(call) = calls.iter().find(|c| c.corner == corner) {
                steps.push(Edit::PlaceNode {
                    corner,
                    word: call.word.clone(),
                    inputs: call.inputs.len() as u16,
                    outputs: call.outputs.len() as u16,
                });
            }
            continue;
        }
        let guard = |row: Option<usize>| {
            diagram
                .texts
                .iter()
                .find(|t| match row {
                    None => t.title_of == Some(b),
                    Some(row) => t.divider_of == Some(b) && t.pos.row == row,
                })
                .map(raw)
        };
        steps.push(Edit::PlaceSwitch { rect: shape.rect, guard: guard(None) });
        steps.extend(shape.ports.iter().map(|p| Edit::AddPort { at: p.pos, name: None }));
        for d in &shape.dividers {
            steps.push(Edit::AddArm { at: Pos::new(d.row, shape.rect.left), guard: guard(Some(d.row)) });
        }
    }

    // 4. Constants and comments inside boxes.
    let mut constants = Vec::new();
    for def in &sheet.definitions {
        collect_constants(&def.body, &mut constants);
    }
    for t in &texts {
        let inside = t.kind == TextKind::Comment && t.owner.is_some();
        if inside || constants.contains(&t.pos) {
            steps.push(Edit::PlaceText { at: t.pos, content: raw(t) });
        }
    }

    // 5. Connections, each inline chain connected straight through and its
    // nodes placed at once.
    let mut inline = Vec::new();
    for def in &sheet.definitions {
        collect_inline(&def.body, &mut inline);
    }
    let all = links(sheet);
    let feeds = |sink: Pos| all.iter().find(|l| l.to.contains(&sink)).map(|l| l.from);
    // The inline nodes between a sink's first source and the sink, first
    // source first.
    let chain = |to: Pos| {
        let mut nodes = Vec::new();
        let mut from = feeds(to).expect("every sink of a valid program is wired");
        while let Some(node) = inline.iter().find(|n| n.output == from) {
            nodes.push(node);
            match feeds(node.input) {
                Some(source) => from = source,
                None => break,
            }
        }
        nodes.reverse();
        (from, nodes)
    };
    let mut sinks: Vec<(bool, Pos, Pos)> = all
        .iter()
        .flat_map(|l| l.to.iter().map(move |&to| (l.from, to)))
        .filter(|&(_, to)| !inline.iter().any(|n| n.input == to))
        .map(|(from, to)| (!inline.iter().any(|n| n.output == from), from, to))
        .collect();
    sinks.sort_by_key(|&(later, _, to)| (later, to));
    let mut placed: Vec<Pos> = Vec::new();
    for (later, from, to) in sinks {
        if later {
            steps.push(Edit::Connect { from, to });
            continue;
        }
        let (source, nodes) = chain(to);
        let done = nodes.iter().rposition(|n| placed.contains(&n.at));
        // A connection names an inline node's output by its word.
        let from = done.map_or(source, |i| nodes[i].at);
        steps.push(Edit::Connect { from, to });
        for n in &nodes[done.map_or(0, |i| i + 1)..] {
            steps.push(Edit::PlaceInline { at: n.at, word: n.word.clone() });
            placed.push(n.at);
        }
    }
    steps
}

/// An inline node: its word's first cell, the word, and its input and
/// output cells.
struct Inline {
    at: Pos,
    word: String,
    input: Pos,
    output: Pos,
}

fn collect_inline(graph: &Graph, out: &mut Vec<Inline>) {
    for node in &graph.nodes {
        match &node.kind {
            NodeKind::Inline(name) => out.push(Inline {
                at: Pos::new(node.rect.top, node.rect.left),
                word: name.text.clone(),
                input: node.inputs[0],
                output: node.outputs[0],
            }),
            NodeKind::Switch(switch) => {
                for arm in &switch.arms {
                    collect_inline(&arm.body, out);
                }
            }
            _ => {}
        }
    }
}

/// Runs `steps` from `src` until one is refused, writing a line per step.
fn walk(out: &mut String, indent: &str, src: &str, steps: &[Edit]) {
    let mut doc = Doc::new(src);
    for step in steps {
        match apply(&doc, step) {
            Ok(next) => {
                writeln!(out, "{indent}{step} ok {}", digest(next.doc.text())).unwrap();
                doc = next.doc;
            }
            Err(refusal) => {
                writeln!(out, "{indent}{step} refused {} {}", refusal.kind, refusal.at).unwrap();
                return;
            }
        }
    }
}

/// The `.studio` dump of a valid program.
pub fn dump(src: &str) -> String {
    let doc = Doc::new(src);
    let sheet = match doc.sheet() {
        Some(sheet) if !doc.has_errors() => sheet,
        _ => return "not a valid program\n".to_owned(),
    };
    let mut out = String::new();
    if let Some(steps) = rewire(src, sheet) {
        out.push_str("rewire\n");
        walk(&mut out, "  ", src, &steps);
    }
    for (call, steps) in rebuilds(sheet) {
        writeln!(out, "rebuild {} {}", call.word, call.corner).unwrap();
        walk(&mut out, "  ", src, &steps);
    }
    for step in moves(src) {
        walk(&mut out, "", src, &[step]);
    }
    let rooms = rooms(src);
    if !rooms.is_empty() {
        out.push_str("room\n");
        for step in rooms {
            walk(&mut out, "  ", src, &[step]);
        }
    }
    for (corner, pairs) in insertions(sheet) {
        writeln!(out, "insert {corner}").unwrap();
        for pair in pairs {
            walk(&mut out, "  ", src, &pair);
        }
    }
    let retexts = retexts(src);
    if !retexts.is_empty() {
        out.push_str("retext\n");
        for steps in retexts {
            walk(&mut out, "  ", src, &steps);
        }
    }
    out.push_str("build\n");
    walk(&mut out, "  ", EMPTY, &build(src, sheet));
    let extractions = extractions(src);
    if !extractions.is_empty() {
        out.push_str("extract\n");
        for step in extractions {
            walk(&mut out, "  ", src, &[step]);
        }
    }
    out
}

/// The name every extraction in `.studio` gives its new definition: 𐑐𐑸𐑑,
/// "part", which no corpus program uses.
pub const EXTRACTED: &str = "𐑐𐑸𐑑";

/// One extraction per node box, of that box on its own.
pub fn extractions(src: &str) -> Vec<Edit> {
    node_corners(src)
        .into_iter()
        .map(|corner| Edit::Extract { picks: vec![corner], name: EXTRACTED.to_owned() })
        .collect()
}

/// The sheet `build` starts from.
pub const EMPTY: &str = "";

/// A short stable hash of a text (64-bit FNV-1a), so a dump line says
/// which text an edit produced without holding it.
pub fn digest(text: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in text.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{:012x}", hash >> 16)
}
