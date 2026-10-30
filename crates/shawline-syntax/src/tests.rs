//! Codes and shapes the corpus doesn't reach.

use shawline_grid::Grid;
use shawline_shape::recognize;

use crate::ast::{Endpoint, Literal, NodeKind, Sheet};
use crate::parse;

fn run(src: &str) -> (Sheet, Vec<String>) {
    let grid = Grid::parse(src).unwrap();
    let diagram = recognize(&grid);
    assert!(diagram.diagnostics.is_empty(), "shape: {:?}", diagram.diagnostics);
    let (sheet, diags) = parse(&grid, &diagram);
    (sheet, diags.iter().map(|d| format!("{} {}", d.at, d.code)).collect())
}

fn codes(src: &str) -> Vec<String> {
    run(src).1
}

/// Each diagnostic as `ROW:COL CODE message`.
fn messages(src: &str) -> Vec<String> {
    let grid = Grid::parse(src).unwrap();
    let diagram = recognize(&grid);
    assert!(diagram.diagnostics.is_empty(), "shape: {:?}", diagram.diagnostics);
    let (_, diags) = parse(&grid, &diagram);
    diags.iter().map(|d| format!("{} {} {}", d.at, d.code, d.message)).collect()
}

#[test]
fn titles_must_be_namer_words_of_two_letters() {
    assert_eq!(codes("╭─ 𐑛𐑳𐑚 ─╮\n│       │\n╰───────╯\n"), ["0:3 Y001"]);
    assert_eq!(codes("╭───╮\n│   │\n╰───╯\n"), ["0:0 Y001"]);
    assert_eq!(codes("╭─ ·𐑛 ─╮\n│      │\n╰──────╯\n"), ["0:3 Y013"]);
}

#[test]
fn node_box_holds_one_word() {
    let src = "\
╭─ ·𐑛𐑳 ──────╮
│ ┌──────┐   │
│ │ 𐑐 𐑚  │   │
│ └──────┘   │
╰────────────╯
";
    assert_eq!(codes(src), ["2:4 Y002"]);
    let empty = "╭─ ·𐑛𐑳 ──╮\n│ ┌───┐  │\n│ │   │  │\n│ └───┘  │\n╰────────╯\n";
    assert_eq!(codes(empty), ["1:2 Y002"]);
}

#[test]
fn dangling_ends_get_no_y005() {
    let src = "╭─ ·𐑛𐑳 ────╮\n│ ───▶     │\n╰──────────╯\n";
    assert_eq!(codes(src), ["1:2 Y003", "1:5 Y004"]);
}

#[test]
fn literal_after_free_arrowhead_is_not_a_constant() {
    let src = "╭─ ·𐑛𐑳 ───────────╮\n│ 5 ──▶ 7 ────────▶ 𐑬\n╰─────────────────╯\n";
    assert_eq!(codes(src), ["1:6 Y004", "1:8 Y012", "1:10 Y003"]);
}

#[test]
fn nameless_ports_still_carry_their_wire() {
    let src = "╭─ ·𐑛𐑳 ──╮\n▶ ───────▶\n╰────────╯\n";
    assert_eq!(
        messages(src),
        ["1:0 Y009 the first input of ·𐑛𐑳 has no name", "1:9 Y009 the first output of ·𐑛𐑳 has no name"]
    );
    let (sheet, _) = run(src);
    let edges = &sheet.definitions[0].body.edges;
    assert_eq!(edges.len(), 1);
    assert_eq!(
        (edges[0].from, edges[0].to.clone()),
        (Endpoint::Param(0), vec![Endpoint::Result(0)])
    );
}

#[test]
fn merged_switch_output_without_a_sink() {
    let src = "\
╭─ ·𐑛𐑳 ──────────╮
│ 1 ───┐         │
│ ┌─ 1 ▼───┐     │
│ │ 2 ─────▶─┐   │
│ ├┄┄┄┄┄┄┄┄┤ │   │
│ │ 3 ─────▶─┘   │
│ └────────┘     │
╰────────────────╯
";
    assert_eq!(messages(src), ["3:12 Y005 the wire from the switch's first merged output reaches no arrowhead"]);
}

#[test]
fn wire_between_two_inputs_has_no_source() {
    let src = "\
╭─ ·𐑛𐑳 ───────╮
│   ┌───┐     │
│ ┌─▶ 𐑐 ├─────▶ 𐑬
│ └─▶   │     │
│   └───┘     │
╰─────────────╯
";
    assert_eq!(
        messages(src),
        ["2:2 Y005 wire with no source reaches the first input of 𐑐 and the second input of 𐑐"]
    );
}

#[test]
fn merge_of_different_result_ordinals() {
    let src = "\
╭─ ·𐑛𐑳 ──────────╮
│ 1 ───┐         │
│ ┌─ 1 ▼───┐     │
│ │ 2 ─────▶─┐   │
│ │ 3 ─────▶─┴───▶ 𐑬
│ ├┄┄┄┄┄┄┄┄┤     │
│ │ 4 ─────▶─────▶ 𐑬𐑬
│ │ 5 ─────▶     │
│ └────────┘     │
╰────────────────╯
";
    assert_eq!(
        messages(src),
        ["3:12 Y006 2 sources on one wire: the first right through-port of the first arm \
          and the second right through-port of the first arm"]
    );
    assert_eq!(
        messages(include_str!("../../../corpus/92-merge.shaw")),
        ["4:4 Y006 2 sources on one wire: the constant 1 and the constant 2"]
    );
}

#[test]
fn default_arms_and_guards() {
    let src = "\
╭─ ·𐑛𐑳 ──────────╮
│ 1 ───┐         │
│ ┌─ 1 ▼───┐     │
│ │        │     │
│ ├┄┄┄┄┄┄┄┄┤     │
│ │        │     │
│ ├┄┄┄┄┄┄┄┄┤     │
│ │        │     │
│ ├┄ 2 ┄┄┄┄┤     │
│ │        │     │
│ └────────┘     │
╰────────────────╯
";
    assert_eq!(codes(src), ["6:2 Y008", "8:5 Y007"]);
}

#[test]
fn switch_without_selector() {
    let src = "\
╭─ ·𐑛𐑳 ──────────╮
│ ┌─ 1 ────┐     │
│ │        │     │
│ ├┄┄┄┄┄┄┄┄┤     │
│ │        │     │
│ └────────┘     │
╰────────────────╯
";
    assert_eq!(codes(src), ["1:2 Y010"]);
}

#[test]
fn arms_must_agree_on_results() {
    let src = "\
╭─ ·𐑛𐑳 ──────────╮
│ 1 ───┐         │
│ ┌─ 1 ▼───┐     │
│ │ 2 ─────▶─────▶ 𐑬
│ ├┄┄┄┄┄┄┄┄┤     │
│ │        │     │
│ └────────┘     │
╰────────────────╯
";
    assert_eq!(
        messages(src),
        ["2:7 Y011 arms with different numbers of results: the first arm has 1 and the second has none"]
    );
}

#[test]
fn stray_text_inside_and_outside() {
    let src = "𐑐 ──\n╭─ ·𐑛𐑳 ──╮\n│ 𐑚 «𐑣»  │\n╰────────╯\n";
    assert_eq!(codes(src), ["0:0 Y012", "0:2 Y003", "0:3 Y003", "2:2 Y012", "2:4 Y012"]);
}

#[test]
fn box_styles() {
    let nested = "╭─ ·𐑛𐑳 ──╮\n│ ╭───╮  │\n│ │ 𐑐 │  │\n│ ╰───╯  │\n╰────────╯\n";
    assert_eq!(codes(nested), ["1:2 Y014"]);
    let mixed = "╭─ ·𐑛𐑳 ──┐\n│        │\n╰────────╯\n";
    assert_eq!(codes(mixed), ["0:0 Y015"]);
}

#[test]
fn guard_on_a_plain_node() {
    let src =
        "╭─ ·𐑛𐑳 ───────╮\n│ ┌─ 1 ─┐     │\n│ │ 𐑐   │     │\n│ └─────┘     │\n╰─────────────╯\n";
    assert_eq!(codes(src), ["1:5 Y016"]);
}

#[test]
fn constants_nil_and_oversized() {
    let src = "\
╭─ ·𐑛𐑳 ──────────────────────╮
│ 𐑯𐑦𐑤 ───────────────────────▶ 𐑬
│ 99999999999999999999 ──────▶ 𐑚
╰────────────────────────────╯
";
    let (sheet, diags) = run(src);
    assert_eq!(diags, ["2:2 Y017"]);
    let kinds: Vec<_> = sheet.definitions[0].body.nodes.iter().map(|n| n.kind.clone()).collect();
    assert_eq!(kinds, [NodeKind::Const(Literal::Nil), NodeKind::Const(Literal::Int(i64::MAX))]);
}

#[test]
fn unfed_arrowheads_are_y019() {
    let output = "╭─ ·𐑛𐑳 ───╮\n│ 5 ──────▶ 𐑬\n│         ▶ 𐑚\n╰─────────╯\n";
    assert_eq!(messages(output), ["2:10 Y019 no wire reaches the second output of ·𐑛𐑳"]);
    let input = "\
╭─ ·𐑛𐑳 ─────────╮
│     ┌───┐     │
│ 5 ──▶ 𐑐 ├─────▶ 𐑬
│     ▶   │     │
│     └───┘     │
╰───────────────╯
";
    assert_eq!(messages(input), ["3:6 Y019 no wire reaches the second input of 𐑐"]);
    // An unfed selector, and an arm whose through-ports are unfed: the left
    // one from outside, the right one from inside.
    let switch = "\
╭─ ·𐑛𐑳 ────────────────╮
│ ┌─ 1 ───▼────────┐   │
│ ▶────────────────▶───▶ 𐑬
│ ├┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┤   │
│ │                ▶   │
│ └────────────────┘   │
╰──────────────────────╯
";
    assert_eq!(
        messages(switch),
        [
            "1:10 Y019 no wire reaches the switch's selector",
            "2:2 Y019 no wire reaches the first left through-port of the first arm from outside the switch",
            "4:19 Y019 no wire reaches the first right through-port of the second arm from inside the arm",
        ]
    );
    assert_eq!(
        messages(include_str!("../../../corpus/static/01-unfed-port.shaw")),
        ["6:8 Y019 no wire reaches the second input of 𐑐"]
    );
}

#[test]
fn unnamed_ports_say_which_port_of_which_definition() {
    assert_eq!(
        messages(include_str!("../../../corpus/static/05-unnamed-port.shaw")),
        ["6:20 Y009 the second output of ·𐑥𐑱𐑯 has no name"]
    );
    let untitled = "╭───────╮\n▶ ──────▶\n╰───────╯\n";
    assert_eq!(
        messages(untitled),
        [
            "0:0 Y001 a definition's title must be one ·-word",
            "1:0 Y009 the first input of the definition has no name",
            "1:8 Y009 the first output of the definition has no name",
        ]
    );
}

/// A wire end at a port that no rule lets it use names the port, and the
/// side it came from.
#[test]
fn unusable_ports_are_named() {
    let outside = "   ╭─ ·𐑛𐑳 ───╮\n ──▶ 𐑯 ──────▶ 𐑬\n   ╰─────────╯\n";
    assert_eq!(
        messages(outside),
        [
            "1:1 Y003 dangling wire end",
            "1:2 Y003 a wire can't meet the first input of ·𐑛𐑳 from outside the definition",
        ]
    );
    let arrowhead_output = "\
╭─ ·𐑛𐑳 ────────╮
│     ┌───┐    │
│ 5 ──▶ 𐑖 ▶────▶ 𐑬
│     └───┘    │
╰──────────────╯
";
    assert_eq!(messages(arrowhead_output), ["2:11 Y003 a wire can't meet this arrowhead on the border of 𐑖"]);
}

#[test]
fn nil_is_a_reserved_name() {
    assert_eq!(codes("╭─ ·𐑯𐑦𐑤 ─╮\n│        │\n╰────────╯\n"), ["0:3 Y013"]);
}
