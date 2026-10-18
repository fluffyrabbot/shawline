//! The P7 gate for Studio Phase 1 (PLAN P7), over every valid corpus
//! program:
//!
//! - **rewire:** remove every editable wire, connect each original (source,
//!   sink) pair again, and get the same program back;
//! - **rebuild:** for each node box, the palette offers it, and removing it,
//!   placing it again and reconnecting its wires gives the same program back;
//! - **trace:** every wire of `·𐑥𐑱𐑯` carries a value, and its outputs carry
//!   exactly what the program prints. A runtime-error program (80–89) may
//!   instead stop at the step budget: the budget keeps a live editor
//!   responsive, and a runaway like `84-heap-runaway` needs about 25M steps
//!   to reach its error.
//!
//! "The same program" means the same non-lint diagnostic codes, the same
//! output, the same wires (each source cell with its sink cells), and a
//! canonical text. Refusals are allowed, each with a reason, but for at most
//! a fifth of the programs (rewire) or nodes (rebuild).

use std::collections::BTreeSet;
use std::fs;
use std::time::{Duration, Instant};

use shawline_corpus::{STAGES, programs, root};
use shawline_grid::{Grid, Pos};
use shawline_syntax::ast::{Endpoint, Graph, NodeKind, Sheet};

fn stage(name: &str) -> fn(&str) -> String {
    STAGES.iter().find(|s| s.name == name).expect("registered stage").run
}

/// Non-lint diagnostic codes, sorted: positions legitimately change.
fn codes(src: &str) -> Vec<String> {
    let diagnostics = match shawline_cli::check(src) {
        Ok(checked) => checked.diagnostics,
        Err(errors) => errors,
    };
    let mut codes: Vec<String> =
        diagnostics.iter().filter(|d| !d.code.starts_with('L')).map(|d| d.code.to_owned()).collect();
    codes.sort();
    codes
}

/// Corpus programs with no static diagnostics, numbered below `limit`.
fn clean_programs(limit: u32) -> Vec<(String, String)> {
    programs(&root())
        .into_iter()
        .filter(|p| {
            let name = p.file_name().unwrap().to_string_lossy();
            name[..2].parse::<u32>().is_ok_and(|n| n < limit)
                && fs::read_to_string(p.with_extension("check")).is_ok_and(|c| c.is_empty())
        })
        .map(|p| (p.file_name().unwrap().to_string_lossy().into_owned(), fs::read_to_string(&p).unwrap()))
        .collect()
}

fn sheet(src: &str) -> Sheet {
    shawline_cli::check(src).expect("on the grid").sheet
}

/// One wire, named by cells as the edit contract names endpoints.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Link {
    from: Pos,
    to: Vec<Pos>,
    /// Editable in Phase 1: no switch output and no inline node involved.
    editable: bool,
    /// An index into `Diagram::wires`.
    wire: usize,
}

/// Every wire of `sheet`, in every definition and every switch arm.
fn links(sheet: &Sheet) -> Vec<Link> {
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

/// What must survive an edit: each wire as its source and sink cells.
fn wiring(src: &str) -> BTreeSet<(Pos, Vec<Pos>)> {
    links(&sheet(src)).into_iter().map(|l| (l.from, l.to)).collect()
}

/// Checks that `text` is the same program as `src`, describing the first
/// difference.
fn same_program(src: &str, text: &str) -> Result<(), String> {
    let out = stage("out");
    if codes(text) != codes(src) {
        return Err(format!("diagnostics change\n{text}"));
    }
    if out(text) != out(src) {
        return Err(format!("output changes\n{text}"));
    }
    if wiring(text) != wiring(src) {
        return Err(format!("wires change\n{text}"));
    }
    if shawline_cli::format(text).as_deref() != Ok(text) {
        return Err(format!("result isn't canonical\n{text}"));
    }
    Ok(())
}

/// Each sink of `link` as a `connect` call, sinks in row-major order.
fn pairs(link: &Link) -> Vec<(Pos, Pos)> {
    link.to.iter().map(|&to| (link.from, to)).collect()
}

#[test]
fn studio_rewire() {
    let (mut tried, mut refused, mut failures) = (0, 0, Vec::new());
    for (name, src) in clean_programs(80) {
        let all = links(&sheet(&src));
        let editable: Vec<&Link> = all.iter().filter(|l| l.editable).collect();
        if editable.is_empty() {
            continue;
        }
        tried += 1;
        let diagram = shawline_shape::recognize(&Grid::parse(&src).unwrap());
        // Wires never move when others are removed, so their cells stay valid.
        let cuts: Vec<Pos> = editable.iter().map(|l| diagram.wires[l.wire].cells[0]).collect();
        let mut connects: Vec<(Pos, Pos)> = editable.iter().flat_map(|l| pairs(l)).collect();
        connects.sort_by_key(|&(_, to)| to);

        let mut text = src.clone();
        let steps = cuts.iter().map(|&at| ("remove_wire", at, None)).chain(
            connects.iter().map(|&(from, to)| ("connect", from, Some(to))),
        );
        let mut refusal = None;
        for (what, at, to) in steps {
            let result = match to {
                None => shawline_edit::remove_wire(&text, at),
                Some(to) => shawline_edit::connect(&text, at, to),
            };
            match result {
                Ok(next) => text = next,
                Err(r) => {
                    refusal = Some(format!("{name}: {what} at {at} refused: {}", r.reason));
                    if r.reason.trim().is_empty() {
                        failures.push(format!("{name}: {what} at {at} refused without a reason"));
                    }
                    break;
                }
            }
        }
        match refusal {
            Some(_) => refused += 1,
            None => {
                if let Err(why) = same_program(&src, &text) {
                    failures.push(format!("{name}: rewired, but {why}"));
                }
            }
        }
    }
    assert!(tried >= 10, "only {tried} programs tried");
    assert!(failures.is_empty(), "{} of {tried} programs fail; first:\n{}", failures.len(), failures[0]);
    assert!(refused * 5 <= tried, "{refused} of {tried} programs refused: more than a fifth");
}

#[test]
fn studio_rebuild() {
    let (mut tried, mut refused, mut failures) = (0, 0, Vec::new());
    for (name, src) in clean_programs(80) {
        let original = sheet(&src);
        let all = links(&original);
        let mut boxes = Vec::new();
        for def in &original.definitions {
            collect_calls(&def.body, &mut boxes);
        }
        for (word, corner, inputs, outputs) in boxes {
            let touching: Vec<&Link> = all
                .iter()
                .filter(|l| outputs.contains(&l.from) || l.to.iter().any(|t| inputs.contains(t)))
                .collect();
            if touching.iter().any(|l| !l.editable) {
                continue;
            }
            tried += 1;
            let what = format!("{name}: {word} at {corner}");
            let from = touching.iter().find(|l| l.to.iter().any(|t| inputs.contains(t))).map(|l| l.from);
            let offered = shawline_edit::palette(&src, from);
            let arity = (inputs.len() as u16, outputs.len() as u16);
            if !offered.iter().any(|c| c.word == word && (c.inputs, c.outputs) == arity) {
                failures.push(format!("{what}: the palette doesn't offer it"));
                continue;
            }
            // Incoming branches first, then everything the node feeds.
            let mut connects: Vec<(Pos, Pos)> = touching
                .iter()
                .flat_map(|l| l.to.iter().filter(|t| inputs.contains(t)).map(|&t| (l.from, t)))
                .collect();
            connects.extend(touching.iter().filter(|l| outputs.contains(&l.from)).flat_map(|l| pairs(l)));

            let mut text = src.clone();
            let mut refusal = None;
            let removed = shawline_edit::remove_node(&text, corner)
                .and_then(|t| shawline_edit::place_node(&t, corner, &word, arity.0, arity.1));
            match removed {
                Ok(next) => text = next,
                Err(r) => refusal = Some(r),
            }
            if refusal.is_none() {
                for &(from, to) in &connects {
                    match shawline_edit::connect(&text, from, to) {
                        Ok(next) => text = next,
                        Err(r) => {
                            refusal = Some(r);
                            break;
                        }
                    }
                }
            }
            match refusal {
                Some(r) => {
                    refused += 1;
                    if r.reason.trim().is_empty() {
                        failures.push(format!("{what}: refused without a reason"));
                    }
                }
                None => {
                    if let Err(why) = same_program(&src, &text) {
                        failures.push(format!("{what}: rebuilt, but {why}"));
                    }
                }
            }
        }
    }
    assert!(tried >= 20, "only {tried} nodes tried");
    assert!(failures.is_empty(), "{} of {tried} nodes fail; first:\n{}", failures.len(), failures[0]);
    assert!(refused * 5 <= tried, "{refused} of {tried} nodes refused: more than a fifth");
}

/// Every boxed call in `graph` and its arms: word, corner, input cells,
/// output cells.
fn collect_calls(graph: &Graph, out: &mut Vec<(String, Pos, Vec<Pos>, Vec<Pos>)>) {
    for node in &graph.nodes {
        match &node.kind {
            NodeKind::Call(name) => out.push((
                name.text.clone(),
                Pos::new(node.rect.top, node.rect.left),
                node.inputs.clone(),
                node.outputs.clone(),
            )),
            NodeKind::Switch(switch) => {
                for arm in &switch.arms {
                    collect_calls(&arm.body, out);
                }
            }
            _ => {}
        }
    }
}

#[test]
fn studio_trace() {
    let mut failures = Vec::new();
    for (name, src) in clean_programs(90) {
        let expected = fs::read_to_string(root().join(name.replace(".shaw", ".out"))).unwrap_or_default();
        let started = Instant::now();
        let trace = match shawline_cli::trace(&src) {
            Ok(trace) => trace,
            Err(errors) => {
                failures.push(format!("{name}: no trace: {errors:?}"));
                continue;
            }
        };
        if started.elapsed() > Duration::from_secs(5) {
            failures.push(format!("{name}: tracing took {:?}", started.elapsed()));
        }
        if trace.stopped {
            if name[..2].parse::<u32>().is_ok_and(|n| n < 80) {
                failures.push(format!("{name}: the trace stopped before the program finished"));
            }
            continue;
        }
        if let Some(e) = &trace.error {
            if format!("error {} {}\n", e.at, e.code) != expected {
                failures.push(format!("{name}: traced error {} {}, expected {expected}", e.at, e.code));
            }
            continue;
        }
        let parsed = sheet(&src);
        let main = parsed.definitions.iter().find(|d| d.name.text == "𐑥𐑱𐑯").expect("·𐑥𐑱𐑯");
        let value = |wire: usize| trace.wires.iter().find(|(w, _)| *w == wire).map(|(_, v)| v.to_string());
        for edge in &main.body.edges {
            if value(edge.wire).is_none() {
                failures.push(format!("{name}: wire {} in ·𐑥𐑱𐑯 has no value", edge.wire));
            }
        }
        let printed: String = (0..main.outputs.len() as u32)
            .map(|i| {
                let edge = main.body.edges.iter().find(|e| e.to.contains(&Endpoint::Result(i)));
                edge.and_then(|e| value(e.wire)).unwrap_or_default() + "\n"
            })
            .collect();
        if printed != expected {
            failures.push(format!("{name}: outputs carry\n{printed}but the program prints\n{expected}"));
        }
    }
    assert!(failures.is_empty(), "{} trace failures; first:\n{}", failures.len(), failures[0]);
}
