//! The P6 spike gate: every node box in every valid corpus program, moved one
//! cell in each direction. A move is either refused with a reason, or its
//! result:
//! - recognizes the box at its new place;
//! - keeps the program's diagnostic codes (lints aside) and its output;
//! - is already canonical: formatting it changes nothing.
//!
//! Refusals are allowed, but at most a fifth of the moves, or the router is
//! too weak to build an editor on.

use std::fs;

use shawline_corpus::{STAGES, programs, root};
use shawline_doc::Doc;
use shawline_grid::{Dir, Grid, Pos};
use shawline_shape::{PortKind, recognize};

fn stage(name: &str) -> fn(&str) -> String {
    STAGES.iter().find(|s| s.name == name).expect("registered stage").run
}

/// Non-lint diagnostic codes, sorted: positions legitimately change when a
/// box moves.
fn codes(src: &str) -> Vec<String> {
    let doc = Doc::new(src);
    let mut codes: Vec<String> =
        doc.diagnostics().iter().filter(|d| !d.code.starts_with('L')).map(|d| d.code.to_owned()).collect();
    codes.sort();
    codes
}

/// Top-left corners of node boxes: square boxes inside a definition that
/// aren't switches (no dividers, no selector on the top edge).
fn node_corners(src: &str) -> Vec<Pos> {
    let diagram = recognize(&Grid::parse(src).unwrap());
    diagram
        .boxes
        .iter()
        .filter(|b| b.parent.is_some() && b.dividers.is_empty())
        .filter(|b| !b.ports.iter().any(|p| p.side == Dir::N && p.kind == PortKind::Arrow))
        .map(|b| Pos::new(b.rect.top, b.rect.left))
        .collect()
}

#[test]
fn every_node_box_moves_one_cell_or_is_refused_with_a_reason() {
    let out = stage("out");
    let (mut moved, mut refused, mut failures) = (0, 0, Vec::new());
    for program in programs(&root()) {
        let name = program.file_name().unwrap().to_string_lossy().into_owned();
        let valid = name[..2].parse::<u32>().is_ok_and(|n| n < 80)
            && fs::read_to_string(program.with_extension("check")).is_ok_and(|c| c.is_empty());
        if !valid {
            continue;
        }
        let src = fs::read_to_string(&program).unwrap();
        let (want_codes, want_out) = (codes(&src), out(&src));
        let doc = Doc::new(src.as_str());
        for corner in node_corners(&src) {
            for (dr, dc) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
                let what = format!("{name}: box at {corner} by ({dr}, {dc})");
                match shawline_edit::move_box(&doc, corner, dr, dc).map(|moved| moved.doc.text().to_owned()) {
                    Err(refusal) => {
                        refused += 1;
                        if refusal.reason.trim().is_empty() {
                            failures.push(format!("{what}: refused without a reason"));
                        }
                    }
                    Ok(text) => {
                        moved += 1;
                        let to = Pos::new(
                            corner.row.checked_add_signed(dr).unwrap(),
                            corner.col.checked_add_signed(dc).unwrap(),
                        );
                        if !node_corners(&text).contains(&to) {
                            failures.push(format!("{what}: no box at {to} afterwards\n{text}"));
                        } else if codes(&text) != want_codes {
                            failures.push(format!("{what}: diagnostics change\n{text}"));
                        } else if out(&text) != want_out {
                            failures.push(format!("{what}: output changes\n{text}"));
                        } else if Doc::new(text.as_str()).formatted() != Some(text.as_str()) {
                            failures.push(format!("{what}: result isn't canonical\n{text}"));
                        }
                    }
                }
            }
        }
    }
    let tried = moved + refused;
    assert!(tried > 100, "only {tried} moves tried");
    assert!(failures.is_empty(), "{} of {tried} moves fail; first:\n{}", failures.len(), failures[0]);
    assert!(refused * 5 <= tried, "{refused} of {tried} moves refused: more than a fifth");
}
