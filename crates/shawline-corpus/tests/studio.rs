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
//!
//! Rewire and rebuild also start from jittered copies of each program (the
//! jitter corpus's perturbations), naming their first edit in the jittered
//! text's own cells, as an editor showing that text would. Every edit
//! returns canonical text, so the later ones name canonical cells. A
//! jittered start must end exactly where the canonical one does: the same
//! text, or the same refusal.

use std::collections::{BTreeSet, HashSet};
use std::fs;
use std::time::{Duration, Instant};

use shawline_corpus::jitter::{Fired, Jittered, Rng, jitter};
use shawline_corpus::studio::{Link, calls, links};
use shawline_corpus::{STAGES, programs, root};
use shawline_doc::{Doc, TRACE_STEPS};
use shawline_grid::{Grid, Pos};
use shawline_syntax::ast::{Endpoint, Sheet};

fn stage(name: &str) -> fn(&str) -> String {
    STAGES.iter().find(|s| s.name == name).expect("registered stage").run
}

/// Non-lint diagnostic codes, sorted: positions legitimately change.
fn codes(src: &str) -> Vec<String> {
    let doc = Doc::new(src);
    let mut codes: Vec<String> =
        doc.diagnostics().iter().filter(|d| !d.code.starts_with('L')).map(|d| d.code.to_owned()).collect();
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

/// Jitter seeds each rewire and rebuild also starts from.
const JITTER_SEEDS: u64 = 1;

/// The texts a loop starts from: `src` itself, then jittered copies.
fn starts(src: &str) -> Vec<(String, Jittered)> {
    let diagram = shawline_shape::recognize(&Grid::parse(src).unwrap());
    let wire: HashSet<Pos> = diagram.wires.iter().flat_map(|w| w.cells.iter().copied()).collect();
    let mut out = vec![("canonical".to_owned(), Jittered::none(src))];
    for seed in 0..JITTER_SEEDS {
        let jittered = jitter(src, &wire, &mut Rng(seed), &mut Fired::default());
        out.push((format!("jitter seed {seed}"), jittered));
    }
    out
}

fn sheet(src: &str) -> Sheet {
    Doc::new(src).sheet().expect("on the grid").clone()
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
    if Doc::new(text).formatted() != Some(text) {
        return Err(format!("result isn't canonical\n{text}"));
    }
    Ok(())
}

/// Each sink of `link` as a `connect` call, sinks in row-major order.
fn pairs(link: &Link) -> Vec<(Pos, Pos)> {
    link.to.iter().map(|&to| (link.from, to)).collect()
}

/// How a loop ended, as compared between starts: its text, or the step
/// that was refused and why.
type Outcome = Result<String, (String, String)>;

/// Checks a jittered start's outcome against the canonical start's.
fn same_end(what: &str, start: &str, got: &Outcome, want: &Outcome, failures: &mut Vec<String>) {
    if got != want {
        let show = |o: &Outcome| match o {
            Ok(text) => format!("ends with\n{text}"),
            Err((step, why)) => format!("refuses {step}: {why}"),
        };
        failures.push(format!("{what} from {start} {}\nbut from canonical text {}", show(got), show(want)));
    }
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
        let steps: Vec<(&str, Pos, Option<Pos>)> = cuts
            .iter()
            .map(|&at| ("remove_wire", at, None))
            .chain(connects.iter().map(|&(from, to)| ("connect", from, Some(to))))
            .collect();

        let rewire = |shown: &Jittered| -> Outcome {
            let mut doc = Doc::new(shown.text.as_str());
            for (k, &(what, at, to)) in steps.iter().enumerate() {
                // Only the first edit sees the shown text.
                let (cell, end) = if k == 0 { (shown.at(at), to.map(|t| shown.at(t))) } else { (at, to) };
                let result = match end {
                    None => shawline_edit::remove_wire(&doc, cell),
                    Some(end) => shawline_edit::connect(&doc, cell, end),
                };
                doc = result.map_err(|r| (format!("{what} at {at}"), r.reason))?;
            }
            Ok(doc.text().to_owned())
        };
        let starts = starts(&src);
        let want = rewire(&starts[0].1);
        match &want {
            Err((step, why)) => {
                refused += 1;
                if why.trim().is_empty() {
                    failures.push(format!("{name}: {step} refused without a reason"));
                }
            }
            Ok(text) => {
                if let Err(why) = same_program(&src, text) {
                    failures.push(format!("{name}: rewired, but {why}"));
                }
            }
        }
        for (start, shown) in &starts[1..] {
            same_end(&name, start, &rewire(shown), &want, &mut failures);
        }
    }
    assert!(tried >= 10, "only {tried} programs tried");
    assert!(failures.is_empty(), "{} failures over {tried} programs; first:\n{}", failures.len(), failures[0]);
    assert!(refused * 5 <= tried, "{refused} of {tried} programs refused: more than a fifth");
}

#[test]
fn studio_rebuild() {
    let (mut tried, mut refused, mut failures) = (0, 0, Vec::new());
    for (name, src) in clean_programs(80) {
        let original = sheet(&src);
        let all = links(&original);
        let starts = starts(&src);
        for call in calls(&original) {
            let (word, corner, inputs, outputs) = (call.word, call.corner, call.inputs, call.outputs);
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
            let arity = (inputs.len() as u16, outputs.len() as u16);
            let offers = |shown: &Jittered| {
                let offered = shawline_edit::palette(&Doc::new(shown.text.as_str()), from.map(|f| shown.at(f)));
                offered.iter().any(|c| c.word == word && (c.inputs, c.outputs) == arity)
            };
            if let Some((start, _)) = starts.iter().find(|(_, shown)| !offers(shown)) {
                failures.push(format!("{what} from {start}: the palette doesn't offer it"));
                continue;
            }
            // Incoming branches first, then everything the node feeds.
            let mut connects: Vec<(Pos, Pos)> = touching
                .iter()
                .flat_map(|l| l.to.iter().filter(|t| inputs.contains(t)).map(|&t| (l.from, t)))
                .collect();
            connects.extend(touching.iter().filter(|l| outputs.contains(&l.from)).flat_map(|l| pairs(l)));

            let rebuild = |shown: &Jittered| -> Outcome {
                // Only the removal sees the shown text.
                let refused = |step: &'static str| move |r: shawline_edit::Refusal| (step.to_owned(), r.reason);
                let mut doc = shawline_edit::remove_node(&Doc::new(shown.text.as_str()), shown.at(corner))
                    .map_err(refused("remove_node"))?;
                doc = shawline_edit::place_node(&doc, corner, &word, arity.0, arity.1)
                    .map_err(refused("place_node"))?
                    .doc;
                for &(from, to) in &connects {
                    doc = shawline_edit::connect(&doc, from, to).map_err(refused("connect"))?;
                }
                Ok(doc.text().to_owned())
            };
            let want = rebuild(&starts[0].1);
            match &want {
                Err((_, why)) => {
                    refused += 1;
                    if why.trim().is_empty() {
                        failures.push(format!("{what}: refused without a reason"));
                    }
                }
                Ok(text) => {
                    if let Err(why) = same_program(&src, text) {
                        failures.push(format!("{what}: rebuilt, but {why}"));
                    }
                }
            }
            for (start, shown) in &starts[1..] {
                same_end(&what, start, &rebuild(shown), &want, &mut failures);
            }
        }
    }
    assert!(tried >= 20, "only {tried} nodes tried");
    assert!(failures.is_empty(), "{} failures over {tried} nodes; first:\n{}", failures.len(), failures[0]);
    assert!(refused * 5 <= tried, "{refused} of {tried} nodes refused: more than a fifth");
}

#[test]
fn studio_trace() {
    let mut failures = Vec::new();
    for (name, src) in clean_programs(90) {
        let expected = fs::read_to_string(root().join(name.replace(".shaw", ".out"))).unwrap_or_default();
        let started = Instant::now();
        let doc = Doc::new(src.as_str());
        let Some(trace) = doc.trace(TRACE_STEPS) else {
            failures.push(format!("{name}: no trace: {:?}", doc.diagnostics()));
            continue;
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

/// A fan-out whose branches cross each other owns both axes of the
/// crossing, so removing the wire must take both (PLAN P8). Editing reads
/// one axis per crossing today and leaves the other behind.
#[test]
#[ignore = "P8 wave 1: edits read crossing ownership from Wire::links"]
fn removing_a_wire_that_crosses_itself_leaves_no_stroke() {
    let src = fs::read_to_string(root().join("15-fanout-cross.shaw")).unwrap();
    let doc = shawline_edit::remove_wire(&Doc::new(src), Pos::new(2, 4)).unwrap_or_else(|r| panic!("{r:?}"));
    assert!(doc.diagram().unwrap().wires.is_empty(), "a stroke is left:\n{}", doc.text());
}
