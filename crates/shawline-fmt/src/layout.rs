//! SPEC §11 rule 7: no trailing spaces, one blank line between top-level items.

use shawline_shape::Diagram;

use crate::canvas::{Canvas, render_rows};

/// The canvas with trailing spaces removed.
pub(crate) fn trimmed(canvas: &Canvas) -> Vec<String> {
    canvas.rows().iter().map(|r| r.iter().collect::<String>().trim_end().to_owned()).collect()
}

/// One blank-line change, by row index before any change is made.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Op {
    /// Remove blank row `r`.
    Drop(usize),
    /// Insert a blank row before row `r`.
    Insert(usize),
}

/// The changes that normalize blank lines: none leading or trailing, runs
/// of blanks collapsed to one, and one inserted where a definition ends on
/// the row right above another's top. A comment directly above or below a
/// definition stays attached to it.
pub(crate) fn ops(rows: &[String], diagram: &Diagram) -> Vec<Op> {
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

    let last = rows.iter().rposition(|r| !r.is_empty());
    let mut ops = Vec::new();
    let mut seen_text = false;
    for (r, row) in rows.iter().enumerate() {
        if row.is_empty() {
            // Keep the first blank of a run between two non-blank rows.
            let keep = seen_text && !rows[r - 1].is_empty() && last.is_some_and(|l| r < l);
            if !keep {
                ops.push(Op::Drop(r));
            }
            continue;
        }
        seen_text = true;
        if boundary(r) && rows.get(r + 1).is_some_and(|next| !next.is_empty()) {
            ops.push(Op::Insert(r + 1));
        }
    }
    ops
}

/// The rows with `ops` applied, as source text.
pub(crate) fn render(rows: &[String], ops: &[Op]) -> String {
    let mut out = Vec::with_capacity(rows.len());
    for (r, row) in rows.iter().enumerate() {
        if ops.contains(&Op::Insert(r)) {
            out.push(String::new());
        }
        if !ops.contains(&Op::Drop(r)) {
            out.push(row.clone());
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
    fn blank_line_edits_stand_alone() {
        // From the formatter audit (/tmp/fmtfuzz/red1.shaw): a blank row
        // between the two boxes would split the wire on the right, and that
        // used to block dropping the leading blank row too.
        let src = "\n      ┌─  ┐                              \n      └───┘                             │\n                              ┌─  ┐     │\n                              └───┘      \n";
        let out = fmt(src);
        let trimmed: String = src[1..].lines().map(|l| format!("{}\n", l.trim_end())).collect();
        assert_eq!(out, trimmed);
        assert_eq!(fmt(&out), out);
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
