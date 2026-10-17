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
    let (sheet, diags) = run("╭─ ·𐑛𐑳 ──╮\n▶ ───────▶\n╰────────╯\n");
    assert_eq!(diags, ["1:0 Y009", "1:9 Y009"]);
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
    assert_eq!(codes(src), ["3:12 Y005"]);
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
    assert_eq!(codes(src), ["2:2 Y005"]);
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
    assert_eq!(codes(src), ["3:12 Y006"]);
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
    assert_eq!(codes(src), ["2:7 Y011"]);
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
