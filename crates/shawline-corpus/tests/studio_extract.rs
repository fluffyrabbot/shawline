//! PLAN P10's extract gate: every single-node extraction the `.studio`
//! dump records keeps the program's codes and output, or is refused with a
//! kind that names a reason, never `unsupported`.

use std::collections::BTreeSet;
use std::fs;

use shawline_corpus::studio::extractions;
use shawline_corpus::{STAGES, programs, root};
use shawline_doc::Doc;
use shawline_edit::{RefusalKind, apply};

fn codes(doc: &Doc) -> BTreeSet<String> {
    doc.diagnostics().iter().filter(|d| !d.code.starts_with('L')).map(|d| d.code.to_owned()).collect()
}

#[test]
fn studio_extract() {
    let out = STAGES.iter().find(|s| s.name == "out").unwrap().run;
    let mut failures = Vec::new();
    let mut extracted = 0;
    for program in programs(&root()) {
        let name = program.file_name().unwrap().to_string_lossy().into_owned();
        let valid = name[..2].parse::<u32>().is_ok_and(|n| n < 80)
            && fs::read_to_string(program.with_extension("check")).is_ok_and(|c| c.is_empty());
        if !valid {
            continue;
        }
        let src = fs::read_to_string(&program).unwrap();
        let doc = Doc::new(src.as_str());
        for step in extractions(&src) {
            match apply(&doc, &step) {
                Ok(edited) => {
                    extracted += 1;
                    let text = edited.doc.text();
                    if codes(&edited.doc) != codes(&doc) || out(text) != out(&src) {
                        failures.push(format!("{name}: {step} changes the program\n{text}"));
                    }
                }
                Err(r) if r.kind == RefusalKind::Unsupported => {
                    failures.push(format!("{name}: {step} refused unsupported: {}", r.reason));
                }
                Err(_) => {}
            }
        }
    }
    assert!(failures.is_empty(), "{} failures; first:\n{}", failures.len(), failures[0]);
    assert!(extracted > 0, "no extraction succeeded");
}
