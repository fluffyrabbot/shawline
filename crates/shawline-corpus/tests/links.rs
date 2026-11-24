//! The `link` oracle (SPEC §17, PLAN P12). Kept apart from `corpus.rs`, so a
//! codec that panics fails these tests alone.
//!
//! The fixed links under `corpus/links/` were encoded by Python's `zlib`
//! (raw DEFLATE, level 9) and `base64.urlsafe_b64encode`, not by
//! `shawline-share`, so they hold it to the format. A link made today must
//! open in every later version: these files never change.
//!
//! Each file holds one fragment, without its `#`, and a final newline.

use std::fs;
use std::path::{Path, PathBuf};

use shawline_corpus::{programs, root};
use shawline_share::{LinkError, MAX_TEXT, decode, encode};

fn fragment(path: &Path) -> String {
    let text = fs::read_to_string(path).unwrap();
    text.strip_suffix('\n').unwrap_or(&text).to_owned()
}

fn links(dir: &str) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = fs::read_dir(root().join(dir))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "link"))
        .collect();
    found.sort();
    found
}

fn stem(path: &Path) -> String {
    path.file_stem().unwrap().to_string_lossy().into_owned()
}

fn examples() -> PathBuf {
    root().join("../examples")
}

/// `corpus/links/<name>.link` opens as `corpus/<name>.shaw`, or as
/// `examples/<name>.shaw`, byte for byte.
#[test]
fn fixed_links_open_as_their_sheets() {
    let fixed = links("links");
    assert!(fixed.len() >= 4, "{fixed:?}");
    for link in fixed {
        let name = stem(&link);
        let sheet = [root().join(format!("{name}.shaw")), examples().join(format!("{name}.shaw"))]
            .into_iter()
            .find(|p| p.exists())
            .unwrap_or_else(|| panic!("{name}.link names no sheet"));
        assert_eq!(decode(&fragment(&link)).as_deref(), Ok(fs::read_to_string(sheet).unwrap().as_str()), "{name}");
    }
}

/// The limit and the bytes no editor shows survive.
#[test]
fn edge_links_open() {
    let expected = |name: &str| match name {
        "empty" => String::new(),
        "exactly-1-mib" => "a".repeat(MAX_TEXT),
        "bom-crlf" => "\u{feff}·𐑛\r\n".to_owned(),
        other => panic!("no expectation for edge/{other}.link"),
    };
    for link in links("links/edge") {
        let name = stem(&link);
        assert_eq!(decode(&fragment(&link)), Ok(expected(&name)), "{name}");
    }
}

/// `corpus/links/refused/<kind>[-detail].link` fails with `<kind>`.
#[test]
fn refused_links_say_why() {
    let expected = |name: &str| match name {
        "not-a-link" => LinkError::NotALink,
        "unknown-version-2" => LinkError::UnknownVersion(2),
        "unknown-version-max" => LinkError::UnknownVersion(u32::MAX),
        "too-large" => LinkError::TooLarge,
        "not-text" => LinkError::NotText,
        damaged if damaged.starts_with("damaged-") => LinkError::Damaged,
        other => panic!("no expectation for refused/{other}.link"),
    };
    let refused = links("links/refused");
    assert!(refused.len() >= 6, "{refused:?}");
    for link in refused {
        let name = stem(&link);
        assert_eq!(decode(&fragment(&link)), Err(expected(&name)), "{name}");
    }
}

/// Every corpus program and every example survives a round trip, and its
/// link starts with the version.
#[test]
fn every_sheet_round_trips() {
    let sheets: Vec<PathBuf> = programs(&root()).into_iter().chain(programs(&examples())).collect();
    assert!(sheets.len() > 20, "{} sheets", sheets.len());
    for sheet in sheets {
        let text = fs::read_to_string(&sheet).unwrap();
        let link = encode(&text);
        assert!(link.starts_with("v1."), "{}: {link}", sheet.display());
        assert_eq!(decode(&link).as_deref(), Ok(text.as_str()), "{}", sheet.display());
    }
}
