//! P10 wave 2: drawing definitions, switches, ports and arms; merged
//! outputs and inline nodes; renaming and re-porting a node; and making
//! room for a drawing edit.

use shawline_doc::Doc;
use shawline_grid::{Pos, Rect};

use crate::{
    Anchor, Blockage, Edit, Edited, RefusalKind, add_arm, add_port, apply, connect, disconnect, endpoints, make_room,
    place_definition, place_inline, place_switch, place_text, preview, remove_arm, remove_box,
    remove_node, remove_port, remove_wire, set_ports, set_text, wire_sources,
};

const DOUBLE: &str = include_str!("../../../corpus/03-double.shaw");
const ABS: &str = include_str!("../../../corpus/04-abs.shaw");
const INLINE: &str = include_str!("../../../corpus/05-inline.shaw");
const GUARDS: &str = include_str!("../../../corpus/12-guards.shaw");
const FANOUT: &str = include_str!("../../../corpus/13-fanout.shaw");

fn ok(result: Result<Edited, crate::Refusal>) -> Edited {
    let edited = result.unwrap_or_else(|r| panic!("refused: {r:?}"));
    let text = edited.doc.text();
    assert_eq!(Doc::new(text).formatted(), Some(text), "not canonical:\n{text}");
    edited
}

fn p(row: usize, col: usize) -> Pos {
    Pos::new(row, col)
}

fn rect(top: usize, left: usize, bottom: usize, right: usize) -> Rect {
    Rect { top, left, bottom, right }
}

fn line(edited: &Edited, row: usize) -> &str {
    edited.doc.text().lines().nth(row).unwrap_or("")
}

/// Non-lint codes, sorted.
fn codes(doc: &Doc) -> Vec<&'static str> {
    let mut codes: Vec<&'static str> =
        doc.diagnostics().iter().map(|d| d.code).filter(|c| !c.starts_with('L')).collect();
    codes.sort_unstable();
    codes.dedup();
    codes
}

/// main's outputs, as printed.
fn outputs(doc: &Doc) -> Vec<String> {
    let run = doc.run().unwrap_or_else(|| panic!("{:?}\n{}", doc.diagnostics(), doc.text()));
    run.unwrap().iter().map(ToString::to_string).collect()
}

/// Applies `steps` in turn from `doc`.
fn steps(doc: &Doc, steps: &[Edit]) -> Edited {
    let mut edited = Edited { doc: Doc::new(doc.text()), map: crate::Mapping::identity(doc.diagram().unwrap()), tidied: false };
    for step in steps {
        edited = apply(&edited.doc, step).unwrap_or_else(|r| panic!("{step}: {r:?}\n{}", edited.doc.text()));
    }
    edited
}

// ---------------------------------------------------------------------------
// Definitions and their ports.

#[test]
fn a_definition_goes_a_blank_line_below_what_ends_above_it() {
    let main = ok(place_definition(&Doc::new(""), rect(0, 0, 4, 20), "𐑥𐑱𐑯"));
    assert_eq!(line(&main, 0), "╭─ ·𐑥𐑱𐑯 ────────────╮");
    assert_eq!(line(&main, 2), format!("│{}│", " ".repeat(19)));
    assert_eq!(line(&main, 4), format!("╰{}╯", "─".repeat(19)));
    // Asked for far below, it comes up to one blank line under main.
    let two = ok(place_definition(&main.doc, rect(9, 0, 12, 20), "𐑛𐑳𐑚𐑩𐑤"));
    assert!(line(&two, 6).starts_with("╭─ ·𐑛𐑳𐑚𐑩𐑤 ─"), "{}", two.doc.text());
    // Between them, the one below moves down by whole rows, and its box and
    // title map there.
    let between = ok(place_definition(&two.doc, rect(5, 0, 7, 20), "𐑐𐑐"));
    assert!(line(&between, 6).starts_with("╭─ ·𐑐𐑐 ─"), "{}", between.doc.text());
    assert!(line(&between, 10).starts_with("╭─ ·𐑛𐑳𐑚𐑩𐑤 ─"), "{}", between.doc.text());
    let boxes = &between.doc.diagram().unwrap().boxes;
    assert_eq!(between.map.boxes, [Some(0), Some(2)]);
    assert_eq!(boxes[2].rect.top, 10);
    assert!(between.map.is_total());
    // Never beside another item, and only with a name and room for it.
    let beside = place_definition(&main.doc, rect(2, 30, 6, 50), "𐑐𐑐").unwrap_err();
    assert_eq!(beside.kind, RefusalKind::Blocked(Blockage::Spacing));
    let dotted = place_definition(&main.doc, rect(9, 0, 12, 20), "·𐑐𐑐").unwrap_err();
    assert_eq!(dotted.kind, RefusalKind::Target);
    let small = place_definition(&main.doc, rect(9, 0, 10, 20), "𐑐𐑐").unwrap_err();
    assert_eq!(small.kind, RefusalKind::Target);
}

#[test]
fn a_definition_takes_named_ports_on_its_sides() {
    let def = ok(place_definition(&Doc::new(""), rect(0, 0, 4, 20), "𐑛𐑳𐑚𐑩𐑤"));
    let input = ok(add_port(&def.doc, p(2, 0), Some("𐑯")));
    assert_eq!(line(&input, 2), format!("▶ 𐑯{}│", " ".repeat(17)));
    let both = ok(add_port(&input.doc, p(2, 20), Some("𐑬")));
    assert_eq!(line(&both, 2), format!("▶ 𐑯{}▶ 𐑬", " ".repeat(17)));
    assert!(codes(&both.doc).contains(&"Y019"), "{:?}", codes(&both.doc));
    // The input's name is a source: wiring it clears the arrowhead.
    let wired = ok(connect(&both.doc, p(2, 0), p(2, 20)));
    assert_eq!(line(&wired, 2), format!("▶ 𐑯 {}▶ 𐑬", "─".repeat(16)));
    assert!(!codes(&wired.doc).contains(&"Y019"), "{:?}", codes(&wired.doc));
    // A definition's port needs a name, and goes only on its sides.
    let nameless = add_port(&def.doc, p(2, 0), None).unwrap_err();
    assert_eq!(nameless.kind, RefusalKind::Target);
    let top = add_port(&def.doc, p(0, 15), Some("𐑯")).unwrap_err();
    assert_eq!(top.kind, RefusalKind::Target);
    let corner = add_port(&def.doc, p(4, 0), Some("𐑯")).unwrap_err();
    assert_eq!(corner.kind, RefusalKind::Target);
    let twice = add_port(&input.doc, p(2, 0), Some("𐑯")).unwrap_err();
    assert_eq!(twice.kind, RefusalKind::Target);
}

// ---------------------------------------------------------------------------
// Switches.

/// abs's switch, drawn again box first after removing it whole.
#[test]
fn a_switch_is_drawn_then_given_its_ports_and_arms() {
    let gone = ok(remove_box(&Doc::new(ABS), p(9, 12)));
    let text = gone.doc.text();
    assert!(!text.contains('┄') && !text.contains('▼'), "{text}");
    // Its outer wires went: 𐑤's output, and 𐑯's branches into it.
    let boxed = ok(place_switch(&gone.doc, rect(9, 12, 16, 26), Some("0")));
    assert_eq!(line(&boxed, 9), "│           ┌─ 0 ─────────┐       │");
    let drawn = codes(&boxed.doc);
    assert!(drawn.contains(&"Y002") && drawn.contains(&"Y016"), "{drawn:?}");
    let rebuilt = steps(
        &boxed.doc,
        &[
            Edit::AddPort { at: p(9, 20), name: None },
            Edit::AddPort { at: p(10, 12), name: None },
            Edit::AddPort { at: p(10, 26), name: None },
            Edit::AddPort { at: p(13, 26), name: None },
            Edit::AddPort { at: p(14, 12), name: None },
            Edit::AddArm { at: p(11, 12), guard: None },
            Edit::PlaceNode { corner: p(12, 18), word: "𐑚".into(), inputs: 2, outputs: 1 },
            Edit::PlaceText { at: p(13, 15), content: "0".into() },
            Edit::Connect { from: p(5, 18), to: p(9, 20) },
            Edit::Connect { from: p(5, 0), to: p(10, 12) },
            Edit::Connect { from: p(10, 12), to: p(10, 26) },
            Edit::Connect { from: p(13, 15), to: p(13, 18) },
            Edit::Connect { from: p(13, 22), to: p(13, 26) },
            Edit::Connect { from: p(10, 26), to: p(13, 34) },
            Edit::Connect { from: p(5, 0), to: p(14, 12) },
            Edit::Connect { from: p(14, 12), to: p(14, 18) },
        ],
    );
    assert_eq!(codes(&rebuilt.doc), Vec::<&str>::new(), "{}", rebuilt.doc.text());
    assert_eq!(outputs(&rebuilt.doc), outputs(&Doc::new(ABS)));
    // Its selector is one, its arms are split only at a blank row, and
    // only its left border starts one.
    let ported = ok(add_port(&boxed.doc, p(9, 20), None));
    assert_eq!(add_port(&ported.doc, p(9, 22), None).unwrap_err().kind, RefusalKind::Target);
    assert_eq!(add_port(&ported.doc, p(10, 12), Some("𐑯")).unwrap_err().kind, RefusalKind::Target);
    assert_eq!(add_arm(&ported.doc, p(11, 26), None).unwrap_err().kind, RefusalKind::Target);
    assert_eq!(add_arm(&ported.doc, p(10, 12), Some("𐑯")).unwrap_err().kind, RefusalKind::Target);
    let first = add_arm(&ported.doc, p(10, 12), None).unwrap_err();
    assert_eq!(first.kind, RefusalKind::Blocked(Blockage::NoRoom), "{first:?}");
    // It goes wholly inside one definition, clear of everything.
    let onto = place_switch(&Doc::new(ABS), rect(4, 3, 8, 12), None).unwrap_err();
    assert_eq!(onto.kind, RefusalKind::Blocked(Blockage::NoRoom));
    let latin = place_switch(&gone.doc, rect(9, 12, 16, 26), Some("𐑯")).unwrap_err();
    assert_eq!(latin.kind, RefusalKind::Target);
}

/// main with a one-arm switch whose row 6 holds a constant wired to its
/// right through-port.
fn crowded_arm() -> Doc {
    let edited = steps(
        &Doc::new(""),
        &[
            Edit::PlaceDefinition { rect: rect(0, 0, 10, 30), name: "𐑥𐑱𐑯".into() },
            Edit::PlaceSwitch { rect: rect(2, 2, 8, 18), guard: Some("1".into()) },
            Edit::AddPort { at: p(2, 8), name: None },
            Edit::AddPort { at: p(6, 18), name: None },
            Edit::PlaceText { at: p(6, 4), content: "5".into() },
            Edit::Connect { from: p(6, 4), to: p(6, 18) },
        ],
    );
    edited.doc
}

#[test]
fn make_room_gives_an_arm_a_blank_row() {
    let doc = crowded_arm();
    let edit = Edit::AddArm { at: p(6, 2), guard: None };
    let full = apply(&doc, &edit).unwrap_err();
    assert_eq!(full.kind, RefusalKind::Blocked(Blockage::NoRoom), "{full:?}");
    // The divider goes on the new blank row, and the constant's row moves
    // down into the new arm.
    let roomed = make_room(&doc, &edit).unwrap_or_else(|r| panic!("{r:?}\n{}", doc.text()));
    let text = roomed.edited.doc.text();
    assert_eq!(text.lines().nth(6), Some("│ ├┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┤           │"), "{text}");
    assert!(text.lines().nth(7).unwrap().contains("5 ───"), "{text}");
    assert_eq!(roomed.inserted.len(), 1);
    assert!(roomed.edited.map.is_total());
}

// ---------------------------------------------------------------------------
// Merged outputs.

#[test]
fn a_merged_output_is_drawn_as_the_corpus_draws_it() {
    let doc = Doc::new(ABS);
    let bare = ok(remove_wire(&doc, p(13, 28)));
    // From the first arm's port, or any arm's at that ordinal.
    for from in [p(10, 26), p(13, 26)] {
        let merged = ok(connect(&bare.doc, from, p(13, 34)));
        assert_eq!(merged.doc.text(), ABS);
    }
    // Its last sink takes its stubs and its run with it.
    let cut = ok(disconnect(&doc, p(13, 34)));
    assert_eq!(cut.doc.text(), bare.doc.text());
    // It is one source, named by its first arm's port.
    let (sources, sinks) = endpoints(&doc);
    assert!(sources.iter().any(|s| s.at == p(10, 26) && s.wired));
    assert!(!sources.iter().any(|s| s.at == p(13, 26)));
    assert!(sinks.iter().any(|s| s.at == p(13, 26) && s.wired));
    let diagram = doc.diagram().unwrap();
    let w = diagram.wires.iter().position(|w| w.cells.contains(&p(12, 29))).unwrap();
    assert_eq!(wire_sources(&doc)[w], Some(p(10, 26)));
}

#[test]
fn removing_a_through_port_takes_its_stub_of_the_merge() {
    let doc = Doc::new(ABS);
    let gone = ok(remove_port(&doc, p(13, 26)));
    assert_eq!(line(&gone, 13), "│     │     │  0 ─▶ 𐑚 ├   │  └────▶ 𐑬");
    assert!(codes(&gone.doc).contains(&"Y011"), "{:?}", codes(&gone.doc));
    // The other stub's port takes the last stub, and the merge, with it.
    let last = ok(remove_port(&gone.doc, p(10, 26)));
    assert!(!line(&last, 10).contains('┐'), "{}", last.doc.text());
    assert!(line(&last, 13).ends_with("│       ▶ 𐑬"), "{}", last.doc.text());
}

// ---------------------------------------------------------------------------
// Arms and boxes.

#[test]
fn removing_an_arm_takes_what_it_holds() {
    let doc = Doc::new(GUARDS);
    let gone = ok(remove_arm(&doc, p(10, 9)));
    let text = gone.doc.text();
    assert_eq!(text.lines().nth(10), Some("│ │               │  │    │"), "{text}");
    assert_eq!(text.lines().nth(11), Some("│ │               │  └────▶ 𐑬"), "{text}");
    assert_eq!(codes(&gone.doc), ["W001"]);
    assert_eq!(gone.map.boxes, (0..6).map(Some).collect::<Vec<_>>());
    // A guarded arm takes its guard; not a divider's cell is refused.
    let guarded = ok(remove_arm(&doc, p(8, 5)));
    assert!(!guarded.doc.text().contains("├┄ 2"), "{}", guarded.doc.text());
    assert_eq!(remove_arm(&doc, p(9, 9)).unwrap_err().kind, RefusalKind::Target);
}

#[test]
fn removing_a_definition_closes_up_its_rows() {
    let doc = Doc::new(ABS);
    let gone = ok(remove_box(&doc, p(2, 0)));
    let text = gone.doc.text();
    assert!(text.starts_with("※ 𐑨𐑚𐑕𐑩𐑤𐑵𐑑\n\n╭─ ·𐑥𐑱𐑯 "), "{text}");
    assert_eq!(text.lines().count(), 12);
    // Its calls stay, unknown now.
    assert!(codes(&gone.doc).contains(&"C001"), "{:?}", codes(&gone.doc));
    let main = abs_main_box();
    assert_eq!(gone.map.boxes[main], Some(0));
    // A node goes as remove_node takes it.
    let node = ok(remove_box(&doc, p(4, 14)));
    assert_eq!(node.doc.text(), ok(remove_node(&doc, p(4, 14))).doc.text());
}

fn abs_main_box() -> usize {
    Doc::new(ABS).diagram().unwrap().boxes.iter().position(|b| b.rect.top == 20).unwrap()
}

// ---------------------------------------------------------------------------
// Nodes: port counts and names.

#[test]
fn a_node_takes_new_port_counts_and_keeps_its_wires() {
    let doc = Doc::new(DOUBLE);
    let wide = ok(set_ports(&doc, p(4, 10), 2, 2));
    assert_eq!(line(&wide, 6), "│     └───▶   ├       │");
    assert_eq!(codes(&wide.doc), ["C002"]);
    assert!(wide.map.is_total());
    let back = ok(set_ports(&wide.doc, p(4, 10), 2, 1));
    assert_eq!(back.doc.text(), DOUBLE);
    // add_port on a node's border is one more on that side.
    assert_eq!(ok(add_port(&doc, p(6, 14), None)).doc.text(), wide.doc.text());
    // A dropped port's wire goes, and the box shrinks.
    let one = ok(set_ports(&doc, p(4, 10), 1, 1));
    assert_eq!(line(&one, 5), "▶ 𐑯 ──────▶ 𐑐 ├───────▶ 𐑬");
    assert_eq!(line(&one, 6), "│         └───┘       │");
    let two = ok(set_ports(&one.doc, p(4, 10), 2, 1));
    let rewired = ok(connect(&two.doc, p(5, 0), p(6, 10)));
    assert_eq!(outputs(&rewired.doc), outputs(&doc));
    // Only a node box with a word.
    assert_eq!(set_ports(&Doc::new(ABS), p(9, 12), 1, 1).unwrap_err().kind, RefusalKind::Target);
    assert_eq!(set_ports(&doc, p(2, 0), 1, 1).unwrap_err().kind, RefusalKind::Target);
}

#[test]
fn removing_a_node_port_moves_the_ports_below_it_up() {
    let doc = Doc::new(DOUBLE);
    let gone = ok(remove_port(&doc, p(5, 10)));
    let text = gone.doc.text();
    // 𐑐's second input is its first now, still fed by 𐑯.
    let node = &gone.doc.diagram().unwrap().boxes[1];
    assert_eq!(node.rect, rect(4, 10, 6, 14), "{text}");
    assert_eq!(codes(&gone.doc), ["C002"], "{text}");
    let sheet = gone.doc.sheet().unwrap();
    let edges: usize = sheet.definitions.iter().map(|d| d.body.edges.len()).sum();
    assert_eq!(edges, 4, "{text}");
}

#[test]
fn removing_a_definition_port_takes_its_name_and_wires() {
    let doc = Doc::new(DOUBLE);
    let input = ok(remove_port(&doc, p(5, 0)));
    assert_eq!(line(&input, 5), "│         ▶ 𐑐 ├───────▶ 𐑬");
    assert_eq!(line(&input, 6), "│         ▶   │       │");
    let output = ok(remove_port(&doc, p(5, 22)));
    assert_eq!(line(&output, 5), "▶ 𐑯 ──┬───▶ 𐑐 ├       │");
    assert_eq!(remove_port(&doc, p(3, 0)).unwrap_err().kind, RefusalKind::Target);
}

#[test]
fn renaming_a_node_redraws_its_box_and_its_wires_follow() {
    let doc = Doc::new(DOUBLE);
    let short = ok(set_text(&doc, p(14, 10), "𐑐𐑐"));
    assert_eq!(line(&short, 14), "│ 21 ───▶ 𐑐𐑐 ├────────▶ 𐑬");
    assert_eq!(line(&short, 13), "│       ┌────┐        │");
    assert_eq!(codes(&short.doc), ["C001"]);
    assert!(short.map.is_total());
    let back = ok(set_text(&short.doc, p(14, 10), "𐑛𐑳𐑚𐑩𐑤"));
    assert_eq!(back.doc.text(), DOUBLE);
    // Same width: only the word changes.
    let same = ok(set_text(&doc, p(5, 12), "𐑚"));
    assert_eq!(same.doc.text(), DOUBLE.replacen("▶ 𐑐 ├", "▶ 𐑚 ├", 1));
    assert_eq!(set_text(&doc, p(14, 10), "𐑐1").unwrap_err().kind, RefusalKind::Target);
    // An inline node's word grows at its right end; the wire gives way.
    let fanout = Doc::new(FANOUT);
    let grown = ok(set_text(&fanout, p(14, 10), "𐑖𐑖"));
    assert_eq!(line(&grown, 14), "│     └─▶ 𐑖𐑖 ─▶ 𐑠 ────────▶ 𐑔𐑮𐑰");
    assert_eq!(ok(set_text(&grown.doc, p(14, 10), "𐑖")).doc.text(), FANOUT);
}

// ---------------------------------------------------------------------------
// Inline nodes.

#[test]
fn an_inline_node_splices_out_and_back_in() {
    for (src, at, word) in [(FANOUT, p(14, 16), "𐑠"), (FANOUT, p(14, 10), "𐑖"), (INLINE, p(4, 28), "𐑖")] {
        let doc = Doc::new(src);
        let out = ok(remove_node(&doc, at));
        assert!(!out.doc.text().contains(&format!(" {word} ")), "{}", out.doc.text());
        // What fed the node feeds its branches now.
        assert_eq!(codes(&out.doc), Vec::<&str>::new(), "{}", out.doc.text());
        let back = ok(place_inline(&out.doc, at, word));
        assert_eq!(back.doc.text(), src);
        assert!(back.map.is_total());
    }
    let doc = Doc::new(FANOUT);
    // A stretch too short for the node, a cell off any wire, a word that
    // isn't Shavian.
    let short = place_inline(&doc, p(14, 13), "𐑖").unwrap_err();
    assert_eq!(short.kind, RefusalKind::Blocked(Blockage::NoRoom), "{short:?}");
    assert_eq!(place_inline(&doc, p(3, 10), "𐑖").unwrap_err().kind, RefusalKind::Target);
    assert_eq!(place_inline(&doc, p(14, 20), "x").unwrap_err().kind, RefusalKind::Target);
}

#[test]
fn an_inline_node_keeps_a_wire_in_and_a_wire_out() {
    let doc = Doc::new(FANOUT);
    let into = disconnect(&doc, p(14, 8)).unwrap_err();
    assert_eq!(into.kind, RefusalKind::Target);
    assert!(into.reason.contains("𐑖"), "{into:?}");
    // 𐑠 feeds a second sink; then its first can go, but not both.
    let free = ok(disconnect(&doc, p(10, 26)));
    let two = ok(connect(&free.doc, p(14, 16), p(10, 26)));
    let one = ok(disconnect(&two.doc, p(14, 26)));
    assert_eq!(disconnect(&one.doc, p(10, 26)).unwrap_err().kind, RefusalKind::Target);
    // Its word is a source, its arrowhead a sink that always has its wire.
    let (sources, sinks) = endpoints(&doc);
    assert!(sources.iter().any(|s| s.at == p(14, 10) && s.wired && s.span == 1));
    assert!(sinks.iter().any(|s| s.at == p(14, 8) && s.wired));
    assert!(preview(&free.doc, Anchor::Source(p(14, 16)), p(10, 26)).is_some());
    assert_eq!(connect(&doc, p(5, 2), p(14, 14)).unwrap_err().kind, RefusalKind::Target);
}

/// 13-fanout from an empty sheet, its inline chain wired straight through
/// from 7 before 7's other branches tee off it, then its nodes placed.
#[test]
fn a_fan_out_into_an_inline_chain_builds_chain_first() {
    let built = steps(
        &Doc::new(""),
        &[
            Edit::PlaceText { at: p(0, 0), content: "※ 𐑓𐑨𐑯".into() },
            Edit::PlaceDefinition { rect: rect(2, 0, 16, 26), name: "𐑥𐑱𐑯".into() },
            Edit::AddPort { at: p(5, 26), name: Some("𐑢𐑳𐑯".into()) },
            Edit::AddPort { at: p(10, 26), name: Some("𐑑𐑵".into()) },
            Edit::AddPort { at: p(14, 26), name: Some("𐑔𐑮𐑰".into()) },
            Edit::PlaceNode { corner: p(4, 10), word: "𐑐".into(), inputs: 2, outputs: 1 },
            Edit::PlaceNode { corner: p(9, 10), word: "𐑑".into(), inputs: 2, outputs: 1 },
            Edit::PlaceText { at: p(5, 2), content: "7".into() },
            Edit::Connect { from: p(5, 2), to: p(14, 26) },
            Edit::Connect { from: p(5, 2), to: p(5, 10) },
            Edit::Connect { from: p(5, 14), to: p(5, 26) },
            Edit::Connect { from: p(5, 2), to: p(6, 10) },
            Edit::Connect { from: p(5, 2), to: p(10, 10) },
            Edit::Connect { from: p(10, 14), to: p(10, 26) },
            Edit::Connect { from: p(5, 2), to: p(11, 10) },
            Edit::PlaceInline { at: p(14, 10), word: "𐑖".into() },
            Edit::PlaceInline { at: p(14, 16), word: "𐑠".into() },
        ],
    );
    assert_eq!(line(&built, 14), "│    └──▶ 𐑖 ──▶ 𐑠 ────────▶ 𐑔𐑮𐑰");
    assert_eq!(codes(&built.doc), Vec::<&str>::new());
    assert_eq!(outputs(&built.doc), outputs(&Doc::new(FANOUT)));
}

#[test]
fn a_constant_waits_before_an_arrowhead_until_it_is_wired() {
    let def = steps(
        &Doc::new(""),
        &[
            Edit::PlaceDefinition { rect: rect(0, 0, 6, 24), name: "𐑥𐑱𐑯".into() },
            Edit::AddPort { at: p(3, 24), name: Some("𐑬".into()) },
            Edit::PlaceNode { corner: p(2, 8), word: "𐑖".into(), inputs: 1, outputs: 1 },
        ],
    );
    let placed = ok(place_text(&def.doc, p(3, 3), "21"));
    assert_eq!(line(&placed, 3), format!("│  21   ▶ 𐑖 ├{}▶ 𐑬", " ".repeat(11)));
    let wired = steps(
        &placed.doc,
        &[Edit::Connect { from: p(3, 3), to: p(3, 8) }, Edit::Connect { from: p(3, 12), to: p(3, 24) }],
    );
    assert_eq!(line(&wired, 3), format!("│  21 ──▶ 𐑖 ├{}▶ 𐑬", "─".repeat(11)));
    assert_eq!(outputs(&wired.doc), ["21"]);
}
