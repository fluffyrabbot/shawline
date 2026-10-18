//! The router's preferences, and one move per way a wire meets a box.

use shawline_grid::{Dir, Pos};

use crate::router::{self, BUDGET, CELL, Cell, Start, Terrain};
use crate::{move_box, route};

const DOUBLE: &str = include_str!("../../../corpus/03-double.shaw");
const ABS: &str = include_str!("../../../corpus/04-abs.shaw");

/// main's outputs, as printed.
fn outputs(src: &str) -> Vec<String> {
    let checked = shawline_cli::check(src).unwrap_or_else(|e| panic!("{e:?}"));
    let program = checked.program.unwrap_or_else(|| panic!("{:?}\n{src}", checked.diagnostics));
    shawline_cli::run(&program).unwrap().iter().map(ToString::to_string).collect()
}

/// Moves the box and checks the result is canonical and means the same.
fn moved(src: &str, corner: Pos, dr: isize, dc: isize) -> String {
    let text = move_box(src, corner, dr, dc).unwrap_or_else(|r| panic!("refused: {r:?}"));
    assert_eq!(shawline_cli::format(&text).unwrap(), text, "not canonical");
    assert_eq!(outputs(&text), outputs(src), "{text}");
    text
}

fn lines(text: &str, rows: std::ops::Range<usize>) -> String {
    text.lines().skip(rows.start).take(rows.len()).collect::<Vec<_>>().join("\n")
}

#[test]
fn route_runs_straight_when_it_can() {
    let blank = "        \n        \n        \n";
    let path = route(blank, Pos::new(1, 0), Pos::new(1, 7)).unwrap();
    assert_eq!(path, (0..8).map(|c| Pos::new(1, c)).collect::<Vec<_>>());
}

#[test]
fn route_detours_rather_than_cross() {
    // A short wire stands in the way. Going round it costs four bends and a
    // few cells, far less than a crossing.
    let src =
        "          \n          \n    │     \n    │     \n          \n          \n          \n";
    let path = route(src, Pos::new(2, 0), Pos::new(2, 9)).unwrap();
    assert!(!path.iter().any(|p| p.col == 4 && (2..=3).contains(&p.row)), "{path:?}");
    // Nor does it touch the wire's open ends.
    assert!(!path.contains(&Pos::new(1, 4)) && !path.contains(&Pos::new(4, 4)), "{path:?}");
    assert_eq!((path[0], path[path.len() - 1]), (Pos::new(2, 0), Pos::new(2, 9)));
}

#[test]
fn route_crosses_a_wire_it_cannot_go_round() {
    let src = "    │     \n    │     \n    │     \n";
    let path = route(src, Pos::new(1, 0), Pos::new(1, 9)).unwrap();
    assert_eq!(path, (0..10).map(|c| Pos::new(1, c)).collect::<Vec<_>>());
}

#[test]
fn route_gives_up_within_its_budget() {
    // A corridor: the route exists, but not within three expansions.
    struct Corridor;
    impl Terrain for Corridor {
        fn cell(&self, p: Pos) -> Cell {
            if p.row == 0 && p.col < 100 { Cell::Free } else { Cell::Blocked }
        }
        fn extra(&self, _: Pos) -> u32 {
            0
        }
        fn goal(&self, p: Pos, _: Dir) -> Option<u32> {
            (p == Pos::new(0, 100)).then_some(CELL)
        }
    }
    let start = Start { pos: Pos::new(0, 0), heading: None, straight: false };
    assert_eq!(router::search(&Corridor, start, 3), None);
    assert!(router::search(&Corridor, start, BUDGET).is_some());
    // A boxed-in cell has no route at all.
    let walled = "┌───┐\n│   │\n└───┘\n      \n";
    assert_eq!(route(walled, Pos::new(1, 2), Pos::new(3, 0)), None);
}

/// Constants feed the inputs and the output runs to the definition's port.
const ARITH: &str = "\
╭─ ·𐑥𐑱𐑯 ──────────╮
│                 │
│       ┌───┐     │
│  6 ───▶ 𐑑 ├─────▶ 𐑬
│  7 ───▶   │     │
│       └───┘     │
│                 │
╰─────────────────╯
";

#[test]
fn constants_and_output_follow_a_sideways_move() {
    let text = moved(ARITH, Pos::new(2, 8), 0, 2);
    assert_eq!(
        lines(&text, 2..6),
        "\
│         ┌───┐   │
│  6 ─────▶ 𐑑 ├───▶ 𐑬
│  7 ─────▶   │   │
│         └───┘   │"
    );
}

#[test]
fn constant_moves_up_to_its_shortened_wire() {
    // The wire from 6 has no cells left, so its end sits against the text
    // and the formatter pulls the text back to one space.
    let text = moved(ARITH, Pos::new(2, 8), 0, -3);
    assert_eq!(lines(&text, 3..5), "│ 6 ─▶ 𐑑 ├────────▶ 𐑬\n│ 7 ─▶   │        │");
}

#[test]
fn a_held_name_refuses_to_be_crowded() {
    // An arrowhead holds a port name in place, so its wire can't shrink to
    // nothing; the refusal names the stranded text.
    let src = "\
╭─ ·𐑨𐑛 ───────────╮
│                 │
│       ┌───┐     │
▶ 𐑨 ────▶ 𐑖 ├─────▶ 𐑬
│       └───┘     │
│                 │
╰─────────────────╯
";
    let refusal = move_box(src, Pos::new(2, 8), 0, -4).unwrap_err();
    assert_eq!(refusal.at, Pos::new(3, 8));
    assert!(refusal.reason.contains("from 𐑨"), "{refusal:?}");
    // One cell less and the wire keeps a cell of its own.
    assert!(move_box(src, Pos::new(2, 8), 0, -3).unwrap().contains("▶ 𐑨 ─▶ 𐑖 ├"));
}

#[test]
fn fan_out_rejoins_with_a_tee() {
    // Both inputs hang off one tee. Moving the box down turns the tee into
    // a corner and makes a new tee where the lower branch now leaves.
    let text = moved(DOUBLE, Pos::new(4, 10), 1, 0);
    assert_eq!(
        lines(&text, 2..10),
        "\
╭─ ·𐑛𐑳𐑚𐑩𐑤 ────────────╮
│                     │
│                     │
▶ 𐑯 ──┐   ┌───┐┌──────▶ 𐑬
│     ├───▶ 𐑐 ├┘      │
│     └───▶   │       │
│         └───┘       │
╰─────────────────────╯"
    );
}

#[test]
fn output_feeding_a_fan_out_keeps_its_branches() {
    let src = "\
╭─ ·𐑥𐑱𐑯 ─────────────────╮
│                        │
│       ┌───┐            │
│  4 ───▶ 𐑖 ├───┬────────▶ 𐑢𐑳𐑯
│       └───┘   │        │
│               └────────▶ 𐑑𐑵
│                        │
╰────────────────────────╯
";
    for (dr, dc) in [(-1, 0), (0, -1), (0, 1)] {
        let text = moved(src, Pos::new(2, 8), dr, dc);
        assert!(text.contains("┬────────▶ 𐑢𐑳𐑯"), "{text}");
    }
}

#[test]
fn a_switch_takes_its_selector_and_through_ports_along() {
    // 04-abs's switch, moved right: the selector, both through-ports and
    // the merge of the arms' results all reattach.
    let text = moved(ABS, Pos::new(9, 12), 0, 1);
    assert_eq!(
        lines(&text, 9..14),
        "\
│     │  │   ┌─ 0 ───▼─────┐      │
│     │  └───▶─────────────▶─┐    │
│     │      ├┄┄┄┄┄┄┄┄┄┄┄┄┄┤ │    │
│     │      │     ┌───┐   │ │    │
│     │      │  0 ─▶ 𐑚 ├───▶─┴────▶ 𐑬"
    );
}

#[test]
fn a_definition_takes_its_output_names_along() {
    let text = moved(ARITH, Pos::new(0, 0), 0, 2);
    assert_eq!(lines(&text, 3..4), "  │  6 ───▶ 𐑑 ├─────▶ 𐑬");
}

#[test]
fn refusals_say_why() {
    let none = move_box(ARITH, Pos::new(3, 3), 0, 1).unwrap_err();
    assert_eq!(none.at, Pos::new(3, 3));
    let out = move_box(ARITH, Pos::new(2, 8), 0, 9).unwrap_err();
    assert!(out.reason.contains("leave its definition"), "{out:?}");
    let onto = move_box(ARITH, Pos::new(2, 8), 0, -6).unwrap_err();
    assert!(onto.reason.contains("land on"), "{onto:?}");
    let arm = move_box(ABS, Pos::new(12, 18), -1, 0).unwrap_err();
    assert!(arm.reason.contains("arm"), "{arm:?}");
}

// ---------------------------------------------------------------------------
// P7: wiring edits and the palette.

use crate::{connect, disconnect, palette, place_node, remove_node, remove_wire};

/// Checks an edit's result is canonical, and returns it.
fn canon(text: String) -> String {
    assert_eq!(shawline_cli::format(&text).unwrap(), text, "not canonical");
    text
}

#[test]
fn a_second_sink_joins_the_wire_with_a_tee() {
    // 03-double from scratch: 𐑯 feeds 𐑐 once, then again through a tee.
    let bare = canon(remove_wire(DOUBLE, Pos::new(5, 4)).unwrap());
    assert!(lines(&bare, 5..7).starts_with(&format!("▶ 𐑯{}▶ 𐑐 ├", " ".repeat(7))), "{bare}");
    let once = canon(connect(&bare, Pos::new(5, 0), Pos::new(5, 10)).unwrap());
    assert!(!once.contains('┬') && !once.contains('┴'), "{once}");
    let twice = canon(connect(&once, Pos::new(5, 0), Pos::new(6, 10)).unwrap());
    assert!(lines(&twice, 5..7).contains('┬'), "{twice}");
    assert_eq!(outputs(&twice), outputs(DOUBLE));
}

#[test]
fn disconnect_turns_the_tee_back_into_a_stroke() {
    let text = canon(disconnect(DOUBLE, Pos::new(6, 10)).unwrap());
    assert_eq!(lines(&text, 5..7), "▶ 𐑯 ──────▶ 𐑐 ├───────▶ 𐑬\n│         ▶   │       │");
    // The last branch takes the whole wire with it.
    let bare = canon(disconnect(&text, Pos::new(5, 10)).unwrap());
    assert!(bare.contains(&format!("▶ 𐑯{}▶ 𐑐", " ".repeat(7))), "{bare}");
}

#[test]
fn a_wired_sink_takes_no_second_wire() {
    let refusal = connect(DOUBLE, Pos::new(5, 0), Pos::new(5, 10)).unwrap_err();
    assert_eq!(refusal.at, Pos::new(5, 10));
    assert!(refusal.reason.contains("already has a wire"), "{refusal:?}");
}

#[test]
fn connect_refuses_what_it_cannot_name_or_reach() {
    let not_source = connect(DOUBLE, Pos::new(3, 3), Pos::new(5, 10)).unwrap_err();
    assert!(not_source.reason.contains("feeds a wire"), "{not_source:?}");
    let not_sink = remove_wire(DOUBLE, Pos::new(5, 4)).unwrap();
    let refusal = connect(&not_sink, Pos::new(5, 0), Pos::new(5, 14)).unwrap_err();
    assert!(refusal.reason.contains("arrowheads"), "{refusal:?}");
    // 𐑯 lives outside the switch; 𐑚's input is inside its second arm.
    let open = disconnect(ABS, Pos::new(14, 18)).unwrap();
    let far = connect(&open, Pos::new(5, 0), Pos::new(14, 18)).unwrap_err();
    assert!(far.reason.contains("different boxes or arms"), "{far:?}");
    // The arm's own input reaches it.
    let near = canon(connect(&open, Pos::new(14, 12), Pos::new(14, 18)).unwrap());
    assert_eq!(outputs(&near), outputs(ABS));
}

#[test]
fn wires_at_inline_nodes_and_switch_outputs_stay_put() {
    let fanout = include_str!("../../../corpus/13-fanout.shaw");
    let inline = remove_wire(fanout, Pos::new(5, 4)).unwrap_err();
    assert!(inline.reason.contains("inline"), "{inline:?}");
    let merged = remove_wire(ABS, Pos::new(13, 28)).unwrap_err();
    assert!(merged.reason.contains("merged output"), "{merged:?}");
    assert!(remove_wire(DOUBLE, Pos::new(3, 3)).unwrap_err().reason.contains("no wire"));
}

#[test]
fn a_node_comes_out_with_its_wires_and_goes_back() {
    let gone = canon(remove_node(DOUBLE, Pos::new(4, 10)).unwrap());
    let blank = format!("│{}│", " ".repeat(21));
    let row = format!("▶ 𐑯{}▶ 𐑬", " ".repeat(19));
    assert_eq!(lines(&gone, 4..8), [blank.as_str(), &row, &blank, &blank].join("\n"));
    let back = canon(place_node(&gone, Pos::new(4, 10), "𐑐", 2, 1).unwrap());
    assert_eq!(lines(&back, 5..7), "▶ 𐑯       ▶ 𐑐 ├       ▶ 𐑬\n│         ▶   │       │");
    let mut text = back;
    for (from, to) in [((5, 0), (5, 10)), ((5, 0), (6, 10)), ((5, 14), (5, 22))] {
        text = canon(connect(&text, Pos::new(from.0, from.1), Pos::new(to.0, to.1)).unwrap());
    }
    assert_eq!(outputs(&text), outputs(DOUBLE));
}

#[test]
fn placing_and_removing_refuse_with_reasons() {
    let onto = place_node(DOUBLE, Pos::new(4, 3), "𐑐", 2, 1).unwrap_err();
    assert!(onto.reason.contains("land on"), "{onto:?}");
    let outside = place_node(DOUBLE, Pos::new(20, 30), "𐑐", 2, 1).unwrap_err();
    assert!(outside.reason.contains("inside a definition"), "{outside:?}");
    let arm = place_node(ABS, Pos::new(10, 20), "𐑖", 1, 1).unwrap_err();
    assert!(arm.reason.contains("arm"), "{arm:?}");
    let latin = place_node(DOUBLE, Pos::new(8, 3), "x", 1, 1).unwrap_err();
    assert!(latin.reason.contains("Shavian"), "{latin:?}");
    let definition = remove_node(DOUBLE, Pos::new(2, 0)).unwrap_err();
    assert!(definition.reason.contains("only a node"), "{definition:?}");
}

#[test]
fn the_palette_lists_primitives_then_definitions() {
    let all = palette(DOUBLE, None);
    let words: Vec<&str> = all.iter().map(|c| c.word.as_str()).collect();
    assert_eq!(
        words,
        ["𐑐", "𐑚", "𐑑", "𐑛", "𐑒", "𐑜", "𐑟", "𐑕", "𐑤", "𐑮", "𐑦", "𐑰", "𐑥", "𐑯", "𐑖", "𐑠", "𐑯𐑦𐑤", "𐑛𐑳𐑚𐑩𐑤"]
    );
    assert_eq!((all[0].gloss.as_str(), all[0].mirror.as_deref()), ("add", Some("𐑚")));
    assert_eq!((all[5].inputs, all[5].outputs), (1, 2));
    assert_eq!((all[17].gloss.as_str(), all[17].inputs, all[17].outputs), ("𐑯 → 𐑬", 1, 1));
    let dragging = palette(DOUBLE, Some(Pos::new(5, 0)));
    assert!(!dragging.iter().any(|c| c.inputs == 0), "{dragging:?}");
    assert_eq!(dragging.len(), all.len() - 1);
}
