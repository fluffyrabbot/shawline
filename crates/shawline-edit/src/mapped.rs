//! Maps for the edits that existed before [`Edit`](crate::Edit), built by
//! place (PLAN P10, "Oracle first"). Wiring edits leave every kept box and
//! text where it was; a move shifts the moved box and what it holds.

use shawline_doc::Doc;
use shawline_grid::{Pos, Rect};
use shawline_shape::Diagram;
use shawline_shape::structure::Mapping;

use crate::Edited;
use crate::canon::Sheets;

/// The diagram a map starts from: the input's canonical revision, which
/// every edit draws on.
fn before(doc: &Doc) -> &Diagram {
    doc.canonical().and_then(Doc::diagram).expect("an edit succeeded, so its input reads")
}

fn after(doc: &Doc) -> &Diagram {
    doc.diagram().expect("an edit returns a sheet that reads")
}

/// Every box and text that is still there stays where it was.
pub(crate) fn kept(old: &Doc, new: Doc) -> Edited {
    let map = Mapping::by_place(before(old), after(&new), |_, r| r, |_, p| p);
    Edited { doc: new, map, tidied: tidied(old) }
}

/// The edit drew on the formatted text, not the one given.
fn tidied(doc: &Doc) -> bool {
    doc.canonical().is_some_and(|c| c.text() != doc.text())
}

/// The box at given cell `corner` moved by `dr`, `dc`, with what it holds.
pub(crate) fn moved(old: &Doc, corner: Pos, dr: isize, dc: isize, new: Doc) -> Edited {
    let sheets = Sheets::read(old, corner).expect("the move read this sheet");
    let index = sheets
        .given()
        .1
        .boxes
        .iter()
        .position(|b| (b.rect.top, b.rect.left) == (corner.row, corner.col))
        .expect("the move found this box");
    let held = sheets.canon().1.boxes[index].rect;
    let shift = |p: Pos| Pos::new(p.row.wrapping_add_signed(dr), p.col.wrapping_add_signed(dc));
    let shifted = |p: Pos| if held.contains(p) { shift(p) } else { p };
    let mut map = Mapping::by_place(
        before(old),
        after(&new),
        |_, r| {
            if held.encloses(&r) || held == r {
                let (a, b) = (shift(Pos::new(r.top, r.left)), shift(Pos::new(r.bottom, r.right)));
                Rect { top: a.row, left: a.col, bottom: b.row, right: b.col }
            } else {
                r
            }
        },
        |_, p| shifted(p),
    );
    map.map_wires(before(old), after(&new), shifted);
    Edited { doc: new, map, tidied: tidied(old) }
}
