//! The formatter's two promises (SPEC §11), checked over every error-free
//! corpus program and its ASCII sketch: formatting is idempotent, and it
//! never changes what its input means (the same diagnostics, the same
//! output). A mechanical ASCII sketch can itself mean something else, where
//! an ASCII joint fuses with a neighbour (SPEC §3), so each variant is
//! compared with itself, not with the canonical program.

use std::fs;

use shawline_corpus::{STAGES, programs, root};
use shawline_doc::Doc;

fn stage(name: &str) -> fn(&str) -> String {
    STAGES.iter().find(|s| s.name == name).expect("registered stage").run
}

/// `src` redrawn with the ASCII stroke fallbacks (SPEC §2).
fn ascii(src: &str) -> String {
    src.chars()
        .map(|ch| match ch {
            '─' => '-',
            '│' => '|',
            '┄' => '~',
            '┆' => ':',
            '╭' | '╮' => '.',
            '╰' | '╯' => '\'',
            '┌' | '┐' | '└' | '┘' | '├' | '┤' | '┬' | '┴' | '┼' => '+',
            '▶' => '>',
            '◀' => '<',
            '▲' => '^',
            '▼' => 'v',
            _ => ch,
        })
        .collect()
}

#[test]
fn formatting_is_idempotent_and_preserves_meaning() {
    let (check, out) = (stage("check"), stage("out"));
    let mut failures = Vec::new();
    let mut checked = 0;

    for program in programs(&root()) {
        let src = fs::read_to_string(&program).unwrap();
        if Doc::new(src.as_str()).has_errors() {
            continue;
        }
        let name = program.file_name().unwrap().to_string_lossy().into_owned();
        for (variant, text) in [("canonical", src.clone()), ("ascii", ascii(&src))] {
            let (want_check, want_out) = (check(&text), out(&text));
            let formatted = Doc::new(text.as_str()).formatted().expect("no grid errors").to_owned();
            checked += 1;
            // A fresh revision, so the formatter really runs again.
            if Doc::new(formatted.as_str()).formatted() != Some(formatted.as_str()) {
                failures.push(format!("{name} ({variant}): formatting twice changes it"));
            }
            if check(&formatted) != want_check {
                failures.push(format!("{name} ({variant}): diagnostics change"));
            }
            if out(&formatted) != want_out {
                failures.push(format!("{name} ({variant}): output changes"));
            }
        }
    }

    assert!(checked > 0, "no error-free programs under {:?}", root());
    assert!(failures.is_empty(), "{} of {checked} variants fail:\n{}", failures.len(), failures.join("\n"));
}
