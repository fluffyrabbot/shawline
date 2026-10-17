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

pub struct Stage {
    pub name: &'static str,
    pub run: fn(&str) -> String,
}

/// Every stage, in pipeline order. Each wave appends its stages here.
pub const STAGES: &[Stage] = &[Stage { name: "diagram", run: diagram }];

fn diagram(src: &str) -> String {
    match Grid::parse(src) {
        Ok(grid) => shawline_shape::recognize(&grid).to_string(),
        Err(errors) => errors.iter().map(|e| format!("error {e}\n")).collect(),
    }
}

pub fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus")
}

/// Every `.shaw` program under `dir`, sorted.
pub fn programs(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(dir) = pending.pop() {
        for entry in fs::read_dir(&dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|e| e == "shaw") {
                found.push(path);
            }
        }
    }
    found.sort();
    found
}
