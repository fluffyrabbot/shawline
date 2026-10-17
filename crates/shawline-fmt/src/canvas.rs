//! A mutable copy of the grid that formatting phases write into.

use shawline_grid::{Grid, Pos};
use shawline_shape::Diagram;

/// A set of cell writes that stand or fall together.
pub(crate) type Edit = Vec<(Pos, char)>;

#[derive(Clone, Debug)]
pub(crate) struct Canvas {
    rows: Vec<Vec<char>>,
}

impl Canvas {
    pub(crate) fn from_grid(grid: &Grid) -> Self {
        Self { rows: (0..grid.height()).map(|r| grid.row(r).to_vec()).collect() }
    }

    pub(crate) fn rows(&self) -> &[Vec<char>] {
        &self.rows
    }

    pub(crate) fn get(&self, p: Pos) -> char {
        self.rows.get(p.row).and_then(|r| r.get(p.col)).copied().unwrap_or(' ')
    }

    /// Writes every cell of `edit`, padding short rows with spaces.
    pub(crate) fn write(&mut self, edit: &Edit) {
        for &(p, ch) in edit {
            let row = &mut self.rows[p.row];
            if row.len() <= p.col {
                row.resize(p.col + 1, ' ');
            }
            row[p.col] = ch;
        }
    }

    /// The text as it stands, rows as written.
    pub(crate) fn render(&self) -> String {
        render_rows(self.rows.iter().map(|r| r.iter().collect::<String>()))
    }

    /// Parses and recognizes the canvas as it stands.
    pub(crate) fn recognize(&self) -> (Grid, Diagram) {
        reparse(&self.render())
    }
}

/// Joins rows into source text with one trailing newline.
pub(crate) fn render_rows(rows: impl IntoIterator<Item = String>) -> String {
    let mut out = String::new();
    for row in rows {
        out.push_str(&row);
        out.push('\n');
    }
    out
}

/// Parses text the formatter wrote. It only ever writes repertoire
/// characters, so the grid always parses.
pub(crate) fn reparse(src: &str) -> (Grid, Diagram) {
    let grid = Grid::parse(src).expect("the formatter writes only repertoire characters");
    let diagram = shawline_shape::recognize(&grid);
    (grid, diagram)
}
