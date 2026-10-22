//! The router's preferences, and one move per way a wire meets a box.

use shawline_doc::Doc;
use shawline_grid::{Dir, Pos};

use crate::router::{self, BUDGET, CELL, Cell, Miss, Start, Terrain};
use text::{move_box, route};

/// The edits over plain text, as these tests were written before edits took
/// revisions (P8 wave 1).
mod text {
    use shawline_doc::Doc;
    use shawline_grid::Pos;

    use crate::{Candidate, Endpoint, Refusal};

    fn text(result: Result<Doc, Refusal>) -> Result<String, Refusal> {
        result.map(|doc| doc.text().to_owned())
    }

    pub fn move_box(src: &str, corner: Pos, dr: isize, dc: isize) -> Result<String, Refusal> {
        text(crate::move_box(&Doc::new(src), corner, dr, dc))
    }

    pub fn route(src: &str, from: Pos, to: Pos) -> Option<Vec<Pos>> {
        crate::route(&Doc::new(src), from, to)
    }

    pub fn connect(src: &str, from: Pos, to: Pos) -> Result<String, Refusal> {
        text(crate::connect(&Doc::new(src), from, to))
    }

    pub fn remove_wire(src: &str, at: Pos) -> Result<String, Refusal> {
        text(crate::remove_wire(&Doc::new(src), at))
    }

    pub fn disconnect(src: &str, to: Pos) -> Result<String, Refusal> {
        text(crate::disconnect(&Doc::new(src), to))
    }

    pub fn place_node(src: &str, corner: Pos, word: &str, inputs: u16, outputs: u16) -> Result<String, Refusal> {
        text(crate::place_node(&Doc::new(src), corner, word, inputs, outputs).map(|placed| placed.doc))
    }

    pub fn remove_node(src: &str, corner: Pos) -> Result<String, Refusal> {
        text(crate::remove_node(&Doc::new(src), corner))
    }

    pub fn palette(src: &str, from: Option<Pos>) -> Vec<Candidate> {
        crate::palette(&Doc::new(src), from)
    }

    pub fn endpoints(src: &str) -> (Vec<Endpoint>, Vec<Endpoint>) {
        crate::endpoints(&Doc::new(src))
    }

    pub fn preview(src: &str, from: Pos, to: Pos) -> Option<Vec<Pos>> {
        crate::preview(&Doc::new(src), from, to)
    }

    pub fn wire_sources(src: &str) -> Vec<Option<Pos>> {
        crate::wire_sources(&Doc::new(src))
    }
}

/// The canonical form of `src`, from a fresh revision.
fn format(src: &str) -> String {
    Doc::new(src).formatted().expect("on the grid").to_owned()
}

const DOUBLE: &str = include_str!("../../../corpus/03-double.shaw");
const ABS: &str = include_str!("../../../corpus/04-abs.shaw");

/// main's outputs, as printed.
fn outputs(src: &str) -> Vec<String> {
    let doc = Doc::new(src);
    let run = doc.run().unwrap_or_else(|| panic!("{:?}\n{src}", doc.diagnostics()));
    run.unwrap().iter().map(ToString::to_string).collect()
}

/// Moves the box and checks the result is canonical and means the same.
fn moved(src: &str, corner: Pos, dr: isize, dc: isize) -> String {
    let text = move_box(src, corner, dr, dc).unwrap_or_else(|r| panic!("refused: {r:?}"));
    assert_eq!(format(&text), text, "not canonical");
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
    assert_eq!(router::search(&Corridor, start, &mut 3), Err(Miss::GaveUp));
    assert!(router::search(&Corridor, start, &mut { BUDGET }).is_ok());
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

use text::{connect, disconnect, palette, place_node, remove_node, remove_wire};

/// Checks an edit's result is canonical, and returns it.
fn canon(text: String) -> String {
    assert_eq!(format(&text), text, "not canonical");
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

// ---------------------------------------------------------------------------
// Targets named on text that isn't canonical.

use std::collections::HashMap;

use shawline_grid::Grid;
use shawline_shape::{EndKind, PortKind, recognize};

use crate::Refusal;
use text::{endpoints, preview, wire_sources};

#[test]
fn a_leading_blank_line_shifts_what_the_corner_names() {
    // The playground reads box corners off the text as shown.
    let shown = format!("\n{DOUBLE}");
    let node = recognize(&Grid::parse(&shown).unwrap()).boxes[1].rect;
    assert_eq!((node.top, node.left), (5, 10));
    let gone = remove_node(&shown, Pos::new(5, 10)).unwrap();
    assert_eq!(gone, remove_node(DOUBLE, Pos::new(4, 10)).unwrap());
    // The canonical corner names nothing in the text as shown.
    let stale = remove_node(&shown, Pos::new(4, 10)).unwrap_err();
    assert_eq!(stale.at, Pos::new(4, 10));
    assert!(stale.reason.contains("no box"), "{stale:?}");
}

#[test]
fn a_doubled_blank_line_shifts_the_second_definition() {
    // No diagnostic, no lint: only the formatter's blank-line rule.
    let shown = DOUBLE.replacen("╯\n\n╭", "╯\n\n\n╭", 1);
    assert!(Doc::new(shown.as_str()).diagnostics().is_empty());
    // main's 21 feeds 𐑛𐑳𐑚𐑩𐑤 on row 14 of the canonical text, 15 here.
    let wire = remove_wire(&shown, Pos::new(15, 6)).unwrap();
    assert_eq!(wire, remove_wire(DOUBLE, Pos::new(14, 6)).unwrap());
    let node = remove_node(&shown, Pos::new(14, 8)).unwrap();
    assert_eq!(node, remove_node(DOUBLE, Pos::new(13, 8)).unwrap());
}

#[test]
fn a_cell_of_a_filled_gap_names_its_wire() {
    // `21──▶`: the cell after 21 is wire here, and blank once formatted.
    let shown = DOUBLE.replace("│ 21 ───▶", "│ 21────▶");
    let gone = remove_wire(&shown, Pos::new(14, 4)).unwrap();
    assert_eq!(gone, remove_wire(DOUBLE, Pos::new(14, 6)).unwrap());
    assert!(gone.contains("│ 21    ▶"), "{gone}");
}

#[test]
fn look_alike_wires_are_told_apart_by_place() {
    // Two stray strokes join only loose ends, so nothing names them but
    // where they are.
    let sheet = "\
╭─ ·𐑛𐑳𐑚𐑩𐑤 ──────────╮
│                   │
▶ 𐑯 ────────────────▶ 𐑬
│                   │
│  ───       ───    │
│                   │
╰───────────────────╯
";
    let shown = format!("\n\n{sheet}");
    let left = remove_wire(&shown, Pos::new(6, 4)).unwrap();
    assert_eq!(left, remove_wire(sheet, Pos::new(4, 4)).unwrap());
    assert!(left.contains("│            ───    │"), "{left}");
    let right = remove_wire(&shown, Pos::new(6, 13)).unwrap();
    assert!(right.contains("│  ───              │"), "{right}");
}

/// A text that formats back to a canonical one, with the row each of the
/// canonical rows lands on. Columns are the same in both.
struct Variant {
    what: &'static str,
    text: String,
    rows: Vec<usize>,
}

impl Variant {
    fn at(&self, p: Pos) -> Pos {
        Pos::new(self.rows[p.row], p.col)
    }
}

/// The jitter corpus's ASCII swaps: strokes whose connections are fixed.
fn ascii(line: &str) -> String {
    line.chars()
        .map(|ch| match ch {
            '─' => '-',
            '│' => '|',
            '┄' => '~',
            '┆' => ':',
            '▶' => '>',
            '◀' => '<',
            '▲' => '^',
            '▼' => 'v',
            _ => ch,
        })
        .collect()
}

/// Every word or literal's space before its wire filled with wire.
fn squeeze(line: &str) -> String {
    let mut row: Vec<char> = line.chars().collect();
    let word = shawline_grid::repertoire::is_word_char;
    for c in 1..row.len().saturating_sub(1) {
        let text = word(row[c - 1]) || row[c - 1] == '»';
        if text && row[c] == ' ' && row[c + 1] == '─' {
            row[c] = '─';
        }
    }
    row.into_iter().collect()
}

fn variants(src: &str) -> Vec<Variant> {
    let build = |what, lead: usize, double: bool, line: &dyn Fn(&str) -> String| {
        let mut text = "\n".repeat(lead);
        let (mut rows, mut row) = (Vec::new(), lead);
        for l in src.lines() {
            rows.push(row);
            text.push_str(&line(l));
            text.push('\n');
            row += 1;
            if double && l.is_empty() {
                text.push('\n');
                row += 1;
            }
        }
        let v = Variant { what, text, rows };
        assert_eq!(format(&v.text), src, "{} doesn't format back", v.what);
        assert_ne!(v.text, src, "{} changes nothing", v.what);
        v
    };
    let same = |l: &str| l.to_owned();
    vec![
        build("leading blank lines", 2, false, &same),
        build("doubled blank lines", 0, true, &same),
        build("trailing spaces", 0, false, &|l| format!("{l}   ")),
        build("ASCII strokes", 0, false, &ascii),
        build("filled text gaps", 0, false, &squeeze),
        build("all at once", 1, true, &|l| format!("{} ", ascii(&squeeze(l)))),
    ]
}

/// What a sheet offers to edit, in its own cells.
struct Targets {
    /// A middle cell of each editable wire.
    wires: Vec<Pos>,
    /// Each wired sink, with the source cell its wire starts at.
    pairs: Vec<(Pos, Pos)>,
    /// Each node box: corner, word, inputs, outputs.
    nodes: Vec<(Pos, String, u16, u16)>,
}

fn targets(src: &str) -> Targets {
    let grid = Grid::parse(src).unwrap();
    let d = recognize(&grid);
    let starts = wire_sources(src);
    let mut wires = Vec::new();
    let mut pairs = Vec::new();
    for (w, start) in starts.iter().enumerate() {
        let Some(from) = *start else { continue };
        let cells = &d.wires[w].cells;
        wires.push(cells[cells.len() / 2]);
        for e in &d.wires[w].ends {
            if let EndKind::Port(p) = e.kind
                && endpoints(src).1.iter().any(|s| s.at == p)
            {
                pairs.push((from, p));
            }
        }
    }
    let mut nodes = Vec::new();
    for (i, b) in d.boxes.iter().enumerate() {
        let node = b.parent.is_some()
            && b.dividers.is_empty()
            && !b.ports.iter().any(|p| p.side == Dir::N);
        let Some(word) = d.texts.iter().find(|t| t.owner == Some(i)) else { continue };
        if !node {
            continue;
        }
        let count =
            |kind, side| b.ports.iter().filter(|p| p.kind == kind && p.side == side).count();
        nodes.push((
            Pos::new(b.rect.top, b.rect.left),
            word.content.clone(),
            count(PortKind::Arrow, Dir::W) as u16,
            count(PortKind::Tee, Dir::E) as u16,
        ));
    }
    Targets { wires, pairs, nodes }
}

/// An edit with its targets, for [`Agree::check`].
type Edit<'a> = &'a dyn Fn(&str, &[Pos]) -> Result<String, Refusal>;

/// Edits tried on canonical text and on its variants, and how many went
/// through, by kind.
#[derive(Default)]
struct Agree {
    done: HashMap<&'static str, usize>,
    turn: usize,
}

impl Agree {
    /// The variants an edit is tried on: every perturbation at once, and
    /// one alone, each taking its turn.
    fn pick<'a>(&mut self, all: &'a [Variant]) -> [&'a Variant; 2] {
        self.turn += 1;
        let (last, alone) = all.split_last().expect("variants");
        [last, &alone[self.turn % alone.len()]]
    }

    /// Runs `edit` on canonical `src` and on variants of it, with the
    /// targets carried to each variant's cells, and checks the results
    /// agree: the same text, or the same refusal at the carried cell.
    fn check(
        &mut self,
        kind: &'static str,
        (src, all): (&str, &[Variant]),
        targets: &[Pos],
        edit: Edit,
    ) {
        let want = edit(src, targets);
        *self.done.entry(kind).or_default() += usize::from(want.is_ok());
        for v in self.pick(all) {
            let carried: Vec<Pos> = targets.iter().map(|&p| v.at(p)).collect();
            let got = edit(&v.text, &carried);
            let what = format!("{kind} at {targets:?} on {}", v.what);
            match (&got, &want) {
                (Ok(a), Ok(b)) => assert_eq!(a, b, "{what}\n{}", v.text),
                (Err(a), Err(b)) => {
                    assert_eq!(a.reason, b.reason, "{what}\n{}", v.text);
                    assert_eq!(a.at, v.at(b.at), "{what}\n{}", v.text);
                }
                _ => panic!("{what}: {got:?}, but on canonical text {want:?}\n{}", v.text),
            }
        }
    }

    /// [`preview`], whose cells are carried back to compare.
    fn preview(&mut self, (src, all): (&str, &[Variant]), from: Pos, to: Pos) {
        let want = preview(src, from, to);
        *self.done.entry("preview").or_default() += usize::from(want.is_some());
        for v in self.pick(all) {
            let carried = want.as_ref().map(|path| path.iter().map(|&p| v.at(p)).collect());
            let got = preview(&v.text, v.at(from), v.at(to));
            assert_eq!(got, carried, "preview {from} to {to} on {}\n{}", v.what, v.text);
        }
    }
}

#[test]
fn every_edit_names_on_any_variant_what_it_names_on_canonical_text() {
    let programs = [
        DOUBLE,
        ABS,
        include_str!("../../../corpus/02-arith.shaw"),
        include_str!("../../../corpus/13-fanout.shaw"),
    ];
    let mut agree = Agree::default();
    for src in programs {
        let t = targets(src);
        let sheet = (src, &variants(src)[..]);
        // A few targets of each kind, spread over the sheet, keep it quick.
        let spread = |n: usize, most: usize| n.div_ceil(most).max(1);
        for &at in t.wires.iter().step_by(spread(t.wires.len(), 6)) {
            agree.check("remove_wire", sheet, &[at], &|s, p| remove_wire(s, p[0]));
        }
        if let Some(&(from, _)) = t.pairs.first() {
            let offered = |s: &str, p: &[Pos]| Ok(format!("{:?}", palette(s, Some(p[0]))));
            agree.check("palette", sheet, &[from], &offered);
        }
        for &(from, to) in t.pairs.iter().step_by(spread(t.pairs.len(), 4)) {
            agree.check("disconnect", sheet, &[to], &|s, p| disconnect(s, p[0]));
            let Ok(open) = disconnect(src, to) else { continue };
            let open = (open.as_str(), &variants(&open)[..]);
            agree.check("connect", open, &[from, to], &|s, p| connect(s, p[0], p[1]));
            // To the sink, and to the blank cell before it.
            agree.preview(open, from, to);
            agree.preview(open, from, Pos::new(to.row, to.col - 1));
        }
        for (corner, word, inputs, outputs) in &t.nodes {
            agree.check("remove_node", sheet, &[*corner], &|s, p| remove_node(s, p[0]));
            agree.check("move_box", sheet, &[*corner], &|s, p| move_box(s, p[0], 1, 0));
            let Ok(bare) = remove_node(src, *corner) else { continue };
            let place = |s: &str, p: &[Pos]| place_node(s, p[0], word, *inputs, *outputs);
            agree.check("place_node", (&bare, &variants(&bare)), &[*corner], &place);
        }
    }
    let kinds = [
        "remove_wire",
        "disconnect",
        "connect",
        "preview",
        "palette",
        "remove_node",
        "move_box",
        "place_node",
    ];
    for kind in kinds {
        let n = agree.done.get(kind).copied().unwrap_or(0);
        assert!(n >= 3, "too few {kind} edits went through: {:?}", agree.done);
    }
}

// ---------------------------------------------------------------------------
// Bounded work.

#[test]
fn a_move_far_past_the_sheet_is_refused_before_the_sheet_grows() {
    // Nothing encloses a definition, so only the sheet bounds its move.
    // Unbounded, these would grow the text to the box's new place first.
    for (dr, dc) in [(1 << 40, 0), (0, 1 << 40), (isize::MAX, isize::MAX)] {
        let far = move_box(DOUBLE, Pos::new(2, 0), dr, dc).unwrap_err();
        assert_eq!(far.at, Pos::new(2, 0));
        assert!(far.reason.contains("that far past"), "{far:?}");
    }
    let before = move_box(DOUBLE, Pos::new(2, 0), -3, 0).unwrap_err();
    assert!(before.reason.contains("edge"), "{before:?}");
}

#[test]
fn a_move_that_runs_out_of_search_says_it_gave_up() {
    let doc = Doc::new(DOUBLE);
    let starved = crate::mover::move_within(&doc, Pos::new(4, 10), 1, 0, 0).unwrap_err();
    assert!(starved.reason.contains("gave up"), "{starved:?}");
    assert!(!starved.reason.contains("no free path"), "{starved:?}");
    assert!(crate::mover::move_within(&doc, Pos::new(4, 10), 1, 0, BUDGET).is_ok());
}

/// A definition `width` by `height` inside, where nothing can reach the
/// input of 𐑖: the 7 sits where a wire from 5 would start.
fn unreachable(width: usize, height: usize) -> String {
    let mut inner = vec![vec![' '; width]; height];
    for (r, c, s) in [(1, 2, "5 7"), (1, 12, "┌───┐"), (2, 12, "▶ 𐑖 ├"), (3, 12, "└───┘")] {
        for (i, ch) in s.chars().enumerate() {
            inner[r][c + i] = ch;
        }
    }
    let mut text = format!("╭─ ·𐑥𐑱𐑯 {}╮\n", "─".repeat(width - 7));
    for row in inner {
        text.push('│');
        text.extend(row);
        text.push_str("│\n");
    }
    text.push_str(&format!("╰{}╯\n", "─".repeat(width)));
    text
}

#[test]
fn a_search_that_gives_up_is_not_a_missing_path() {
    let (five, input) = (Pos::new(2, 3), Pos::new(3, 13));
    let small = connect(&unreachable(30, 6), five, input).unwrap_err();
    assert!(small.reason.contains("no free path"), "{small:?}");
    // Too many cells to try them all within the budget.
    let large = connect(&unreachable(250, 60), five, input).unwrap_err();
    assert!(large.reason.contains("gave up"), "{large:?}");
    assert_eq!(large.at, input);
}

// ---------------------------------------------------------------------------
// P8: what an edit returns, and what kind of refusal stops it.

use crate::RefusalKind;

#[test]
fn a_placed_node_names_its_ports_in_the_result() {
    let gone = crate::remove_node(&Doc::new(DOUBLE), Pos::new(4, 10)).unwrap();
    // The corner names the text as shown, one row down; the ports name the
    // canonical result.
    let shown = Doc::new(format!("\n{}", gone.text()));
    let placed = crate::place_node(&shown, Pos::new(5, 10), "𐑐", 2, 1).unwrap();
    assert_eq!(placed.inputs, [Pos::new(5, 10), Pos::new(6, 10)]);
    assert_eq!(placed.outputs, [Pos::new(5, 14)]);
    let (sources, sinks) = crate::endpoints(&placed.doc);
    assert!(placed.inputs.iter().all(|&p| sinks.iter().any(|s| s.at == p && !s.wired)));
    assert!(placed.outputs.iter().all(|&p| sources.iter().any(|s| s.at == p && !s.wired)));
    let input = Pos::new(5, 0);
    let output = Pos::new(5, 22);
    let wires = [(input, placed.inputs[0]), (input, placed.inputs[1]), (placed.outputs[0], output)];
    let mut doc = placed.doc;
    for (from, to) in wires {
        doc = crate::connect(&doc, from, to).unwrap();
    }
    assert_eq!(outputs(doc.text()), outputs(DOUBLE));
}

#[test]
fn a_wire_crossing_itself_goes_with_both_axes() {
    // One fan-out whose branches cross: the wire owns both axes of 4:6.
    let src = include_str!("../../../corpus/15-fanout-cross.shaw");
    let gone = crate::remove_wire(&Doc::new(src), Pos::new(2, 4)).unwrap();
    assert!(gone.diagram().unwrap().wires.is_empty(), "{}", gone.text());
    let mut doc = gone;
    for to in [Pos::new(6, 22), Pos::new(7, 22)] {
        doc = crate::connect(&doc, Pos::new(2, 2), to).unwrap();
    }
    assert_eq!(outputs(doc.text()), outputs(src));
    // One branch alone takes only its own axis, and the crossing goes.
    let one = crate::disconnect(&Doc::new(src), Pos::new(6, 22)).unwrap();
    let wires = &one.diagram().unwrap().wires;
    assert!(wires.len() == 1 && wires[0].crossings.is_empty(), "{}", one.text());
}

#[test]
fn refusals_say_what_kind_of_obstacle_they_are() {
    let kind = |r: Result<Doc, Refusal>| r.unwrap_err().kind;
    let double = Doc::new(DOUBLE);

    let unreadable = crate::remove_wire(&Doc::new("\t·𐑛\n"), Pos::new(0, 1)).unwrap_err();
    assert_eq!((unreadable.kind, unreadable.at), (RefusalKind::Unreadable, Pos::new(0, 0)));

    assert_eq!(kind(crate::remove_wire(&double, Pos::new(3, 3))), RefusalKind::Target);
    assert_eq!(kind(crate::connect(&double, Pos::new(5, 0), Pos::new(5, 10))), RefusalKind::Target);
    assert_eq!(kind(crate::remove_node(&double, Pos::new(2, 0))), RefusalKind::Target);
    let outside = crate::place_node(&double, Pos::new(20, 30), "𐑐", 2, 1).unwrap_err();
    assert_eq!(outside.kind, RefusalKind::Target);

    let fanout = Doc::new(include_str!("../../../corpus/13-fanout.shaw"));
    assert_eq!(kind(crate::remove_wire(&fanout, Pos::new(5, 4))), RefusalKind::Unsupported);
    let merged = crate::remove_wire(&Doc::new(ABS), Pos::new(13, 28));
    assert_eq!(kind(merged), RefusalKind::Unsupported);
    let latin = crate::place_node(&double, Pos::new(8, 3), "x", 1, 1).unwrap_err();
    assert_eq!(latin.kind, RefusalKind::Unsupported);

    assert_eq!(kind(crate::move_box(&double, Pos::new(4, 10), 0, 9)), RefusalKind::Blocked);
    let onto = crate::place_node(&double, Pos::new(4, 3), "𐑐", 2, 1).unwrap_err();
    assert_eq!(onto.kind, RefusalKind::Blocked);
    let (five, input) = (Pos::new(2, 3), Pos::new(3, 13));
    let small = crate::connect(&Doc::new(unreachable(30, 6)), five, input);
    assert_eq!(kind(small), RefusalKind::Blocked);

    let large = crate::connect(&Doc::new(unreachable(250, 60)), five, input);
    assert_eq!(kind(large), RefusalKind::GaveUp);
    let starved = crate::mover::move_within(&double, Pos::new(4, 10), 1, 0, 0);
    assert_eq!(kind(starved), RefusalKind::GaveUp);
}
