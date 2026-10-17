//! The jitter corpus (PLAN P5): every canonical program, perturbed in ways
//! that keep its structure, must format back to exactly itself.
//!
//! Perturbations are seeded, so a failure names a seed that reproduces it.
//! Only fixed-connectivity strokes are swapped for ASCII: the joints
//! `+ . '` take their shape from their neighbours and can fuse with them
//! (SPEC §3), which would change the program rather than jitter it.

use std::collections::HashSet;
use std::fs;

use shawline_grid::{Grid, Pos};

use shawline_corpus::{programs, root};

const SEEDS: u64 = 24;

/// A small deterministic generator (SplitMix64), so the test needs no crates.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    fn chance(&mut self, one_in: u64) -> bool {
        self.next().is_multiple_of(one_in)
    }
}

fn ascii(ch: char) -> Option<char> {
    Some(match ch {
        '─' => '-',
        '│' => '|',
        '┄' => '~',
        '┆' => ':',
        '▶' => '>',
        '◀' => '<',
        '▲' => '^',
        '▼' => 'v',
        _ => return None,
    })
}

/// Rule 4: the space between a word or literal and the wire it feeds is
/// filled with wire (`0 ─▶` becomes `0──▶`). Rule 4 starts a wire one space
/// after its text, so the formatter trims it back. A `«literal»` is skipped
/// as one piece; its inside is never touched.
fn squeeze(row: &mut [char], rng: &mut Rng) -> usize {
    let mut filled = 0;
    let word = shawline_grid::repertoire::is_word_char;
    let mut c = 1;
    while c < row.len() {
        let end = if row[c] == '«' {
            match row[c..].iter().position(|&ch| ch == '»') {
                Some(n) => c + n + 1,
                None => return filled,
            }
        } else if word(row[c]) {
            (c..row.len()).find(|&i| !word(row[i])).unwrap_or(row.len())
        } else {
            c += 1;
            continue;
        };
        let touches_wire = end + 1 < row.len() && row[end] == ' ' && row[end + 1] == '─';
        if touches_wire && rng.chance(3) {
            row[end] = '─';
            filled += 1;
        }
        c = end + 1;
    }
    filled
}

/// Rule 5: one-cell bumps in a straight horizontal wire (never a box
/// border), where the cells above are free and the row above that keeps
/// them clear. Sometimes two bumps sit side by side (`┌─┐┌─┐` over
/// `┘ └┘ └`), which a single straightening pass can merge into one wider
/// dip instead of removing.
fn bump(rows: &mut [Vec<char>], wire: &HashSet<Pos>, rng: &mut Rng) -> bool {
    let count = if rng.chance(3) { 2 } else { 1 };
    let span = 3 * count;
    for r in 2..rows.len() {
        let at = |rows: &[Vec<char>], r: usize, c: usize| rows[r].get(c).copied().unwrap_or(' ');
        for c in 1..rows[r].len().saturating_sub(span + 1) {
            let straight = (c - 1..c + span + 1).all(|i| at(rows, r, i) == '─' && wire.contains(&Pos::new(r, i)));
            let room = (c..c + span).all(|i| at(rows, r - 1, i) == ' ' && at(rows, r - 2, i) == ' ');
            if straight && room && rng.chance(8) {
                let width = rows[r - 1].len().max(c + span);
                rows[r - 1].resize(width, ' ');
                for b in 0..count {
                    let c = c + 3 * b;
                    rows[r][c] = '┘';
                    rows[r][c + 1] = ' ';
                    rows[r][c + 2] = '└';
                    rows[r - 1][c] = '┌';
                    rows[r - 1][c + 1] = '─';
                    rows[r - 1][c + 2] = '┐';
                }
                return true;
            }
        }
    }
    false
}

/// How often each structural perturbation fired, so the test can't pass
/// vacuously.
#[derive(Default)]
struct Fired {
    bumps: usize,
    fills: usize,
}

/// `src` with a bumped wire, filled text gaps, some strokes in ASCII, some
/// trailing spaces, and some blank lines doubled.
fn jitter(src: &str, wire: &HashSet<Pos>, rng: &mut Rng, fired: &mut Fired) -> String {
    let mut rows: Vec<Vec<char>> = src.lines().map(|l| l.chars().collect()).collect();
    if rng.chance(2) && bump(&mut rows, wire, rng) {
        fired.bumps += 1;
    }
    let mut out = String::new();
    for mut row in rows {
        fired.fills += squeeze(&mut row, rng);
        let line: String = row.iter().collect::<String>().trim_end().to_owned();
        let jittered: String = line
            .chars()
            .map(|ch| match ascii(ch) {
                Some(a) if rng.chance(3) => a,
                _ => ch,
            })
            .collect();
        out.push_str(&jittered);
        if rng.chance(4) {
            out.push_str(&" ".repeat(1 + (rng.next() % 3) as usize));
        }
        out.push('\n');
        if line.is_empty() && rng.chance(2) {
            out.push('\n');
        }
    }
    out
}

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
            let jittered = jitter(&src, &wire, &mut Rng(seed), &mut fired);
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
