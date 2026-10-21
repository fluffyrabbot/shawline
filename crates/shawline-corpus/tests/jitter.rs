//! The jitter corpus (PLAN P5): every canonical program, perturbed in ways
//! that keep its structure, must format back to exactly itself.
//!
//! The perturbations are `shawline_corpus::jitter`.

use std::collections::HashSet;
use std::fs;

use shawline_grid::{Grid, Pos};

use shawline_corpus::jitter::{Fired, Rng, jitter};
use shawline_corpus::{programs, root};

const SEEDS: u64 = 24;

#[test]
fn jittered_programs_format_back_to_canonical() {
    let mut failures = Vec::new();
    let mut first = None;
    let mut tried = 0;
    let mut fired = Fired::default();
    for program in programs(&root()) {
        // Canonical programs: no diagnostics, and no `.fmt.shaw` of their own.
        let check = fs::read_to_string(program.with_extension("check")).unwrap_or_default();
        if !check.is_empty() || program.with_extension("fmt.shaw").exists() {
            continue;
        }
        let src = fs::read_to_string(&program).unwrap();
        let name = program.file_name().unwrap().to_string_lossy().into_owned();
        let diagram = shawline_shape::recognize(&Grid::parse(&src).unwrap());
        let wire: HashSet<Pos> = diagram.wires.iter().flat_map(|w| w.cells.iter().copied()).collect();
        for seed in 0..SEEDS {
            let jittered = jitter(&src, &wire, &mut Rng(seed), &mut fired).text;
            tried += 1;
            let formatted = shawline_cli::format(&jittered);
            if formatted.as_deref() != Ok(src.as_str()) {
                failures.push(format!("{name} seed {seed}"));
                first.get_or_insert_with(|| format!("{name} seed {seed}\njittered:\n{jittered}\nformatted:\n{}", formatted.unwrap_or_default()));
            }
        }
    }
    assert!(tried > 0, "no canonical programs under {:?}", root());
    assert!(fired.bumps > 20 && fired.fills > 20, "too few perturbations fired: {} bumps, {} fills", fired.bumps, fired.fills);
    assert!(
        failures.is_empty(),
        "{} of {tried} jittered programs don't format back:\n{}\n\nfirst failure, {}",
        failures.len(),
        failures.join("\n"),
        first.unwrap_or_default()
    );
}
