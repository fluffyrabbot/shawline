//! The check on a move (the contract's module docs): the result must have
//! the input's [`Structure`], with each box and text named by where the move
//! should have put it.
//!
//! The move re-routes the stretches of wire that follow the box, so
//! crossings may come and go, but tees are the author's and stay
//! ([`Routing::Rerouted`]).

use std::collections::HashSet;

use shawline_grid::{Grid, Pos, Rect};
use shawline_shape::Diagram;
use shawline_shape::structure::{Mapping, Part, Routing, Structure};

/// Why an edit is refused when its result's diagnostics differ.
pub(crate) const DIAGNOSTICS: &str = "it would change what the program means";

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
    let map = Mapping::by_place(
        od,
        nd,
        |i, r| if shift.boxes.contains(&i) { shift.rect(r) } else { r },
        |i, p| if shift.texts.contains(&i) || shift.rect.contains(p) { shift.pos(p) } else { p },
    );
    if map.boxes.contains(&None) {
        return Err("it would break a box");
    }
    if map.texts.contains(&None) {
        return Err("it would separate text from its place");
    }
    let expected = Structure::of(og, od).relabel(&map);
    expected.expect(&Structure::of(ng, nd), Routing::Rerouted).map_err(|part| match part {
        Part::Boxes => "it would change a box or what its ports meet",
        Part::Texts => "it would move text out of its box",
        Part::Wires => "it would change what a wire connects",
        Part::Codes => DIAGNOSTICS,
    })
}
