//! P10 wave 3: restyling, fixing slips, default arms, naming ports,
//! dropping an unused input's arguments, connecting a dangling end,
//! extracting definitions, and repairs.

use shawline_doc::Doc;
use shawline_grid::{Pos, Rect};

use crate::{
    BoxKind, Edit, Edited, Followup, RefusalKind, add_default_arm, apply, box_kind, connect, extract, fix,
    make_room, remove_port, repairs, restyle, set_text,
};

fn corpus(name: &str) -> String {
    std::fs::read_to_string(format!("{}/../../corpus/{name}.shaw", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

fn p(row: usize, col: usize) -> Pos {
    Pos::new(row, col)
}

fn ok(result: Result<Edited, crate::Refusal>) -> Edited {
    let edited = result.unwrap_or_else(|r| panic!("refused: {r:?}"));
    let text = edited.doc.text();
    assert_eq!(Doc::new(text).formatted(), Some(text), "not canonical:\n{text}");
    edited
}

fn line(edited: &Edited, row: usize) -> &str {
    edited.doc.text().lines().nth(row).unwrap_or("")
}

/// Non-lint codes with their cells.
fn codes(doc: &Doc) -> Vec<(Pos, &'static str)> {
    doc.diagnostics().iter().filter(|d| !d.code.starts_with('L')).map(|d| (d.at, d.code)).collect()
}

/// main's outputs, as printed.
fn outputs(doc: &Doc) -> Vec<String> {
    let run = doc.run().unwrap_or_else(|| panic!("{:?}\n{}", doc.diagnostics(), doc.text()));
    run.unwrap().iter().map(ToString::to_string).collect()
}

// ---------------------------------------------------------------------------
// Restyling and fixing.

#[test]
fn restyling_redraws_only_the_corners() {
    let rounded = Doc::new(corpus("static/07-rounded-node"));
    let square = ok(restyle(&rounded, p(4, 6), false));
    assert_eq!(line(&square, 4), "│     ┌───┐         │");
    assert_eq!(line(&square, 6), "│     └───┘         │");
    assert_eq!(codes(&square.doc), []);
    assert!(square.map.is_total());
    // A definition drawn square is rounded.
    let top = ok(restyle(&Doc::new(corpus("99-square-top")), p(2, 0), true));
    assert!(line(&top, 2).starts_with("╭─ ·𐑥𐑱𐑯 ─"), "{}", top.doc.text());
    assert_eq!(codes(&top.doc), []);
    assert_eq!(restyle(&rounded, p(5, 6), true).unwrap_err().kind, RefusalKind::Target);
}

#[test]
fn fixing_moves_every_slip_home_as_fmt_fix_does() {
    for name in ["fix/01-slipped-edge", "fix/02-slipped-corner"] {
        let doc = Doc::new(corpus(name));
        let at = doc.diagnostics().iter().find(|d| d.code == "S012").unwrap().at;
        let fixed = ok(fix(&doc, at));
        assert_eq!(fixed.doc.text(), corpus(&format!("{name}.fix")), "{name}");
        assert!(fixed.map.is_total(), "{name}");
    }
    let whole = Doc::new(corpus("03-double"));
    assert_eq!(fix(&whole, p(0, 0)).unwrap_err().kind, RefusalKind::Target);
}

// ---------------------------------------------------------------------------
// Ports and arms.

#[test]
fn naming_an_unnamed_port_writes_its_name_after_the_arrowhead() {
    let doc = Doc::new(corpus("static/05-unnamed-port"));
    let named = ok(set_text(&doc, p(6, 20), "𐑑𐑵"));
    assert_eq!(line(&named, 6), "│ 2 ────────────────▶ 𐑑𐑵");
    assert_eq!(codes(&named.doc), []);
    assert!(named.map.is_total());
    assert_eq!(set_text(&doc, p(6, 20), "21").unwrap_err().kind, RefusalKind::Target);
}

#[test]
fn a_default_arm_has_a_port_per_result_joined_to_its_merge() {
    let doc = Doc::new(corpus("static/11-no-default-arm"));
    let armed = ok(add_default_arm(&doc, p(6, 2)));
    let text = armed.doc.text();
    // The divider and the arm go in above the bottom border, and the arm's
    // port joins the merge from below.
    assert_eq!(line(&armed, 10), "│ ├┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┤ │     │", "{text}");
    assert!(line(&armed, 11).starts_with("│ │               ▶─"), "{text}");
    assert_eq!(codes(&armed.doc), [(p(11, 18), "Y019")], "{text}");
    let switch = &armed.doc.diagram().unwrap().boxes[armed.map.boxes[1].unwrap()];
    assert_eq!(switch.dividers.len(), 2);
    // A switch with a default arm, and a node, are refused.
    let guards = Doc::new(corpus("12-guards"));
    assert_eq!(add_default_arm(&guards, p(6, 2)).unwrap_err().kind, RefusalKind::Target);
    let double = Doc::new(corpus("03-double"));
    assert_eq!(add_default_arm(&double, p(4, 10)).unwrap_err().kind, RefusalKind::Target);
}

#[test]
fn removing_an_unused_input_drops_each_matching_call_s_argument() {
    let doc = Doc::new(corpus("static/12-unused-input"));
    let gone = ok(remove_port(&doc, p(6, 0)));
    let text = gone.doc.text();
    assert_eq!(codes(&gone.doc), [], "{text}");
    // main's call takes one input, and the constant that fed the other goes.
    assert_eq!(line(&gone, 13), "│  1 ──▶ 𐑒𐑰𐑐 ├─────────▶ 𐑬", "{text}");
    assert!(!text.contains(" 2 "), "{text}");
    assert_eq!(outputs(&gone.doc), ["1"]);
    // A used input's calls keep their arguments, for set_ports to match.
    let double = ok(remove_port(&Doc::new(corpus("03-double")), p(5, 0)));
    assert_eq!(line(&double, 14), "│ 21 ───▶ 𐑛𐑳𐑚𐑩𐑤 ├─────▶ 𐑬", "{}", double.doc.text());
}

/// main with a dangling wire and an unwired output.
const DANGLING: &str = "\
╭─ ·𐑥𐑱𐑯 ────────────╮
│                   │
│ 1 ────────────────▶ 𐑬
│                   │
│ 2 ──────          ▶ 𐑚
│                   │
╰───────────────────╯
";

#[test]
fn connecting_a_dangling_end_runs_its_wire_on() {
    let doc = Doc::new(DANGLING);
    let wired = ok(connect(&doc, p(4, 9), p(4, 20)));
    assert_eq!(line(&wired, 4), "│ 2 ────────────────▶ 𐑚");
    assert_eq!(codes(&wired.doc), []);
    assert_eq!(outputs(&wired.doc), ["1", "2"]);
    // Any other wire cell is still not a source.
    assert_eq!(connect(&doc, p(4, 6), p(4, 20)).unwrap_err().kind, RefusalKind::Target);
}

// ---------------------------------------------------------------------------
// Extracting a definition.

#[test]
fn extracting_a_node_moves_it_below_the_sheet_and_calls_it() {
    let doc = Doc::new(corpus("03-double"));
    let extracted = extract(&doc, &[p(4, 10)], "𐑐𐑸𐑑").unwrap();
    let edited = ok(Ok(extracted.edited));
    let text = edited.doc.text();
    assert_eq!((extracted.definition, extracted.call), (p(19, 0), p(4, 10)));
    // One input per source: 𐑯 fed both of 𐑐's inputs.
    assert_eq!(line(&edited, 5), "▶ 𐑯 ──────▶ 𐑐𐑸𐑑 ├─────▶ 𐑬", "{text}");
    assert!(line(&edited, 19).starts_with("╭─ ·𐑐𐑸𐑑 ─"), "{text}");
    assert_eq!(line(&edited, 22), "▶ 𐑯 ──┬─▶ 𐑐 ├────▶ 𐑨", "{text}");
    assert_eq!(outputs(&edited.doc), outputs(&doc));
    // The node and its word map to their copies.
    let new = edited.doc.diagram().unwrap();
    let node = edited.map.boxes[1].unwrap();
    assert_eq!(new.boxes[node].rect, Rect { top: 21, left: 8, bottom: 24, right: 12 });
    let word = doc.diagram().unwrap().texts.iter().position(|t| t.content == "𐑐").unwrap();
    assert_eq!(new.texts[edited.map.texts[word].unwrap()].pos, p(22, 10));
    assert_eq!(box_kind(&edited.doc, p(4, 10)), Some(BoxKind::Node));
}

#[test]
fn extracting_several_parts_or_a_switch_keeps_the_program() {
    for (name, picks) in [
        ("11-even-odd", vec![p(9, 9)]),
        ("04-abs", vec![p(12, 18), p(13, 15)]),
        ("02-arith", vec![p(4, 8), p(5, 3), p(6, 3)]),
    ] {
        let doc = Doc::new(corpus(name));
        let edited = ok(extract(&doc, &picks, "𐑐𐑸𐑑").map(|e| e.edited));
        assert_eq!(codes(&edited.doc), [], "{name}\n{}", edited.doc.text());
        assert_eq!(outputs(&edited.doc), outputs(&doc), "{name}\n{}", edited.doc.text());
        assert!(edited.map.is_total(), "{name}");
    }
}

#[test]
fn extract_refuses_what_it_cannot_name() {
    let doc = Doc::new(corpus("04-abs"));
    let target = |picks: &[Pos], name: &str| extract(&doc, picks, name).unwrap_err().kind;
    // A name of one letter, or one a definition has.
    assert_eq!(target(&[p(4, 14)], "𐑐"), RefusalKind::Target);
    assert_eq!(target(&[p(4, 14)], "𐑨𐑚𐑕"), RefusalKind::Target);
    // A cell naming no part, and parts in two scopes.
    assert_eq!(target(&[p(5, 4)], "𐑐𐑸𐑑"), RefusalKind::Target);
    assert_eq!(target(&[p(4, 14), p(12, 18)], "𐑐𐑸𐑑"), RefusalKind::Target);
    // The definition itself is no pick.
    assert_eq!(target(&[p(2, 0)], "𐑐𐑸𐑑"), RefusalKind::Target);
}

#[test]
fn make_room_widens_the_sheet_for_a_wider_call() {
    let doc = Doc::new(corpus("07-list"));
    let edit = Edit::Extract { picks: vec![p(4, 32)], name: "𐑐𐑸𐑑".to_owned() };
    assert_eq!(apply(&doc, &edit).unwrap_err().kind, RefusalKind::Blocked(crate::Blockage::NoRoom));
    let roomed = make_room(&doc, &edit).unwrap();
    assert!(!roomed.inserted.is_empty());
    assert_eq!(outputs(&roomed.edited.doc), outputs(&doc), "{}", roomed.edited.doc.text());
}

#[test]
fn an_inline_node_gives_way_to_its_call_in_its_row() {
    // 𐑖 feeds 𐑠's arrowhead, which can't be left without its wire, so the
    // call is drawn over 𐑖, ports on its row, once Make room has stretched
    // the output wire for it.
    let doc = Doc::new(corpus("05-inline"));
    let edit = Edit::Extract { picks: vec![p(4, 28)], name: "𐑐𐑸𐑑".to_owned() };
    assert_eq!(apply(&doc, &edit).unwrap_err().kind, RefusalKind::Blocked(crate::Blockage::NoRoom));
    let edited = ok(make_room(&doc, &edit).map(|r| r.edited));
    let text = edited.doc.text();
    assert_eq!(line(&edited, 3), "│                         ┌─────┐                   │", "{text}");
    assert_eq!(line(&edited, 4), "│ 5 ──▶ 𐑛𐑳𐑚𐑩𐑤 ──▶ 𐑛𐑳𐑚𐑩𐑤 ──▶ 𐑐𐑸𐑑 ├─▶ 𐑠 ──▶ 𐑛𐑳𐑚𐑩𐑤 ────▶ 𐑬", "{text}");
    assert_eq!(codes(&edited.doc), [], "{text}");
    assert_eq!(outputs(&edited.doc), outputs(&doc), "{text}");
    // A constant feeding an inline node goes the same way.
    let edit = Edit::Extract { picks: vec![p(4, 2)], name: "𐑐𐑸𐑑".to_owned() };
    let edited = ok(make_room(&doc, &edit).map(|r| r.edited));
    assert!(line(&edited, 4).starts_with("│ │ 𐑐𐑸𐑑 ├─▶ 𐑛𐑳𐑚𐑩𐑤 ──▶"), "{}", edited.doc.text());
    assert_eq!(outputs(&edited.doc), outputs(&doc));
}

#[test]
fn a_call_with_no_room_at_its_pick_goes_to_the_nearest_clear_spot() {
    // 04-abs's 0 sits against the 𐑤 node it feeds, so its call can't take
    // its corner; there is clear sheet in the definition to put it in.
    let doc = Doc::new(corpus("04-abs"));
    let extracted = extract(&doc, &[p(6, 11)], "𐑐𐑸𐑑").unwrap();
    assert_ne!(extracted.call, p(6, 11));
    let edited = ok(Ok(extracted.edited));
    assert_eq!(codes(&edited.doc), [], "{}", edited.doc.text());
    assert_eq!(outputs(&edited.doc), outputs(&doc), "{}", edited.doc.text());
}

#[test]
fn every_inline_node_and_constant_extracts_at_most_after_make_room() {
    for name in ["02-arith", "04-abs", "05-inline", "11-even-odd"] {
        let doc = Doc::new(corpus(name));
        let diagram = doc.diagram().unwrap();
        for t in diagram.texts.iter().map(|t| t.pos) {
            let edit = Edit::Extract { picks: vec![t], name: "𐑐𐑸𐑑".to_owned() };
            let edited = match apply(&doc, &edit) {
                Err(r) if r.kind == RefusalKind::Target => continue,
                Err(_) => make_room(&doc, &edit).map(|r| r.edited).unwrap_or_else(|r| panic!("{name} {t:?}: {r:?}")),
                Ok(edited) => edited,
            };
            let text = edited.doc.text();
            assert_eq!(codes(&edited.doc), codes(&doc), "{name} {t:?}\n{text}");
            assert_eq!(outputs(&edited.doc), outputs(&doc), "{name} {t:?}\n{text}");
        }
    }
}

// ---------------------------------------------------------------------------
// Repairs.

/// Each repair of corpus case `name`, as `.repair` names it: its cell,
/// code, edit and follow-up.
fn offered(name: &str) -> Vec<(Pos, &'static str, Option<String>, Followup)> {
    repairs(&Doc::new(corpus(name))).into_iter().map(|r| (r.at, r.code, r.edit.map(|e| e.to_string()), r.then)).collect()
}

#[test]
fn repairs_follow_the_table() {
    let edit = |e: &str| Some(e.to_owned());
    assert_eq!(offered("97-dangling"), [(p(6, 9), "Y003", edit("remove_text 6:2"), Followup::None)]);
    assert_eq!(
        offered("static/04-dangling-arrowhead"),
        [(p(6, 5), "Y003", edit("remove_wire 6:5"), Followup::None), (p(6, 11), "Y004", edit("remove_wire 6:11"), Followup::None)]
    );
    // Nothing in reach takes the stray 7, so it goes.
    assert_eq!(offered("static/06-stray-text"), [(p(6, 2), "Y012", edit("remove_text 6:2"), Followup::None)]);
    assert_eq!(offered("static/05-unnamed-port"), [(p(6, 20), "Y009", None, Followup::NameField(p(6, 20)))]);
    assert_eq!(offered("static/03-no-namer-dot"), [(p(2, 3), "Y001", edit("set_text 2:3 ·𐑥𐑱𐑯"), Followup::None)]);
    assert_eq!(offered("static/08-mixed-corners"), [(p(4, 6), "Y015", edit("restyle 4:6 square"), Followup::None)]);
    assert_eq!(offered("99-square-top"), [(p(2, 0), "Y018", edit("restyle 2:0 rounded"), Followup::None)]);
    assert_eq!(offered("static/13-near-name"), [(p(14, 9), "C001", edit("set_text 14:9 𐑛𐑳𐑚𐑩𐑤"), Followup::None)]);
    // Two edits from every name: no guess.
    assert_eq!(offered("95-unknown"), []);
    assert_eq!(
        offered("static/10-main-inputs"),
        [(p(2, 3), "C006", edit("remove_port 4:0"), Followup::None), (p(4, 0), "W002", edit("remove_port 4:0"), Followup::None)]
    );
    assert_eq!(offered("fix/01-slipped-edge"), [(p(7, 23), "S012", edit("fix 7:23"), Followup::None)]);
    // A follow-up's cell is on the sheet the edit leaves: the new arm's port.
    assert_eq!(
        offered("static/11-no-default-arm"),
        [(p(6, 9), "W001", edit("add_default_arm 6:2"), Followup::DrawWire(p(11, 18)))]
    );
    assert_eq!(offered("93-arity"), [(p(5, 8), "C002", edit("set_ports 4:6 2 1"), Followup::DrawWire(p(6, 6)))]);
}

#[test]
fn a_dangling_end_is_connected_before_it_is_removed() {
    let doc = Doc::new(DANGLING);
    let all = repairs(&doc);
    let y003 = all.iter().find(|r| r.code == "Y003").unwrap();
    assert_eq!(y003.edit, Some(Edit::Connect { from: p(4, 9), to: p(4, 20) }));
    assert_eq!(y003.label, "Connect the wire to the nearest free arrowhead");
}

#[test]
fn labels_say_fix_exactly_when_a_follow_up_remains() {
    let cases = [
        "static/01-unfed-port",
        "static/02-reserved-nil",
        "static/05-unnamed-port",
        "static/06-stray-text",
        "static/09-duplicate-name",
        "static/11-no-default-arm",
        "93-arity",
        "97-dangling",
        "static/13-near-name",
    ];
    for name in cases {
        for r in repairs(&Doc::new(corpus(name))) {
            assert_eq!(r.label.starts_with("Fix…"), r.then != Followup::None, "{name}: {r:?}");
        }
    }
}

#[test]
fn on_a_badly_broken_sheet_only_clean_repairs_are_offered() {
    let doc = Doc::new(corpus("fix/03-too-far"));
    let errors = |d: &Doc| d.diagnostics().iter().filter(|x| x.severity() == shawline_grid::Severity::Error).count();
    let all = repairs(&doc);
    assert!(!all.is_empty());
    for r in all.iter().filter(|r| r.edit.is_some()) {
        let done = apply(&doc, r.edit.as_ref().unwrap()).unwrap();
        assert!(errors(&done.doc) < errors(&doc), "{r:?}");
    }
    // Removing the lone arrowhead at 5:0 would leave its neighbours dangling.
    assert!(!all.iter().any(|r| r.at == p(5, 0) && r.code == "Y004"));
}

#[test]
fn a_stray_literal_is_wired_where_an_arrowhead_can_take_it() {
    let sheet = "\
╭─ ·𐑥𐑱𐑯 ────────────╮
│                   │
│ 1 ────────────────▶ 𐑬
│                   │
│ 7                 ▶ 𐑑𐑵
│                   │
╰───────────────────╯
";
    let offered: Vec<_> = repairs(&Doc::new(sheet)).into_iter().map(|r| (r.at, r.code, r.edit, r.then)).collect();
    assert_eq!(
        offered,
        [
            (p(4, 2), "Y012", None, Followup::DrawWire(p(4, 2))),
            (p(4, 20), "Y019", None, Followup::DrawWire(p(4, 20))),
        ]
    );
}
