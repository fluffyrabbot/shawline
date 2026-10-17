//! Lowering cases the corpus doesn't pin down.

use shawline_grid::{Grid, Pos, Rect};
use shawline_syntax::ast::{Definition, Edge, Endpoint, Graph, Name, Node, NodeKind, Sheet};
use shawline_vm::bytecode::{Instr, Value};

use crate::{compile, lower};

fn sheet(src: &str) -> Sheet {
    let grid = Grid::parse(src).unwrap();
    let diagram = shawline_shape::recognize(&grid);
    let (sheet, diags) = shawline_syntax::parse(&grid, &diagram);
    assert!(diagram.diagnostics.is_empty() && diags.is_empty(), "{diags:?}");
    sheet
}

fn codes(sheet: &Sheet) -> Vec<String> {
    lower(sheet).1.iter().map(|d| format!("{} {}", d.at, d.code)).collect()
}

fn run(src: &str) -> Result<Vec<Value>, String> {
    let (program, diags) = lower(&sheet(src));
    let program = program.unwrap_or_else(|| panic!("{diags:?}"));
    shawline_vm::run(&compile(&program)).map_err(|d| format!("{} {}", d.at, d.code))
}

#[test]
fn boxed_nil_is_the_empty_list() {
    let src = "╭─ ·𐑥𐑱𐑯 ─────╮\n│ ┌─────┐    │\n│ │ 𐑯𐑦𐑤 ├────▶ 𐑬\n│ └─────┘    │\n╰────────────╯\n";
    assert_eq!(run(src), Ok(vec![Value::List(None)]));
}

#[test]
fn inline_nil_is_an_arity_error() {
    let src = "╭─ ·𐑥𐑱𐑯 ─────────╮\n│ 1 ─▶ 𐑯𐑦𐑤 ──────▶ 𐑬\n╰────────────────╯\n";
    assert_eq!(codes(&sheet(src)), ["1:7 C002"]);
}

/// Independent nodes run in the row-major order of their words, not of their
/// boxes' corners: the inline `𐑠` fails before the taller box above it runs.
#[test]
fn independent_nodes_run_in_word_order() {
    let src = "\
╭─ ·𐑚𐑨𐑛 ─────────╮
│ 5 ─▶ 𐑠 ────────▶ 𐑬
╰────────────────╯

╭─ ·𐑥𐑱𐑯 ────────────────╮
│            ┌─────┐    │
│ 7 ─▶ 𐑠 ─┐  │     │    │
│         │  │ 𐑚𐑨𐑛 ├────▶ 𐑬
│         │  └─────┘    │
│         └─────────────▶ 𐑢
╰───────────────────────╯
";
    assert_eq!(run(src), Err("6:7 R004".into()));
}

/// Main keeps its frame to the end, so `R009` can point at its namer dot.
#[test]
fn main_returns_from_its_namer_dot() {
    let src = "\
╭─ ·𐑛𐑳𐑚 ─────────╮
│ 5 ─▶ 𐑖 ────────▶ 𐑬
╰────────────────╯

╭─ ·𐑥𐑱𐑯 ───────────╮
│ ┌─────┐          │
│ │ 𐑛𐑳𐑚 ├──────────▶ 𐑬
│ └─────┘          │
╰──────────────────╯
";
    let program = lower(&sheet(src)).0.unwrap();
    let module = compile(&program);
    let main = &module.functions[module.main as usize];
    let code: Vec<_> = main.code.iter().zip(&main.spans).collect();
    assert!(matches!(code[..], [(Instr::Call { .. }, _), (Instr::Return { .. }, &at)] if at == Pos::new(4, 3)));
    assert_eq!(run(src), Ok(vec![Value::Text("5".into())]));
}

fn node(corner: Pos, word: Pos) -> Node {
    let rect = Rect { top: corner.row, left: corner.col, bottom: word.row + 1, right: word.col + 2 };
    Node {
        kind: NodeKind::Call(Name { text: "𐑖".into(), at: word }),
        rect,
        inputs: vec![Pos::new(word.row, corner.col)],
        outputs: vec![Pos::new(word.row, word.col + 2)],
    }
}

fn main_with(nodes: Vec<Node>, edges: Vec<Edge>) -> Sheet {
    Sheet {
        definitions: vec![Definition {
            name: Name { text: "𐑥𐑱𐑯".into(), at: Pos::new(0, 3) },
            rect: Rect { top: 0, left: 0, bottom: 9, right: 30 },
            inputs: Vec::new(),
            outputs: Vec::new(),
            body: Graph { nodes, edges },
        }],
    }
}

/// The first node by corner has the later word; `C003` takes the earlier.
#[test]
fn cycle_points_at_its_first_word() {
    let a = node(Pos::new(1, 2), Pos::new(5, 4));
    let b = node(Pos::new(2, 10), Pos::new(3, 12));
    let edges = vec![
        Edge { wire: 0, from: Endpoint::Output(0, 0), to: vec![Endpoint::Input(1, 0)] },
        Edge { wire: 1, from: Endpoint::Output(1, 0), to: vec![Endpoint::Input(0, 0)] },
    ];
    assert_eq!(codes(&main_with(vec![a, b], edges)), ["3:12 C003"]);
}

/// A slot syntax should have flagged still gets a diagnostic, never a silent
/// `None`.
#[test]
fn unfed_slot_is_reported_by_lowering_too() {
    let (program, diags) = lower(&main_with(vec![node(Pos::new(1, 2), Pos::new(2, 4))], vec![]));
    assert!(program.is_none());
    let got: Vec<_> = diags.iter().map(|d| (d.at, d.code)).collect();
    assert_eq!(got, [(Pos::new(2, 2), "Y019")]);
}
