//! The move gesture: a box and its contents shift, and each wire attached
//! to it from outside is cut back and re-routed.
//!
//! For every port of the moved box with a wire outside it, the wire's
//! **chain** is the run of cells from the port back to the first fan-out
//! tee or the wire's far end. The chain is erased, and the router draws a
//! new stretch from the port's new place back to the wire. Its goals are
//! the chain's own cells, so a route that rejoins the chain early restores
//! the rest of it as the author drew it; only the stretch that must change
//! is redrawn. A route may also join the rest of the wire as a new fan-out
//! tee, at a cost.

use std::collections::{HashMap, HashSet};

use shawline_doc::Doc;
use shawline_grid::{Dir, Grid, Pos, Rect};
use shawline_shape::stroke::{self, Kind};
use shawline_shape::{Diagram, EndKind, PortKind};

use crate::canon::{Sheets, tidy};
use crate::check::{DIAGNOSTICS, Shift, same};
use crate::router::{self, BEND, BUDGET, CLEARANCE, Cell, Found, Miss, Start, TEE, Terrain, toward};
use crate::sheet::{across, borders, glyph, is_corner, keep_clear, perimeter, text_cells};
use crate::{Blockage, Refusal, RefusalKind};

/// Using a cell of another pending wire's chain: it may want it back.
const OTHER_CHAIN: u32 = 2;
/// Ending a constant's wire against its text. Allowed only where the
/// formatter will pull the text back to one space ([`Open::tight`]), and
/// never preferred.
const TIGHT: u32 = 4;

/// A move's result, and the text it drew before tidying: the formatter
/// keeps every box and text in order, so a map runs through `drawn`.
#[derive(Debug)]
pub(crate) struct Moved {
    pub doc: Doc,
    pub drawn: Doc,
}

pub(crate) fn move_box(doc: &Doc, corner: Pos, dr: isize, dc: isize) -> Result<Moved, Refusal> {
    move_within(doc, corner, dr, dc, BUDGET)
}

/// [`move_box`], with `budget` router expansions for the whole move.
pub(crate) fn move_within(
    doc: &Doc,
    corner: Pos,
    dr: isize,
    dc: isize,
    budget: usize,
) -> Result<Moved, Refusal> {
    // The box is found on the text as given, then carried over to the
    // canonical text the move draws on (`crate::canon`). Refusals point at
    // canonical cells, so they're carried back.
    let sheets = Sheets::read(doc, corner)?;
    let Some(index) = sheets
        .given()
        .1
        .boxes
        .iter()
        .position(|b| (b.rect.top, b.rect.left) == (corner.row, corner.col))
    else {
        let why = "there is no box with its top-left corner here";
        return Err(Refusal::new(corner, RefusalKind::Target, why));
    };
    moved(&sheets, index, dr, dc, budget)
        .map_err(|r| Refusal { at: sheets.back(index, r.at).unwrap_or(corner), ..r })
}

fn moved(
    sheets: &Sheets,
    index: usize,
    dr: isize,
    dc: isize,
    budget: usize,
) -> Result<Moved, Refusal> {
    let canon = sheets.doc();
    if dr == 0 && dc == 0 {
        return Ok(Moved { doc: Doc::new(canon.text()), drawn: Doc::new(canon.text()) });
    }
    let (grid, diagram) = sheets.canon();
    let rect = diagram.boxes[index].rect;
    let at = Pos::new(rect.top, rect.left);
    let changed = |why: &str| {
        Refusal::new(at, RefusalKind::Changed, format!("the box can't go there: {why}; try another spot"))
    };

    let drawn = Doc::new(Mover::new(grid, diagram, index, dr, dc)?.run(budget)?);

    // The check (contract module docs): same structure, and the same
    // diagnostics, syntax codes included, which aren't structure.
    let new_grid = drawn.grid().expect("edits write only repertoire characters");
    let new_diagram = drawn.diagram().expect("a sheet on the grid is recognized");
    let shift = shift_of(grid, diagram, index, dr, dc);
    same((grid, diagram), (new_grid, new_diagram), &shift).map_err(changed)?;
    if codes(canon) != codes(&drawn) {
        return Err(changed(DIAGNOSTICS));
    }
    // The formatter tidies what the edit left loose (a constant pulled up to
    // its shortened wire). It keeps the structure, but not necessarily the box.
    let result = tidy(Doc::new(drawn.text())).map_err(changed)?;
    let moved = shift.rect(rect);
    let boxes = &result.diagram().expect("a canonical sheet is on the grid").boxes;
    if !boxes.iter().any(|b| b.rect == moved) {
        let why = "the box wouldn't stay there once the sheet is tidied; try one cell further";
        return Err(Refusal::new(at, RefusalKind::Changed, why));
    }
    Ok(Moved { doc: result, drawn })
}

/// Node box `index` redrawn in place as `cells`, which cover all of
/// `new_rect`, with each port in `ports` taken to its new cell. The wire at
/// a port that moves is re-routed as a move re-routes it; every other wire
/// stays as drawn. Returns the drawing, for the caller to check.
pub(crate) fn reshape(
    grid: &Grid,
    diagram: &Diagram,
    index: usize,
    new_rect: Rect,
    cells: &[(Pos, char)],
    ports: &HashMap<Pos, Pos>,
) -> Result<String, Refusal> {
    Mover::reshaped(grid, diagram, index, new_rect, cells, ports)?.run(BUDGET)
}

/// Non-lint diagnostic codes, sorted: positions move with the box.
fn codes(doc: &Doc) -> Vec<&'static str> {
    let mut codes: Vec<&'static str> =
        doc.diagnostics().iter().map(|d| d.code).filter(|c| !c.starts_with('L')).collect();
    codes.sort_unstable();
    codes
}

/// Indices of `index` and every box inside it.
fn moved_boxes(diagram: &Diagram, index: usize) -> HashSet<usize> {
    let rect = diagram.boxes[index].rect;
    (0..diagram.boxes.len())
        .filter(|&i| i == index || rect.encloses(&diagram.boxes[i].rect))
        .collect()
}

/// Texts outside the box that travel with it: names after its outward
/// arrowheads (a definition's outputs), with the spaces before them.
fn trailing_texts(grid: &Grid, diagram: &Diagram, index: usize) -> Vec<(usize, Vec<Pos>)> {
    let texts = text_cells(diagram);
    let b = &diagram.boxes[index];
    b.ports
        .iter()
        .filter(|p| p.kind == PortKind::Arrow && p.dir == p.side && p.side.is_horizontal())
        .filter_map(|p| {
            let (mut gap, text) = across(grid, &texts, p.pos, p.dir);
            let i = text?;
            let t = &diagram.texts[i];
            gap.extend((t.pos.col..t.pos.col + t.width).map(|c| Pos::new(t.pos.row, c)));
            Some((i, gap))
        })
        .collect()
}

pub(crate) fn shift_of(grid: &Grid, diagram: &Diagram, index: usize, dr: isize, dc: isize) -> Shift {
    Shift {
        dr,
        dc,
        boxes: moved_boxes(diagram, index),
        rect: diagram.boxes[index].rect,
        texts: trailing_texts(grid, diagram, index).into_iter().map(|(i, _)| i).collect(),
    }
}

fn add(p: Pos, dr: isize, dc: isize) -> Option<Pos> {
    Some(Pos::new(p.row.checked_add_signed(dr)?, p.col.checked_add_signed(dc)?))
}

/// The stretch of a wire between one of the moved box's ports and the
/// first point that stays: a fan-out tee, or the wire's far end.
#[derive(Clone, Debug)]
struct Chain {
    /// Erased cells, from the port outward.
    cells: Vec<Pos>,
    /// Each cell's own connections, as drawn.
    orig: Vec<u8>,
    /// The direction from each cell back toward the port.
    back: Vec<Dir>,
    /// The cell that stays, and the direction from it into the chain.
    anchor: Option<(Pos, Dir)>,
    /// When the far end is an open end meeting text (a constant, a port
    /// name), it is erased too and may be redrawn anywhere in this gap
    /// before the text, facing it.
    open: Option<Open>,
}

#[derive(Clone, Debug)]
struct Open {
    /// The way the end faces its text.
    facing: Dir,
    /// The text, to name in a refusal.
    text: String,
    gap: Vec<Pos>,
    /// The end may touch the text: the formatter will pull the text back
    /// one cell (SPEC §11 rule 4), as it does for a constant with room
    /// behind it, but never for text an arrowhead holds in place.
    tight: bool,
}

/// One wire end to reconnect.
#[derive(Clone, Debug)]
struct End {
    wire: usize,
    /// The port's place before and after the move.
    port: Pos,
    new_port: Pos,
    /// The side of the box the port is on: the way out of it.
    side: Dir,
    arrow: bool,
    /// The cell outside the port's new place, where the route starts.
    target: Pos,
    chain: Chain,
}

/// The sheet being edited: the original characters with the box moved,
/// plus an overlay of wire connections for every cell the edit rewires.
#[derive(Clone)]
struct Work {
    base: Vec<Vec<char>>,
    /// Current connections of rewired cells.
    mask: HashMap<Pos, u8>,
    /// Connections of every stroke as drawn, for cells outside the box.
    orig: HashMap<Pos, u8>,
}

impl Work {
    fn base(&self, p: Pos) -> char {
        self.base.get(p.row).and_then(|r| r.get(p.col)).copied().unwrap_or(' ')
    }

    fn ch(&self, p: Pos) -> char {
        match self.mask.get(&p) {
            Some(m) if self.orig.get(&p) == Some(m) => self.base(p),
            Some(&m) => glyph(m),
            None => self.base(p),
        }
    }

    fn mask(&self, p: Pos) -> u8 {
        self.mask.get(&p).or_else(|| self.orig.get(&p)).copied().unwrap_or(0)
    }

    fn set(&mut self, p: Pos, m: u8) {
        self.mask.insert(p, m);
    }

    fn write(&mut self, p: Pos, ch: char) {
        if self.base.len() <= p.row {
            self.base.resize(p.row + 1, Vec::new());
        }
        let row = &mut self.base[p.row];
        if row.len() <= p.col {
            row.resize(p.col + 1, ' ');
        }
        row[p.col] = ch;
        self.mask.remove(&p);
        self.orig.remove(&p);
    }

    fn render(&self) -> String {
        let mut out = String::new();
        for (r, row) in self.base.iter().enumerate() {
            let line: String = (0..row.len()).map(|c| self.ch(Pos::new(r, c))).collect();
            out.push_str(line.trim_end());
            out.push('\n');
        }
        out
    }
}

struct Mover<'a> {
    grid: &'a Grid,
    rect: Rect,
    work: Work,
    ends: Vec<End>,
    /// Where routes may run: the parent's interior, or the arm of a switch.
    region: Rect,
    /// Cells no route may take: other boxes, kept-clear cells, every gap.
    blocked: HashSet<Pos>,
    /// The moved box's new cells.
    footprint: HashSet<Pos>,
    /// Each wire's cells as they stand.
    wires: HashMap<usize, HashSet<Pos>>,
}

impl<'a> Mover<'a> {
    fn new(
        grid: &'a Grid,
        diagram: &'a Diagram,
        index: usize,
        dr: isize,
        dc: isize,
    ) -> Result<Self, Refusal> {
        let b = &diagram.boxes[index];
        let rect = b.rect;
        let at = Pos::new(rect.top, rect.left);
        let refuse = |kind, reason: &str| Refusal::new(at, kind, reason);
        let shifted = |p: Pos| add(p, dr, dc);
        let (Some(tl), Some(br)) =
            (shifted(Pos::new(rect.top, rect.left)), shifted(Pos::new(rect.bottom, rect.right)))
        else {
            let why = "the box is at the edge of the sheet; it can't go further this way";
            return Err(refuse(RefusalKind::Blocked(Blockage::Edge), why));
        };
        let new_rect = Rect { top: tl.row, left: tl.col, bottom: br.row, right: br.col };

        let region = match b.parent {
            None => {
                // Room for the definition past everything drawn, and no
                // more: the sheet grows to hold it before any route is tried.
                let region = Rect {
                    top: 0,
                    left: 0,
                    bottom: grid.height() + rect.bottom + 2,
                    right: grid.width() + rect.right + 2,
                };
                if new_rect.bottom > region.bottom || new_rect.right > region.right {
                    let why = "that's too far past the end of the sheet; move the box in smaller steps";
                    return Err(refuse(RefusalKind::Blocked(Blockage::Edge), why));
                }
                region
            }
            Some(p) => {
                let parent = &diagram.boxes[p];
                let what = if parent.parent.is_none() { "definition" } else { "switch" };
                if !parent.rect.encloses(&new_rect) {
                    // Inserting room pushes a parent's bottom and right
                    // edges away, never its top or left.
                    let r = parent.rect;
                    if new_rect.top <= r.top || new_rect.left <= r.left {
                        let why = format!("the box is at the edge of its {what}; it can't go further this way");
                        return Err(refuse(RefusalKind::Blocked(Blockage::Edge), &why));
                    }
                    let why = format!("the box would leave its {what}; try Make room to grow it");
                    return Err(refuse(RefusalKind::Blocked(Blockage::NoRoom), &why));
                }
                let r = parent.rect;
                let above = parent.dividers.iter().map(|d| d.row).filter(|&d| d < rect.top).max();
                let below =
                    parent.dividers.iter().map(|d| d.row).filter(|&d| d > rect.bottom).min();
                let top = above.unwrap_or(r.top) + 1;
                let bottom = below.unwrap_or(r.bottom) - 1;
                if new_rect.top < top {
                    let why = "the box is at the top of its arm; it can't cross into the arm above";
                    return Err(refuse(RefusalKind::Blocked(Blockage::Edge), why));
                }
                if new_rect.bottom > bottom {
                    let why = "the box would cross into the next arm; try Make room to grow its arm";
                    return Err(refuse(RefusalKind::Blocked(Blockage::NoRoom), why));
                }
                Rect { top, left: r.left + 1, bottom, right: r.right - 1 }
            }
        };

        // The ends to reconnect: wires meeting the box's ports from outside.
        let ports: HashMap<Pos, (Dir, PortKind)> =
            b.ports.iter().map(|p| (p.pos, (p.side, p.kind))).collect();
        let border = borders(diagram);
        let texts = text_cells(diagram);
        let mut ends = Vec::new();
        for (w, wire) in diagram.wires.iter().enumerate() {
            for e in &wire.ends {
                let EndKind::Port(port) = e.kind else { continue };
                let Some(&(side, kind)) = ports.get(&port) else { continue };
                if rect.contains(e.pos) {
                    continue;
                }
                let chain = walk(grid, diagram, &border, &texts, e.pos, e.facing);
                let new_port = shifted(port).expect("on the moved box");
                let Some(target) = new_port.step(side) else {
                    let why = "the wire at this port would run off the sheet; move the box the other way";
                    return Err(Refusal::new(port, RefusalKind::Blocked(Blockage::Edge), why));
                };
                ends.push(End {
                    wire: w,
                    port,
                    new_port,
                    side,
                    arrow: kind == PortKind::Arrow,
                    target,
                    chain,
                });
            }
        }
        ends.sort_by_key(|e| e.port);
        let starts: HashSet<Pos> =
            ends.iter().filter_map(|e| e.chain.cells.first().copied()).collect();
        for e in &ends {
            if let Some((anchor, _)) = e.chain.anchor
                && (starts.contains(&anchor) || ports.keys().any(|&p| e.chain.cells.contains(&p)))
            {
                let why = "this wire loops back into the same box";
                return Err(Refusal::new(e.port, RefusalKind::Unsupported, why));
            }
        }

        let orig: HashMap<Pos, u8> = diagram
            .wires
            .iter()
            .flat_map(|w| &w.cells)
            .filter_map(|&p| diagram.strokes.get(p).map(|s| (p, s.conn)))
            .collect();
        let base = (0..grid.height()).map(|r| grid.row(r).to_vec()).collect();
        let mut work = Work { base, mask: HashMap::new(), orig };

        // Erase every chain, leaving each anchor without its chain side.
        for e in &ends {
            let c = &e.chain;
            for (i, &p) in c.cells.iter().enumerate() {
                let m = work.mask(p) & !c.orig[i];
                work.set(p, m);
            }
            if let Some((anchor, into)) = c.anchor {
                let m = work.mask(anchor) & !into.bit();
                work.set(anchor, m);
            }
        }
        let erased: HashSet<Pos> =
            ends.iter().flat_map(|e| e.chain.cells.iter().copied()).collect();

        // Lift the box (and a definition's output names), then put it down.
        let mut content: Vec<Pos> = (rect.top..=rect.bottom)
            .flat_map(|r| (rect.left..=rect.right).map(move |c| Pos::new(r, c)))
            .collect();
        let trailing = trailing_texts(grid, diagram, index);
        content.extend(trailing.iter().flat_map(|(_, cells)| cells.iter().copied()));
        let lifted: Vec<(Pos, char)> = content.iter().map(|&p| (p, grid.get(p))).collect();
        for &(p, _) in &lifted {
            work.write(p, ' ');
        }
        let mut footprint = HashSet::new();
        for &(p, ch) in &lifted {
            let q = shifted(p).expect("inside the moved rectangle");
            let there = work.ch(q);
            if there != ' ' {
                let what = if texts.contains_key(&q) {
                    "text"
                } else if border.contains(&q) {
                    "another box"
                } else {
                    "a wire"
                };
                // Room is inserted below and to the right of a cell, so it
                // can't clear what lies above or to the left of the box.
                if dr < 0 || dc < 0 {
                    let why = format!("the box would land on {what}; move that out of the way first");
                    return Err(refuse(RefusalKind::Blocked(Blockage::Edge), &why));
                }
                let why = format!("the box would land on {what}; try Make room or another spot");
                return Err(refuse(RefusalKind::Blocked(Blockage::NoRoom), &why));
            }
            footprint.insert(q);
            if ch != ' ' {
                work.write(q, ch);
            }
        }

        // What no route may take.
        let moved_cells: HashSet<Pos> = content.iter().copied().collect();
        let skip: HashSet<(Pos, Dir)> = ends
            .iter()
            .flat_map(|e| {
                let first = e.chain.cells.first().map(|&p| (p, e.chain.back[0]));
                let open = e
                    .chain
                    .open
                    .as_ref()
                    .and_then(|open| e.chain.cells.last().map(|&leaf| (leaf, open.facing)));
                first.into_iter().chain(open)
            })
            .collect();
        let mut blocked: HashSet<Pos> = HashSet::new();
        for (anchor, cell) in keep_clear(grid, diagram, |p, d| skip.contains(&(p, d))) {
            let cell = if moved_cells.contains(&anchor) { shifted(cell) } else { Some(cell) };
            blocked.extend(cell);
        }
        let moved = moved_boxes(diagram, index);
        let ancestors: HashSet<usize> =
            std::iter::successors(b.parent, |&p| diagram.boxes[p].parent).collect();
        for (i, other) in diagram.boxes.iter().enumerate() {
            if ancestors.contains(&i) || moved.contains(&i) {
                continue;
            }
            let r = other.rect;
            if !r.intersects(&region) {
                continue;
            }
            blocked.extend(
                (r.top..=r.bottom)
                    .flat_map(|row| (r.left..=r.right).map(move |c| Pos::new(row, c))),
            );
        }
        blocked.extend(perimeter(new_rect));
        blocked.extend(footprint.iter().copied());
        for e in &ends {
            if let Some(open) = &e.chain.open {
                blocked.extend(open.gap.iter().copied());
            }
        }

        let mut wires: HashMap<usize, HashSet<Pos>> = HashMap::new();
        for e in &ends {
            wires.entry(e.wire).or_insert_with(|| {
                diagram.wires[e.wire]
                    .cells
                    .iter()
                    .copied()
                    .filter(|p| !erased.contains(p))
                    .collect()
            });
        }
        Ok(Mover { grid, rect, work, ends, region, blocked, footprint, wires })
    }

    /// [`reshape`]'s sheet: the box lifted and `cells` put down in its
    /// place, and each wire at a moving port cut back to its chain, as
    /// [`Mover::new`] does for a move.
    fn reshaped(
        grid: &'a Grid,
        diagram: &'a Diagram,
        index: usize,
        new_rect: Rect,
        cells: &[(Pos, char)],
        ports: &HashMap<Pos, Pos>,
    ) -> Result<Self, Refusal> {
        let b = &diagram.boxes[index];
        let rect = b.rect;
        let at = Pos::new(rect.top, rect.left);
        let no_room = |why: &str| Refusal::new(at, RefusalKind::Blocked(Blockage::NoRoom), why);
        let Some(parent) = b.parent.map(|p| &diagram.boxes[p]) else {
            return Err(Refusal::new(at, RefusalKind::Target, "only a box inside a definition is redrawn this way"));
        };
        if !parent.rect.encloses(&new_rect) {
            return Err(no_room("the box would leave its definition; try Make room to grow it"));
        }
        let r = parent.rect;
        let above = parent.dividers.iter().map(|d| d.row).filter(|&d| d < rect.top).max();
        let below = parent.dividers.iter().map(|d| d.row).filter(|&d| d > rect.bottom).min();
        let (top, bottom) = (above.unwrap_or(r.top) + 1, below.unwrap_or(r.bottom) - 1);
        if new_rect.top < top || new_rect.bottom > bottom {
            return Err(no_room("the box would cross into the next arm; try Make room to grow its arm"));
        }
        let region = Rect { top, left: r.left + 1, bottom, right: r.right - 1 };

        let border = borders(diagram);
        let texts = text_cells(diagram);
        let mut ends = Vec::new();
        for (w, wire) in diagram.wires.iter().enumerate() {
            for e in &wire.ends {
                let EndKind::Port(port) = e.kind else { continue };
                let Some(p) = b.ports.iter().find(|p| p.pos == port) else { continue };
                let Some(&new_port) = ports.get(&port).filter(|&&q| q != port) else { continue };
                if rect.contains(e.pos) {
                    continue;
                }
                let chain = walk(grid, diagram, &border, &texts, e.pos, e.facing);
                let Some(target) = new_port.step(p.side) else {
                    let why = "the wire at this port would run off the sheet";
                    return Err(Refusal::new(port, RefusalKind::Blocked(Blockage::Edge), why));
                };
                let arrow = p.kind == PortKind::Arrow;
                ends.push(End { wire: w, port, new_port, side: p.side, arrow, target, chain });
            }
        }
        ends.sort_by_key(|e| e.port);
        let starts: HashSet<Pos> = ends.iter().filter_map(|e| e.chain.cells.first().copied()).collect();
        for e in &ends {
            if let Some((anchor, _)) = e.chain.anchor
                && (starts.contains(&anchor) || b.ports.iter().any(|p| e.chain.cells.contains(&p.pos)))
            {
                let why = "this wire loops back into the same box";
                return Err(Refusal::new(e.port, RefusalKind::Unsupported, why));
            }
        }

        let orig: HashMap<Pos, u8> = diagram
            .wires
            .iter()
            .flat_map(|w| &w.cells)
            .filter_map(|&p| diagram.strokes.get(p).map(|s| (p, s.conn)))
            .collect();
        let base = (0..grid.height()).map(|r| grid.row(r).to_vec()).collect();
        let mut work = Work { base, mask: HashMap::new(), orig };
        for e in &ends {
            let c = &e.chain;
            for (i, &p) in c.cells.iter().enumerate() {
                let m = work.mask(p) & !c.orig[i];
                work.set(p, m);
            }
            if let Some((anchor, into)) = c.anchor {
                let m = work.mask(anchor) & !into.bit();
                work.set(anchor, m);
            }
        }
        let erased: HashSet<Pos> = ends.iter().flat_map(|e| e.chain.cells.iter().copied()).collect();

        // Lift the box, then draw it anew.
        for row in rect.top..=rect.bottom {
            for col in rect.left..=rect.right {
                work.write(Pos::new(row, col), ' ');
            }
        }
        let mut footprint = HashSet::new();
        for &(q, ch) in cells {
            if work.ch(q) != ' ' {
                return Err(no_room("the redrawn box would land on something; try Make room"));
            }
            footprint.insert(q);
            if ch != ' ' {
                work.write(q, ch);
            }
        }

        // What no route may take: as for a move, with each port's kept-clear
        // cell going where the port goes.
        let skip: HashSet<(Pos, Dir)> = ends
            .iter()
            .flat_map(|e| {
                let first = e.chain.cells.first().map(|&p| (p, e.chain.back[0]));
                let open = e
                    .chain
                    .open
                    .as_ref()
                    .and_then(|open| e.chain.cells.last().map(|&leaf| (leaf, open.facing)));
                first.into_iter().chain(open)
            })
            .collect();
        let mut blocked: HashSet<Pos> = HashSet::new();
        for (anchor, cell) in keep_clear(grid, diagram, |p, d| skip.contains(&(p, d))) {
            if !rect.contains(anchor) {
                blocked.insert(cell);
            } else if let Some(&to) = ports.get(&anchor) {
                let dr = to.row as isize - anchor.row as isize;
                let dc = to.col as isize - anchor.col as isize;
                blocked.extend(add(cell, dr, dc));
            }
        }
        let inside = moved_boxes(diagram, index);
        let ancestors: HashSet<usize> = std::iter::successors(b.parent, |&p| diagram.boxes[p].parent).collect();
        for (i, other) in diagram.boxes.iter().enumerate() {
            if ancestors.contains(&i) || inside.contains(&i) || !other.rect.intersects(&region) {
                continue;
            }
            let r = other.rect;
            blocked.extend((r.top..=r.bottom).flat_map(|row| (r.left..=r.right).map(move |c| Pos::new(row, c))));
        }
        blocked.extend(perimeter(new_rect));
        blocked.extend(footprint.iter().copied());
        for e in &ends {
            if let Some(open) = &e.chain.open {
                blocked.extend(open.gap.iter().copied());
            }
        }
        let mut wires: HashMap<usize, HashSet<Pos>> = HashMap::new();
        for e in &ends {
            wires.entry(e.wire).or_insert_with(|| {
                diagram.wires[e.wire].cells.iter().copied().filter(|p| !erased.contains(p)).collect()
            });
        }
        Ok(Mover { grid, rect, work, ends, region, blocked, footprint, wires })
    }

    /// Routes every end, trying the ports in order and in reverse, and keeps
    /// the cheaper drawing. Both tries draw on one `budget` of router
    /// expansions.
    fn run(self, mut budget: usize) -> Result<String, Refusal> {
        let order: Vec<usize> = (0..self.ends.len()).collect();
        let mut orders = vec![order.clone()];
        if order.len() > 1 {
            orders.push(order.iter().rev().copied().collect());
        }
        let mut best: Option<(u32, Work)> = None;
        let mut failure: Option<(usize, Miss)> = None;
        for order in orders {
            match self.attempt(&order, &mut budget) {
                Ok((cost, work)) => {
                    if best.as_ref().is_none_or(|(c, _)| cost < *c) {
                        best = Some((cost, work));
                    }
                }
                // A search that gave up proves nothing, so it outranks one
                // that found no path.
                Err((end, miss)) => {
                    if failure.is_none_or(|(_, m)| m == Miss::NoPath && miss == Miss::GaveUp) {
                        failure = Some((end, miss));
                    }
                }
            }
        }
        let Some((_, work)) = best else {
            let corner = Pos::new(self.rect.top, self.rect.left);
            let Some((end, miss)) = failure.map(|(i, miss)| (&self.ends[i], miss)) else {
                return Err(Refusal::new(
                    corner,
                    RefusalKind::Blocked(Blockage::NoRoom),
                    "no free path for a wire; try Make room or a shorter move",
                ));
            };
            let reason = match (miss, &end.chain.open) {
                (Miss::GaveUp, _) => {
                    "the search for a path for the wire at this port gave up; try a shorter move".to_owned()
                }
                (Miss::NoPath, Some(open)) => format!(
                    "no room for the wire from {}; move {} or try Make room",
                    open.text, open.text
                ),
                (Miss::NoPath, None) => {
                    "no free path for the wire at this port; try Make room or a shorter move"
                        .to_owned()
                }
            };
            return Err(Refusal::new(end.port, miss.kind(), reason));
        };
        Ok(work.render())
    }

    /// Routes the ends in `order` on a copy of the sheet, drawing on
    /// `budget`. `Err` names the end that couldn't be routed, and why.
    fn attempt(&self, order: &[usize], budget: &mut usize) -> Result<(u32, Work), (usize, Miss)> {
        let mut work = self.work.clone();
        let mut wires = self.wires.clone();
        let mut pending: HashSet<usize> = order.iter().copied().collect();
        let mut total = 0;
        for &i in order {
            pending.remove(&i);
            let end = &self.ends[i];
            let others: HashSet<Pos> =
                pending.iter().flat_map(|&j| self.ends[j].chain.cells.iter().copied()).collect();
            let targets: HashSet<Pos> = self
                .ends
                .iter()
                .enumerate()
                .filter(|&(j, _)| j != i)
                .map(|(_, e)| e.target)
                .collect();
            let terrain = EndTerrain {
                mover: self,
                work: &work,
                end,
                own: &wires[&end.wire],
                others: &others,
                targets: &targets,
                usable_from: usable_from(end, &work, &self.footprint, &targets),
            };
            let start = Start { pos: end.new_port, heading: Some(end.side), straight: true };
            let found = router::search(&terrain, start, budget).map_err(|miss| (i, miss))?;
            total += found.cost;
            let drawn = apply(&mut work, end, &found);
            wires.get_mut(&end.wire).expect("every end's wire is tracked").extend(drawn);
        }
        Ok((total, work))
    }
}

/// The first chain cell from which the rest of the chain can be restored:
/// every cell from there out must still be blank and unclaimed.
fn usable_from(end: &End, work: &Work, footprint: &HashSet<Pos>, targets: &HashSet<Pos>) -> usize {
    let cells = &end.chain.cells;
    let free = |p: &Pos| work.ch(*p) == ' ' && !footprint.contains(p) && !targets.contains(p);
    let mut from = cells.len();
    while from > 0 && free(&cells[from - 1]) {
        from -= 1;
    }
    from
}

/// Follows a wire from `start` away from the port it faces (`facing`) to
/// the first cell that stays: a tee, or the far end.
fn walk(
    grid: &Grid,
    diagram: &Diagram,
    border: &HashSet<Pos>,
    texts: &HashMap<Pos, usize>,
    start: Pos,
    facing: Dir,
) -> Chain {
    let strokes = &diagram.strokes;
    let is_wire = |p: Pos| strokes.get(p).is_some() && !border.contains(&p);
    // A crossing's two axes are separate wires; this one owns `d`'s axis.
    let own = |p: Pos, d: Dir| -> u8 {
        let s = strokes.get(p).expect("wire cells are strokes");
        match s.kind {
            Kind::Crossing if d.is_horizontal() => stroke::E | stroke::W,
            Kind::Crossing => stroke::N | stroke::S,
            _ => s.conn,
        }
    };
    let joins = |p: Pos, d: Dir| {
        p.step(d)
            .is_some_and(|q| is_wire(q) && strokes.get(q).is_some_and(|s| s.connects(d.opposite())))
    };

    // Every wire is a tree, so this only stops a malformed walk.
    let limit = diagram.wires.iter().map(|w| w.cells.len()).sum::<usize>();
    let (mut cells, mut orig, mut back) = (Vec::new(), Vec::new(), Vec::new());
    let (mut p, mut from) = (start, facing);
    loop {
        let mask = own(p, from);
        let rest: Vec<Dir> =
            Dir::ALL.into_iter().filter(|&d| d != from && mask & d.bit() != 0).collect();
        let onward: Vec<Dir> = rest.iter().copied().filter(|&d| joins(p, d)).collect();
        if let ([d], [_]) = (&rest[..], &onward[..]) {
            let d = *d;
            cells.push(p);
            orig.push(mask);
            back.push(from);
            if cells.len() > limit {
                break;
            }
            p = p.step(d).expect("joined cells exist");
            from = d.opposite();
            continue;
        }
        // The far end. An open end meeting text is erased with the chain.
        if let ([d], []) = (&rest[..], &onward[..]) {
            let d = *d;
            let plain =
                strokes.get(p).is_some_and(|s| matches!(s.kind, Kind::Line | Kind::Corner { .. }));
            if d.is_horizontal()
                && plain
                && let (gap, Some(t)) = across(grid, texts, p, d)
            {
                let t = &diagram.texts[t];
                let behind = Pos::new(t.pos.row, t.pos.col);
                let (room, what) = across(grid, texts, behind, Dir::W);
                let held = what.is_none()
                    && room.last().copied().unwrap_or(behind).step(Dir::W).is_some_and(|q| {
                        matches!(strokes.get(q).map(|s| s.kind), Some(Kind::Arrow(_)))
                    });
                let tight = d == Dir::W && room.len() >= 2 && !held;
                cells.push(p);
                orig.push(mask);
                back.push(from);
                let open = Open { facing: d, text: t.content.clone(), gap, tight };
                return Chain { cells, orig, back, anchor: None, open: Some(open) };
            }
        }
        return Chain { cells, orig, back, anchor: Some((p, from)), open: None };
    }
    Chain { cells, orig, back, anchor: Some((p, from)), open: None }
}

/// The router's view while reconnecting one end.
struct EndTerrain<'a> {
    mover: &'a Mover<'a>,
    work: &'a Work,
    end: &'a End,
    /// This wire's cells as they stand.
    own: &'a HashSet<Pos>,
    /// Chain cells of ends still to route.
    others: &'a HashSet<Pos>,
    /// Other ends' start cells.
    targets: &'a HashSet<Pos>,
    usable_from: usize,
}

/// How a route ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Join {
    /// At the chain's cell `i`, restoring the rest of the chain.
    Chain(usize),
    /// At the chain's anchor.
    Anchor,
    /// In the gap before an open end's text, as the new open end.
    Gap,
    /// Anywhere else on the wire, as a new tee.
    Tee,
}

impl EndTerrain<'_> {
    fn join(&self, p: Pos) -> Option<Join> {
        let c = &self.end.chain;
        if let Some(i) = c.cells.iter().position(|&q| q == p) {
            return (i >= self.usable_from).then_some(Join::Chain(i));
        }
        if c.anchor.is_some_and(|(a, _)| a == p) {
            return Some(Join::Anchor);
        }
        if c.open.as_ref().is_some_and(|open| open.gap.contains(&p)) {
            return Some(Join::Gap);
        }
        self.own.contains(&p).then_some(Join::Tee)
    }

    /// The connections `p` would have after a route arrives from `back`.
    fn joined_mask(&self, how: Join, p: Pos, back: Dir) -> Option<u8> {
        let c = &self.end.chain;
        let ch = self.work.ch(p);
        let plain =
            matches!(stroke::fixed(ch).map(|s| s.kind), Some(Kind::Line | Kind::Corner { .. }));
        match how {
            Join::Chain(i) => {
                // Restoring the rest of the chain gives the anchor its side back.
                let anchor_ok = c.anchor.is_none_or(|(a, into)| {
                    let m = self.work.mask(a);
                    m & into.bit() == 0 && (m | into.bit()).count_ones() <= 3
                });
                if !anchor_ok {
                    return None;
                }
                if back == c.back[i] {
                    return Some(c.orig[i]);
                }
                let rest = c.orig[i] & !c.back[i].bit();
                let s = stroke::fixed(self.mover.grid.get(p))?;
                let bendable = matches!(s.kind, Kind::Line | Kind::Corner { .. });
                (bendable && rest & back.bit() == 0).then_some(rest | back.bit())
            }
            Join::Anchor => {
                let m = self.work.mask(p);
                let s = stroke::fixed(self.mover.grid.get(p))?;
                if m & back.bit() != 0 || (m | back.bit()).count_ones() > 3 {
                    return None;
                }
                let fixed_shape = matches!(s.kind, Kind::Arrow(_) | Kind::Crossing);
                if fixed_shape && (m | back.bit()) != s.conn {
                    return None;
                }
                Some(m | back.bit())
            }
            Join::Gap => {
                let open = c.open.as_ref()?;
                let touches = p.step(open.facing).is_some_and(|q| self.work.ch(q) != ' ');
                (back != open.facing && (open.tight || !touches))
                    .then_some(open.facing.bit() | back.bit())
            }
            Join::Tee => {
                let m = self.work.mask(p);
                (plain && m.count_ones() == 2 && m & back.bit() == 0).then_some(m | back.bit())
            }
        }
    }
}

impl Terrain for EndTerrain<'_> {
    fn cell(&self, p: Pos) -> Cell {
        let m = self.mover;
        let r = m.region;
        if p.row < r.top || p.row > r.bottom || p.col < r.left || p.col > r.right {
            return Cell::Blocked;
        }
        if p != self.end.target && (m.blocked.contains(&p) || self.targets.contains(&p)) {
            return Cell::Blocked;
        }
        if self.join(p).is_some() || self.own.contains(&p) {
            return Cell::Blocked;
        }
        match self.work.ch(p) {
            ' ' => Cell::Free,
            '─' => Cell::Line(true),
            '│' => Cell::Line(false),
            _ => Cell::Blocked,
        }
    }

    fn extra(&self, p: Pos) -> u32 {
        let near = Dir::ALL
            .into_iter()
            .filter_map(|d| p.step(d))
            .filter(|q| !self.own.contains(q) && *q != self.end.new_port && self.work.ch(*q) != ' ')
            .filter(|&q| stroke::fixed(self.work.ch(q)).is_some())
            .count() as u32;
        let reuse = if self.others.contains(&p) { OTHER_CHAIN } else { 0 };
        near * CLEARANCE + reuse
    }

    fn goal(&self, p: Pos, heading: Dir) -> Option<u32> {
        if self.mover.footprint.contains(&p) || self.targets.contains(&p) {
            return None;
        }
        let how = self.join(p)?;
        let back = heading.opposite();
        let mask = self.joined_mask(how, p, back)?;
        let c = &self.end.chain;
        let before = match how {
            Join::Chain(i) => c.orig[i],
            Join::Gap => c.open.as_ref().map_or(0, |o| o.facing.bit() | o.facing.opposite().bit()),
            Join::Anchor | Join::Tee => self.mover.work.orig.get(&p).copied().unwrap_or(0),
        };
        let mut cost = 0;
        if mask != before && is_corner(mask) && !is_corner(before) {
            cost += BEND;
        }
        if mask.count_ones() == 3 && before.count_ones() < 3 {
            cost += TEE;
        }
        if how == Join::Gap {
            let facing = c.open.as_ref()?.facing;
            if p.step(facing).is_some_and(|q| self.work.ch(q) != ' ') {
                cost += TIGHT;
            }
        }
        Some(cost)
    }

    fn bend_extra(&self, p: Pos) -> u32 {
        // A bend right before an arrowhead is lint L002.
        if self.end.arrow && p == self.end.target { BEND } else { 0 }
    }
}

/// Draws a found route into `work`, joining it to the wire as its goal
/// says. Returns the cells the wire now has that it didn't.
fn apply(work: &mut Work, end: &End, found: &Found) -> Vec<Pos> {
    let path = &found.path;
    let goal = *path.last().expect("a route ends somewhere");
    let mut drawn = Vec::new();
    // path[0] is the port; path[1..len-1] are new cells.
    for k in 1..path.len() - 1 {
        let p = path[k];
        let m = toward(p, path[k - 1]).bit() | toward(p, path[k + 1]).bit();
        let m = work.mask(p) | m;
        work.set(p, m);
        drawn.push(p);
    }
    let back = toward(goal, path[path.len() - 2]);
    let c = &end.chain;
    drawn.push(goal);
    if let Some(i) = c.cells.iter().position(|&q| q == goal) {
        let rest =
            if back == c.back[i] { c.orig[i] } else { (c.orig[i] & !c.back[i].bit()) | back.bit() };
        let m = work.mask(goal) | rest;
        work.set(goal, m);
        for j in i + 1..c.cells.len() {
            let p = c.cells[j];
            let m = work.mask(p) | c.orig[j];
            work.set(p, m);
            drawn.push(p);
        }
        if let Some((a, into)) = c.anchor {
            let m = work.mask(a) | into.bit();
            work.set(a, m);
        }
    } else if let Some(open) = c.open.as_ref().filter(|open| open.gap.contains(&goal)) {
        work.set(goal, open.facing.bit() | back.bit());
    } else {
        let m = work.mask(goal) | back.bit();
        work.set(goal, m);
    }
    drawn
}
