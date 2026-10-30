//! P10 wave 1: rows and columns, text edits, making room, whole previews,
//! compound gestures, exact maps and refusal remedies.

use shawline_doc::Doc;
use shawline_grid::Pos;
use shawline_shape::structure::{Routing, Structure};

use crate::{
    Anchor, Axis, Blockage, Edit, Edited, Insertion, RefusalKind, Remedy, apply, branch, connect, delete, disconnect,
    insert, make_room, move_box, place_connected, place_text, preview, remove_text, remove_wire, rewire, set_text,
};

const ARITH: &str = include_str!("../../../corpus/02-arith.shaw");
const DOUBLE: &str = include_str!("../../../corpus/03-double.shaw");
const DOUBLE_ASCII: &str = include_str!("../../../corpus/03-double-ascii.shaw");
const ABS: &str = include_str!("../../../corpus/04-abs.shaw");
const GUARDS: &str = include_str!("../../../corpus/12-guards.shaw");

fn ok(result: Result<Edited, crate::Refusal>) -> Edited {
    result.unwrap_or_else(|r| panic!("refused: {r:?}"))
}

fn line(text: &str, row: usize) -> &str {
    text.lines().nth(row).unwrap()
}

/// main's outputs, as printed.
fn outputs(text: &str) -> Vec<String> {
    let doc = Doc::new(text);
    doc.run().unwrap().unwrap().iter().map(ToString::to_string).collect()
}

/// The edit's result is canonical and has the input's structure, relabelled
/// through its map, routing kept.
fn keeps_structure(before: &str, edited: &Edited) {
    let text = edited.doc.text();
    assert_eq!(Doc::new(text).formatted(), Some(text), "not canonical:\n{text}");
    let old = Doc::new(before);
    let old = old.canonical().unwrap();
    let expected = Structure::of(old.grid().unwrap(), old.diagram().unwrap()).relabel(&edited.map);
    let found = Structure::of(edited.doc.grid().unwrap(), edited.doc.diagram().unwrap());
    assert_eq!(expected.expect(&found, Routing::Kept), Ok(()), "{text}");
}

// ---------------------------------------------------------------------------
// Rows and columns.

fn round_trip(src: &str, i: Insertion) -> String {
    let inserted = ok(insert(&Doc::new(src), i));
    keeps_structure(src, &inserted);
    assert_eq!(outputs(inserted.doc.text()), outputs(src));
    let deleted = ok(delete(&inserted.doc, i));
    assert_eq!(deleted.doc.text(), src, "delete isn't insert's inverse");
    inserted.doc.text().to_owned()
}

#[test]
fn a_row_stretches_every_stroke_across_it() {
    let text = round_trip(DOUBLE, Insertion { axis: Axis::Row, at: Pos::new(5, 0), count: 2 });
    assert_eq!(line(&text, 5), "│         │   │       │");
    assert_eq!(line(&text, 7), "▶ 𐑯 ──┬───▶ 𐑐 ├───────▶ 𐑬");
    // Between two input arrowheads, the node's border runs on.
    let text = round_trip(ABS, Insertion { axis: Axis::Row, at: Pos::new(24, 0), count: 1 });
    assert_eq!(line(&text, 24), "│     │   │     │     │       │");
}

#[test]
fn a_column_steps_round_text_and_the_gap_before_it() {
    // Column 11 falls between 𐑐's arrowhead and its word, so on that row
    // the cut moves past the word; the node grows on its far side.
    let text = round_trip(DOUBLE, Insertion { axis: Axis::Column, at: Pos::new(2, 11), count: 1 });
    assert_eq!(line(&text, 4), "│         ┌────┐       │");
    assert_eq!(line(&text, 5), "▶ 𐑯 ──┬───▶ 𐑐  ├───────▶ 𐑬");
    // Column 3 falls between `21` and its wire: the wire stretches.
    let text = round_trip(DOUBLE, Insertion { axis: Axis::Column, at: Pos::new(11, 4), count: 1 });
    assert_eq!(line(&text, 14), "│ 21 ────▶ 𐑛𐑳𐑚𐑩𐑤 ├─────▶ 𐑬");
    // A title's rule stretches too, and a switch's divider stays dashed.
    let text = round_trip(ABS, Insertion { axis: Axis::Column, at: Pos::new(2, 17), count: 2 });
    assert_eq!(line(&text, 2), "╭─ ·𐑨𐑚𐑕 ────────────────────────────╮");
    assert_eq!(line(&text, 11), "│     │     ├┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┤  │    │");
}

#[test]
fn a_column_leaves_other_definitions_alone() {
    let text = round_trip(DOUBLE, Insertion { axis: Axis::Column, at: Pos::new(2, 22), count: 3 });
    assert_eq!(line(&text, 2), "╭─ ·𐑛𐑳𐑚𐑩𐑤 ───────────────╮");
    assert_eq!(line(&text, 5), "▶ 𐑯 ──┬───▶ 𐑐 ├──────────▶ 𐑬");
    assert_eq!(line(&text, 11), line(DOUBLE, 11));
}

#[test]
fn rows_and_columns_stay_inside_a_definition() {
    let doc = Doc::new(DOUBLE);
    let outside = insert(&doc, Insertion { axis: Axis::Row, at: Pos::new(0, 0), count: 1 }).unwrap_err();
    assert_eq!(outside.kind, RefusalKind::Target);
    let border = insert(&doc, Insertion { axis: Axis::Row, at: Pos::new(2, 0), count: 1 }).unwrap_err();
    assert_eq!(border.kind, RefusalKind::Target);
    let none = insert(&doc, Insertion { axis: Axis::Row, at: Pos::new(5, 0), count: 0 }).unwrap_err();
    assert_eq!(none.kind, RefusalKind::Target);
}

#[test]
fn only_blank_cells_and_straight_strokes_are_deleted() {
    let doc = Doc::new(DOUBLE);
    // Row 5 holds the input's name and the node's word.
    let text = delete(&doc, Insertion { axis: Axis::Row, at: Pos::new(5, 0), count: 1 }).unwrap_err();
    assert_eq!(text.kind, RefusalKind::Target);
    assert!(text.reason.contains("row 6"), "{text:?}");
    // A blank row inside a definition goes.
    let gone = ok(delete(&doc, Insertion { axis: Axis::Row, at: Pos::new(3, 0), count: 1 }));
    keeps_structure(DOUBLE, &gone);
    assert_eq!(line(gone.doc.text(), 3), "│         ┌───┐       │");
}

#[test]
fn an_insertion_on_untidy_text_is_made_on_the_formatted_sheet() {
    let doc = Doc::new(DOUBLE_ASCII);
    let i = Insertion { axis: Axis::Column, at: Pos::new(2, 11), count: 1 };
    let edited = ok(insert(&doc, i));
    assert!(edited.tidied);
    let canonical = ok(insert(&Doc::new(doc.formatted().unwrap()), i));
    assert_eq!(edited.doc.text(), canonical.doc.text());
    assert!(!canonical.tidied);
}

// ---------------------------------------------------------------------------
// Text.

#[test]
fn a_constant_grows_and_shrinks_at_its_left_end() {
    let doc = Doc::new(ARITH);
    let grown = ok(set_text(&doc, Pos::new(5, 3), "600"));
    keeps_structure_but(ARITH, &grown, "600");
    assert_eq!(line(grown.doc.text(), 5), "│600 ───▶ 𐑑 ├─────▶ 𐑢𐑳𐑯");
    let back = ok(set_text(&grown.doc, Pos::new(5, 1), "6"));
    assert_eq!(back.doc.text(), ARITH);
    // No room for a fourth digit before the border.
    let full = set_text(&grown.doc, Pos::new(5, 1), "6000").unwrap_err();
    assert_eq!((full.kind, full.remedy), (RefusalKind::Blocked(Blockage::NoRoom), Some(Remedy::MakeRoom)));
    // It stays a constant.
    let word = set_text(&doc, Pos::new(5, 3), "𐑯").unwrap_err();
    assert_eq!(word.kind, RefusalKind::Target);
}

/// [`keeps_structure`], with the one retexted text changed.
fn keeps_structure_but(before: &str, edited: &Edited, content: &str) {
    let old = Doc::new(before);
    let expected = Structure::of(old.grid().unwrap(), old.diagram().unwrap()).relabel(&edited.map);
    let found = Structure::of(edited.doc.grid().unwrap(), edited.doc.diagram().unwrap());
    let differing: Vec<usize> = (0..found.texts.len()).filter(|&j| expected.texts[j] != found.texts[j]).collect();
    assert_eq!(differing.len(), 1, "{}", edited.doc.text());
    assert_eq!(found.texts[differing[0]].content, content);
    assert!(edited.map.is_total());
}

#[test]
fn a_name_grows_at_its_right_end_and_its_wire_gives_way() {
    let doc = Doc::new(DOUBLE);
    let renamed = ok(set_text(&doc, Pos::new(5, 2), "𐑯𐑳"));
    assert_eq!(line(renamed.doc.text(), 5), "▶ 𐑯𐑳 ─┬───▶ 𐑐 ├───────▶ 𐑬");
    keeps_structure_but(DOUBLE, &renamed, "𐑯𐑳");
    let back = ok(set_text(&renamed.doc, Pos::new(5, 3), "𐑯"));
    assert_eq!(back.doc.text(), DOUBLE);
    // The wire keeps at least a stroke before its tee.
    let long = set_text(&doc, Pos::new(5, 2), "𐑯𐑳𐑥").unwrap_err();
    assert_eq!(long.kind, RefusalKind::Blocked(Blockage::NoRoom));
}

#[test]
fn a_title_grows_into_its_rule() {
    let doc = Doc::new(DOUBLE);
    let renamed = ok(set_text(&doc, Pos::new(2, 3), "·𐑛𐑳𐑚𐑩𐑤𐑦𐑙"));
    assert_eq!(line(renamed.doc.text(), 2), "╭─ ·𐑛𐑳𐑚𐑩𐑤𐑦𐑙 ──────────╮");
    let back = ok(set_text(&renamed.doc, Pos::new(2, 3), "·𐑛𐑳𐑚𐑩𐑤"));
    assert_eq!(back.doc.text(), DOUBLE);
    let plain = set_text(&doc, Pos::new(2, 3), "𐑛𐑳𐑚𐑩𐑤").unwrap_err();
    assert_eq!(plain.kind, RefusalKind::Target);
}

#[test]
fn a_guard_keeps_its_dashes() {
    let doc = Doc::new(GUARDS);
    let edited = ok(set_text(&doc, Pos::new(8, 5), "22"));
    assert_eq!(line(edited.doc.text(), 8), "│ ├┄ 22 ┄┄┄┄┄┄┄┄┄┄┤  │    │");
}

#[test]
fn placed_text_stays_clear_of_wires_until_connected() {
    let doc = Doc::new(ARITH);
    // Row 3 is blank inside main: a constant there is stray until wired.
    let placed = ok(place_text(&doc, Pos::new(3, 3), "9"));
    assert_eq!(line(placed.doc.text(), 3), "│  9              │");
    assert!(placed.doc.diagnostics().iter().any(|d| d.code == "Y012"));
    // Before a wire's open end it would meet it, and become its constant.
    let dangling = Doc::new(ARITH.replacen("│  6 ───▶", "│    ───▶", 1));
    let near = place_text(&dangling, Pos::new(5, 3), "4").unwrap_err();
    assert_eq!(near.kind, RefusalKind::Blocked(Blockage::Spacing), "{near:?}");
    // Before an unwired node input it reads nothing, and waits for a wire.
    let unwired = ok(remove_wire(&doc, Pos::new(5, 6)));
    let cut = ok(remove_text(&unwired.doc, Pos::new(5, 3)));
    let waiting = ok(place_text(&cut.doc, Pos::new(5, 3), "4"));
    assert_eq!(line(waiting.doc.text(), 5), "│  4    ▶ 𐑑 ├─────▶ 𐑢𐑳𐑯");
    let wired = ok(connect(&waiting.doc, Pos::new(5, 3), Pos::new(5, 8)));
    assert_eq!(line(wired.doc.text(), 5), "│  4 ───▶ 𐑑 ├─────▶ 𐑢𐑳𐑯");
    // A top-level comment on an empty sheet.
    let empty = ok(place_text(&Doc::new(""), Pos::new(0, 0), "※ 𐑛𐑳𐑚𐑩𐑤"));
    assert_eq!(empty.doc.text(), "※ 𐑛𐑳𐑚𐑩𐑤\n");
    let word = place_text(&Doc::new(""), Pos::new(0, 0), "𐑛𐑳𐑚𐑩𐑤").unwrap_err();
    assert_eq!(word.kind, RefusalKind::Target);
}

#[test]
fn removing_a_constant_takes_its_wire() {
    let doc = Doc::new(ARITH);
    let gone = ok(remove_text(&doc, Pos::new(5, 3)));
    assert_eq!(line(gone.doc.text(), 5), "│       ▶ 𐑑 ├─────▶ 𐑢𐑳𐑯");
    // The constant and its wire map to nothing; everything else maps.
    let old = doc.diagram().unwrap();
    let t = old.texts.iter().position(|t| t.pos == Pos::new(5, 3)).unwrap();
    assert_eq!(gone.map.texts.iter().filter(|m| m.is_none()).count(), 1);
    assert_eq!(gone.map.texts[t], None);
    assert_eq!(gone.map.wires.iter().filter(|m| m.is_none()).count(), 1);
    let name = remove_text(&doc, Pos::new(5, 19)).unwrap_err();
    assert_eq!(name.kind, RefusalKind::Target);
    let comment = ok(remove_text(&doc, Pos::new(0, 2)));
    assert!(comment.doc.text().starts_with("╭─ ·𐑥𐑱𐑯"), "{}", comment.doc.text());
}

// ---------------------------------------------------------------------------
// Making room, and what refusals offer.

#[test]
fn make_room_grows_what_a_move_needs() {
    let doc = Doc::new(ABS);
    let edit = Edit::Move { corner: Pos::new(22, 6), dr: 1, dc: 0 };
    let refused = apply(&doc, &edit).unwrap_err();
    assert_eq!((refused.kind, refused.remedy), (RefusalKind::Blocked(Blockage::NoRoom), Some(Remedy::MakeRoom)));
    let roomed = make_room(&doc, &edit).unwrap_or_else(|r| panic!("{r:?}"));
    assert!(!roomed.inserted.is_empty());
    assert!(roomed.edited.map.is_total());
    assert_eq!(outputs(roomed.edited.doc.text()), outputs(ABS));
    let old = doc.diagram().unwrap();
    let b = old.boxes.iter().position(|b| (b.rect.top, b.rect.left) == (22, 6)).unwrap();
    let new = roomed.edited.doc.diagram().unwrap();
    let r = new.boxes[roomed.edited.map.boxes[b].unwrap()].rect;
    assert_eq!((r.top, r.left), (23, 6), "{}", roomed.edited.doc.text());
    // As data, the same.
    let applied = ok(apply(&doc, &Edit::MakeRoom(Box::new(edit))));
    assert_eq!(applied.doc.text(), roomed.edited.doc.text());
}

#[test]
fn make_room_grows_a_switch_by_what_a_move_overshoots() {
    let src = include_str!("../../../corpus/10-countdown.shaw");
    let doc = Doc::new(src);
    let edit = Edit::Move { corner: Pos::new(12, 17), dr: 1, dc: 0 };
    let refused = apply(&doc, &edit).unwrap_err();
    assert!(refused.reason.contains("leave its switch"), "{refused:?}");
    assert_eq!(refused.remedy, Some(Remedy::MakeRoom));
}

#[test]
fn an_edge_offers_no_room() {
    let doc = Doc::new(ABS);
    // Up across a divider: inserting rows pushes the divider along.
    let arm = move_box(&doc, Pos::new(12, 18), -1, 0).unwrap_err();
    assert_eq!((arm.kind, arm.remedy), (RefusalKind::Blocked(Blockage::Edge), None));
    let roomed = make_room(&doc, &Edit::Move { corner: Pos::new(12, 18), dr: -1, dc: 0 }).unwrap_err();
    assert_eq!((roomed.at, roomed.kind, roomed.remedy), (arm.at, arm.kind, None));
    // Up onto a box: the same.
    let onto = move_box(&doc, Pos::new(25, 16), -1, 0).unwrap_err();
    assert_eq!((onto.kind, onto.remedy), (RefusalKind::Blocked(Blockage::Edge), None));
    // Past the sheet's top.
    let top = move_box(&Doc::new(DOUBLE), Pos::new(2, 0), -3, 0).unwrap_err();
    assert_eq!(top.kind, RefusalKind::Blocked(Blockage::Edge));
}

#[test]
fn make_room_succeeds_at_once_when_the_edit_fits() {
    let doc = Doc::new(DOUBLE);
    let edit = Edit::Move { corner: Pos::new(4, 10), dr: 1, dc: 0 };
    let roomed = make_room(&doc, &edit).unwrap();
    assert!(roomed.inserted.is_empty());
    assert_eq!(roomed.edited.doc.text(), ok(apply(&doc, &edit)).doc.text());
}

// ---------------------------------------------------------------------------
// Previews and compound gestures.

#[test]
fn a_preview_runs_from_the_source_to_the_sink() {
    let gone = ok(remove_wire(&Doc::new(DOUBLE), Pos::new(14, 5))).doc;
    let p = preview(&gone, Anchor::Source(Pos::new(14, 2)), Pos::new(14, 8)).unwrap();
    assert!(!p.joins_wire);
    // From `21`'s last cell, over the gap, to the arrowhead.
    assert_eq!(p.path.first(), Some(&Pos::new(14, 3)));
    assert_eq!(p.path.last(), Some(&Pos::new(14, 8)));
    assert_eq!(p.path.len(), 6);
    // Exactly what connect draws, between the ends.
    let wired = ok(connect(&gone, Pos::new(14, 2), Pos::new(14, 8))).doc;
    for p in &p.path[2..p.path.len() - 1] {
        assert_eq!(wired.grid().unwrap().get(*p), '─', "{p}");
    }
}

#[test]
fn a_preview_from_a_wired_source_starts_at_the_new_tee() {
    let cut = ok(disconnect(&Doc::new(DOUBLE), Pos::new(6, 10))).doc;
    let p = preview(&cut, Anchor::Source(Pos::new(5, 0)), Pos::new(6, 10)).unwrap();
    assert!(p.joins_wire);
    let wired = ok(connect(&cut, Pos::new(5, 0), Pos::new(6, 10))).doc;
    let tee = wired.grid().unwrap().get(p.path[0]);
    assert!(matches!(tee, '┬' | '┴' | '├' | '┤'), "{tee} at {}", p.path[0]);
    assert_eq!(p.path.last(), Some(&Pos::new(6, 10)));
}

#[test]
fn a_preview_from_an_unwired_sink() {
    let cut = ok(disconnect(&Doc::new(DOUBLE), Pos::new(6, 10))).doc;
    // Towards the source: the same path as from the source.
    let to_source = preview(&cut, Anchor::Sink(Pos::new(6, 10)), Pos::new(5, 0)).unwrap();
    let from_source = preview(&cut, Anchor::Source(Pos::new(5, 0)), Pos::new(6, 10)).unwrap();
    assert_eq!(to_source, from_source);
    // Towards a blank cell: from there to the sink.
    let blank = preview(&cut, Anchor::Sink(Pos::new(6, 10)), Pos::new(7, 3)).unwrap();
    assert_eq!((blank.path[0], *blank.path.last().unwrap()), (Pos::new(7, 3), Pos::new(6, 10)));
    assert!(!blank.joins_wire);
    // A wired sink anchors nothing.
    assert_eq!(preview(&Doc::new(DOUBLE), Anchor::Sink(Pos::new(6, 10)), Pos::new(7, 3)), None);
}

#[test]
fn rewire_moves_a_branch_whole_or_not_at_all() {
    let doc = Doc::new(ARITH);
    // 𐑑's second input takes 6 instead of 7, which is left stray.
    let moved = ok(rewire(&doc, Pos::new(5, 3), Pos::new(6, 8)));
    let sources = crate::wire_sources(&moved.doc);
    assert_eq!(sources.iter().filter(|s| **s == Some(Pos::new(5, 3))).count(), 1);
    assert!(!sources.contains(&Some(Pos::new(6, 3))));
    assert!(moved.doc.diagnostics().iter().any(|d| d.code == "Y012"), "{}", moved.doc.text());
    // From a source in no scope with the sink: nothing changes.
    let far = rewire(&doc, Pos::new(0, 2), Pos::new(6, 8)).unwrap_err();
    assert_eq!(far.kind, RefusalKind::Target);
}

#[test]
fn place_connected_places_and_wires_one_node() {
    // A wire from 𐑑's output dropped below the sheet's nodes.
    let src = "\
╭─ ·𐑥𐑱𐑯 ──────────────────────╮
│                             │
│       ┌───┐                 │
│  6 ───▶ 𐑑 ├─────────────────▶ 𐑢𐑳𐑯
│  7 ───▶   │                 │
│       └───┘                 │
│                             │
│                             │
│                             │
╰─────────────────────────────╯
";
    let doc = Doc::new(src);
    let edited = ok(place_connected(&doc, Pos::new(7, 16), "𐑦", 1, 1, Pos::new(3, 12)));
    let text = edited.doc.text();
    assert!(text.contains("▶ 𐑦 ├"), "{text}");
    let new = edited.doc.diagram().unwrap();
    assert_eq!(new.boxes.len(), 3);
    assert!(edited.map.is_total());
    // A word with no input can't be wired in.
    let none = place_connected(&doc, Pos::new(7, 16), "𐑯𐑦𐑤", 0, 1, Pos::new(3, 12)).unwrap_err();
    assert_eq!(none.kind, RefusalKind::Target);
}

// ---------------------------------------------------------------------------
// Maps.

#[test]
fn a_wire_is_mapped_through_every_edit() {
    let doc = Doc::new(DOUBLE);
    let d = doc.diagram().unwrap();
    let w = d.wires.iter().position(|w| w.cells.contains(&Pos::new(5, 4))).unwrap();
    // Cutting a branch keeps the wire, with other cells.
    let cut = ok(disconnect(&doc, Pos::new(6, 10)));
    let v = cut.map.wires[w].expect("the wire is still there");
    assert!(cut.doc.diagram().unwrap().wires[v].cells.contains(&Pos::new(5, 4)));
    // Connecting it again keeps it too.
    let again = ok(connect(&cut.doc, Pos::new(5, 0), Pos::new(6, 10)));
    assert!(again.map.wires[v].is_some());
    // Removing it maps it to nothing.
    let gone = ok(remove_wire(&doc, Pos::new(5, 4)));
    assert_eq!(gone.map.wires[w], None);
    assert_eq!(gone.map.wires.iter().filter(|m| m.is_some()).count(), d.wires.len() - 1);
    // A move re-routes the wires at the box and keeps every one.
    let moved = ok(move_box(&doc, Pos::new(4, 10), 1, 0));
    assert!(moved.map.wires.iter().all(Option::is_some), "{:?}", moved.map);
    // An insertion keeps every wire.
    let wide = ok(insert(&doc, Insertion { axis: Axis::Column, at: Pos::new(2, 8), count: 2 }));
    assert!(wide.map.wires.iter().all(Option::is_some));
}

#[test]
fn branch_names_the_cells_of_the_text_as_given() {
    let canonical = branch(&Doc::new(DOUBLE), Pos::new(6, 10));
    let ascii = branch(&Doc::new(DOUBLE_ASCII), Pos::new(6, 10));
    assert_eq!(ascii, canonical);
    assert!(canonical.contains(&Pos::new(6, 6)) && canonical.contains(&Pos::new(5, 6)), "{canonical:?}");
}

// ---------------------------------------------------------------------------
// Feeding an unwired arrowhead.

/// A node whose input nothing feeds yet, with room to its left.
const HUNGRY: &str = "\
╭─ ·𐑥𐑱𐑯 ──────────────────────╮
│                             │
│                ┌───┐        │
│                ▶ 𐑦 ├────────▶ 𐑢𐑳𐑯
│                └───┘        │
│                             │
╰─────────────────────────────╯
";

#[test]
fn feeding_a_constant_writes_it_wired() {
    // 7's text and wire go; feeding 7 back gives the program back.
    let cut = ok(remove_text(&Doc::new(ARITH), Pos::new(6, 3)));
    let fed = ok(crate::feed(&cut.doc, Pos::new(6, 8), &crate::NewSource::Constant("7".to_owned())));
    assert_eq!(fed.doc.text(), ARITH);
    assert!(fed.map.is_total());
    let fed = ok(crate::feed(&Doc::new(HUNGRY), Pos::new(3, 17), &crate::NewSource::Constant("«𐑣𐑦»".to_owned())));
    assert_eq!(line(fed.doc.text(), 3), "│        «𐑣𐑦» ───▶ 𐑦 ├────────▶ 𐑢𐑳𐑯");
    assert!(!fed.doc.diagnostics().iter().any(|d| d.code == "Y019" || d.code == "Y012"));
    // Only a constant, and only into an unwired arrowhead.
    let word = crate::feed(&Doc::new(HUNGRY), Pos::new(3, 17), &crate::NewSource::Constant("𐑦".to_owned()));
    assert_eq!(word.unwrap_err().kind, RefusalKind::Target);
    let wired = crate::feed(&Doc::new(ARITH), Pos::new(6, 8), &crate::NewSource::Constant("7".to_owned()));
    assert_eq!(wired.unwrap_err().kind, RefusalKind::Target);
}

#[test]
fn feeding_a_node_places_it_wired() {
    let nil = crate::NewSource::Node { word: "𐑯𐑦𐑤".to_owned(), inputs: 0, outputs: 1 };
    let fed = ok(crate::feed(&Doc::new(HUNGRY), Pos::new(3, 17), &nil));
    let text = fed.doc.text();
    assert!(text.contains("│      │ 𐑯𐑦𐑤 ├───▶ 𐑦 ├"), "{text}");
    assert!(!fed.doc.diagnostics().iter().any(|d| d.code == "Y019"), "{text}");
    assert!(fed.map.is_total());
    assert_eq!(fed.doc.diagram().unwrap().boxes.len(), 3);
    // No room left of the arrowhead: refused whole, offering room.
    let cut = ok(remove_text(&Doc::new(ARITH), Pos::new(6, 3)));
    let cramped = crate::feed(&cut.doc, Pos::new(6, 8), &nil).unwrap_err();
    assert_eq!((cramped.kind, cramped.remedy), (RefusalKind::Blocked(Blockage::NoRoom), Some(Remedy::MakeRoom)));
}
