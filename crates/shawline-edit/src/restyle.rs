//! Redrawing a box's border without changing what it holds: its corners
//! rounded or square (the repair for `Y014`, `Y015` and `Y018`), and every
//! slipped border cell moved home (the repair for `S012`, as `shawline fmt
//! --fix` does it).

use shawline_doc::Doc;
use shawline_grid::Pos;
use shawline_shape::BoxStyle;
use shawline_shape::structure::{Routing, Structure};

use crate::canon::Sheets;
use crate::cut::render;
use crate::drawing::{by_cell, put, rows, settle};
use crate::mapped;
use crate::model::Model;
use crate::{Edited, Refusal, RefusalKind};

pub(crate) fn restyle(doc: &Doc, corner: Pos, rounded: bool) -> Result<Edited, Refusal> {
    let sheets = Sheets::read(doc, corner)?;
    let (g, d) = sheets.given();
    let b = Model::new(g, d).box_at(corner).ok_or_else(|| {
        Refusal::new(corner, RefusalKind::Target, "there is no box with its top-left corner here")
    })?;
    let (grid, diagram) = sheets.canon();
    let shape = &diagram.boxes[b];
    let r = shape.rect;
    let homes = [
        Pos::new(r.top, r.left),
        Pos::new(r.top, r.right),
        Pos::new(r.bottom, r.left),
        Pos::new(r.bottom, r.right),
    ];
    let glyphs = if rounded { ['╭', '╮', '╰', '╯'] } else { ['┌', '┐', '└', '┘'] };
    let mut lines = rows(grid);
    for (home, ch) in homes.into_iter().zip(glyphs) {
        // A slipped corner is redrawn where it sits: moving it is `fix`'s.
        let cell = shape.slips.iter().find(|s| s.home == home).map_or(home, |s| s.at);
        put(&mut lines, cell, ch);
    }
    let style = if rounded { BoxStyle::Rounded } else { BoxStyle::Square };
    let what = "the corners can't be redrawn";
    settle(&sheets, render(&lines), corner, what, Routing::Kept, by_cell(Some), |expected, map, new| {
        let nb = map.boxes[b]?;
        expected.boxes[nb].style = style;
        // A box's style is what its place diagnoses, so the codes may go.
        expected.codes = Structure::of(new.0, new.1).codes;
        Some(())
    })
}

pub(crate) fn fix(doc: &Doc, at: Pos) -> Result<Edited, Refusal> {
    let refuse = |kind, why: &str| Refusal::new(at, kind, why);
    let unreadable = || refuse(RefusalKind::Unreadable, "the sheet has a character Shawline can't read; fix it first");
    let diagram = doc.diagram().ok_or_else(unreadable)?;
    if diagram.boxes.iter().all(|b| b.slips.is_empty()) {
        return Err(refuse(RefusalKind::Target, "no border cell has slipped off its line"));
    }
    let fixed = doc.fixed().ok_or_else(unreadable)?;
    let result = Doc::new(fixed.to_owned());
    let changed = |why: &str| refuse(RefusalKind::Changed, &format!("the slipped cells can't be moved home: {why}"));
    if result.formatted() != Some(result.text()) {
        return Err(changed("the sheet wouldn't stay tidy"));
    }
    let canonical = doc.canonical().ok_or_else(unreadable)?;
    let old = mapped::read(canonical);
    let new = mapped::read(&result);
    // A slipped box keeps its rectangle, so every part is where it was.
    let map = mapped::map(old, new, Some);
    if !map.is_total() {
        return Err(changed("a box or text would move"));
    }
    let mut expected = Structure::of(old.0, old.1).relabel(&map);
    for part in &mut expected.boxes {
        part.slips = 0;
    }
    expected
        .expect(&Structure::of(new.0, new.1), Routing::Ignored)
        .map_err(|_| changed("it would change what the drawing means"))?;
    Ok(Edited { doc: result, map, tidied: mapped::tidied(doc) })
}
