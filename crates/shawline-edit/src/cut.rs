//! Inserting and deleting rows and columns (PLAN P10, "Where rows and
//! columns go" and "How a cut stretches").
//!
//! A row cut runs straight across the sheet, between the top and bottom
//! borders of one top-level box. A column cut runs down the rows of that
//! box only, and may step sideways from row to row: on a row where it would
//! split a text, or widen the gap between a text and the stroke it meets
//! (SPEC §6, canonically one space), it moves to the nearest place that
//! does neither, preferring to stretch the stroke. It never steps past a
//! cell that joins the row above or below, so boxes and vertical wires stay
//! whole. An inserted cell continues a stroke that joins across the cut and
//! is blank elsewhere; deleting takes exactly such cells away again.

use std::collections::{HashMap, HashSet};

use shawline_doc::Doc;
use shawline_grid::{Dir, Grid, Pos};
use shawline_shape::Diagram;
use shawline_shape::stroke::{Kind, Stroke};
use shawline_shape::structure::{Routing, Structure};

use crate::canon::Sheets;
use crate::mapped::{self, Read};
use crate::sheet::{perimeter, text_cells};
use crate::{Axis, Blockage, Edited, Insertion, Refusal, RefusalKind};

/// The most rows or columns one edit inserts or deletes.
pub(crate) const MAX_COUNT: usize = 64;

/// Where a cut went, as a map of cells.
#[derive(Clone, Debug)]
pub(crate) struct Cut {
    axis: Axis,
    count: usize,
    delete: bool,
    /// The row the cut is before (rows), or the first row it spans
    /// (columns).
    row: usize,
    /// For columns: the column the cut is before on each spanned row.
    cols: Vec<usize>,
}

impl Cut {
    /// Where cell `p` went; `None` for a deleted cell.
    pub(crate) fn cell(&self, p: Pos) -> Option<Pos> {
        match self.axis {
            Axis::Row => Some(Pos::new(self.shift(p.row, self.row)?, p.col)),
            Axis::Column => match p.row.checked_sub(self.row).and_then(|k| self.cols.get(k)) {
                Some(&at) => Some(Pos::new(p.row, self.shift(p.col, at)?)),
                None => Some(p),
            },
        }
    }

    fn shift(&self, i: usize, at: usize) -> Option<usize> {
        match (self.delete, i >= at) {
            (_, false) => Some(i),
            (false, true) => Some(i + self.count),
            (true, true) if i < at + self.count => None,
            (true, true) => Some(i - self.count),
        }
    }
}

pub(crate) fn insert(doc: &Doc, insertion: Insertion) -> Result<Edited, Refusal> {
    cut(doc, insertion, false).map(|(edited, _)| edited)
}

pub(crate) fn delete(doc: &Doc, deletion: Insertion) -> Result<Edited, Refusal> {
    cut(doc, deletion, true).map(|(edited, _)| edited)
}

/// Inserts (or deletes) `i`, giving the edit and where its cells went.
pub(crate) fn cut(doc: &Doc, i: Insertion, delete: bool) -> Result<(Edited, Cut), Refusal> {
    let refuse = |kind, why: &str| Refusal::new(i.at, kind, why);
    let what = if delete { "delete" } else { "insert" };
    if i.count == 0 || i.count > MAX_COUNT {
        let why = format!("{what} between 1 and {MAX_COUNT} rows or columns at a time");
        return Err(refuse(RefusalKind::Target, &why));
    }
    let sheets = Sheets::read(doc, i.at)?;
    let outside = || refuse(RefusalKind::Target, "rows and columns go inside a definition; pick a cell in one");
    let at = sheets.place(i.at).ok_or_else(outside)?;
    let (grid, diagram) = sheets.canon();
    let top = diagram
        .boxes
        .iter()
        .find(|b| b.parent.is_none() && b.rect.contains(at))
        .ok_or_else(outside)?
        .rect;
    let sheet = Sheet::new(grid, diagram);
    let (rows, cut) = match i.axis {
        Axis::Row => {
            let fits = if delete {
                top.top < at.row && at.row + i.count <= top.bottom
            } else {
                top.top < at.row && at.row <= top.bottom
            };
            if !fits {
                let why = "rows go between a definition's top and bottom borders";
                return Err(refuse(RefusalKind::Target, why));
            }
            let rows = if delete { sheet.delete_rows(at.row, i.count) } else { Ok(sheet.insert_rows(at.row, i.count)) };
            let rows = rows.map_err(|why| refuse(RefusalKind::Target, &why))?;
            (rows, Cut { axis: Axis::Row, count: i.count, delete, row: at.row, cols: Vec::new() })
        }
        Axis::Column => {
            let mut cols = Vec::new();
            for row in top.top..=top.bottom {
                let found = if delete { sheet.deletion(row, at.col, i.count) } else { sheet.insertion(row, at.col) };
                let Some(col) = found else {
                    let why = if delete {
                        format!(
                            "row {} has no blank or straight cell near here to delete; delete a column elsewhere",
                            row + 1
                        )
                    } else {
                        format!("a column here would split the text on row {}; try a column beside it", row + 1)
                    };
                    let kind = if delete { RefusalKind::Target } else { RefusalKind::Blocked(Blockage::Spacing) };
                    return Err(refuse(kind, &why));
                };
                cols.push(col);
            }
            let cut = Cut { axis: Axis::Column, count: i.count, delete, row: top.top, cols };
            (sheet.cut_columns(&cut), cut)
        }
    };
    let text = render(&rows);
    let edited = check(&sheets, text, &cut).map_err(|why| refuse(RefusalKind::Blocked(Blockage::Spacing), why))?;
    Ok((edited, cut))
}

/// The result, if it keeps everything a cut must keep: canonical text, the
/// same structure relabelled at [`Routing::Kept`], and the same codes.
fn check(sheets: &Sheets, text: String, cut: &Cut) -> Result<Edited, &'static str> {
    let doc = Doc::new(text);
    let why = "the strokes here wouldn't line up again; try a row or column beside it";
    if doc.formatted() != Some(doc.text()) {
        return Err(why);
    }
    let old: Read = sheets.canon();
    let new = mapped::read(&doc);
    let map = mapped::map(old, new, |p| cut.cell(p));
    if !map.is_total() {
        return Err(why);
    }
    let expected = Structure::of(old.0, old.1).relabel(&map);
    expected.expect(&Structure::of(new.0, new.1), Routing::Kept).map_err(|_| why)?;
    if codes(sheets.doc()) != codes(&doc) {
        return Err(why);
    }
    Ok(Edited { doc, map, tidied: sheets.tidied() })
}

/// Non-lint diagnostic codes, sorted.
pub(crate) fn codes(doc: &Doc) -> Vec<&'static str> {
    let mut codes: Vec<&'static str> =
        doc.diagnostics().iter().map(|d| d.code).filter(|c| !c.starts_with('L')).collect();
    codes.sort_unstable();
    codes
}

pub(crate) fn render(rows: &[Vec<char>]) -> String {
    let mut out = String::new();
    for row in rows {
        let line: String = row.iter().collect();
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out
}

/// The canonical sheet as a cut sees it.
struct Sheet<'a> {
    grid: &'a Grid,
    diagram: &'a Diagram,
    texts: HashMap<Pos, usize>,
    /// Each border cell's boxes.
    borders: HashMap<Pos, Vec<usize>>,
}

impl<'a> Sheet<'a> {
    fn new(grid: &'a Grid, diagram: &'a Diagram) -> Self {
        let mut borders: HashMap<Pos, Vec<usize>> = HashMap::new();
        for (i, b) in diagram.boxes.iter().enumerate() {
            for p in perimeter(b.rect) {
                borders.entry(p).or_default().push(i);
            }
        }
        Self { grid, diagram, texts: text_cells(diagram), borders }
    }

    /// Whether `a` and `b` are neighbours along one side of a box. Its
    /// arrowheads join the border on either side, though their strokes
    /// don't reach that way.
    fn along(&self, a: Pos, b: Pos) -> bool {
        let (Some(x), Some(y)) = (self.borders.get(&a), self.borders.get(&b)) else { return false };
        x.iter().filter(|i| y.contains(i)).any(|&i| {
            let r = self.diagram.boxes[i].rect;
            if a.row == b.row {
                a.row == r.top || a.row == r.bottom
            } else {
                a.col == r.left || a.col == r.right
            }
        })
    }

    fn stroke(&self, p: Pos) -> Option<Stroke> {
        if p.row >= self.grid.height() {
            return None;
        }
        self.diagram.strokes.get(p)
    }

    /// Whether `a` and `b`, its neighbour in direction `dir`, hold one
    /// stroke between them: either reaches toward the other, which is a
    /// stroke. A cut between them must stretch it.
    fn joins(&self, a: Option<Pos>, dir: Dir, b: Option<Pos>) -> bool {
        let (Some(a), Some(b)) = (a, b) else { return false };
        let (Some(x), Some(y)) = (self.stroke(a), self.stroke(b)) else { return false };
        x.connects(dir) || y.connects(dir.opposite()) || self.along(a, b)
    }

    /// The cell a cut between `a` and its neighbour `b` in direction `dir`
    /// fills in: the stroke stretched, or a blank.
    fn filler(&self, a: Option<Pos>, dir: Dir, b: Option<Pos>) -> char {
        if !self.joins(a, dir, b) {
            return ' ';
        }
        let dashed = [a, b].into_iter().flatten().any(|p| self.stroke(p).is_some_and(|s| s.kind == Kind::Dashed));
        match (dir.is_horizontal(), dashed) {
            (true, false) => '─',
            (true, true) => '┄',
            (false, false) => '│',
            (false, true) => '┆',
        }
    }

    fn row(&self, row: usize) -> Vec<char> {
        self.grid.row(row).to_vec()
    }

    fn rows(&self) -> Vec<Vec<char>> {
        (0..self.grid.height()).map(|r| self.row(r)).collect()
    }

    fn insert_rows(&self, at: usize, count: usize) -> Vec<Vec<char>> {
        let up = |c: usize| Some(Pos::new(at - 1, c));
        let filled: Vec<char> =
            (0..self.grid.width()).map(|c| self.filler(up(c), Dir::S, Some(Pos::new(at, c)))).collect();
        let mut rows = self.rows();
        for _ in 0..count {
            rows.insert(at, filled.clone());
        }
        rows
    }

    fn delete_rows(&self, at: usize, count: usize) -> Result<Vec<Vec<char>>, String> {
        let up = |c: usize| Some(Pos::new(at - 1, c));
        for c in 0..self.grid.width() {
            let want = self.filler(up(c), Dir::S, Some(Pos::new(at + count, c)));
            for row in at..at + count {
                let p = Pos::new(row, c);
                if self.grid.get(p) != want || self.texts.contains_key(&p) {
                    let why = if self.grid.get(p) == ' ' { "a blank cell holds two strokes apart" } else { "it isn't blank" };
                    return Err(format!(
                        "row {} can't be deleted: {why} at column {}; only blank rows and straight strokes go",
                        row + 1,
                        c + 1
                    ));
                }
            }
        }
        let mut rows = self.rows();
        rows.drain(at..at + count);
        Ok(rows)
    }

    /// Columns `x` on `row` whose cell joins the row above or below.
    fn upright(&self, row: usize) -> HashSet<usize> {
        let width = self.grid.row(row).len();
        (0..width)
            .filter(|&c| {
                let p = Pos::new(row, c);
                let up = row.checked_sub(1).map(|r| Pos::new(r, c));
                self.joins(Some(p), Dir::N, up) || self.joins(Some(p), Dir::S, Some(Pos::new(row + 1, c)))
            })
            .collect()
    }

    /// Cells of `row` that must keep their place relative to their
    /// neighbours: text, and the spaces between a text and the stroke it
    /// meets. Also the boundaries a cut may not take: inside a text, or
    /// within such a gap.
    fn held(&self, row: usize) -> (HashSet<usize>, HashSet<usize>) {
        let (mut cells, mut bounds) = (HashSet::new(), HashSet::new());
        let blank = |c: usize| self.grid.get(Pos::new(row, c)) == ' ';
        for t in self.diagram.texts.iter().filter(|t| t.pos.row == row) {
            let (start, end) = (t.pos.col, t.pos.col + t.width - 1);
            cells.extend(start..=end);
            bounds.extend(start + 1..=end);
            // Left: a stroke reaching east, or an arrowhead pointing east.
            let mut q = start;
            while q > 0 && blank(q - 1) {
                q -= 1;
            }
            if q > 0 && self.meets(Pos::new(row, q - 1), Dir::E) {
                cells.extend(q..start);
                bounds.extend(q..=start);
            }
            let len = self.grid.row(row).len();
            let mut q = end + 1;
            while q < len && blank(q) {
                q += 1;
            }
            if q < len && self.meets(Pos::new(row, q), Dir::W) {
                cells.extend(end + 1..q);
                bounds.extend(end + 1..=q);
            }
        }
        (cells, bounds)
    }

    /// Whether the stroke at `p` meets text across spaces in direction
    /// `dir`: it reaches that way, or is an arrowhead pointing that way.
    fn meets(&self, p: Pos, dir: Dir) -> bool {
        self.stroke(p).is_some_and(|s| s.connects(dir) || s.kind == Kind::Arrow(dir))
    }

    /// Candidates around `from`, nearest first, the right before the left,
    /// each paired with the cells between it and `from` that it passes.
    fn around(from: usize, limit: usize) -> impl Iterator<Item = usize> {
        (0..=limit).flat_map(move |k| {
            let right = Some(from + k);
            let left = (k > 0).then(|| from.checked_sub(k)).flatten();
            right.into_iter().chain(left)
        })
    }

    /// Whether a cut can move along `row` from boundary `from` to `to`
    /// without passing a cell that joins another row.
    fn reaches(upright: &HashSet<usize>, from: usize, to: usize) -> bool {
        !(from.min(to)..from.max(to)).any(|x| upright.contains(&x))
    }

    /// The boundary on `row` nearest `from` where a column can go in.
    fn insertion(&self, row: usize, from: usize) -> Option<usize> {
        let upright = self.upright(row);
        let (_, bounds) = self.held(row);
        let limit = self.grid.row(row).len().max(from) + 1;
        Self::around(from, limit).find(|&b| !bounds.contains(&b) && Self::reaches(&upright, from, b))
    }

    /// The first cell on `row` nearest `from` where `count` cells can be
    /// deleted: [`Sheet::insertion`] the other way.
    fn deletion(&self, row: usize, from: usize, count: usize) -> Option<usize> {
        let upright = self.upright(row);
        let (cells, _) = self.held(row);
        let limit = self.grid.row(row).len().max(from) + 1;
        let at = |c: usize| Pos::new(row, c);
        Self::around(from, limit).find(|&d| {
            if !Self::reaches(&upright, from, d) {
                return false;
            }
            let left = d.checked_sub(1).map(at);
            let want = self.filler(left, Dir::E, Some(at(d + count)));
            (d..d + count).all(|c| {
                !cells.contains(&c) && !upright.contains(&c) && self.grid.get(at(c)) == want
            })
        })
    }

    fn cut_columns(&self, cut: &Cut) -> Vec<Vec<char>> {
        let mut rows = self.rows();
        for (k, &c) in cut.cols.iter().enumerate() {
            let row = cut.row + k;
            let line = &mut rows[row];
            if line.len() < c {
                line.resize(c, ' ');
            }
            if cut.delete {
                let end = (c + cut.count).min(line.len());
                line.drain(c..end);
            } else {
                let left = c.checked_sub(1).map(|c| Pos::new(row, c));
                let fill = self.filler(left, Dir::E, Some(Pos::new(row, c)));
                line.splice(c..c, std::iter::repeat_n(fill, cut.count));
            }
        }
        rows
    }
}
