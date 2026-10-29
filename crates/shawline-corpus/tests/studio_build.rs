//! PLAN P10's build gate: every valid corpus program is rebuilt from an
//! empty sheet by the `.studio` dump's `build` steps.
//!
//! Every intermediate state must be canonical and may gain only codes from
//! `WHILE_BUILDING`. The final sheet must have the original's codes, output
//! and wiring (each source cell with its sink cells).
//!
//! Red until P10's wave 2 lands the edits that draw boxes, ports, arms and
//! inline nodes, so it is ignored until then: run it with `--ignored`.

use std::collections::BTreeSet;
use std::fs;

use shawline_corpus::studio::{EMPTY, Link, build, links};
use shawline_corpus::{STAGES, programs, root};
use shawline_doc::Doc;
use shawline_edit::apply;
use shawline_grid::Pos;

/// Codes a sheet may have on the way. Measured on the first run, as
/// `studio_sequences`' `WHILE_UNWIRED` was; set in wave 2's contract.
const WHILE_BUILDING: &[&str] = &[];

fn codes(doc: &Doc) -> BTreeSet<String> {
    doc.diagnostics().iter().filter(|d| !d.code.starts_with('L')).map(|d| d.code.to_owned()).collect()
}

fn wiring(doc: &Doc) -> BTreeSet<(Pos, Vec<Pos>)> {
    links(doc.sheet().expect("on the grid")).into_iter().map(|l: Link| (l.from, l.to)).collect()
}

#[test]
#[ignore = "red until P10 wave 2"]
fn studio_build() {
    let out = STAGES.iter().find(|s| s.name == "out").unwrap().run;
    let mut failures = Vec::new();
    let mut built = 0;
    for program in programs(&root()) {
        let name = program.file_name().unwrap().to_string_lossy().into_owned();
        let valid = name[..2].parse::<u32>().is_ok_and(|n| n < 80)
            && fs::read_to_string(program.with_extension("check")).is_ok_and(|c| c.is_empty());
        if !valid {
            continue;
        }
        let src = fs::read_to_string(&program).unwrap();
        let original = Doc::new(src.as_str());
        let base = codes(&original);
        let mut doc = Doc::new(EMPTY);
        let steps = build(&src, original.sheet().unwrap());
        let mut done = true;
        for step in &steps {
            let what = format!("{name}: {step}");
            match apply(&doc, step) {
                Ok(edited) => doc = edited.doc,
                Err(r) => {
                    failures.push(format!("{what}: refused {} {}: {}", r.kind, r.at, r.reason));
                    done = false;
                    break;
                }
            }
            let text = doc.text();
            if Doc::new(text).formatted() != Some(text) {
                failures.push(format!("{what}: result isn't canonical\n{text}"));
            }
            for code in codes(&doc).difference(&base) {
                if !WHILE_BUILDING.contains(&code.as_str()) {
                    failures.push(format!("{what}: gains {code}\n{text}"));
                }
            }
        }
        if !done {
            continue;
        }
        built += 1;
        if codes(&doc) != base || out(doc.text()) != out(&src) || wiring(&doc) != wiring(&original) {
            failures.push(format!("{name}: the build doesn't end with the same program\n{}", doc.text()));
        }
    }
    assert!(failures.is_empty(), "{} failures; first:\n{}", failures.len(), failures[0]);
    assert!(built >= 17, "only {built} programs built");
}
