//! P10 wave 3: restyling, fixing slips, default arms, naming ports,
//! extracting definitions and repairs.

use shawline_doc::Doc;
use shawline_grid::Pos;

use crate::{add_default_arm, fix, restyle, set_text};

fn p(row: usize, col: usize) -> Pos {
    Pos::new(row, col)
}

#[test]
fn scratch_repairs() {
    for name in ["static/12-unused-input", "fix/03-too-far", "93-arity", "static/13-near-name", "98-no-main"] {
        let src = std::fs::read_to_string(format!("../../corpus/{name}.shaw")).unwrap();
        let doc = Doc::new(src);
        for r in crate::repairs(&doc) {
            println!("== {name} {} {} {:?} {:?} {:?}", r.at, r.code, r.label, r.edit.as_ref().map(ToString::to_string), r.then);
            if let Some(e) = &r.edit {
                let done = crate::apply(&doc, e).unwrap();
                println!("{}{:?}", done.doc.text(), done.doc.diagnostics().iter().map(|d| (d.at, d.code)).collect::<Vec<_>>());
            }
        }
    }
}

#[test]
fn scratch() {
    let cases = [
        ("static/07-rounded-node", "restyle", p(4, 6)),
        ("static/08-mixed-corners", "restyle", p(4, 6)),
        ("99-square-top", "round", p(2, 0)),
        ("fix/01-slipped-edge", "fix", p(7, 23)),
        ("fix/02-slipped-corner", "fix", p(7, 20)),
        ("static/11-no-default-arm", "arm", p(6, 2)),
        ("12-guards", "arm", p(6, 2)),
        ("static/05-unnamed-port", "name", p(6, 20)),
    ];
    for (name, what, at) in cases {
        let src = std::fs::read_to_string(format!("../../corpus/{name}.shaw")).unwrap();
        let doc = Doc::new(src);
        let r = match what {
            "restyle" => restyle(&doc, at, false),
            "round" => restyle(&doc, at, true),
            "fix" => fix(&doc, at),
            "arm" => add_default_arm(&doc, at),
            _ => set_text(&doc, at, "𐑑𐑵"),
        };
        match r {
            Ok(e) => println!("== {name} ok\n{}{:?}", e.doc.text(), e.doc.diagnostics()),
            Err(r) => println!("== {name} refused {r:?}"),
        }
    }
}
