//! PLAN P10's room gate, over every valid corpus program and the `.studio`
//! dump's `room`, `insert` and `retext` sequences:
//!
//! - inserting and then deleting, or retexting and then setting back,
//!   returns the original text exactly;
//! - every success is canonical, keeps the codes, and matches the original
//!   structure relabelled through its map at `Routing::Kept` (for retext,
//!   with that one text's content changed). Insertions and `make_room` also
//!   keep the output;
//! - a `make_room` success leaves the moved box where the move asked,
//!   relative to its parent.
//!
//! Until wave 1 lands the edits refuse, and only the refusals' kinds are
//! checked here; the dump records them.

use std::collections::BTreeSet;
use std::fs;

use shawline_corpus::studio::{insertions, retexts, rooms};
use shawline_corpus::{STAGES, programs, root};
use shawline_doc::Doc;
use shawline_edit::{Edit, Edited, RefusalKind, apply};
use shawline_shape::structure::{Routing, Structure};

fn codes(doc: &Doc) -> BTreeSet<String> {
    doc.diagnostics().iter().filter(|d| !d.code.starts_with('L')).map(|d| d.code.to_owned()).collect()
}

fn structure(doc: &Doc) -> Structure {
    Structure::of(doc.grid().unwrap(), doc.diagram().unwrap())
}

/// What one success must keep. `content` is the text a retext writes.
fn check(before: &Doc, edited: &Edited, keep_output: bool, content: Option<&str>) -> Result<(), String> {
    let out = STAGES.iter().find(|s| s.name == "out").unwrap().run;
    let text = edited.doc.text();
    if Doc::new(text).formatted() != Some(text) {
        return Err("isn't canonical".to_owned());
    }
    if codes(&edited.doc) != codes(before) {
        return Err("changes the codes".to_owned());
    }
    if keep_output && out(text) != out(before.text()) {
        return Err("changes the output".to_owned());
    }
    if !edited.map.is_total() {
        return Err("loses a box or text".to_owned());
    }
    let mut expected = structure(before).relabel(&edited.map);
    let found = structure(&edited.doc);
    if let Some(content) = content {
        let differing: Vec<usize> =
            (0..expected.texts.len().min(found.texts.len())).filter(|&j| expected.texts[j] != found.texts[j]).collect();
        match differing[..] {
            [j] if found.texts[j].content == content => expected.texts[j].content = content.to_owned(),
            _ => return Err(format!("changes texts {differing:?}, not the one retexted")),
        }
    }
    expected.expect(&found, Routing::Kept).map_err(|part| format!("changes the structure: {part:?}"))
}

/// The moved box's corner relative to its parent's, before and after.
fn stays_put(before: &Doc, edited: &Edited, edit: &Edit) -> Result<(), String> {
    let Edit::Move { corner, dr, dc } = *edit else { return Ok(()) };
    let (old, new) = (before.diagram().unwrap(), edited.doc.diagram().unwrap());
    let i = old.boxes.iter().position(|b| (b.rect.top, b.rect.left) == (corner.row, corner.col)).unwrap();
    let j = edited.map.boxes[i].ok_or("loses the moved box")?;
    let offset = |rect: shawline_grid::Rect, parent: Option<shawline_grid::Rect>| {
        let p = parent.map_or((0, 0), |p| (p.top, p.left));
        (rect.top as isize - p.0 as isize, rect.left as isize - p.1 as isize)
    };
    let was = offset(old.boxes[i].rect, old.boxes[i].parent.map(|p| old.boxes[p].rect));
    let now = offset(new.boxes[j].rect, new.boxes[j].parent.map(|p| new.boxes[p].rect));
    if now != (was.0 + dr, was.1 + dc) {
        return Err(format!("leaves the box at {now:?} from its parent, not {:?}", (was.0 + dr, was.1 + dc)));
    }
    Ok(())
}

#[test]
fn studio_room() {
    let mut failures = Vec::new();
    let (mut programs_tried, mut sequences, mut successes) = (0, 0, 0);
    let refused = |what: &str, kind: RefusalKind, failures: &mut Vec<String>| {
        // A stub refuses as unsupported; a real edit may refuse for want
        // of room or a target, never by changing what it promises to keep.
        if kind == RefusalKind::Changed {
            failures.push(format!("{what}: refused as changed"));
        }
    };
    for program in programs(&root()) {
        let name = program.file_name().unwrap().to_string_lossy().into_owned();
        let valid = name[..2].parse::<u32>().is_ok_and(|n| n < 80)
            && fs::read_to_string(program.with_extension("check")).is_ok_and(|c| c.is_empty());
        if !valid {
            continue;
        }
        programs_tried += 1;
        let src = fs::read_to_string(&program).unwrap();
        let original = Doc::new(src.as_str());

        for edit in rooms(&src) {
            sequences += 1;
            let what = format!("{name}: {edit}");
            match apply(&original, &edit) {
                Ok(edited) => {
                    successes += 1;
                    let Edit::MakeRoom(inner) = &edit else { unreachable!() };
                    if let Err(why) = check_room(&original, &edited, inner) {
                        failures.push(format!("{what}: {why}\n{}", edited.doc.text()));
                    }
                }
                Err(r) => refused(&what, r.kind, &mut failures),
            }
        }

        let sheet = original.sheet().unwrap();
        for (_, pairs) in insertions(sheet) {
            for [insert, delete] in pairs {
                sequences += 1;
                let what = format!("{name}: {insert}");
                let inserted = match apply(&original, &insert) {
                    Ok(edited) => edited,
                    Err(r) => {
                        refused(&what, r.kind, &mut failures);
                        continue;
                    }
                };
                successes += 1;
                if let Err(why) = check(&original, &inserted, true, None) {
                    failures.push(format!("{what}: {why}\n{}", inserted.doc.text()));
                }
                match apply(&inserted.doc, &delete) {
                    Ok(deleted) if deleted.doc.text() == src => successes += 1,
                    Ok(deleted) => failures.push(format!("{what}, then {delete}: not the original\n{}", deleted.doc.text())),
                    Err(r) => failures.push(format!("{what}, then {delete}: refused {} {}", r.kind, r.at)),
                }
            }
        }

        for steps in retexts(&src) {
            sequences += 1;
            let mut doc = Doc::new(src.as_str());
            for (k, step) in steps.iter().enumerate() {
                let what = format!("{name}: {step}");
                let edited = match apply(&doc, step) {
                    Ok(edited) => edited,
                    Err(r) => {
                        // Setting back must succeed once the others have.
                        if k == steps.len() - 1 {
                            failures.push(format!("{what}: setting back refused {} {}", r.kind, r.at));
                        }
                        refused(&what, r.kind, &mut failures);
                        break;
                    }
                };
                successes += 1;
                let Edit::SetText { content, .. } = step else { unreachable!() };
                if let Err(why) = check(&doc, &edited, false, Some(content)) {
                    failures.push(format!("{what}: {why}\n{}", edited.doc.text()));
                }
                if k == steps.len() - 1 && edited.doc.text() != src {
                    failures.push(format!("{what}: setting back isn't the original\n{}", edited.doc.text()));
                }
                doc = edited.doc;
            }
        }
    }
    eprintln!("studio_room: {programs_tried} programs, {sequences} sequences, {successes} successful steps");
    assert!(programs_tried >= 15, "only {programs_tried} programs tried");
    assert!(sequences >= 100, "only {sequences} sequences");
    assert!(failures.is_empty(), "{} failures; first:\n{}", failures.len(), failures[0]);
}

/// A `make_room` success: canonical, the same codes and output, the moved
/// box where the move asked, and the structure relabelled through its map
/// up to re-routing.
fn check_room(before: &Doc, edited: &Edited, inner: &Edit) -> Result<(), String> {
    let out = STAGES.iter().find(|s| s.name == "out").unwrap().run;
    let text = edited.doc.text();
    if Doc::new(text).formatted() != Some(text) {
        return Err("isn't canonical".to_owned());
    }
    if codes(&edited.doc) != codes(before) || out(text) != out(before.text()) {
        return Err("changes the codes or the output".to_owned());
    }
    if !edited.map.is_total() {
        return Err("loses a box or text".to_owned());
    }
    stays_put(before, edited, inner)?;
    let expected = structure(before).relabel(&edited.map);
    expected.expect(&structure(&edited.doc), Routing::Rerouted).map_err(|part| format!("changes the structure: {part:?}"))
}
