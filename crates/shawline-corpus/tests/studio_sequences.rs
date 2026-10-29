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
use shawline_corpus::studio::{Link, links, rebuilds};
use shawline_corpus::{STAGES, programs, root};
use shawline_doc::Doc;
use shawline_edit::{Edit, Edited, apply};
use shawline_grid::{Grid, Pos, Rect};
use shawline_shape::structure::{Part, Routing, Structure};

/// Each wire as its source cell and its sink cells, the sinks row-major.
type Wiring = BTreeSet<(Pos, Vec<Pos>)>;

/// What a missing wire may cause on the way: an arrowhead nothing reaches
/// (`Y019`), and a constant or name left with no wire, which reads as stray
/// text (`Y012`). Nothing else appeared over the corpus at `8d99495`.
const WHILE_UNWIRED: &[&str] = &["Y012", "Y019"];

fn wiring_of(list: &[Link]) -> Wiring {
    list.iter().map(|l| (l.from, l.to.clone())).collect()
}

fn wiring(doc: &Doc) -> Wiring {
    wiring_of(&links(doc.sheet().expect("on the grid")))
}

fn codes(doc: &Doc) -> BTreeSet<String> {
    doc.diagnostics().iter().filter(|d| !d.code.starts_with('L')).map(|d| d.code.to_owned()).collect()
}

/// A step and the wiring it must leave.
struct Expect {
    step: Edit,
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
    let doc = Doc::new(src);
    let sheet = doc.sheet()?;
    let all = links(sheet);
    let whole = wiring_of(&all);
    let mut out = Vec::new();

    // Cut one wire, then reconnect it sink by sink.
    let link = all.iter().find(|l| l.editable)?;
    let diagram = shawline_shape::recognize(&Grid::parse(src).unwrap());
    let mut now: Wiring = whole.iter().filter(|(f, _)| *f != link.from).cloned().collect();
    out.push(Expect { step: Edit::RemoveWire(diagram.wires[link.wire].cells[0]), wiring: now.clone() });
    for &to in &link.to {
        add(&mut now, link.from, to);
        out.push(Expect { step: Edit::Connect { from: link.from, to }, wiring: now.clone() });
    }

    // Remove a node with its wires, place it again, reconnect it, move it.
    if let Some((call, steps)) = rebuilds(sheet).into_iter().next() {
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
            if let Edit::Connect { from, to } = step {
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
        out.push(Expect { step: Edit::Move { corner: call.corner, dr: 0, dc: 1 }, wiring: moved });
    }
    Some(out)
}

/// Whether `edited.map` sends each box and text of `before` (canonical)
/// where `step` says: a move shifts the moved box and what it holds, a
/// removed node loses its box and word, and nothing else moves. Mapped
/// parts keep their content, and only a placed node is new.
fn check_map(before: &Doc, edited: &Edited, step: &Edit) -> Result<(), String> {
    let old = before.diagram().unwrap();
    let new = edited.doc.diagram().unwrap();
    let map = &edited.map;
    if map.boxes.len() != old.boxes.len() || map.texts.len() != old.texts.len() {
        return Err("doesn't cover the old sheet".to_owned());
    }
    let (held, shift): (Option<Rect>, (isize, isize)) = match *step {
        Edit::Move { corner, dr, dc } => {
            (old.boxes.iter().map(|b| b.rect).find(|r| (r.top, r.left) == (corner.row, corner.col)), (dr, dc))
        }
        _ => (None, (0, 0)),
    };
    let moved = |p: Pos| match held {
        Some(r) if r.contains(p) => Pos::new(p.row.wrapping_add_signed(shift.0), p.col.wrapping_add_signed(shift.1)),
        _ => p,
    };
    let removed: Option<Rect> = match *step {
        Edit::RemoveNode(corner) => old.boxes.iter().map(|b| b.rect).find(|r| (r.top, r.left) == (corner.row, corner.col)),
        _ => None,
    };
    for (i, b) in old.boxes.iter().enumerate() {
        let gone = removed == Some(b.rect);
        match (map.boxes[i], gone) {
            (None, true) => {}
            (Some(j), false) => {
                let r = new.boxes[j].rect;
                if Pos::new(r.top, r.left) != moved(Pos::new(b.rect.top, b.rect.left))
                    || Pos::new(r.bottom, r.right) != moved(Pos::new(b.rect.bottom, b.rect.right))
                {
                    return Err(format!("sends box {} to {}", b.rect, r));
                }
            }
            (to, _) => return Err(format!("sends box {} to {to:?}", b.rect)),
        }
    }
    for (i, t) in old.texts.iter().enumerate() {
        let gone = removed.is_some_and(|r| r.interior_contains(t.pos));
        match (map.texts[i], gone) {
            (None, true) => {}
            (Some(j), false) => {
                let u = &new.texts[j];
                if u.pos != moved(t.pos) || u.content != t.content {
                    return Err(format!("sends text {} {:?} to {} {:?}", t.pos, t.content, u.pos, u.content));
                }
            }
            (to, _) => return Err(format!("sends text {} {:?} to {to:?}", t.pos, t.content)),
        }
    }
    let mut image: Vec<usize> = map.boxes.iter().flatten().copied().collect();
    image.sort_unstable();
    image.dedup();
    let new_boxes = new.boxes.len() - image.len();
    let mut image_texts: Vec<usize> = map.texts.iter().flatten().copied().collect();
    image_texts.sort_unstable();
    image_texts.dedup();
    let new_texts = new.texts.len() - image_texts.len();
    let placed = usize::from(matches!(step, Edit::PlaceNode { .. }));
    if (new_boxes, new_texts) != (placed, placed) {
        return Err(format!("leaves {new_boxes} boxes and {new_texts} texts unmapped"));
    }
    if placed == 0 {
        let (old_grid, new_grid) = (before.grid().unwrap(), edited.doc.grid().unwrap());
        let expected = Structure::of(old_grid, old).relabel(map);
        let found = Structure::of(new_grid, new);
        match (step, expected.expect(&found, Routing::Rerouted)) {
            (_, Ok(())) | (Edit::Connect { .. } | Edit::RemoveWire(_) | Edit::RemoveNode(_), Err(Part::Wires | Part::Codes)) => {}
            (_, Err(part)) => return Err(format!("relabels the structure wrongly: {part:?} differs")),
        }
    }
    Ok(())
}

/// How each step went: the revision it produced, or the cell it was refused
/// at.
type Trail = Vec<Result<Edited, Pos>>;

/// Walks `expects` from `start`, the first step naming `start`'s cells
/// through `at`. Stops at the first refusal.
fn walk(start: &str, at: impl Fn(Pos) -> Pos, expects: &[Expect]) -> Trail {
    let first = Doc::new(start);
    let mut trail: Trail = Vec::new();
    for (k, e) in expects.iter().enumerate() {
        // Each step edits the revision the last one returned.
        let doc = match trail.last() {
            Some(Ok(edited)) => &edited.doc,
            _ => &first,
        };
        let step = if k > 0 {
            e.step.clone()
        } else {
            match &e.step {
                Edit::RemoveWire(p) => Edit::RemoveWire(at(*p)),
                other => other.clone(),
            }
        };
        match apply(doc, &step) {
            Ok(next) => trail.push(Ok(next)),
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
        let base = codes(&Doc::new(src.as_str()));
        let trail = walk(&src, |p| p, &expects);
        for (k, (e, got)) in expects.iter().zip(&trail).enumerate() {
            let Ok(edited) = got else { continue };
            steps_checked += 1;
            let what = format!("{name}: {}", e.step);
            let before = match k {
                0 => Doc::new(src.as_str()),
                _ => match &trail[k - 1] {
                    Ok(prev) => Doc::new(prev.doc.text()),
                    Err(_) => unreachable!("a walk stops at its first refusal"),
                },
            };
            if let Err(why) = check_map(&before, edited, &e.step) {
                failures.push(format!("{what}: map {why}\n{}", edited.doc.text()));
            }
            let doc = &edited.doc;
            let text = doc.text();
            // A fresh revision, so the formatter really runs.
            if Doc::new(text).formatted() != Some(text) {
                failures.push(format!("{what}: result isn't canonical\n{text}"));
            }
            let found = wiring(doc);
            if found != e.wiring {
                failures.push(format!("{what}: wires are\n{found:?}\nexpected\n{:?}\n{text}", e.wiring));
            }
            for code in codes(doc).difference(&base) {
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
            let last = &last.doc;
            if codes(last) != base || out(last.text()) != out(&src) {
                failures.push(format!("{name}: the sequence doesn't end with the same program\n{}", last.text()));
            }
        }
        // The same steps from a jittered copy of the text.
        let diagram = shawline_shape::recognize(&Grid::parse(&src).unwrap());
        let wire: HashSet<Pos> = diagram.wires.iter().flat_map(|w| w.cells.iter().copied()).collect();
        let shown = jitter(&src, &wire, &mut Rng(0), &mut Fired::default());
        // Refusals point at the text each start showed, so only which step
        // refused is compared.
        let texts = |t: &Trail| -> Vec<Option<String>> {
            t.iter().map(|r| r.as_ref().ok().map(|e| e.doc.text().to_owned())).collect()
        };
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
