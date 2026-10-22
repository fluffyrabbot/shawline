//! The P9 gate for the gloss layer (PLAN P9), over every valid corpus
//! program: no gloss the strip shows overlaps a vertical stroke, leaves its
//! strip, sits off its word's centre, or comes within one gloss character of
//! another gloss.
//!
//! Strokes are read from the recognized diagram rather than from the
//! renderer, so this check doesn't share the placement's reasoning.

use std::fs;

use shawline_corpus::{programs, root};
use shawline_doc::Doc;
use shawline_grid::{Dir, Grid, Pos};
use shawline_render::{Gloss, glosses};
use shawline_shape::Diagram;

/// Whether a vertical stroke crosses the strip under `row` at `col`: a box's
/// side, or a stroke reaching into the strip from above or below.
fn blocked(diagram: &Diagram, row: usize, col: usize) -> bool {
    let side = diagram.boxes.iter().any(|b| {
        let r = b.rect;
        (r.top..r.bottom).contains(&row) && (col == r.left || col == r.right)
    });
    let reaches = |p: Pos, dir: Dir| diagram.strokes.get(p).is_some_and(|s| s.connects(dir));
    side || reaches(Pos::new(row, col), Dir::S) || reaches(Pos::new(row + 1, col), Dir::N)
}

/// Every way the shown glosses break PLAN P9's placement rule.
fn faults(grid: &Grid, diagram: &Diagram, glosses: &[Gloss]) -> Vec<String> {
    let mut faults = Vec::new();
    let mut placed: Vec<(usize, f64, f64, f64)> = Vec::new();
    for g in glosses {
        let Some((left, right)) = g.span else { continue };
        let n = g.text().chars().count();
        let advance = (right - left) / n as f64;
        let centre = g.at.col as f64 + g.word.chars().count() as f64 / 2.0;
        if n == 0 || advance <= 0.0 {
            faults.push(format!("{}: empty span ({left}, {right})", g.at));
            continue;
        }
        if ((left + right) / 2.0 - centre).abs() > 1e-9 {
            faults.push(format!("{}: ({left}, {right}) is not centred on {centre}", g.at));
        }
        if left < 0.0 || right > grid.width() as f64 {
            faults.push(format!("{}: ({left}, {right}) leaves the strip's 0..{}", g.at, grid.width()));
        }
        let first = left.max(0.0).floor() as usize;
        for col in (first..grid.width()).take_while(|&c| (c as f64) < right) {
            if blocked(diagram, g.at.row, col) {
                faults.push(format!("{}: ({left}, {right}) covers the stroke at column {col}", g.at));
            }
        }
        for &(row, l, r, a) in &placed {
            let gap = if l < left { left - r } else { l - right };
            if row == g.at.row && gap < advance.max(a) - 1e-9 {
                faults.push(format!("{}: ({left}, {right}) is within a character of ({l}, {r})", g.at));
            }
        }
        placed.push((g.at.row, left, right, advance));
    }
    faults
}

#[test]
fn shown_glosses_keep_clear_of_strokes_and_each_other() {
    let mut failures = Vec::new();
    let (mut shown, mut total) = (0, 0);
    for program in programs(&root()) {
        let name = program.file_name().unwrap().to_string_lossy().into_owned();
        let valid = name[..2].parse::<u32>().is_ok_and(|n| n < 80)
            && fs::read_to_string(program.with_extension("check")).is_ok_and(|c| c.is_empty());
        if !valid {
            continue;
        }
        let doc = Doc::new(fs::read_to_string(&program).unwrap());
        let (grid, diagram) = (doc.grid().unwrap(), doc.diagram().unwrap());
        let found = glosses(grid);
        total += found.len();
        shown += found.iter().filter(|g| g.shown()).count();
        failures.extend(faults(grid, diagram, &found).into_iter().map(|f| format!("{name} {f}")));
    }
    assert!(total > 0, "no words found");
    assert!(failures.is_empty(), "{} of {shown} shown glosses misplaced:\n{}", failures.len(), failures.join("\n"));
}

/// The check bites: each rule, broken once on `03-double`.
#[test]
fn misplaced_glosses_are_caught() {
    let doc = Doc::new(include_str!("../../../corpus/03-double.shaw"));
    let (grid, diagram) = (doc.grid().unwrap(), doc.diagram().unwrap());
    // Row 5 is `▶ 𐑯 ──┬───▶ 𐑐 ├───────▶ 𐑬`: the definition's side is column
    // 0, the node box's sides are columns 10 and 14, and the tee at column 6
    // sends a branch down through the strip.
    let gloss = |col: usize, word: &str, reading: &str, gloss: Option<&'static str>, span| Gloss {
        at: Pos::new(5, col),
        word: word.into(),
        reading: reading.into(),
        gloss,
        span: Some(span),
    };
    let add = |span| gloss(12, "𐑐", "p", Some("add"), span);
    let n = |reading: &str, span| gloss(2, "𐑯", reading, None, span);
    let fine = [add((11.75, 13.25)), n("abcd", (1.5, 3.5)), gloss(4, "𐑯", "ab", None, (4.0, 5.0))];
    assert_eq!(faults(grid, diagram, &fine), Vec::<String>::new());
    let cases = [
        ("not centred", vec![add((11.5, 13.0))]),
        ("stroke at column 14", vec![gloss(12, "𐑐", "abcdefgh", None, (10.5, 14.5))]),
        ("leaves the strip", vec![n("abcdefghijkl", (-0.5, 5.5))]),
        ("stroke at column 6", vec![gloss(5, "𐑯𐑯𐑯", "abcd", None, (5.5, 7.5))]),
        ("within a character", vec![n("abcd", (1.5, 3.5)), gloss(4, "𐑯", "abc", None, (3.75, 5.25))]),
    ];
    for (fault, glosses) in cases {
        let found = faults(grid, diagram, &glosses);
        assert!(found.iter().any(|f| f.contains(fault)), "no {fault:?} in {found:?}");
    }
}
