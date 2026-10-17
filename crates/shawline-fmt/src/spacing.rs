//! SPEC §11 rules 3–4: spacing around titles, guards, port names and the
//! text that sources or sinks a wire. Text only ever moves along its own row.

use std::collections::{HashMap, HashSet};

use shawline_grid::{Dir, Grid, Pos};
use shawline_shape::stroke::{E, Kind, Stroke, W};
use shawline_shape::{BoxShape, Diagram, EndKind, Port, Text, TextKind};

use crate::canvas::Edit;

/// Rule 3: titles and guards read `─ name ─`. One edit per title or guard.
pub(crate) fn titles(grid: &Grid, diagram: &Diagram) -> Vec<Edit> {
    let mut edits = Vec::new();
    for (i, b) in diagram.boxes.iter().enumerate() {
        let r = b.rect;
        if let Some(gap) = b.title_gap {
            let held: Vec<&Text> = diagram.texts.iter().filter(|t| t.title_of == Some(i)).collect();
            // Ports in the top border (a switch's selector) must not move.
            let keep: HashSet<usize> =
                b.ports.iter().filter(|p| p.pos.row == r.top).map(|p| p.pos.col).collect();
            if let [t] = held[..] {
                edits.extend(rule_line(grid, b, r.top, gap, t, '─', &keep));
            }
        }
        for d in &b.dividers {
            let Some(gap) = d.gap else { continue };
            let held: Vec<&Text> = diagram
                .texts
                .iter()
                .filter(|t| t.divider_of == Some(i) && t.pos.row == d.row)
                .collect();
            if let [t] = held[..] {
                edits.extend(rule_line(grid, b, d.row, gap, t, '┄', &HashSet::new()));
            }
        }
    }
    edits
}

/// Rewrites the border or divider on `row` as corner, rule, space, name,
/// space, then rules to the far side, keeping the columns in `keep`.
fn rule_line(
    grid: &Grid,
    b: &BoxShape,
    row: usize,
    (start, end): (usize, usize),
    t: &Text,
    rule: char,
    keep: &HashSet<usize>,
) -> Option<Edit> {
    let (left, right) = (b.rect.left, b.rect.right);
    let at = |c| grid.get(Pos::new(row, c));
    let own = t.pos.col..t.pos.col + t.width;
    if (start..end).any(|c| !own.contains(&c) && at(c) != ' ') {
        return None;
    }
    let name = left + 3;
    let after = name + t.width;
    // Room is needed for at least one rule after the trailing space.
    if after + 1 >= right || keep.iter().any(|&c| c <= after) {
        return None;
    }
    let edit: Edit = (left + 1..right)
        .filter_map(|c| {
            let ch = match c {
                _ if c == left + 1 => rule,
                _ if c == left + 2 || c == after => ' ',
                _ if c >= name && c < after => at(t.pos.col + c - name),
                _ if keep.contains(&c) => at(c),
                _ => rule,
            };
            (ch != at(c)).then_some((Pos::new(row, c), ch))
        })
        .collect();
    (!edit.is_empty()).then_some(edit)
}

/// Rule 4: a port name sits one space after its arrowhead, an inline node's
/// word one space after the arrowhead that feeds it, and a wire starts one
/// space after the word or literal it comes from. One edit per text.
pub(crate) fn ports(grid: &Grid, diagram: &Diagram) -> Vec<Edit> {
    let open_west: HashSet<Pos> = diagram
        .wires
        .iter()
        .flat_map(|w| &w.ends)
        .filter(|e| e.kind == EndKind::Open && e.facing == Dir::W)
        .map(|e| e.pos)
        .collect();
    let ports: HashMap<Pos, (usize, &Port)> = diagram
        .boxes
        .iter()
        .enumerate()
        .flat_map(|(i, b)| b.ports.iter().map(move |p| (p.pos, (i, p))))
        .collect();
    let is_hline =
        |p: Pos| diagram.strokes.get(p) == Some(Stroke { conn: E | W, kind: Kind::Line });

    let mut edits = Vec::new();
    for t in &diagram.texts {
        if t.kind == TextKind::Comment || t.title_of.is_some() || t.divider_of.is_some() {
            continue;
        }
        let row = t.pos.row;
        let cells = grid.row(row);
        let west = (0..t.pos.col).rev().find(|&c| cells[c] != ' ');
        let east = (t.pos.col + t.width..cells.len()).find(|&c| cells[c] != ' ');

        let arrow = west.filter(|&c| {
            matches!(
                diagram.strokes.get(Pos::new(row, c)),
                Some(Stroke { kind: Kind::Arrow(Dir::E), .. })
            )
        });
        let anchor = match arrow.map(|c| (c, ports.get(&Pos::new(row, c)))) {
            None => None,
            Some((c, None)) => Some(c),
            Some((c, Some(&(b, port)))) => {
                let top_level = diagram.boxes[b].parent.is_none();
                match port.side {
                    // A node's word is centred in its box, not spaced from the arrow.
                    Dir::W if t.owner == Some(b) && !top_level => continue,
                    Dir::W if t.owner == Some(b) => Some(c),
                    Dir::E if t.owner != Some(b) => Some(c),
                    _ => None,
                }
            }
        };
        let wire = east.filter(|&c| open_west.contains(&Pos::new(row, c)));
        let plan = Plan {
            cells,
            start: t.pos.col,
            width: t.width,
            west,
            east,
            wire,
            is_hline: &|c| is_hline(Pos::new(row, c)),
        };
        let new_row = match anchor {
            Some(a) => plan.respace(a + 2),
            None => plan.respace(t.pos.col).or_else(|| plan.pull_left()),
        };
        if let Some(new_row) = new_row {
            let edit: Edit = new_row
                .iter()
                .enumerate()
                .filter(|&(c, &ch)| cells.get(c).copied().unwrap_or(' ') != ch)
                .map(|(c, &ch)| (Pos::new(row, c), ch))
                .collect();
            if !edit.is_empty() {
                edits.push(edit);
            }
        }
    }
    edits
}

/// One text on its row, with what lies either side of it.
struct Plan<'a> {
    cells: &'a [char],
    start: usize,
    width: usize,
    /// The nearest non-space cells west and east of the text.
    west: Option<usize>,
    east: Option<usize>,
    /// `east`, when it is a wire's open end facing the text.
    wire: Option<usize>,
    is_hline: &'a dyn Fn(usize) -> bool,
}

impl Plan<'_> {
    /// The row with the text at `to` and its wire, if any, starting one
    /// space after it: lengthened with `─`, or shortened while what remains
    /// still starts with `─`.
    fn respace(&self, to: usize) -> Option<Vec<char>> {
        let mut row = self.cells.to_vec();
        let text: Vec<char> = row[self.start..self.start + self.width].to_vec();
        row[self.start..self.start + self.width].fill(' ');
        let text_end = to + self.width;
        match self.wire {
            Some(e) => {
                let want = text_end + 1;
                if want > e {
                    if !(e..=want).all(|c| (self.is_hline)(c)) {
                        return None;
                    }
                    row[e..want].fill(' ');
                } else {
                    row[want..e].fill('─');
                }
            }
            // Moving right must not run into what follows.
            None if to > self.start && self.east.is_some_and(|x| text_end >= x) => return None,
            None => {}
        }
        if row.len() < text_end {
            row.resize(text_end, ' ');
        }
        if row[to..text_end].iter().any(|&ch| ch != ' ') {
            return None;
        }
        row[to..text_end].copy_from_slice(&text);
        (row != self.cells).then_some(row)
    }

    /// For text with no arrowhead to keep it in place: when its wire can't
    /// be shortened, move the text up to the wire instead, keeping at least
    /// one space west of it.
    fn pull_left(&self) -> Option<Vec<char>> {
        let e = self.wire?;
        let to = e.checked_sub(self.width + 1)?;
        if to >= self.start || self.west.is_some_and(|x| to < x + 2) {
            return None;
        }
        self.respace(to)
    }
}

#[cfg(test)]
mod tests {
    use crate::format;
    use shawline_grid::Grid;

    fn fmt(src: &str) -> String {
        let grid = Grid::parse(src).unwrap();
        format(&grid, &shawline_shape::recognize(&grid))
    }

    #[test]
    fn title_spacing() {
        let src = "╭──   ·𐑨𐑛──────╮\n│ 1 ───────────▶ 𐑚\n╰──────────────╯\n";
        assert_eq!(fmt(src), "╭─ ·𐑨𐑛 ────────╮\n│ 1 ───────────▶ 𐑚\n╰──────────────╯\n");
    }

    #[test]
    fn title_keeps_selector_in_place() {
        let src = "╭─ ·𐑨𐑛 ──────────╮\n│ 1 ─┐           │\n│ ┌───▼──────┐   │\n│ │ 2 ───────▶───▶ 𐑚\n│ └──────────┘   │\n╰────────────────╯\n";
        assert_eq!(fmt(src), src);
        let rough = src.replace("┌───▼", "┌─  ▼");
        assert_eq!(fmt(&rough), rough, "no room for the guard spacing: left alone");
    }

    #[test]
    fn guard_spacing() {
        let src = "╭─ ·𐑨𐑛 ────────────╮\n▶ 𐑯 ─┐             │\n│ ┌──▼─────────┐   │\n│ │ 1 ─────────▶─┐ │\n│ ├┄┄  2┄┄┄┄┄┄┄┤ │ │\n│ │ 3 ─────────▶─┴─▶ 𐑚\n│ └────────────┘   │\n╰──────────────────╯\n";
        let want = src.replace("├┄┄  2┄┄┄┄┄┄┄┤", "├┄ 2 ┄┄┄┄┄┄┄┄┤");
        assert_eq!(fmt(src), want);
    }

    #[test]
    fn port_names_and_wires() {
        let src = "╭─ ·𐑨𐑛 ──────╮\n▶   𐑯    ────▶   𐑚\n╰────────────╯\n";
        assert_eq!(fmt(src), "╭─ ·𐑨𐑛 ──────╮\n▶ 𐑯 ─────────▶ 𐑚\n╰────────────╯\n");
    }

    #[test]
    fn port_name_pushes_into_a_wire() {
        let src = "╭─ ·𐑨𐑛 ──────╮\n▶𐑯───────────▶ 𐑚\n╰────────────╯\n";
        assert_eq!(fmt(src), "╭─ ·𐑨𐑛 ──────╮\n▶ 𐑯 ─────────▶ 𐑚\n╰────────────╯\n");
    }

    #[test]
    fn constants_and_inline_nodes() {
        let src =
            "╭─ ·𐑨𐑛 ─────────────────╮\n│ 5   ──▶𐑖   ──▶  𐑠 ────▶ 𐑚\n╰───────────────────────╯\n";
        let want =
            "╭─ ·𐑨𐑛 ─────────────────╮\n│ 5 ────▶ 𐑖 ───▶ 𐑠 ─────▶ 𐑚\n╰───────────────────────╯\n";
        assert_eq!(fmt(src), want);
    }

    #[test]
    fn constant_moves_when_its_wire_is_too_short() {
        let src = "╭─ ·𐑨𐑛 ────╮\n│  10─▶ 𐑖 ─▶ 𐑚\n╰──────────╯\n";
        assert_eq!(fmt(src), "╭─ ·𐑨𐑛 ────╮\n│ 10 ─▶ 𐑖 ─▶ 𐑚\n╰──────────╯\n");
    }
}
