//! Making room (PLAN P10, "How room is found"): insert rows or columns
//! until a refused edit fits, then apply it, as one checked edit.
//!
//! The candidates come in a fixed order: first a move's parent grown by
//! exactly what the move overshoots, then one to three rows or columns at
//! the refusal's cell (and, for a move, just past the box in the way it
//! goes). Each is a checked compound edit, at most [`ATTEMPTS`] of them, and
//! the first that succeeds wins. The order and the budget are private:
//! they appear in no reason and no dump.

use shawline_doc::Doc;
use shawline_grid::{Pos, Rect};
use shawline_shape::Diagram;
use shawline_shape::structure::Mapping;

use crate::canon::Sheets;
use crate::cut::{self, MAX_COUNT};
use crate::mapped;
use crate::{Axis, Edit, Edited, Insertion, Refusal, RefusalKind, Remedy, Roomed, apply};

/// The most compound edits one search tries.
const ATTEMPTS: usize = 12;

pub(crate) fn make_room(doc: &Doc, edit: &Edit) -> Result<Roomed, Refusal> {
    if let Edit::MakeRoom(_) = edit {
        let why = "make room for one edit at a time";
        return Err(Refusal::new(edit.at(), RefusalKind::Target, why));
    }
    let refusal = match apply(doc, edit) {
        Ok(edited) => return Ok(Roomed { edited, inserted: Vec::new() }),
        Err(refusal) => refusal,
    };
    let refused = || Refusal { remedy: None, ..refusal.clone() };
    if refusal.remedy != Some(Remedy::MakeRoom) {
        return Err(refused());
    }
    let sheets = Sheets::read(doc, refusal.at)?;
    let carry = |p: Pos| sheets.entity(p).or_else(|| sheets.place(p)).unwrap_or(p);
    let edit = readdress(edit, &carry);
    let at = carry(refusal.at);
    let canon = sheets.doc();
    let diagram = mapped::read(canon).1;

    for candidate in candidates(diagram, &edit, at).into_iter().take(ATTEMPTS) {
        if let Some((edited, inserted)) = attempt(canon, diagram, &edit, &candidate) {
            let edited = Edited { tidied: sheets.tidied(), ..edited };
            return Ok(Roomed { edited, inserted });
        }
    }
    Err(refused())
}

/// One compound edit: the insertions, each re-addressed through those
/// before it, then `edit` re-addressed through them all.
fn attempt(canon: &Doc, diagram: &Diagram, edit: &Edit, candidate: &[Insertion]) -> Option<(Edited, Vec<Insertion>)> {
    // The moved box, by its index before any insertion.
    let moved = match *edit {
        Edit::Move { corner, dr, dc } => {
            let b = diagram.boxes.iter().position(|b| (b.rect.top, b.rect.left) == (corner.row, corner.col))?;
            Some((b, dr, dc))
        }
        _ => None,
    };
    let mut map = Mapping::identity(diagram);
    let mut edit = edit.clone();
    let mut doc: Option<Doc> = None;
    let mut inserted = Vec::new();
    let mut cuts: Vec<cut::Cut> = Vec::new();
    for insertion in candidate {
        let at = cuts.iter().fold(insertion.at, |p, c| c.cell(p).unwrap_or(p));
        let insertion = Insertion { at, ..*insertion };
        let (edited, cut) = cut::cut(doc.as_ref().unwrap_or(canon), insertion, false).ok()?;
        map = mapped::compose(&map, &edited.map);
        edit = readdress(&edit, &|p| cut.cell(p).unwrap_or(p));
        doc = Some(edited.doc);
        inserted.push(insertion);
        cuts.push(cut);
    }
    let done = apply(doc.as_ref().unwrap_or(canon), &edit).ok()?;
    let map = mapped::compose(&map, &done.map);
    let edited = Edited { doc: done.doc, map, tidied: false };
    if let Some((b, dr, dc)) = moved
        && !stays_put(diagram, &edited, b, dr, dc)
    {
        return None;
    }
    Some((edited, inserted))
}

/// Whether box `i` of `diagram` ends `dr`, `dc` from where it was relative
/// to its parent, as the move asked.
fn stays_put(diagram: &Diagram, edited: &Edited, i: usize, dr: isize, dc: isize) -> bool {
    let offset = |d: &Diagram, b: usize| {
        let r = d.boxes[b].rect;
        let p = d.boxes[b].parent.map_or(Pos::new(0, 0), |p| Pos::new(d.boxes[p].rect.top, d.boxes[p].rect.left));
        (r.top as isize - p.row as isize, r.left as isize - p.col as isize)
    };
    let Some(j) = edited.map.boxes.get(i).copied().flatten() else { return false };
    let Some(new) = edited.doc.diagram() else { return false };
    let was = offset(diagram, i);
    offset(new, j) == (was.0 + dr, was.1 + dc)
}

/// The insertions to try, in order.
fn candidates(diagram: &Diagram, edit: &Edit, at: Pos) -> Vec<Vec<Insertion>> {
    let mut out: Vec<Vec<Insertion>> = Vec::new();
    let moved = match *edit {
        Edit::Move { corner, dr, dc } => diagram
            .boxes
            .iter()
            .position(|b| (b.rect.top, b.rect.left) == (corner.row, corner.col))
            .map(|b| (b, dr, dc)),
        _ => None,
    };
    if let Some((parent, from, target)) = landing(diagram, edit)
        && let Some(grow) = overshoot(diagram, parent, from, target)
    {
        out.push(grow);
    }
    let grown = out.len();
    for count in 1..=3 {
        if let Some((b, dr, dc)) = moved {
            let r = diagram.boxes[b].rect;
            if dr > 0 {
                out.push(vec![Insertion { axis: Axis::Row, at: Pos::new(r.bottom + 1, r.left), count }]);
            }
            if dc > 0 {
                out.push(vec![Insertion { axis: Axis::Column, at: Pos::new(r.top, r.right + 1), count }]);
            }
        }
        out.push(vec![Insertion { axis: Axis::Row, at, count }]);
        out.push(vec![Insertion { axis: Axis::Column, at, count }]);
    }
    let mut seen = Vec::new();
    out.retain(|c| {
        let fresh = !seen.contains(c);
        seen.push(c.clone());
        fresh
    });
    // Growing a box the edit happens in is what room is for; stretching a
    // node beside it only comes after everything else.
    let held = moved.map(|(b, _, _)| diagram.boxes[b].rect);
    out[grown..].sort_by_key(|c| c.iter().any(|i| stretches(diagram, i, held)));
    out
}

/// Whether `i` cuts through a box inside a definition that doesn't hold
/// the moved box (`held`): a node it would stretch.
fn stretches(diagram: &Diagram, i: &Insertion, held: Option<Rect>) -> bool {
    diagram.boxes.iter().any(|b| {
        let r = b.rect;
        let inner = b.parent.is_some() && held.is_none_or(|h| !r.encloses(&h) || r == h);
        inner
            && match i.axis {
                Axis::Row => r.top < i.at.row && i.at.row <= r.bottom,
                Axis::Column => r.left < i.at.col && i.at.col <= r.right,
            }
    })
}

/// Rows and columns that grow box `parent` (or the arm of it below row
/// `from`) by exactly what `target` overshoots past its bottom or right
/// edge.
fn overshoot(diagram: &Diagram, parent: usize, from: usize, target: Rect) -> Option<Vec<Insertion>> {
    let shape = &diagram.boxes[parent];
    let p = shape.rect;
    let mut grow = Vec::new();
    // The border, or the divider, below the arm.
    let floor = shape.dividers.iter().map(|d| d.row).filter(|&d| d > from).min().unwrap_or(p.bottom);
    if target.bottom >= floor {
        let count = target.bottom - floor + 1;
        grow.push(Insertion { axis: Axis::Row, at: Pos::new(floor, p.left), count: count.min(MAX_COUNT) });
    }
    if target.right >= p.right {
        let count = target.right - p.right + 1;
        grow.push(Insertion { axis: Axis::Column, at: Pos::new(p.top, p.right), count: count.min(MAX_COUNT) });
    }
    (!grow.is_empty()).then_some(grow)
}

/// The box an edit would put down, and the box it goes in: a move's box
/// where it would land, or a placed node.
fn landing(diagram: &Diagram, edit: &Edit) -> Option<(usize, usize, Rect)> {
    match edit {
        Edit::Move { corner, dr, dc } => {
            let b = diagram.boxes.iter().position(|b| (b.rect.top, b.rect.left) == (corner.row, corner.col))?;
            let r = diagram.boxes[b].rect;
            let target = Rect {
                top: r.top.checked_add_signed(*dr)?,
                left: r.left.checked_add_signed(*dc)?,
                bottom: r.bottom.checked_add_signed(*dr)?,
                right: r.right.checked_add_signed(*dc)?,
            };
            Some((diagram.boxes[b].parent?, r.bottom, target))
        }
        Edit::PlaceNode { corner, word, inputs, outputs } => {
            let target = crate::wiring::node_rect(*corner, word, *inputs, *outputs);
            let holder = (0..diagram.boxes.len())
                .filter(|&b| diagram.boxes[b].rect.interior_contains(*corner))
                .min_by_key(|&b| diagram.boxes[b].rect.area())?;
            Some((holder, corner.row, target))
        }
        _ => None,
    }
}

/// `edit` with every cell it names taken through `f`.
pub(crate) fn readdress(edit: &Edit, f: &impl Fn(Pos) -> Pos) -> Edit {
    let rect = |r: Rect| {
        let (a, b) = (f(Pos::new(r.top, r.left)), f(Pos::new(r.bottom, r.right)));
        Rect { top: a.row, left: a.col, bottom: b.row, right: b.col }
    };
    let insertion = |i: Insertion| Insertion { at: f(i.at), ..i };
    match edit.clone() {
        Edit::Move { corner, dr, dc } => Edit::Move { corner: f(corner), dr, dc },
        Edit::Connect { from, to } => Edit::Connect { from: f(from), to: f(to) },
        Edit::RemoveWire(at) => Edit::RemoveWire(f(at)),
        Edit::Disconnect(at) => Edit::Disconnect(f(at)),
        Edit::PlaceNode { corner, word, inputs, outputs } => Edit::PlaceNode { corner: f(corner), word, inputs, outputs },
        Edit::RemoveNode(at) => Edit::RemoveNode(f(at)),
        Edit::Rewire { from, to } => Edit::Rewire { from: f(from), to: f(to) },
        Edit::PlaceConnected { at, word, inputs, outputs, from } => {
            Edit::PlaceConnected { at: f(at), word, inputs, outputs, from: f(from) }
        }
        Edit::Insert(i) => Edit::Insert(insertion(i)),
        Edit::Delete(i) => Edit::Delete(insertion(i)),
        Edit::MakeRoom(inner) => Edit::MakeRoom(Box::new(readdress(&inner, f))),
        Edit::SetText { at, content } => Edit::SetText { at: f(at), content },
        Edit::PlaceText { at, content } => Edit::PlaceText { at: f(at), content },
        Edit::RemoveText(at) => Edit::RemoveText(f(at)),
        Edit::PlaceDefinition { rect: r, name } => Edit::PlaceDefinition { rect: rect(r), name },
        Edit::PlaceSwitch { rect: r, guard } => Edit::PlaceSwitch { rect: rect(r), guard },
        Edit::AddPort { at, name } => Edit::AddPort { at: f(at), name },
        Edit::AddArm { at, guard } => Edit::AddArm { at: f(at), guard },
        Edit::PlaceInline { at, word } => Edit::PlaceInline { at: f(at), word },
    }
}
