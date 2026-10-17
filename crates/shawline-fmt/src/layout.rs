//! SPEC §11 rule 7: no trailing spaces, one blank line between top-level items.

use shawline_shape::Diagram;

use crate::canvas::{Canvas, render_rows};

/// The canvas with trailing spaces removed.
pub(crate) fn trimmed(canvas: &Canvas) -> Vec<String> {
    canvas.rows().iter().map(|r| r.iter().collect::<String>().trim_end().to_owned()).collect()
}

/// Rows with blank lines normalized: none leading or trailing, runs of
/// blanks collapsed to one, and one inserted where a definition ends on the
/// row right above another's top. A comment directly above or below a
/// definition stays attached to it.
pub(crate) fn spaced(rows: &[String], diagram: &Diagram) -> String {
    let defs: Vec<(usize, usize)> = diagram
        .boxes
        .iter()
        .filter(|b| b.parent.is_none())
        .map(|b| (b.rect.top, b.rect.bottom))
        .collect();
    let boundary = |r: usize| {
        defs.iter().any(|&(_, bottom)| bottom == r)
            && defs.iter().any(|&(top, _)| top == r + 1)
            && !defs.iter().any(|&(top, bottom)| top <= r && r < bottom)
    };

    let mut out: Vec<String> = Vec::new();
    let mut pending_blank = false;
    for (r, row) in rows.iter().enumerate() {
        if row.is_empty() {
            pending_blank = !out.is_empty();
            continue;
        }
        if pending_blank {
            out.push(String::new());
            pending_blank = false;
        }
        out.push(row.clone());
        if boundary(r) && rows.get(r + 1).is_some_and(|next| !next.is_empty()) {
            pending_blank = true;
        }
    }
    render_rows(out)
}

#[cfg(test)]
mod tests {
    use crate::format;
    use shawline_grid::Grid;

    fn fmt(src: &str) -> String {
        let grid = Grid::parse(src).unwrap();
        format(&grid, &shawline_shape::recognize(&grid))
    }

    const A: &str = "╭─ ·𐑨𐑛 ──╮\n│ 1 ─────▶ 𐑚\n╰────────╯\n";

    #[test]
    fn blank_lines_between_items() {
        let src = format!("\n\n※ 𐑨\n\n\n{A}\n\n\n{A}{A}\n\n");
        assert_eq!(fmt(&src), format!("※ 𐑨\n\n{A}\n{A}\n{A}"));
    }

    #[test]
    fn trailing_spaces() {
        let src = "※ 𐑨   \n\n╭─ ·𐑨𐑛 ──╮  \n│ 1 ─────▶ 𐑚 \n╰────────╯\n";
        assert_eq!(fmt(src), format!("※ 𐑨\n\n{A}"));
    }

    #[test]
    fn crlf_and_bom() {
        let src = format!("\u{feff}{}", A.replace('\n', "\r\n"));
        assert_eq!(fmt(&src), A);
    }
}
