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

mod order {
    //! Evaluation order around results-producing instructions (SPEC §10),
    //! built as IR so each case is exactly the shape under test.

    use shawline_grid::Pos;
    use shawline_syntax::ast::Literal;
    use shawline_vm::bytecode::{Instr, Prim, Value as VmValue};

    use crate::compile;
    use crate::ir::{Arm, Block, Function, Inst, Op, Program, Value};

    fn at(row: usize) -> Pos {
        Pos::new(row, 0)
    }

    fn inst(op: Op, row: usize) -> Inst {
        Inst { op, at: at(row) }
    }

    fn int(n: i64) -> Op {
        Op::Const(Literal::Int(n))
    }

    fn out(inst: u32) -> Value {
        Value::Out { inst, output: 0 }
    }

    fn prim(prim: Prim, args: &[Value]) -> Op {
        Op::Prim { prim, args: args.to_vec() }
    }

    fn function(name: &str, body: Block, row: usize) -> Function {
        Function { name: name.into(), body, at: at(row) }
    }

    /// `1 ÷ 0`, whose output nothing reads, at `row`.
    fn dead_division(first: u32, row: usize) -> [Inst; 3] {
        let div = prim(Prim::Div, &[out(first), out(first + 1)]);
        [inst(int(1), row), inst(int(0), row), inst(div, row)]
    }

    fn run(functions: Vec<Function>, main: u32) -> Result<Vec<VmValue>, String> {
        let module = compile(&Program { functions, main });
        shawline_vm::run(&module).map_err(|d| format!("{} {}", d.at, d.code))
    }

    /// A switch is not a tail call: its `R005` comes before a later node's
    /// `R002`, even though the switch's outputs are main's results.
    #[test]
    fn results_switch_keeps_its_place() {
        let body = Block { params: 0, insts: vec![inst(int(9), 2)], results: vec![out(0)] };
        let arm = Arm { guard: Some(Literal::Int(0)), args: Vec::new(), body };
        let mut insts = vec![
            inst(int(1), 1),
            inst(Op::Switch { selector: out(0), arms: vec![arm], outputs: 1 }, 2),
        ];
        insts.extend(dead_division(2, 3));
        let main = function("𐑥𐑱𐑯", Block { params: 0, insts, results: vec![out(1)] }, 0);
        assert_eq!(run(vec![main], 0), Err("2:0 R005".into()));
    }

    /// Main's calls are never tail calls, so a call feeding main's results
    /// runs before a later independent node.
    #[test]
    fn main_call_keeps_its_place() {
        let failing = Block {
            params: 0,
            insts: vec![
                inst(Op::Const(Literal::Text("𐑐".into())), 1),
                inst(prim(Prim::Read, &[out(0)]), 1),
            ],
            results: vec![out(1)],
        };
        let mut insts = vec![inst(Op::Call { func: 0, args: Vec::new() }, 5)];
        insts.extend(dead_division(1, 6));
        let main = function("𐑥𐑱𐑯", Block { params: 0, insts, results: vec![out(0)] }, 4);
        assert_eq!(run(vec![function("𐑓𐑱𐑤", failing, 0), main], 1), Err("1:0 R006".into()));
    }

    /// `·𐑒𐑬𐑯𐑑(n)`: a switch on `n` whose default arm tail-calls itself with
    /// `n − 1`, then `trailing` after the switch. With `fail_in_arm`, the arm
    /// first reads a word as a number, which fails.
    fn countdown(fail_in_arm: bool, trailing: Vec<Inst>) -> Function {
        let mut insts = vec![
            inst(int(1), 3),
            inst(prim(Prim::Sub, &[Value::Param(0), out(0)]), 3),
            inst(Op::Call { func: 0, args: vec![out(1)] }, 3),
        ];
        if fail_in_arm {
            insts.push(inst(Op::Const(Literal::Text("𐑐".into())), 4));
            insts.push(inst(prim(Prim::Read, &[out(3)]), 4));
        }
        let default = Block { params: 1, insts, results: vec![out(2)] };
        let base = Block { params: 0, insts: vec![inst(int(0), 2)], results: vec![out(0)] };
        let arms = vec![
            Arm { guard: Some(Literal::Int(0)), args: Vec::new(), body: base },
            Arm { guard: None, args: vec![Value::Param(0)], body: default },
        ];
        let mut insts = vec![inst(Op::Switch { selector: Value::Param(0), arms, outputs: 1 }, 1)];
        insts.extend(trailing);
        function("𐑒𐑬𐑯𐑑", Block { params: 1, insts, results: vec![out(0)] }, 0)
    }

    fn calling(n: i64) -> Function {
        let insts = vec![inst(int(n), 9), inst(Op::Call { func: 0, args: vec![out(0)] }, 9)];
        function("𐑥𐑱𐑯", Block { params: 0, insts, results: vec![out(1)] }, 8)
    }

    /// The arm's own nodes run where the switch is, before the nodes after
    /// it; only the tail call itself waits until last.
    #[test]
    fn switch_arm_runs_before_later_nodes() {
        let mut f = countdown(true, Vec::new());
        f.body.insts.extend(dead_division(1, 6));
        assert_eq!(run(vec![f, calling(3)], 1), Err("4:0 R006".into()));
    }

    /// Nodes after a tail-position switch still let its arms tail-call, and
    /// they run on every pass, before the tail call.
    #[test]
    fn deferred_nodes_keep_tail_calls() {
        // `10 ÷ n`: fails only once the countdown reaches 0.
        let trailing = vec![inst(int(10), 6), inst(prim(Prim::Div, &[out(1), Value::Param(0)]), 7)];
        let f = countdown(false, trailing.clone());
        let module = compile(&Program { functions: vec![f.clone(), calling(3)], main: 1 });
        assert!(module.functions[0].code.iter().any(|i| matches!(i, Instr::TailCall { .. })));
        assert_eq!(run(vec![f, calling(3)], 1), Err("7:0 R002".into()));

        // `10 + n` never fails, and deep recursion still runs in constant stack.
        let mut fine = trailing;
        fine[1].op = prim(Prim::Add, &[out(1), Value::Param(0)]);
        let f = countdown(false, fine);
        assert_eq!(run(vec![f, calling(2_000_000)], 1), Ok(vec![VmValue::Int(0)]));
    }
}
