//! The golden corpus: the one oracle every stage is checked against.
//!
//! Each `corpus/**/*.shaw` program may have an expected-output file per stage
//! beside it, named `<program>.<stage>`. A stage is checked where its
//! expected file exists, or where its fallback supplies an expectation.
//!
//! `SHAWLINE_BLESS=<stage>[,<stage>…]` (or `all`) writes actual output over the
//! expected files for those stages instead of comparing. Selective stages
//! only rewrite files that already exist; to add one, create it empty first.
//! Only the lead blesses; see `CLAUDE.md`.

pub mod jitter;

use std::fs;
use std::path::{Path, PathBuf};

use shawline_cli::Theme;
use shawline_grid::{Diagnostic, Grid};
use shawline_shape::Diagram;

pub struct Stage {
    pub name: &'static str,
    pub run: fn(&str) -> String,
    /// The expectation when the stage's file is missing.
    pub fallback: Option<fn(&Path, &str) -> Option<String>>,
    /// Kept for a handful of programs, not all of them.
    pub selective: bool,
}

impl Stage {
    const fn new(name: &'static str, run: fn(&str) -> String) -> Self {
        Stage { name, run, fallback: None, selective: false }
    }
}

/// Every stage, in pipeline order.
pub const STAGES: &[Stage] = &[
    Stage::new("diagram", |src| with_diagram(src, |d| d.to_string())),
    Stage::new("wires", |src| with_diagram(src, |d| shawline_shape::wire::dump(&d.wires))),
    Stage::new("ast", |src| match shawline_cli::check(src) {
        Ok(checked) => checked.sheet.to_string(),
        Err(errors) => lines(&errors),
    }),
    Stage::new("check", |src| match shawline_cli::check(src) {
        Ok(checked) => lines(&checked.diagnostics),
        Err(errors) => lines(&errors),
    }),
    Stage::new("out", out),
    Stage {
        name: "fmt.shaw",
        run: |src| shawline_cli::format(src).unwrap_or_else(|errors| lines(&errors)),
        // A program with no diagnostics and no `.fmt.shaw` is canonical.
        fallback: Some(|program, src| {
            let check = fs::read_to_string(program.with_extension("check")).ok()?;
            check.is_empty().then(|| src.to_owned())
        }),
        selective: true,
    },
    Stage {
        name: "fix.shaw",
        run: |src| shawline_cli::fix(src).unwrap_or_else(|errors| lines(&errors)),
        fallback: None,
        selective: true,
    },
    Stage {
        name: "svg",
        run: |src| shawline_cli::render(src, Theme::LIGHT).unwrap_or_else(|errors| lines(&errors)),
        fallback: None,
        selective: true,
    },
];

/// Program stdout, or `error ROW:COL CODE` (SPEC §9).
fn out(src: &str) -> String {
    let program = match shawline_cli::check(src) {
        Ok(checked) => checked.program,
        Err(_) => None,
    };
    let Some(program) = program else {
        return "static errors\n".to_owned();
    };
    match shawline_cli::run(&program) {
        Ok(values) => values.iter().map(|v| format!("{v}\n")).collect(),
        Err(e) => format!("error {} {}\n", e.at, e.code),
    }
}

/// `ROW:COL CODE` per line, the `.check` format.
fn lines(diagnostics: &[Diagnostic]) -> String {
    diagnostics.iter().map(|d| format!("{} {}\n", d.at, d.code)).collect()
}

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
/// (`*.fmt.shaw`, `*.fix.shaw`) is not itself a program.
pub fn programs(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(dir) = pending.pop() {
        for entry in fs::read_dir(&dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|e| e == "shaw")
                && ![".fmt.shaw", ".fix.shaw"].iter().any(|e| path.to_string_lossy().ends_with(e))
            {
                found.push(path);
            }
        }
    }
    found.sort();
    found
}
