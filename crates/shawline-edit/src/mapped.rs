//! Where an edit sent the old sheet's parts (PLAN P10, "Selection across
//! edits"). Boxes and texts are found by place, through the cell map the
//! edit knows. Wires are found by what they join: a port, or a text from
//! one side, belongs to one wire only, so a re-routed wire is still found,
//! and a wire that kept none of what it joined is gone.

use std::collections::HashMap;

use shawline_doc::Doc;
use shawline_grid::{Grid, Pos, Rect};
use shawline_shape::Diagram;
use shawline_shape::structure::{Across, End, Mapping, Structure, Term};

use crate::Edited;

/// A read sheet: its grid and diagram.
pub(crate) type Read<'a> = (&'a Grid, &'a Diagram);

/// The sheet a map starts from: the input's canonical revision, which
/// every edit draws on.
pub(crate) fn before(doc: &Doc) -> Read<'_> {
    let canon = doc.canonical().expect("an edit succeeded, so its input reads");
    read(canon)
}

/// The grid and diagram of a revision an edit returned.
pub(crate) fn read(doc: &Doc) -> Read<'_> {
    let grid = doc.grid().expect("an edit returns a sheet on the grid");
    (grid, doc.diagram().expect("a sheet on the grid is recognized"))
}

/// The edit drew on the formatted text, not the one given.
pub(crate) fn tidied(doc: &Doc) -> bool {
    doc.canonical().is_some_and(|c| c.text() != doc.text())
}

/// The map from `from` to `to`, where every cell `p` of `from` that stayed
/// went to `cell(p)`: boxes by their corners, texts by their first cells,
/// wires by what they join and, failing that, by their cells.
pub(crate) fn map(from: Read, to: Read, cell: impl Fn(Pos) -> Option<Pos>) -> Mapping {
    let none = Pos::new(usize::MAX, usize::MAX);
    let at = |p: Pos| cell(p).unwrap_or(none);
    let rect = |_, r: Rect| {
        let (a, b) = (at(Pos::new(r.top, r.left)), at(Pos::new(r.bottom, r.right)));
        Rect { top: a.row, left: a.col, bottom: b.row, right: b.col }
    };
    let mut mapping = Mapping::by_place(from.1, to.1, rect, |_, p| at(p));
    mapping.wires = wires(from, to, &mapping, &cell);
    mapping
}

/// Every part that is still there stays where it was.
pub(crate) fn kept(old: &Doc, new: Doc) -> Edited {
    let map = map(before(old), read(&new), Some);
    Edited { doc: new, map, tidied: tidied(old) }
}

/// The box at given cell `corner` moved by `dr`, `dc`, with what it holds
/// and the names that follow its outputs. Boxes and texts are found by
/// place in the drawing the move made, then by order in its tidied form,
/// which the formatter keeps.
pub(crate) fn moved(old: &Doc, corner: Pos, dr: isize, dc: isize, moved: crate::mover::Moved) -> Edited {
    let sheets = crate::canon::Sheets::read(old, corner).expect("the move read this sheet");
    let index = sheets
        .given()
        .1
        .boxes
        .iter()
        .position(|b| (b.rect.top, b.rect.left) == (corner.row, corner.col))
        .expect("the move found this box");
    let (grid, diagram) = sheets.canon();
    let shift = crate::mover::shift_of(grid, diagram, index, dr, dc);
    let trailing: std::collections::HashSet<Pos> =
        shift.texts.iter().flat_map(|&t| diagram.texts[t].cells()).collect();
    let cell = |p: Pos| Some(if shift.rect.contains(p) || trailing.contains(&p) { shift.pos(p) } else { p });
    let drawn = read(&moved.drawn);
    let mut map = map(before(old), drawn, cell);
    let new = read(&moved.doc);
    map.wires = wires(before(old), new, &map, &cell);
    Edited { doc: moved.doc, map, tidied: tidied(old) }
}

/// Wires found by what they join, for a step whose boxes and texts `map`
/// already places.
pub(crate) fn wires_by_terms(from: Read, to: Read, map: &Mapping) -> Vec<Option<usize>> {
    wires(from, to, map, &Some)
}

/// `first` then `second`.
pub(crate) fn compose(first: &Mapping, second: &Mapping) -> Mapping {
    let then = |a: &[Option<usize>], b: &[Option<usize>]| -> Vec<Option<usize>> {
        a.iter().map(|i| i.and_then(|i| b.get(i).copied().flatten())).collect()
    };
    Mapping {
        boxes: then(&first.boxes, &second.boxes),
        texts: then(&first.texts, &second.texts),
        wires: then(&first.wires, &second.wires),
    }
}

/// The ends of a wire that name it alone: ports, and texts met from one
/// side.
fn names(term: &Term) -> bool {
    matches!(term.end, End::Port(..) | End::Open(Across::Text(_)) | End::Arrow(Across::Text(_)))
}

/// `term` of `from`, in the indices of `to`.
fn carried(term: &Term, map: &Mapping) -> Option<Term> {
    let text = |t: usize| map.texts.get(t).copied().flatten();
    let end = match term.end {
        End::Port(b, k) => End::Port(map.boxes.get(b).copied().flatten()?, k),
        End::Open(Across::Text(t)) => End::Open(Across::Text(text(t)?)),
        End::Arrow(Across::Text(t)) => End::Arrow(Across::Text(text(t)?)),
        _ => return None,
    };
    Some(Term { facing: term.facing, end })
}

/// Each wire of `from` to the wire of `to` it became.
///
/// A wire sharing a named end with one of `to`'s is that wire; a source
/// (an end facing west, into a tee or a text) decides first, so a branch
/// moved from one wire to another leaves each wire with its own source. A
/// wire naming nothing (a stray stroke) is found by a cell it kept.
fn wires(from: Read, to: Read, map: &Mapping, cell: &impl Fn(Pos) -> Option<Pos>) -> Vec<Option<usize>> {
    let (old, new) = (from.1, to.1);
    let new_terms: Vec<Vec<Term>> =
        (0..new.wires.len()).map(|w| Structure::wire(to.0, new, w).terms).collect();
    let owner: HashMap<Term, usize> = new_terms
        .iter()
        .enumerate()
        .flat_map(|(w, terms)| terms.iter().filter(|t| names(t)).map(move |&t| (t, w)))
        .collect();
    let unnamed: HashMap<Pos, usize> = new
        .wires
        .iter()
        .enumerate()
        .filter(|(w, _)| !new_terms[*w].iter().any(names))
        .flat_map(|(w, wire)| wire.cells.iter().map(move |&p| (p, w)))
        .collect();
    let mut taken = vec![false; new.wires.len()];
    let mut out = vec![None; old.wires.len()];
    for (w, slot) in out.iter_mut().enumerate() {
        let terms: Vec<Term> =
            Structure::wire(from.0, old, w).terms.iter().filter(|t| names(t)).filter_map(|t| carried(t, map)).collect();
        let found = if terms.is_empty() && !Structure::wire(from.0, old, w).terms.iter().any(names) {
            let mut votes: HashMap<usize, usize> = HashMap::new();
            for &p in &old.wires[w].cells {
                if let Some(v) = cell(p).and_then(|q| unnamed.get(&q)) {
                    *votes.entry(*v).or_default() += 1;
                }
            }
            votes.into_iter().filter(|&(v, _)| !taken[v]).max_by_key(|&(v, n)| (n, std::cmp::Reverse(v))).map(|(v, _)| v)
        } else {
            let source = terms.iter().filter(|t| t.facing == shawline_grid::Dir::W).find_map(|t| owner.get(t));
            let by_source = source.copied().filter(|&v| !taken[v]);
            by_source.or_else(|| {
                let mut votes: HashMap<usize, usize> = HashMap::new();
                for t in &terms {
                    if let Some(&v) = owner.get(t) {
                        *votes.entry(v).or_default() += 1;
                    }
                }
                votes.into_iter().filter(|&(v, _)| !taken[v]).max_by_key(|&(v, n)| (n, std::cmp::Reverse(v))).map(|(v, _)| v)
            })
        };
        if let Some(v) = found {
            taken[v] = true;
            *slot = Some(v);
        }
    }
    out
}
