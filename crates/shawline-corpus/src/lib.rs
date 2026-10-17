//! The golden corpus: the one oracle every stage is checked against.
//!
//! Each `corpus/**/*.shaw` program may have an expected-output file per stage
//! beside it, named `<program>.<stage>`. A stage is checked only where its
//! expected file exists, so stages come online one at a time.
//!
//! `SHAWLINE_BLESS=<stage>[,<stage>…]` (or `all`) writes actual output over the
//! expected files for those stages instead of comparing. Only the lead blesses;
//! see `CLAUDE.md`.

use std::fs;
use std::path::{Path, PathBuf};

use shawline_grid::Grid;
use shawline_shape::Diagram;

pub struct Stage {
    pub name: &'static str,
    pub run: fn(&str) -> String,
}

/// Every stage, in pipeline order. Each wave appends its stages here.
/// Expected files for stages not listed yet (`.ast`, `.check`, `.out`,
/// `.fmt.shaw`) already exist; they come online at Gate B.
pub const STAGES: &[Stage] = &[
    Stage { name: "diagram", run: |src| with_diagram(src, |d| d.to_string()) },
    Stage { name: "wires", run: |src| with_diagram(src, |d| shawline_shape::wire::dump(&d.wires)) },
];

fn with_diagram(src: &str, show: fn(&Diagram) -> String) -> String {
    match Grid::parse(src) {
        Ok(grid) => show(&shawline_shape::recognize(&grid)),
        Err(errors) => errors.iter().map(|e| format!("error {e}\n")).collect(),
    }
}

pub fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus")
}

/// Every `.shaw` program under `dir`, sorted. Expected formatter output
/// (`*.fmt.shaw`) is not itself a program.
pub fn programs(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(dir) = pending.pop() {
        for entry in fs::read_dir(&dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|e| e == "shaw")
                && !path.to_string_lossy().ends_with(".fmt.shaw")
            {
                found.push(path);
            }
        }
    }
    found.sort();
    found
}
