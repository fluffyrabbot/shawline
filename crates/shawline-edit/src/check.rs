//! The check on every edit (the contract's module docs): the result must
//! recognize to the same boxes, wire connections and text adjacency as the
//! input, allowing only for the deliberate move.
//!
//! Positions may change, so boxes and texts are matched by where the move
//! should have put them, and everything else is compared by those matches.

use std::collections::{HashMap, HashSet};

use shawline_grid::{Dir, Grid, Pos, Rect};
use shawline_shape::{BoxStyle, Diagram, EndKind, PortKind, TextKind};

use crate::sheet::{across, text_cells};

/// What a move shifts: some boxes with everything inside them, plus a few
/// texts outside (a definition's output names).
pub(crate) struct Shift {
    pub dr: isize,
    pub dc: isize,
    /// Indices of the moved boxes in the input diagram.
    pub boxes: HashSet<usize>,
    /// The moved box's rectangle in the input.
    pub rect: Rect,
    /// Indices of moved texts outside `rect`.
    pub texts: HashSet<usize>,
}

impl Shift {
    pub(crate) fn pos(&self, p: Pos) -> Pos {
        Pos::new(p.row.wrapping_add_signed(self.dr), p.col.wrapping_add_signed(self.dc))
    }

    pub(crate) fn rect(&self, r: Rect) -> Rect {
        let (a, b) = (self.pos(Pos::new(r.top, r.left)), self.pos(Pos::new(r.bottom, r.right)));
        Rect { top: a.row, left: a.col, bottom: b.row, right: b.col }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Target {
    Text(usize),
    Stroke,
    Nothing,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum EndPrint {
    Port(usize, usize),
    Arrow(Target),
    Open(Target),
    Blocked,
}

#[derive(Debug, PartialEq, Eq)]
struct BoxPrint {
    style: BoxStyle,
    titled: bool,
    slips: usize,
    parent: Option<usize>,
    ports: Vec<(Dir, PortKind, Dir, Target)>,
    dividers: Vec<bool>,
}

#[derive(Debug, PartialEq, Eq)]
struct TextPrint {
    kind: TextKind,
    content: String,
    owner: Option<usize>,
    title_of: Option<usize>,
    divider_of: Option<usize>,
}

type WirePrint = (Vec<(Dir, EndPrint)>, usize);

/// One side of the comparison, with indices already in the result's terms.
#[derive(Debug, PartialEq, Eq)]
struct Print {
    boxes: Vec<BoxPrint>,
    texts: Vec<TextPrint>,
    wires: Vec<WirePrint>,
}

/// `Ok` if `new` is `old` with `shift` applied; otherwise a sentence
/// saying what changed.
pub(crate) fn same(
    old: (&Grid, &Diagram),
    new: (&Grid, &Diagram),
    shift: &Shift,
) -> Result<(), &'static str> {
    let (og, od) = old;
    let (ng, nd) = new;
    if od.boxes.len() != nd.boxes.len() {
        return Err("it would change the number of boxes");
    }
    if od.texts.len() != nd.texts.len() {
        return Err("it would change the text on the sheet");
    }
    let rect_index: HashMap<Rect, usize> =
        nd.boxes.iter().enumerate().map(|(i, b)| (b.rect, i)).collect();
    let mut box_map = Vec::with_capacity(od.boxes.len());
    for (i, b) in od.boxes.iter().enumerate() {
        let want = if shift.boxes.contains(&i) { shift.rect(b.rect) } else { b.rect };
        box_map.push(*rect_index.get(&want).ok_or("it would break a box")?);
    }

    let text_index: HashMap<Pos, usize> =
        nd.texts.iter().enumerate().map(|(i, t)| (t.pos, i)).collect();
    let mut text_map = Vec::with_capacity(od.texts.len());
    for (i, t) in od.texts.iter().enumerate() {
        let moved = shift.texts.contains(&i) || shift.rect.contains(t.pos);
        let want = if moved { shift.pos(t.pos) } else { t.pos };
        text_map.push(*text_index.get(&want).ok_or("it would separate text from its place")?);
    }

    let ident: Vec<usize> = (0..nd.boxes.len()).collect();
    let ident_text: Vec<usize> = (0..nd.texts.len()).collect();
    let before = print(og, od, &box_map, &text_map);
    let after = print(ng, nd, &ident, &ident_text);
    if before.boxes != after.boxes {
        return Err("it would change a box or what its ports meet");
    }
    if before.texts != after.texts {
        return Err("it would move text out of its box");
    }
    if before.wires != after.wires {
        return Err("it would change what a wire connects");
    }
    Ok(())
}

/// The structure of one diagram, with box and text indices mapped through
/// `boxes` and `texts`, and orders made position-free.
fn print(grid: &Grid, d: &Diagram, boxes: &[usize], texts: &[usize]) -> Print {
    let cells = text_cells(d);
    let target = |p: Pos, dir: Dir| -> Target {
        if !dir.is_horizontal() {
            return Target::Nothing;
        }
        let (gap, text) = across(grid, &cells, p, dir);
        match text {
            Some(i) => Target::Text(texts[i]),
            None => {
                let next = gap.last().copied().unwrap_or(p).step(dir);
                if next.is_some_and(|q| d.strokes.get(q).is_some()) {
                    Target::Stroke
                } else {
                    Target::Nothing
                }
            }
        }
    };
    let port_index: HashMap<Pos, (usize, usize)> = d
        .boxes
        .iter()
        .enumerate()
        .flat_map(|(b, shape)| shape.ports.iter().enumerate().map(move |(i, p)| (p.pos, (b, i))))
        .collect();

    let mut box_prints: Vec<(usize, BoxPrint)> = d
        .boxes
        .iter()
        .enumerate()
        .map(|(i, b)| {
            let ports = b
                .ports
                .iter()
                .map(|p| {
                    let t = match p.kind {
                        PortKind::Arrow => target(p.pos, p.dir),
                        PortKind::Tee => Target::Nothing,
                    };
                    (p.side, p.kind, p.dir, t)
                })
                .collect();
            let print = BoxPrint {
                style: b.style,
                titled: b.title_gap.is_some(),
                slips: b.slips.len(),
                parent: b.parent.map(|p| boxes[p]),
                ports,
                dividers: b.dividers.iter().map(|d| d.gap.is_some()).collect(),
            };
            (boxes[i], print)
        })
        .collect();
    box_prints.sort_by_key(|&(i, _)| i);

    let mut text_prints: Vec<(usize, TextPrint)> = d
        .texts
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let print = TextPrint {
                kind: t.kind,
                content: t.content.clone(),
                owner: t.owner.map(|b| boxes[b]),
                title_of: t.title_of.map(|b| boxes[b]),
                divider_of: t.divider_of.map(|b| boxes[b]),
            };
            (texts[i], print)
        })
        .collect();
    text_prints.sort_by_key(|&(i, _)| i);

    let mut wires: Vec<WirePrint> = d
        .wires
        .iter()
        .map(|w| {
            let mut ends: Vec<(Dir, EndPrint)> = w
                .ends
                .iter()
                .map(|e| {
                    let print = match e.kind {
                        EndKind::Port(p) => port_index
                            .get(&p)
                            .map_or(EndPrint::Blocked, |&(b, i)| EndPrint::Port(boxes[b], i)),
                        EndKind::Arrow => EndPrint::Arrow(target(e.pos, e.facing)),
                        EndKind::Open => EndPrint::Open(target(e.pos, e.facing)),
                        EndKind::Blocked => EndPrint::Blocked,
                    };
                    (e.facing, print)
                })
                .collect();
            ends.sort();
            (ends, w.tees.len())
        })
        .collect();
    wires.sort();

    Print {
        boxes: box_prints.into_iter().map(|(_, p)| p).collect(),
        texts: text_prints.into_iter().map(|(_, p)| p).collect(),
        wires,
    }
}
