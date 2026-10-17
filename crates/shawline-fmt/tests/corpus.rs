//! The formatter against the corpus: every program with an empty `.check`
//! formats to its `.fmt.shaw` (or to itself), formatting is idempotent on
//! every program, and the canonical programs are lint-free apart from the
//! findings listed here.

use std::fs;
use std::path::{Path, PathBuf};

use shawline_grid::Grid;

/// Lints the canonical corpus is known to trigger. Empty since Gate B raised
/// `L003` to more than 4 bends: the U-shaped detours in `06-pair` and
/// `08-factorial` take four bends however they are routed.
const KNOWN_LINTS: &[(&str, &str)] = &[];

fn corpus() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus")
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

fn format(src: &str) -> String {
    let grid = Grid::parse(src).expect("corpus programs are in the repertoire");
    shawline_fmt::format(&grid, &shawline_shape::recognize(&grid))
}

/// `corpus/fix/`: each sheet fixes to its `.fix.shaw`, fixing is idempotent,
/// and plain formatting never repairs a slip.
#[test]
fn corpus_fixes_near_boxes() {
    let dir = corpus().join("fix");
    let mut programs: Vec<PathBuf> = fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| {
            let name = p.to_string_lossy();
            name.ends_with(".shaw") && !name.ends_with(".fix.shaw")
        })
        .collect();
    programs.sort();
    let fix = |src: &str| {
        let grid = Grid::parse(src).unwrap();
        shawline_fmt::fix(&grid, &shawline_shape::recognize(&grid))
    };
    let slips = |src: &str| {
        let diagram = shawline_shape::recognize(&Grid::parse(src).unwrap());
        diagram.boxes.iter().map(|b| b.slips.len()).sum::<usize>()
    };

    let mut failures = Vec::new();
    for path in &programs {
        let name = path.file_stem().unwrap().to_string_lossy().into_owned();
        let src = fs::read_to_string(path).unwrap();
        let expected = fs::read_to_string(path.with_extension("fix.shaw")).unwrap();
        let fixed = fix(&src);
        if fixed != expected {
            failures.push(format!("{name}: fixes to\n{fixed}\nexpected\n{expected}"));
        }
        if fix(&fixed) != fixed {
            failures.push(format!("{name}: fixing twice changes it"));
        }
        if slips(&format(&src)) != slips(&src) {
            failures.push(format!("{name}: plain formatting moved a slipped cell"));
        }
    }
    assert!(programs.len() >= 3, "only {} programs under {dir:?}", programs.len());
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn corpus_formats_canonically() {
    let mut programs: Vec<PathBuf> = fs::read_dir(corpus())
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| {
            let name = p.to_string_lossy();
            name.ends_with(".shaw") && !name.ends_with(".fmt.shaw")
        })
        .collect();
    programs.sort();

    let mut failures = Vec::new();
    let mut lints = Vec::new();
    let mut checked = 0;
    let mut sketched = 0;
    for path in &programs {
        let name = path.file_stem().unwrap().to_string_lossy().into_owned();
        let src = fs::read_to_string(path).unwrap();
        let check = fs::read_to_string(path.with_extension("check")).unwrap_or_default();
        if !check.trim().is_empty() {
            // Rejected programs need only survive formatting unchanged in meaning,
            // which the formatter checks itself; idempotence is checked here.
            if Grid::parse(&src).is_ok() {
                let once = format(&src);
                if format(&once) != once {
                    failures.push(format!("{name}: not idempotent"));
                }
            }
            continue;
        }
        checked += 1;
        let expected = fs::read_to_string(path.with_extension("fmt.shaw")).unwrap_or(src.clone());

        let once = format(&src);
        if once != expected {
            failures.push(format!("{name}: formats to\n{once}\nexpected\n{expected}"));
        }
        let twice = format(&once);
        if twice != once {
            failures.push(format!("{name}: not idempotent; second pass gives\n{twice}"));
        }

        if expected == src {
            let grid = Grid::parse(&src).unwrap();
            let diagram = shawline_shape::recognize(&grid);
            for d in shawline_fmt::lint(&grid, &diagram) {
                lints.push((name.clone(), format!("{} {}", d.at, d.code), d.message));
            }

            // An ASCII sketch of the program formats back to it, wherever
            // ASCII can draw the same thing: its joints join every neighbour,
            // and a `+` can't be a tee with nothing attached.
            let sketch = ascii(&src);
            let rough = shawline_shape::recognize(&Grid::parse(&sketch).unwrap());
            let dump = |d: &shawline_shape::Diagram| {
                format!("{d}{}", shawline_shape::wire::dump(&d.wires))
            };
            if dump(&rough) == dump(&diagram) {
                sketched += 1;
                if format(&sketch) != src {
                    failures.push(format!("{name}: ASCII sketch formats to\n{}", format(&sketch)));
                }
            }
        }
    }
    assert!(sketched >= 10, "only {sketched} programs survive as ASCII sketches");
    assert!(checked >= 17, "only {checked} canonical programs found");
    assert!(failures.is_empty(), "{}", failures.join("\n"));

    for (name, at, message) in &lints {
        eprintln!("lint {name} {at}: {message}");
    }
    let found: Vec<(&str, &str)> = lints.iter().map(|(n, a, _)| (n.as_str(), a.as_str())).collect();
    assert_eq!(found, KNOWN_LINTS, "the canonical corpus should be lint-free");
}
