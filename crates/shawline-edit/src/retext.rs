//! Text edits (PLAN P10, "Text edits" and "Placing text"): replacing a
//! text's content, writing a new one, and removing one.
//!
//! A constant grows and shrinks at its left end, so the wire it feeds stays
//! put. Every other text grows and shrinks at its right end; a stroke that
//! meets it there (an input's wire, a title's rule) gives way or stretches,
//! so the gap stays one space. A text keeps its role, so recognition must
//! read the result as the same structure with that one text's content
//! changed.

use shawline_doc::Doc;
use shawline_grid::{Dir, Grid, Pos};
use shawline_shape::stroke::Kind;
use shawline_shape::structure::{self, Across, Mapping, Routing, Structure, TextPart};
use shawline_shape::{Diagram, PortKind, Text, TextKind, recognize};

use crate::canon::Sheets;
use crate::cut::render;
use crate::mapped::{self, Read};
use crate::model::{Model, NIL};
use crate::sheet::across;
use crate::{BoxKind, Blockage, Edited, Refusal, RefusalKind, Remedy};

/// What a text is to the sheet, which decides how it may change.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Role {
    Constant,
    /// A definition input's name, which its wire starts after.
    Input,
    /// A definition output's name, outside its box.
    Output,
    Title,
    Guard,
    Comment,
    /// A node's word, or an inline node's.
    Call,
    /// Anything else: stray text.
    Other,
}

impl Role {
    fn of(model: &Model, t: usize) -> Role {
        let d = model.diagram;
        let text = &d.texts[t];
        if text.kind == TextKind::Comment {
            return Role::Comment;
        }
        if let Some(b) = text.title_of {
            return if model.kind(b) == BoxKind::Definition { Role::Title } else { Role::Guard };
        }
        if text.divider_of.is_some() {
            return Role::Guard;
        }
        if model.constant(t).is_some() {
            return Role::Constant;
        }
        if model.input_named(t).is_some() {
            return Role::Input;
        }
        if text.owner.is_some_and(|b| model.kind(b) == BoxKind::Node) || model.after_free_arrow(t) {
            return Role::Call;
        }
        if output_named(model, t) {
            return Role::Output;
        }
        Role::Other
    }

    /// What it is called in a refusal.
    fn name(self) -> &'static str {
        match self {
            Role::Constant => "a constant",
            Role::Input | Role::Output => "a port's name",
            Role::Title => "a definition's name",
            Role::Guard => "a guard",
            Role::Comment => "a comment",
            Role::Call => "a node's word",
            Role::Other => "this text",
        }
    }

    /// Whether `kind` and `content` may stand in this role.
    fn takes(self, kind: TextKind, content: &str, old: TextKind) -> bool {
        let value = matches!(kind, TextKind::Number | TextKind::Literal) || (kind == TextKind::Word && content == NIL);
        let name = kind == TextKind::Word && !content.contains('·');
        match self {
            Role::Constant | Role::Guard => value,
            Role::Input | Role::Output => name,
            Role::Title => {
                kind == TextKind::Word && content.strip_prefix('·').is_some_and(|rest| !rest.is_empty() && !rest.contains('·'))
            }
            Role::Comment => kind == TextKind::Comment,
            Role::Call | Role::Other => kind == old,
        }
    }
}

/// Whether text `t` names a definition's output: an arrowhead on the
/// definition's right border points at it across spaces.
fn output_named(model: &Model, t: usize) -> bool {
    let start = model.diagram.texts[t].pos;
    let mut q = start;
    while let Some(p) = q.step(Dir::W) {
        if model.grid.get(p) != ' ' {
            return model.port(p).is_some_and(|(b, port)| {
                model.kind(b) == BoxKind::Definition && port.side == Dir::E && port.kind == PortKind::Arrow
            });
        }
        q = p;
    }
    false
}

/// One text, written on its own: its kind, content and width.
pub(crate) fn token(content: &str) -> Option<Text> {
    if content.is_empty() || content.contains(['\n', '\r']) || content.trim() != content {
        return None;
    }
    let grid = Grid::parse(content).ok()?;
    let diagram = recognize(&grid);
    let [text] = &diagram.texts[..] else { return None };
    let whole = text.pos.col == 0 && text.width == content.chars().count();
    (whole && diagram.diagnostics.is_empty() && diagram.wires.is_empty()).then(|| text.clone())
}

/// The sheet's rows, to rewrite.
fn rows(grid: &Grid) -> Vec<Vec<char>> {
    (0..grid.height()).map(|r| grid.row(r).to_vec()).collect()
}

fn put(rows: &mut Vec<Vec<char>>, p: Pos, ch: char) {
    if rows.len() <= p.row {
        rows.resize(p.row + 1, Vec::new());
    }
    let row = &mut rows[p.row];
    if row.len() <= p.col {
        row.resize(p.col + 1, ' ');
    }
    row[p.col] = ch;
}

/// Whether the cell beside a text, in direction `dir` from it, lets it
/// stand there: blank, or a stroke that doesn't reach toward it.
fn clear_beside(grid: &Grid, diagram: &Diagram, model: &Model, p: Option<Pos>, dir: Dir) -> bool {
    let Some(p) = p else { return true };
    if grid.get(p) == ' ' {
        return true;
    }
    if model.text_at(p).is_some() {
        return false;
    }
    let toward = dir.opposite();
    diagram.strokes.get(p).is_none_or(|s| !s.connects(toward) && s.kind != Kind::Arrow(toward))
}

pub(crate) fn set_text(doc: &Doc, at: Pos, content: &str) -> Result<Edited, Refusal> {
    let refuse = |kind, why: &str| Refusal::new(at, kind, why);
    let sheets = Sheets::read(doc, at)?;
    if let Some(named) = name_port(&sheets, at, content) {
        return named;
    }
    let none = || refuse(RefusalKind::Target, "there is no text here; place one with the palette or type it");
    let cell = sheets.entity(at).ok_or_else(none)?;
    let (grid, diagram) = sheets.canon();
    let model = Model::new(grid, diagram);
    let t = model.text_at(cell).ok_or_else(none)?;
    let old = &diagram.texts[t];
    let role = Role::of(&model, t);
    if role == Role::Call {
        if !crate::drawing::is_word(content) {
            return Err(refuse(RefusalKind::Target, "a node's word is one Shavian word"));
        }
        // A node box is redrawn for its new word; an inline node's word
        // grows or shrinks at its right end, as a name does.
        if let Some(b) = old.owner.filter(|&b| model.kind(b) == BoxKind::Node && !model.inline_word(t)) {
            return crate::clearing::rename(&sheets, &model, b, content, at);
        }
    }
    let Some(new) = token(content).filter(|new| role.takes(new.kind, &new.content, old.kind)) else {
        let why = match role {
            Role::Constant | Role::Guard => "a constant is a number, a «text» or 𐑯𐑦𐑤".to_owned(),
            Role::Input | Role::Output => "a port's name is one Shavian word".to_owned(),
            Role::Title => "a definition's name is one Shavian word after a namer dot (·)".to_owned(),
            Role::Comment => "a comment starts with ※ and holds no strokes".to_owned(),
            Role::Call | Role::Other => format!("{} stays the kind of text it is", role.name()),
        };
        return Err(refuse(RefusalKind::Target, &why));
    };
    let (row, start, end) = (old.pos.row, old.pos.col, old.pos.col + old.width - 1);
    let width = new.width;
    let at_row = |c: usize| Pos::new(row, c);
    let mut lines = rows(grid);
    let no_room = |what: &str| {
        let why = format!("there's no room for {what} here; try Make room or move what's beside it");
        Refusal::new(at, RefusalKind::Blocked(Blockage::NoRoom), why)
    };
    let new_start = if role == Role::Constant {
        // The right end stays, by the wire.
        let Some(first) = (end + 1).checked_sub(width) else { return Err(no_room("the longer text")) };
        if first < start {
            let blank = (first..start).all(|c| grid.get(at_row(c)) == ' ');
            let beside = first.checked_sub(1).map(at_row);
            if !blank || !clear_beside(grid, diagram, &model, beside, Dir::W) {
                return Err(no_room("the longer text"));
            }
        }
        for c in start.min(first)..=end {
            put(&mut lines, at_row(c), ' ');
        }
        first
    } else {
        // The left end stays; what meets it on the right gives way.
        let len = grid.row(row).len();
        let mut q = end + 1;
        while q < len && grid.get(at_row(q)) == ' ' {
            q += 1;
        }
        let follower = (q < len)
            .then(|| diagram.strokes.get(at_row(q)))
            .flatten()
            .filter(|s| s.connects(Dir::W))
            .map(|s| if s.kind == Kind::Dashed { '┄' } else { '─' });
        let new_end = start + width - 1;
        match follower {
            Some(line) => {
                // The stroke starts one space after the text, wherever it ends.
                let from = new_end + 2;
                let plain = |c: usize| {
                    diagram.strokes.get(at_row(c)).is_some_and(|s| {
                        s.conn == shawline_shape::stroke::E | shawline_shape::stroke::W
                            && matches!(s.kind, Kind::Line | Kind::Dashed)
                    })
                };
                if from > q && !(q..=from).all(plain) {
                    return Err(no_room("the longer text"));
                }
                for c in start..from.max(q) {
                    put(&mut lines, at_row(c), ' ');
                }
                for c in new_end + 2..q {
                    put(&mut lines, at_row(c), line);
                }
            }
            None => {
                if new_end > end {
                    let blank = (end + 1..=new_end).all(|c| grid.get(at_row(c)) == ' ');
                    if !blank || !clear_beside(grid, diagram, &model, Some(at_row(new_end + 1)), Dir::E) {
                        return Err(no_room("the longer text"));
                    }
                }
                for c in start..=end {
                    put(&mut lines, at_row(c), ' ');
                }
            }
        }
        start
    };
    for (k, ch) in content.chars().enumerate() {
        put(&mut lines, at_row(new_start + k), ch);
    }
    let result = Doc::new(render(&lines));
    let moved = |p: Pos| if p == old.pos { Pos::new(row, new_start) } else { p };
    let edited = settle(&sheets, result, |p| Some(moved(p)), |expected, map| {
        if let Some(j) = map.texts[t] {
            expected.texts[j].content = new.content.clone();
            expected.texts[j].kind = new.kind;
        }
    })
    .map_err(|why| Refusal::new(at, RefusalKind::Changed, why))?;
    Ok(edited)
}

/// Writes `content` as the name of the definition port whose arrowhead is
/// at given cell `at`, when that port has no name (`Y009`), where
/// [`crate::add_port`] writes a name: one space after the arrowhead, inside
/// for an input and outside for an output. `None` when `at` is no such
/// port, for the other text edits to try.
fn name_port(sheets: &Sheets, at: Pos, content: &str) -> Option<Result<Edited, Refusal>> {
    let (b, k) = structure::port_index(&sheets.given().1.boxes, at)?;
    let (grid, diagram) = sheets.canon();
    let model = Model::new(grid, diagram);
    let port = diagram.boxes[b].ports[k];
    let named = across(grid, &model.texts, port.pos, Dir::E).1.is_some();
    if model.kind(b) != BoxKind::Definition || port.kind != PortKind::Arrow || !port.side.is_horizontal() || named {
        return None;
    }
    let refuse = |kind, why: &str| Refusal::new(at, kind, why);
    if !crate::drawing::is_word(content) {
        return Some(Err(refuse(RefusalKind::Target, "a port's name is one Shavian word")));
    }
    let rect = diagram.boxes[b].rect;
    let row = port.pos.row;
    let start = if port.side == Dir::W { rect.left + 2 } else { rect.right + 2 };
    let n = content.chars().count();
    // The arrowhead's space, the name and the space after it.
    let clear = (start - 1..=start + n).all(|col| grid.get(Pos::new(row, col)) == ' ');
    let inside = port.side == Dir::E || start + n < rect.right;
    if !clear || !inside {
        let why = "there's no room for the port's name here; try Make room or move what's beside it";
        return Some(Err(refuse(RefusalKind::Blocked(Blockage::NoRoom), why)));
    }
    let mut lines = rows(grid);
    for (j, ch) in content.chars().enumerate() {
        put(&mut lines, Pos::new(row, start + j), ch);
    }
    let first = Pos::new(row, start);
    let index = diagram.texts.iter().filter(|t| t.pos < first).count();
    let owner = (port.side == Dir::W).then_some(b);
    let part = TextPart { kind: TextKind::Word, content: content.to_owned(), owner, title_of: None, divider_of: None };
    let result = Doc::new(render(&lines));
    let settled = settle(sheets, result, Some, |expected, map| {
        let nb = map.boxes[b].expect("a box keeps its place when a name is written");
        expected.boxes[nb].ports[k].meets = Across::Text(index);
        expected.texts.insert(index.min(expected.texts.len()), TextPart { owner: owner.map(|_| nb), ..part });
    });
    Some(settled.map_err(|why| refuse(RefusalKind::Changed, &why)))
}

/// Checks a text edit's result: canonical, and the structure `change` says,
/// routing kept. Gives the edit mapped through `cell`.
fn settle(
    sheets: &Sheets,
    result: Doc,
    cell: impl Fn(Pos) -> Option<Pos>,
    change: impl FnOnce(&mut Structure, &Mapping),
) -> Result<Edited, String> {
    if result.formatted() != Some(result.text()) {
        return Err("the sheet wouldn't stay tidy with that text; try another place or content".to_owned());
    }
    let old: Read = sheets.canon();
    let new = mapped::read(&result);
    let map = mapped::map(old, new, cell);
    let mut expected = Structure::of(old.0, old.1).relabel(&map);
    change(&mut expected, &map);
    expected
        .expect(&Structure::of(new.0, new.1), Routing::Kept)
        .map_err(|_| "it would change what the sheet means; try another place or content".to_owned())?;
    Ok(Edited { doc: result, map, tidied: sheets.tidied() })
}

pub(crate) fn place_text(doc: &Doc, at: Pos, content: &str) -> Result<Edited, Refusal> {
    let refuse = |kind, why: &str| Refusal::new(at, kind, why);
    let Some(new) = token(content) else {
        let why = "write one number, «text», word or ※ comment, without strokes";
        return Err(refuse(RefusalKind::Target, why));
    };
    let sheets = Sheets::read(doc, at)?;
    let (grid, diagram) = sheets.canon();
    let p = match sheets.place(at) {
        Some(p) => p,
        None if !sheets.tidied() => at,
        None => {
            let why = "format the sheet first, then place text outside the definitions";
            return Err(Refusal { remedy: Some(Remedy::Format), ..refuse(RefusalKind::Unreadable, why) });
        }
    };
    let model = Model::new(grid, diagram);
    let cells: Vec<Pos> = (0..new.width).map(|k| Pos::new(p.row, p.col + k)).collect();
    if cells.iter().any(|&c| grid.get(c) != ' ') || model.borders.iter().any(|b| cells.contains(b)) {
        return Err(refuse(RefusalKind::Blocked(Blockage::NoRoom), "the text would land on something; find a clear spot"));
    }
    let owner = (0..diagram.boxes.len())
        .filter(|&b| diagram.boxes[b].rect.interior_contains(p))
        .min_by_key(|&b| diagram.boxes[b].rect.area());
    if owner.is_some_and(|b| model.kind(b) == BoxKind::Node) {
        return Err(refuse(RefusalKind::Target, "a node holds only its word; place text beside it"));
    }
    if let Some(b) = owner
        && !cells.iter().all(|&c| diagram.boxes[b].rect.interior_contains(c))
    {
        return Err(refuse(RefusalKind::Blocked(Blockage::NoRoom), "the text would run into a border; make room first"));
    }
    // Across spaces on its row, nothing may meet it: a new text is loose
    // until a connect wires it.
    let len = grid.row(p.row).len();
    let last = p.col + new.width - 1;
    let reach = |from: Pos, dir: Dir| -> Option<Pos> {
        let mut q = from;
        while let Some(n) = q.step(dir).filter(|n| n.col < len.max(1)) {
            if grid.get(n) != ' ' {
                return Some(n);
            }
            q = n;
        }
        None
    };
    for (from, dir) in [(p, Dir::W), (Pos::new(p.row, last), Dir::E)] {
        let Some(q) = reach(from, dir) else { continue };
        let touching = q.col.abs_diff(from.col) == 1;
        // A box's border meets a text only with an arrowhead pointing at
        // it, as a port meets its name. Any other border stroke, a node's
        // input arrowhead or output tee say, reads nothing across the gap.
        let meets = diagram.strokes.get(q).is_some_and(|s| {
            s.kind == Kind::Arrow(dir.opposite()) || (!model.borders.contains(&q) && s.connects(dir.opposite()))
        });
        let comment_swallows = new.kind == TextKind::Comment && dir == Dir::E && model.text_at(q).is_some();
        if meets || comment_swallows || (touching && model.text_at(q).is_some()) {
            let why = if meets {
                "the text would meet a wire or an arrowhead; place it clear of them and connect it after"
            } else {
                "the text would run into other text; place it clear of it"
            };
            return Err(refuse(RefusalKind::Blocked(Blockage::Spacing), why));
        }
    }
    if owner.is_none() && new.kind != TextKind::Comment {
        return Err(refuse(RefusalKind::Target, "outside the definitions, only ※ comments go"));
    }
    let mut lines = rows(grid);
    for (k, ch) in content.chars().enumerate() {
        put(&mut lines, Pos::new(p.row, p.col + k), ch);
    }
    let result = Doc::new(render(&lines));
    let index = diagram.texts.iter().filter(|t| t.pos < p).count();
    let part = TextPart { kind: new.kind, content: new.content.clone(), owner, title_of: None, divider_of: None };
    settle(&sheets, result, Some, |expected, map| {
        let mut part = part;
        part.owner = part.owner.and_then(|b| map.boxes[b]);
        expected.texts.insert(index.min(expected.texts.len()), part);
    })
    .map_err(|why| {
        let kind = RefusalKind::Blocked(Blockage::Spacing);
        refuse(kind, &why)
    })
}

pub(crate) fn remove_text(doc: &Doc, at: Pos) -> Result<Edited, Refusal> {
    let refuse = |kind, why: &str| Refusal::new(at, kind, why);
    let sheets = Sheets::read(doc, at)?;
    let none = || refuse(RefusalKind::Target, "there is no text here");
    let cell = sheets.entity(at).ok_or_else(none)?;
    let (grid, diagram) = sheets.canon();
    let model = Model::new(grid, diagram);
    let t = model.text_at(cell).ok_or_else(none)?;
    let role = Role::of(&model, t);
    match role {
        Role::Input | Role::Output | Role::Title | Role::Guard => {
            let why = format!("{} stays; edit it instead of removing it", role.name());
            return Err(refuse(RefusalKind::Target, &why));
        }
        Role::Call => {
            let why = "a node's word goes with its node; delete the node instead";
            return Err(refuse(RefusalKind::Target, why));
        }
        Role::Constant | Role::Comment | Role::Other => {}
    }
    let text = diagram.texts[t].clone();
    // A constant goes with its wire, as a node does with the wires it feeds.
    let wired = (role == Role::Constant)
        .then(|| model.wire_of(crate::model::Term::Text(text.pos, Dir::W)))
        .flatten();
    let (first, base) = match wired {
        Some(w) => {
            let cut = crate::wiring::remove_wire(sheets.doc(), diagram.wires[w].cells[0])
                .map_err(|r| Refusal { at, ..r })?;
            let edited = mapped::kept(sheets.doc(), cut);
            (Some(edited.map), edited.doc)
        }
        None => (None, Doc::new(sheets.doc().text())),
    };
    let (grid, diagram_now) = mapped::read(&base);
    let mut lines = rows(grid);
    for c in text.cells() {
        put(&mut lines, c, ' ');
    }
    let result = crate::canon::tidy(Doc::new(render(&lines)))
        .map_err(|why| refuse(RefusalKind::Changed, &format!("the text can't be removed: {why}")))?;
    // Removing one text keeps every other part in its order, though
    // tidying may close up the blank lines it leaves.
    let now = diagram_now.texts.iter().position(|u| u.pos == text.pos).expect("the text is still there");
    let mut map = Mapping {
        boxes: (0..diagram_now.boxes.len()).map(Some).collect(),
        texts: (0..diagram_now.texts.len()).map(|i| (i != now).then(|| if i < now { i } else { i - 1 })).collect(),
        wires: Vec::new(),
    };
    let new = mapped::read(&result);
    let expected = Structure::of(grid, diagram_now).relabel(&map);
    expected.expect(&Structure::of(new.0, new.1), Routing::Kept).map_err(|_| {
        refuse(RefusalKind::Changed, "removing it would change what the sheet means; edit it in the text pane")
    })?;
    map.wires = mapped::wires_by_terms((grid, diagram_now), new, &map);
    let map = match first {
        Some(first) => mapped::compose(&first, &map),
        None => map,
    };
    Ok(Edited { doc: result, map, tidied: sheets.tidied() })
}
