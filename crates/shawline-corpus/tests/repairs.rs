//! PLAN P10's repair gate: every code in the repairs table offers its
//! repair on a corpus case, and a repair's edit clears its diagnostic
//! without adding an error, except the codes its follow-up resolves
//! (`Y019` until a `DrawWire` is drawn).

use std::collections::{BTreeMap, BTreeSet};
use std::fs;

use shawline_corpus::{programs, root};
use shawline_doc::Doc;
use shawline_edit::{Followup, apply, repairs};
use shawline_grid::{Diagnostic, Severity};

/// The codes PLAN P10's repairs table names.
const TABLE: &[&str] = &[
    "S012", "Y001", "Y003", "Y004", "Y009", "Y012", "Y013", "Y014", "Y015", "Y018", "Y019", "C001", "C002",
    "C004", "C005", "C006", "W001", "W002",
];

/// Each error code's count.
fn errors(diagnostics: &[Diagnostic]) -> BTreeMap<&'static str, usize> {
    let mut counts = BTreeMap::new();
    for d in diagnostics.iter().filter(|d| d.severity() == Severity::Error) {
        *counts.entry(d.code).or_default() += 1;
    }
    counts
}

#[test]
fn repairs_clear_their_codes() {
    let mut offered = BTreeSet::new();
    let mut failures = Vec::new();
    for program in programs(&root()) {
        let name = program.strip_prefix(root()).unwrap_or(&program).display().to_string();
        let src = fs::read_to_string(&program).unwrap();
        let doc = Doc::new(src.as_str());
        let before = doc.diagnostics();
        for repair in repairs(&doc) {
            offered.insert(repair.code);
            let what = format!("{name}: {} {}", repair.at, repair.code);
            if !before.iter().any(|d| d.at == repair.at && d.code == repair.code) {
                failures.push(format!("{what}: no such diagnostic"));
            }
            let Some(edit) = &repair.edit else { continue };
            let edited = match apply(&doc, edit) {
                Ok(edited) => edited,
                Err(r) => {
                    failures.push(format!("{what}: {edit} refused {} {}: {}", r.kind, r.at, r.reason));
                    continue;
                }
            };
            let after = edited.doc.diagnostics();
            let count = |ds: &[Diagnostic]| ds.iter().filter(|d| d.code == repair.code).count();
            if count(after) >= count(before) {
                failures.push(format!("{what}: {edit} doesn't clear it"));
            }
            let resolved: &[&str] = match repair.then {
                Followup::DrawWire(_) => &["Y019"],
                Followup::None | Followup::NameField(_) => &[],
            };
            let (old, new) = (errors(before), errors(after));
            for (code, n) in new {
                if n > old.get(code).copied().unwrap_or(0) && !resolved.contains(&code) {
                    failures.push(format!("{what}: {edit} adds {code}\n{}", edited.doc.text()));
                }
            }
        }
    }
    for code in TABLE.iter().filter(|c| !offered.contains(*c)) {
        failures.push(format!("no corpus case offers a repair for {code}"));
    }
    assert!(failures.is_empty(), "{} failures:\n{}", failures.len(), failures.join("\n"));
}
