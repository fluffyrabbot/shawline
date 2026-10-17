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
