//! Every corpus program's `Y` diagnostics match the `Y` lines of its
//! `.check` file. Programs the grid rejects (`G` codes) never reach syntax.

use std::fs;
use std::path::Path;

use shawline_grid::Grid;

/// `ROW:COL CODE` lines sorted by position, then code.
fn sorted(mut lines: Vec<(usize, usize, String)>) -> Vec<String> {
    lines.sort();
    lines.into_iter().map(|(r, c, code)| format!("{r}:{c} {code}")).collect()
}

fn expected_y(check: &str) -> Vec<String> {
    let lines = check
        .lines()
        .filter_map(|line| {
            let (pos, code) = line.trim().split_once(' ')?;
            let (r, c) = pos.split_once(':')?;
            if !code.starts_with('Y') {
                return None;
            }
            Some((r.parse().ok()?, c.parse().ok()?, code.to_owned()))
        })
        .collect();
    sorted(lines)
}

#[test]
fn y_diagnostics_match_corpus_checks() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus");
    let mut programs: Vec<_> = fs::read_dir(&root)
        .expect("corpus directory")
        .map(|e| e.expect("corpus entry").path())
        .filter(|p| {
            let name = p.to_string_lossy();
            name.ends_with(".shaw") && !name.ends_with(".fmt.shaw")
        })
        .collect();
    programs.sort();
    assert!(!programs.is_empty(), "no corpus programs under {}", root.display());

    let mut failures = Vec::new();
    let mut checked = 0;
    for path in &programs {
        let src = fs::read_to_string(path).expect("readable program");
        let Ok(grid) = Grid::parse(&src) else { continue };
        let diagram = shawline_shape::recognize(&grid);
        let (sheet, diags) = shawline_syntax::parse(&grid, &diagram);
        // The dump must render for every program, valid or not.
        let _ = sheet.to_string();

        let stray: Vec<_> = diags.iter().filter(|d| !d.code.starts_with('Y')).collect();
        assert!(stray.is_empty(), "{}: syntax reported non-Y codes {stray:?}", path.display());
        let actual =
            sorted(diags.iter().map(|d| (d.at.row, d.at.col, d.code.to_owned())).collect());
        let check = fs::read_to_string(path.with_extension("check")).unwrap_or_default();
        let expected = expected_y(&check);
        if actual != expected {
            failures.push(format!(
                "{}\n  expected {expected:?}\n  actual   {actual:?}\n  {diags:#?}",
                path.display()
            ));
        }
        checked += 1;
    }
    assert!(checked > 0);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
