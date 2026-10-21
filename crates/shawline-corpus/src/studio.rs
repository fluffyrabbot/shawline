//! The Studio edit sequences the gates walk, and the `.studio` dump that
//! records how each step ends (PLAN P8, "Oracle first").
//!
//! Three sequences per valid program, all from the canonical text:
//!
//! - **rewire:** remove every wire the mouse can edit, then connect each
//!   original (source, sink) pair again, sinks in row-major order;
//! - **rebuild,** per node box whose wires are all editable: remove it,
//!   place it again at its corner, and reconnect its wires, incoming
//!   branches first;
//! - **move,** per node box: one cell up, down, left and right, each from
//!   the original text.
//!
//! A sequence stops at its first refusal. The dump has one line per step:
//! the step, then `ok` with a hash of the resulting text, or `refused` with
//! the cell the refusal points at. Wording is never recorded, as with
//! diagnostics. The hash makes a refactor that should change nothing prove
//! it: any different text, even an equally good route, shows as a diff.

use std::fmt::{self, Write};

use shawline_grid::{Dir, Grid, Pos};
use shawline_shape::{PortKind, recognize};
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

/// One checked edit, as a gate or the dump applies it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    RemoveWire(Pos),
    Connect(Pos, Pos),
    RemoveNode(Pos),
    PlaceNode { corner: Pos, word: String, inputs: u16, outputs: u16 },
    Move { corner: Pos, dr: isize, dc: isize },
}

impl Step {
    /// Applies the step to `text`.
    pub fn apply(&self, text: &str) -> Result<String, shawline_edit::Refusal> {
        match self {
            Step::RemoveWire(at) => shawline_edit::remove_wire(text, *at),
            Step::Connect(from, to) => shawline_edit::connect(text, *from, *to),
            Step::RemoveNode(corner) => shawline_edit::remove_node(text, *corner),
            Step::PlaceNode { corner, word, inputs, outputs } => {
                shawline_edit::place_node(text, *corner, word, *inputs, *outputs)
            }
            Step::Move { corner, dr, dc } => shawline_edit::move_box(text, *corner, *dr, *dc),
        }
    }
}

impl fmt::Display for Step {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Step::RemoveWire(at) => write!(f, "remove_wire {at}"),
            Step::Connect(from, to) => write!(f, "connect {from} {to}"),
            Step::RemoveNode(corner) => write!(f, "remove_node {corner}"),
            Step::PlaceNode { corner, word, inputs, outputs } => {
                write!(f, "place_node {corner} {word} {inputs} {outputs}")
            }
            Step::Move { corner, dr, dc } => write!(f, "move {corner} {dr} {dc}"),
        }
    }
}

/// The rewire sequence for `src`, or `None` when it has no editable wire.
/// Wires never move when others are removed, so each removal's cell stays
/// valid.
pub fn rewire(src: &str, sheet: &Sheet) -> Option<Vec<Step>> {
    let editable: Vec<Link> = links(sheet).into_iter().filter(|l| l.editable).collect();
    if editable.is_empty() {
        return None;
    }
    let diagram = recognize(&Grid::parse(src).expect("a valid program"));
    let mut steps: Vec<Step> =
        editable.iter().map(|l| Step::RemoveWire(diagram.wires[l.wire].cells[0])).collect();
    let mut connects: Vec<(Pos, Pos)> =
        editable.iter().flat_map(|l| l.to.iter().map(|&to| (l.from, to))).collect();
    connects.sort_by_key(|&(_, to)| to);
    steps.extend(connects.into_iter().map(|(from, to)| Step::Connect(from, to)));
    Some(steps)
}

/// The rebuild sequence for each node box whose wires are all editable.
pub fn rebuilds(sheet: &Sheet) -> Vec<(Call, Vec<Step>)> {
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
            Step::RemoveNode(call.corner),
            Step::PlaceNode {
                corner: call.corner,
                word: call.word.clone(),
                inputs: call.inputs.len() as u16,
                outputs: call.outputs.len() as u16,
            },
        ];
        // Incoming branches first, then everything the node feeds.
        for l in &touching {
            for &to in l.to.iter().filter(|t| call.inputs.contains(t)) {
                steps.push(Step::Connect(l.from, to));
            }
        }
        for l in touching.iter().filter(|l| call.outputs.contains(&l.from)) {
            steps.extend(l.to.iter().map(|&to| Step::Connect(l.from, to)));
        }
        out.push((call, steps));
    }
    out
}

/// One move per node box and direction: up, down, left, right.
pub fn moves(src: &str) -> Vec<Step> {
    node_corners(src)
        .into_iter()
        .flat_map(|corner| {
            [(-1, 0), (1, 0), (0, -1), (0, 1)].map(|(dr, dc)| Step::Move { corner, dr, dc })
        })
        .collect()
}

/// Runs `steps` from `src` until one is refused, writing a line per step.
fn walk(out: &mut String, indent: &str, src: &str, steps: &[Step]) {
    let mut text = src.to_owned();
    for step in steps {
        match step.apply(&text) {
            Ok(next) => {
                writeln!(out, "{indent}{step} ok {}", digest(&next)).unwrap();
                text = next;
            }
            Err(refusal) => {
                writeln!(out, "{indent}{step} refused {}", refusal.at).unwrap();
                return;
            }
        }
    }
}

/// The `.studio` dump of a valid program.
pub fn dump(src: &str) -> String {
    let sheet = match shawline_cli::check(src) {
        Ok(checked) if !checked.has_errors() => checked.sheet,
        _ => return "not a valid program\n".to_owned(),
    };
    let mut out = String::new();
    if let Some(steps) = rewire(src, &sheet) {
        out.push_str("rewire\n");
        walk(&mut out, "  ", src, &steps);
    }
    for (call, steps) in rebuilds(&sheet) {
        writeln!(out, "rebuild {} {}", call.word, call.corner).unwrap();
        walk(&mut out, "  ", src, &steps);
    }
    for step in moves(src) {
        walk(&mut out, "", src, &[step]);
    }
    out
}

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
