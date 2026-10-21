//! PLAN P8's sequence gate: every intermediate state of a Studio edit
//! sequence is checked, not just where the sequence ends.
//!
//! Per valid program, from the canonical text and from one jittered copy:
//! cut one wire and reconnect it sink by sink; remove one node, place it
//! again and reconnect it; then move that node one cell. After every step
//! the text must be canonical and have exactly the wires the step declares,
//! each as its source cell and sink cells. Diagnostics may gain only the
//! codes a missing wire causes, and the program must end with its own codes
//! and output. A jittered start must produce the same texts and be refused
//! at the same step.
//!
//! A refused step ends that program's sequence: which steps succeed is the
//! `.studio` dump's business. This gate is about what a success leaves.

use std::collections::{BTreeSet, HashSet};
use std::fs;

use shawline_corpus::jitter::{Fired, Rng, jitter};
use shawline_corpus::studio::{Link, Step, links, rebuilds};
use shawline_corpus::{STAGES, programs, root};
use shawline_grid::{Grid, Pos};

/// Each wire as its source cell and its sink cells, the sinks row-major.
type Wiring = BTreeSet<(Pos, Vec<Pos>)>;

/// What a missing wire may cause on the way: an arrowhead nothing reaches
/// (`Y019`), and a constant or name left with no wire, which reads as stray
/// text (`Y012`). Nothing else appeared over the corpus at `8d99495`.
const WHILE_UNWIRED: &[&str] = &["Y012", "Y019"];

fn wiring_of(list: &[Link]) -> Wiring {
    list.iter().map(|l| (l.from, l.to.clone())).collect()
}

fn wiring(text: &str) -> Wiring {
    wiring_of(&links(&shawline_cli::check(text).expect("on the grid").sheet))
}

fn codes(text: &str) -> BTreeSet<String> {
    let found = match shawline_cli::check(text) {
        Ok(checked) => checked.diagnostics,
        Err(errors) => errors,
    };
    found.iter().filter(|d| !d.code.starts_with('L')).map(|d| d.code.to_owned()).collect()
}

/// A step and the wiring it must leave.
struct Expect {
    step: Step,
    wiring: Wiring,
}

/// Adds `to` to the wire from `from`, or starts that wire.
fn add(wiring: &mut Wiring, from: Pos, to: Pos) {
    let mut sinks = wiring.iter().find(|(f, _)| *f == from).map(|(_, t)| t.clone()).unwrap_or_default();
    wiring.retain(|(f, _)| *f != from);
    sinks.push(to);
    sinks.sort();
    wiring.insert((from, sinks));
}

/// The sequence for one program, or `None` when it has nothing editable.
fn sequence(src: &str) -> Option<Vec<Expect>> {
    let sheet = shawline_cli::check(src).ok()?.sheet;
    let all = links(&sheet);
    let whole = wiring_of(&all);
    let mut out = Vec::new();

    // Cut one wire, then reconnect it sink by sink.
    let link = all.iter().find(|l| l.editable)?;
    let diagram = shawline_shape::recognize(&Grid::parse(src).unwrap());
    let mut now: Wiring = whole.iter().filter(|(f, _)| *f != link.from).cloned().collect();
    out.push(Expect { step: Step::RemoveWire(diagram.wires[link.wire].cells[0]), wiring: now.clone() });
    for &to in &link.to {
        add(&mut now, link.from, to);
        out.push(Expect { step: Step::Connect(link.from, to), wiring: now.clone() });
    }

    // Remove a node with its wires, place it again, reconnect it, move it.
    if let Some((call, steps)) = rebuilds(&sheet).into_iter().next() {
        let mut now: Wiring = BTreeSet::new();
        for (from, to) in &whole {
            if call.outputs.contains(from) {
                continue;
            }
            let kept: Vec<Pos> = to.iter().copied().filter(|t| !call.inputs.contains(t)).collect();
            if !kept.is_empty() {
                now.insert((*from, kept));
            }
        }
        for step in steps {
            if let Step::Connect(from, to) = step {
                add(&mut now, from, to);
            }
            out.push(Expect { step, wiring: now.clone() });
        }
        // One cell right: the node's own cells shift, nothing else does.
        let shift = |p: Pos| {
            if call.inputs.contains(&p) || call.outputs.contains(&p) { Pos::new(p.row, p.col + 1) } else { p }
        };
        let moved: Wiring = whole
            .iter()
            .map(|(f, t)| {
                let mut t: Vec<Pos> = t.iter().map(|&p| shift(p)).collect();
                t.sort();
                (shift(*f), t)
            })
            .collect();
        out.push(Expect { step: Step::Move { corner: call.corner, dr: 0, dc: 1 }, wiring: moved });
    }
    Some(out)
}

/// How each step went: the text it produced, or the cell it was refused at.
type Trail = Vec<Result<String, Pos>>;

/// Walks `expects` from `start`, the first step naming `start`'s cells
/// through `at`. Stops at the first refusal.
fn walk(start: &str, at: impl Fn(Pos) -> Pos, expects: &[Expect]) -> Trail {
    let mut trail = Vec::new();
    let mut text = start.to_owned();
    for (k, e) in expects.iter().enumerate() {
        let step = if k > 0 {
            e.step.clone()
        } else {
            match &e.step {
                Step::RemoveWire(p) => Step::RemoveWire(at(*p)),
                other => other.clone(),
            }
        };
        match step.apply(&text) {
            Ok(next) => {
                trail.push(Ok(next.clone()));
                text = next;
            }
            Err(r) => {
                trail.push(Err(r.at));
                break;
            }
        }
    }
    trail
}

#[test]
fn studio_sequences() {
    let out = STAGES.iter().find(|s| s.name == "out").unwrap().run;
    let mut failures = Vec::new();
    let (mut programs_tried, mut steps_checked, mut finished) = (0, 0, 0);
    for program in programs(&root()) {
        let name = program.file_name().unwrap().to_string_lossy().into_owned();
        let valid = name[..2].parse::<u32>().is_ok_and(|n| n < 80)
            && fs::read_to_string(program.with_extension("check")).is_ok_and(|c| c.is_empty());
        if !valid {
            continue;
        }
        let src = fs::read_to_string(&program).unwrap();
        let Some(expects) = sequence(&src) else { continue };
        programs_tried += 1;
        let base = codes(&src);
        let trail = walk(&src, |p| p, &expects);
        for (e, got) in expects.iter().zip(&trail) {
            let Ok(text) = got else { continue };
            steps_checked += 1;
            let what = format!("{name}: {}", e.step);
            if shawline_cli::format(text).as_deref() != Ok(text.as_str()) {
                failures.push(format!("{what}: result isn't canonical\n{text}"));
            }
            let found = wiring(text);
            if found != e.wiring {
                failures.push(format!("{what}: wires are\n{found:?}\nexpected\n{:?}\n{text}", e.wiring));
            }
            for code in codes(text).difference(&base) {
                if !WHILE_UNWIRED.contains(&code.as_str()) {
                    failures.push(format!("{what}: gains {code}\n{text}"));
                }
            }
        }
        // A sequence that runs to its end must give the program back.
        if trail.len() == expects.len()
            && let Some(Ok(last)) = trail.last()
        {
            finished += 1;
            if codes(last) != base || out(last) != out(&src) {
                failures.push(format!("{name}: the sequence doesn't end with the same program\n{last}"));
            }
        }
        // The same steps from a jittered copy of the text.
        let diagram = shawline_shape::recognize(&Grid::parse(&src).unwrap());
        let wire: HashSet<Pos> = diagram.wires.iter().flat_map(|w| w.cells.iter().copied()).collect();
        let shown = jitter(&src, &wire, &mut Rng(0), &mut Fired::default());
        // Refusals point at the text each start showed, so only which step
        // refused is compared.
        let texts = |t: &Trail| -> Vec<Option<String>> { t.iter().map(|r| r.as_ref().ok().cloned()).collect() };
        let (want, got) = (texts(&trail), texts(&walk(&shown.text, |p| shown.at(p), &expects)));
        if got != want {
            let k = got.iter().zip(&want).position(|(a, b)| a != b).unwrap_or(want.len().min(got.len()));
            failures.push(format!("{name}: from jittered text, step {k} ends differently: {:?} vs {:?}", got.get(k), want.get(k)));
        }
    }
    assert!(programs_tried >= 15, "only {programs_tried} programs tried");
    assert!(steps_checked >= 100 && finished >= 12, "only {steps_checked} steps and {finished} whole sequences");
    assert!(failures.is_empty(), "{} failures; first:\n{}", failures.len(), failures[0]);
}
