//! The formatter's promises under random damage: every corpus program,
//! mutated a few cells at a time, must format idempotently and keep its
//! diagnostics (lints aside, since reflowing wires legitimately changes them).
//!
//! Mutants are never run. A mutated tail loop can spin forever in constant
//! memory, which no VM limit catches, so meaning is checked through the
//! diagnostic codes alone.

use std::fs;

use shawline_corpus::{programs, root};

const MUTANTS_PER_PROGRAM: u64 = 40;

/// SplitMix64, so the test needs no crates and every failure names a seed.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

const ALPHABET: &str = " ─│┌┐└┘├┤┬┴┼╭╮╯╰┄▶◀▼-|+.'~>v𐑐𐑚𐑯𐑦𐑤1·«» ";

fn mutate(src: &str, rng: &mut Rng) -> String {
    let alphabet: Vec<char> = ALPHABET.chars().collect();
    let mut rows: Vec<Vec<char>> = src.lines().map(|l| l.chars().collect()).collect();
    for _ in 0..1 + rng.below(4) {
        let r = rng.below(rows.len());
        let row = &mut rows[r];
        match rng.below(4) {
            0 if !row.is_empty() => {
                let c = rng.below(row.len());
                row[c] = alphabet[rng.below(alphabet.len())];
            }
            1 => {
                let c = rng.below(row.len() + 1);
                row.insert(c, ' ');
            }
            2 if !row.is_empty() => {
                let c = rng.below(row.len());
                row.remove(c);
            }
            3 if row.len() > 1 => {
                let c = rng.below(row.len() - 1);
                row.swap(c, c + 1);
            }
            _ => {}
        }
    }
    rows.iter().map(|r| r.iter().collect::<String>() + "\n").collect()
}

/// Every non-lint diagnostic code, sorted: what a sheet means, positions aside.
fn codes(src: &str) -> Vec<String> {
    let diagnostics = match shawline_cli::check(src) {
        Ok(checked) => checked.diagnostics,
        Err(errors) => errors,
    };
    let mut codes: Vec<String> =
        diagnostics.iter().filter(|d| !d.code.starts_with('L')).map(|d| d.code.to_owned()).collect();
    codes.sort();
    codes
}

#[test]
fn mutants_format_idempotently_and_keep_their_diagnostics() {
    let mut failures = Vec::new();
    let mut tried = 0;
    for program in programs(&root()) {
        let src = fs::read_to_string(&program).unwrap();
        let name = program.file_name().unwrap().to_string_lossy().into_owned();
        for seed in 0..MUTANTS_PER_PROGRAM {
            let mutant = mutate(&src, &mut Rng(seed));
            let Ok(once) = shawline_cli::format(&mutant) else { continue };
            tried += 1;
            let twice = shawline_cli::format(&once).expect("formatted text stays on the grid");
            if twice != once {
                failures.push(format!("{name} seed {seed}: formatting twice changes it\n{mutant}"));
            } else if codes(&once) != codes(&mutant) {
                failures.push(format!("{name} seed {seed}: diagnostics change\n{mutant}"));
            }
        }
    }
    assert!(tried > 500, "only {tried} mutants stayed on the grid");
    assert!(
        failures.is_empty(),
        "{} of {tried} mutants fail; first:\n{}",
        failures.len(),
        failures.first().cloned().unwrap_or_default()
    );
}
