//! Drawing boxes, ports and arms (PLAN P10, "Drawing boxes"): one edit per
//! Box-tool gesture, so a build can replay any corpus program's geometry.
//!
//! Each edit draws on the canonical sheet and is checked by structure, not
//! by codes: the result is canonical, and its structure is the input's with
//! exactly the parts the edit names added or removed. A drawing in progress
//! has unwired arrowheads and stray constants, so its syntax codes are free
//! to change; its shape codes are not.

use shawline_doc::Doc;
use shawline_grid::{Dir, Grid, Pos, Rect};
use shawline_shape::structure::{Across, BoxPart, End, Mapping, Part, PortPart, Routing, Structure, TextPart};
use shawline_shape::{BoxStyle, Diagram, PortKind, Text, TextKind};

use crate::canon::{Sheets, tidy};
use crate::cut::render;
use crate::mapped::{self, Read};
use crate::model::{Model, NIL};
use crate::retext::token;
use crate::{Blockage, BoxKind, Edited, Refusal, RefusalKind, Remedy};

/// Whether `word` is one Shavian word: Shavian letters only, so no namer
/// dot and no digits.
pub(crate) fn is_word(word: &str) -> bool {
    !word.is_empty() && word.chars().all(|c| matches!(c, '\u{10450}'..='\u{1047F}'))
}

/// `content` read as one constant: a number, a `«»` text or `𐑯𐑦𐑤`.
pub(crate) fn constant(content: &str) -> Option<Text> {
    token(content).filter(|t| {
        matches!(t.kind, TextKind::Number | TextKind::Literal) || (t.kind == TextKind::Word && t.content == NIL)
    })
}

/// The sheet's rows, to draw on.
pub(crate) fn rows(grid: &Grid) -> Vec<Vec<char>> {
    (0..grid.height()).map(|r| grid.row(r).to_vec()).collect()
}

pub(crate) fn put(rows: &mut Vec<Vec<char>>, p: Pos, ch: char) {
    if rows.len() <= p.row {
        rows.resize(p.row + 1, Vec::new());
    }
    let row = &mut rows[p.row];
    if row.len() <= p.col {
        row.resize(p.col + 1, ' ');
    }
    row[p.col] = ch;
}

/// Writes `line` from cell `at` eastward.
pub(crate) fn write(rows: &mut Vec<Vec<char>>, at: Pos, line: &str) {
    for (k, ch) in line.chars().enumerate() {
        put(rows, Pos::new(at.row, at.col + k), ch);
    }
}

/// A box's top border or divider: `left`, a rule with `title` in a gap
/// after one rule cell (`─ title ─…`), and `right`, `width` cells in all.
pub(crate) fn rule(left: char, line: char, title: Option<&str>, right: char, width: usize) -> String {
    let mut out = String::from(left);
    if let Some(title) = title {
        out.push(line);
        out.push(' ');
        out.push_str(title);
        out.push(' ');
    }
    while out.chars().count() < width - 1 {
        out.push(line);
    }
    out.push(right);
    out
}

/// Checks a drawing edit's result: canonical once tidied, and the input's
/// structure, every part carried through `map`, with what `change` adds or
/// removes, at `routing`. `change` returns `None` when the result lacks
/// what the edit drew.
pub(crate) fn settle(
    sheets: &Sheets,
    text: String,
    at: Pos,
    what: &str,
    routing: Routing,
    map: impl FnOnce(Read, Read) -> Mapping,
    change: impl FnOnce(&mut Structure, &Mapping, Read) -> Option<()>,
) -> Result<Edited, Refusal> {
    let refuse = |why: &str| Refusal::new(at, RefusalKind::Changed, format!("{what}: {why}"));
    let doc = tidy(Doc::new(text)).map_err(refuse)?;
    let old = sheets.canon();
    let new = mapped::read(&doc);
    let map = map(old, new);
    let mut expected = Structure::of(old.0, old.1).relabel(&map);
    change(&mut expected, &map, new).ok_or_else(|| refuse("it would join what is beside it; try another spot"))?;
    expected.expect(&Structure::of(new.0, new.1), routing).map_err(|part| {
        refuse(match part {
            Part::Boxes => "it would disturb a box; try another spot",
            Part::Texts => "it would disturb a text; try another spot",
            Part::Wires => "it would change what a wire connects; try another spot",
            Part::Codes => "it would break the drawing around it; try another spot",
        })
    })?;
    Ok(Edited { doc, map, tidied: sheets.tidied() })
}

/// The map of a step that moved every old cell `p` to `cell(p)`.
pub(crate) fn by_cell(cell: impl Fn(Pos) -> Option<Pos>) -> impl FnOnce(Read, Read) -> Mapping {
    move |old, new| mapped::map(old, new, cell)
}

/// Inserts `part` at index `i` of `parts`, if `i` is a place in it.
fn insert_at<T>(parts: &mut Vec<T>, i: usize, part: T) -> Option<()> {
    (i <= parts.len()).then(|| parts.insert(i, part))
}

/// Rewrites every wire term at box `b`'s ports through `renumber`, which
/// gives each old port index its new one.
pub(crate) fn renumber(expected: &mut Structure, b: usize, renumber: impl Fn(usize) -> usize) {
    for w in &mut expected.wires {
        for t in &mut w.terms {
            if let End::Port(box_, k) = t.end
                && box_ == b
            {
                t.end = End::Port(b, renumber(k));
            }
        }
        w.terms.sort();
    }
    expected.wires.sort();
}

// ---------------------------------------------------------------------------
// Definitions.

/// The rows of each top-level item, top to bottom: a top-level box with
/// everything on its rows, or a run of rows holding anything outside every
/// top-level box's rows.
pub(crate) fn items(grid: &Grid, diagram: &Diagram) -> Vec<(usize, usize)> {
    let tops: Vec<(usize, usize)> =
        diagram.boxes.iter().filter(|b| b.parent.is_none()).map(|b| (b.rect.top, b.rect.bottom)).collect();
    let mut spans = tops.clone();
    for row in 0..grid.height() {
        let inked = grid.row(row).iter().any(|&c| c != ' ');
        if inked && !tops.iter().any(|&(first, last)| (first..=last).contains(&row)) {
            spans.push((row, row));
        }
    }
    spans.sort_unstable();
    let mut items: Vec<(usize, usize)> = Vec::new();
    for (first, last) in spans {
        match items.last_mut() {
            Some(item) if first <= item.1 + 1 => item.1 = item.1.max(last),
            _ => items.push((first, last)),
        }
    }
    items
}

pub(crate) fn place_definition(doc: &Doc, rect: Rect, name: &str) -> Result<Edited, Refusal> {
    let at = Pos::new(rect.top, rect.left);
    let refuse = |kind, why: &str| Refusal::new(at, kind, why);
    if !is_word(name) {
        let why = "a definition's name is one Shavian word, written without its namer dot";
        return Err(refuse(RefusalKind::Target, why));
    }
    let n = name.chars().count();
    if rect.bottom < rect.top + 2 || rect.right < rect.left + n + 6 {
        let why = "the box is too small for its name and a row inside; draw it larger";
        return Err(refuse(RefusalKind::Target, why));
    }
    let sheets = Sheets::read(doc, at)?;
    if sheets.tidied() {
        let why = "format the sheet first, then draw a definition";
        return Err(Refusal { remedy: Some(Remedy::Format), ..refuse(RefusalKind::Unreadable, why) });
    }
    let (grid, diagram) = sheets.canon();
    let items = items(grid, diagram);
    if items.iter().any(|&(first, last)| (first..=last).contains(&rect.top)) {
        let why = "definitions are stacked a blank line apart, never side by side; start it on a blank row";
        return Err(refuse(RefusalKind::Blocked(Blockage::Spacing), why));
    }
    // One blank line below what ends above it, and one above what follows.
    let top = items.iter().filter(|&&(_, last)| last < rect.top).map(|&(_, last)| last + 2).max().unwrap_or(0);
    let height = rect.bottom - rect.top;
    let below = items.iter().map(|&(first, _)| first).filter(|&first| first > rect.top).min();
    let shift = below.map_or(0, |first| (top + height + 2).saturating_sub(first));
    let mut lines = rows(grid);
    if let Some(first) = below {
        for _ in 0..shift {
            lines.insert(first.min(lines.len()), Vec::new());
        }
    }
    let new_rect = Rect { top, left: rect.left, bottom: top + height, right: rect.right };
    let width = rect.right - rect.left + 1;
    let title = format!("·{name}");
    write(&mut lines, Pos::new(top, rect.left), &rule('╭', '─', Some(&title), '╮', width));
    for row in top + 1..new_rect.bottom {
        put(&mut lines, Pos::new(row, rect.left), '│');
        put(&mut lines, Pos::new(row, rect.right), '│');
    }
    write(&mut lines, Pos::new(new_rect.bottom, rect.left), &rule('╰', '─', None, '╯', width));
    let moved = move |p: Pos| Some(if below.is_some_and(|first| p.row >= first) { Pos::new(p.row + shift, p.col) } else { p });
    let what = "the definition can't go there";
    settle(&sheets, render(&lines), at, what, Routing::Kept, by_cell(moved), |expected, _, new| {
        let b = new.1.boxes.iter().position(|b| b.rect == new_rect)?;
        let t = new.1.texts.iter().position(|t| t.pos == Pos::new(top, rect.left + 3))?;
        let part = BoxPart {
            style: BoxStyle::Rounded,
            titled: true,
            slips: 0,
            parent: None,
            ports: Vec::new(),
            dividers: Vec::new(),
        };
        insert_at(&mut expected.boxes, b, part)?;
        let text =
            TextPart { kind: TextKind::Word, content: title, owner: None, title_of: Some(b), divider_of: None };
        insert_at(&mut expected.texts, t, text)
    })
}

// ---------------------------------------------------------------------------
// Switches.

/// The innermost box whose interior holds all of `rect`.
fn holder(diagram: &Diagram, rect: Rect) -> Option<usize> {
    (0..diagram.boxes.len())
        .filter(|&i| diagram.boxes[i].rect.encloses(&rect))
        .min_by_key(|&i| diagram.boxes[i].rect.area())
}

/// Why box `rect` can't go in its holder, if it can't: it sticks out,
/// lands in a node, crosses a divider or overlaps another box.
fn room_for(model: &Model, rect: Rect, what: &str, at: Pos) -> Result<usize, Refusal> {
    let refuse = |kind, why: String| Refusal::new(at, kind, why);
    let diagram = model.diagram;
    let Some(holder) = holder(diagram, rect) else {
        let why = format!("the {what} would stick out of its definition; try Make room or a spot further in");
        return Err(refuse(RefusalKind::Blocked(Blockage::NoRoom), why));
    };
    match model.kind(holder) {
        BoxKind::Node if !model.switch_like(holder) => {
            return Err(refuse(RefusalKind::Target, format!("a {what} can't go inside a node")));
        }
        BoxKind::Definition => {}
        _ => {
            if diagram.boxes[holder].dividers.iter().any(|d| (rect.top..=rect.bottom).contains(&d.row)) {
                let why = format!("the {what} would cross from one arm of the switch into another; try Make room");
                return Err(refuse(RefusalKind::Blocked(Blockage::NoRoom), why));
            }
        }
    }
    let overlaps = diagram.boxes.iter().enumerate().any(|(i, b)| {
        i != holder && b.rect.intersects(&rect) && !b.rect.encloses(&rect)
    });
    if overlaps {
        let why = format!("the {what} would overlap another box; find a clear spot");
        return Err(refuse(RefusalKind::Blocked(Blockage::NoRoom), why));
    }
    Ok(holder)
}

/// Whether every cell of `rect` is blank in `grid`.
fn blank(grid: &Grid, rect: Rect) -> bool {
    (rect.top..=rect.bottom).all(|row| (rect.left..=rect.right).all(|col| grid.get(Pos::new(row, col)) == ' '))
}

pub(crate) fn place_switch(doc: &Doc, rect: Rect, guard: Option<&str>) -> Result<Edited, Refusal> {
    let at = Pos::new(rect.top, rect.left);
    let refuse = |kind, why: &str| Refusal::new(at, kind, why);
    let guarded = match guard {
        Some(g) => Some(constant(g).ok_or_else(|| refuse(RefusalKind::Target, "a guard is a number, a «text» or 𐑯𐑦𐑤"))?),
        None => None,
    };
    let width = guard.map_or(0, |g| g.chars().count() + 3);
    if rect.bottom < rect.top + 2 || rect.right < rect.left + width + 2 {
        let why = "the box is too small for its guard and a row inside; draw it larger";
        return Err(refuse(RefusalKind::Target, why));
    }
    let sheets = Sheets::read(doc, at)?;
    let corner = sheets.place(at).ok_or_else(|| refuse(RefusalKind::Target, "a switch goes inside a definition"))?;
    let (grid, diagram) = sheets.canon();
    let model = Model::new(grid, diagram);
    let rect = Rect {
        top: corner.row,
        left: corner.col,
        bottom: corner.row + (rect.bottom - rect.top),
        right: corner.col + (rect.right - rect.left),
    };
    let holder = room_for(&model, rect, "switch", at)?;
    let shown = Rect { top: at.row, left: at.col, bottom: at.row + rect.bottom - rect.top, right: at.col + rect.right - rect.left };
    if !blank(grid, rect) || !blank(sheets.given().0, shown) {
        return Err(refuse(RefusalKind::Blocked(Blockage::NoRoom), "the switch would land on something; find a clear spot"));
    }
    let mut lines = rows(grid);
    let cells = rect.right - rect.left + 1;
    write(&mut lines, corner, &rule('┌', '─', guard, '┐', cells));
    for row in rect.top + 1..rect.bottom {
        put(&mut lines, Pos::new(row, rect.left), '│');
        put(&mut lines, Pos::new(row, rect.right), '│');
    }
    write(&mut lines, Pos::new(rect.bottom, rect.left), &rule('└', '─', None, '┘', cells));
    let what = "the switch can't go there";
    settle(&sheets, render(&lines), at, what, Routing::Kept, by_cell(Some), |expected, map, new| {
        let b = new.1.boxes.iter().position(|b| b.rect == rect)?;
        let parent = map.boxes[holder];
        let part = BoxPart {
            style: BoxStyle::Square,
            titled: guarded.is_some(),
            slips: 0,
            parent,
            ports: Vec::new(),
            dividers: Vec::new(),
        };
        insert_at(&mut expected.boxes, b, part)?;
        if let Some(g) = guarded {
            let t = new.1.texts.iter().position(|t| t.pos == Pos::new(rect.top, rect.left + 3))?;
            let text = TextPart { kind: g.kind, content: g.content, owner: parent, title_of: Some(b), divider_of: None };
            insert_at(&mut expected.texts, t, text)?;
        }
        Some(())
    })
}

/// The side of `rect`'s border that `p` lies on, other than at a corner.
fn side(rect: Rect, p: Pos) -> Option<Dir> {
    let corner = (p.row == rect.top || p.row == rect.bottom) && (p.col == rect.left || p.col == rect.right);
    if corner || !rect.contains(p) {
        return None;
    }
    if p.row == rect.top {
        Some(Dir::N)
    } else if p.row == rect.bottom {
        Some(Dir::S)
    } else if p.col == rect.left {
        Some(Dir::W)
    } else if p.col == rect.right {
        Some(Dir::E)
    } else {
        None
    }
}

/// The box whose border holds given cell `at`, its side there, and `at`
/// in the canonical sheet.
fn bordered(sheets: &Sheets, at: Pos) -> Option<(usize, Dir, Pos)> {
    let given = sheets.given().1;
    let b = (0..given.boxes.len()).find(|&b| side(given.boxes[b].rect, at).is_some())?;
    let cell = sheets.place(at)?;
    Some((b, side(sheets.canon().1.boxes[b].rect, cell)?, cell))
}

/// Whether border cell `cell` of box `b` on side `side` is plain border:
/// a straight stroke, not a port, a title or guard and its rules, or a
/// divider's end.
fn plain_border(grid: &Grid, diagram: &Diagram, b: usize, side: Dir, cell: Pos) -> bool {
    let line = if side.is_horizontal() { '│' } else { '─' };
    let shape = &diagram.boxes[b];
    let titled = side == Dir::N
        && shape.title_gap.is_some_and(|(start, end)| (start.saturating_sub(1)..=end).contains(&cell.col));
    grid.get(cell) == line && !titled && !shape.ports.iter().any(|p| p.pos == cell)
}

pub(crate) fn add_port(doc: &Doc, at: Pos, name: Option<&str>) -> Result<Edited, Refusal> {
    let refuse = |kind, why: &str| Refusal::new(at, kind, why);
    let sheets = Sheets::read(doc, at)?;
    let Some((b, side, cell)) = bordered(&sheets, at) else {
        return Err(refuse(RefusalKind::Target, "a port goes on a box's border; pick a cell of one"));
    };
    let (grid, diagram) = sheets.canon();
    let model = Model::new(grid, diagram);
    if !plain_border(grid, diagram, b, side, cell) {
        let why = "this cell can't take a port: it is a corner, a port, a title or the end of a divider";
        return Err(refuse(RefusalKind::Target, why));
    }
    let rect = diagram.boxes[b].rect;
    let kind = model.kind(b);
    if kind == BoxKind::Node && !model.switch_like(b) {
        if name.is_some() {
            return Err(refuse(RefusalKind::Target, "a node's ports take no name"));
        }
        let ins = diagram.boxes[b].ports.iter().filter(|p| p.side == Dir::W).count() as u16;
        let outs = diagram.boxes[b].ports.iter().filter(|p| p.side == Dir::E).count() as u16;
        let corner = Pos::new(sheets.given().1.boxes[b].rect.top, sheets.given().1.boxes[b].rect.left);
        return match side {
            Dir::W => crate::set_ports(doc, corner, ins + 1, outs),
            Dir::E => crate::set_ports(doc, corner, ins, outs + 1),
            _ => Err(refuse(RefusalKind::Target, "a node takes inputs on its left border and outputs on its right")),
        };
    }
    let mut lines = rows(grid);
    // The name's first cell, for a definition's port.
    let mut named: Option<(Pos, &str)> = None;
    let dir = match kind {
        BoxKind::Definition => {
            let Some(name) = name.filter(|n| is_word(n)) else {
                let why = "a definition's port needs a name: one Shavian word";
                return Err(refuse(RefusalKind::Target, why));
            };
            let n = name.chars().count();
            let start = match side {
                Dir::W => Pos::new(cell.row, rect.left + 2),
                Dir::E => Pos::new(cell.row, rect.right + 2),
                _ => {
                    let why = "a definition takes inputs on its left border and outputs on its right";
                    return Err(refuse(RefusalKind::Target, why));
                }
            };
            // The arrowhead's space, the name and the space after it.
            let clear = (start.col - 1..=start.col + n).all(|col| grid.get(Pos::new(cell.row, col)) == ' ');
            let inside = side == Dir::E || start.col + n < rect.right;
            if !clear || !inside {
                let why = "there's no room for the port's name here; try Make room or another row";
                return Err(refuse(RefusalKind::Blocked(Blockage::NoRoom), why));
            }
            write(&mut lines, start, name);
            named = Some((start, name));
            Dir::E
        }
        _ => {
            if name.is_some() {
                return Err(refuse(RefusalKind::Target, "a switch's ports take no name"));
            }
            match side {
                Dir::N if diagram.boxes[b].ports.iter().any(|p| p.side == Dir::N) => {
                    return Err(refuse(RefusalKind::Target, "a switch has one selector; this one has it already"));
                }
                Dir::N => Dir::S,
                Dir::W | Dir::E => Dir::E,
                _ => {
                    let why = "a switch takes its selector on its top border and through-ports on its sides";
                    return Err(refuse(RefusalKind::Target, why));
                }
            }
        }
    };
    put(&mut lines, cell, if dir == Dir::S { '▼' } else { '▶' });
    let what = "the port can't go here";
    settle(&sheets, render(&lines), at, what, Routing::Kept, by_cell(Some), |expected, map, new| {
        let nb = map.boxes[b]?;
        let k = new.1.boxes[nb].ports.iter().position(|p| p.pos == cell)?;
        let meets = match named {
            Some((start, _)) => Across::Text(new.1.texts.iter().position(|t| t.pos == start)?),
            None => Structure::of(new.0, new.1).boxes[nb].ports[k].meets,
        };
        renumber(expected, nb, |j| if j >= k { j + 1 } else { j });
        insert_at(&mut expected.boxes[nb].ports, k, PortPart { side, kind: PortKind::Arrow, dir, meets })?;
        if let Some((start, name)) = named {
            let t = new.1.texts.iter().position(|t| t.pos == start)?;
            let owner = (side == Dir::W).then_some(nb);
            let text =
                TextPart { kind: TextKind::Word, content: name.to_owned(), owner, title_of: None, divider_of: None };
            insert_at(&mut expected.texts, t, text)?;
        }
        Some(())
    })
}

// ---------------------------------------------------------------------------
// Arms.

pub(crate) fn add_arm(doc: &Doc, at: Pos, guard: Option<&str>) -> Result<Edited, Refusal> {
    let refuse = |kind, why: &str| Refusal::new(at, kind, why);
    let guarded = match guard {
        Some(g) => Some(constant(g).ok_or_else(|| refuse(RefusalKind::Target, "a guard is a number, a «text» or 𐑯𐑦𐑤"))?),
        None => None,
    };
    let sheets = Sheets::read(doc, at)?;
    let not = || refuse(RefusalKind::Target, "an arm starts at a cell of a switch's left border");
    let (b, side, cell) = bordered(&sheets, at).ok_or_else(not)?;
    let (grid, diagram) = sheets.canon();
    let model = Model::new(grid, diagram);
    if side != Dir::W || !model.switch_like(b) || !plain_border(grid, diagram, b, side, cell) {
        return Err(not());
    }
    let shape = &diagram.boxes[b];
    let rect = shape.rect;
    let no_room = |why: &str| Err(refuse(RefusalKind::Blocked(Blockage::NoRoom), why));
    let right = Pos::new(cell.row, rect.right);
    let inside_blank = (rect.left + 1..rect.right).all(|col| grid.get(Pos::new(cell.row, col)) == ' ');
    if !inside_blank || !plain_border(grid, diagram, b, Dir::E, right) {
        return no_room("the row isn't blank across the switch; try Make room or another row");
    }
    let above = shape.dividers.iter().map(|d| d.row).filter(|&d| d < cell.row).max().unwrap_or(rect.top);
    let below = shape.dividers.iter().map(|d| d.row).filter(|&d| d > cell.row).min().unwrap_or(rect.bottom);
    if cell.row < above + 2 || below < cell.row + 2 {
        return no_room("each arm needs a row of its own; try Make room");
    }
    let width = rect.right - rect.left + 1;
    if guard.is_some_and(|g| width < g.chars().count() + 6) {
        return no_room("the switch is too narrow for this guard; try Make room");
    }
    let mut lines = rows(grid);
    write(&mut lines, cell, &rule('├', '┄', guard, '┤', width));
    let i = shape.dividers.iter().filter(|d| d.row < cell.row).count();
    let what = "the arm can't go here";
    settle(&sheets, render(&lines), at, what, Routing::Kept, by_cell(Some), |expected, map, new| {
        let nb = map.boxes[b]?;
        insert_at(&mut expected.boxes[nb].dividers, i, guarded.is_some())?;
        if let Some(g) = guarded {
            let t = new.1.texts.iter().position(|t| t.pos == Pos::new(cell.row, rect.left + 3))?;
            let text = TextPart { kind: g.kind, content: g.content, owner: Some(nb), title_of: None, divider_of: Some(nb) };
            insert_at(&mut expected.texts, t, text)?;
        }
        Some(())
    })
}
