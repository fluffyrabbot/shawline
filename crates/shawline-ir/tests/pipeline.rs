//! Hand-built sheets, compact equivalents of corpus programs, run through
//! `lower` → `compile` → `shawline_vm::run`.

use shawline_grid::{Diagnostic, Pos, Rect};
use shawline_ir::{compile, lower};
use shawline_syntax::ast::{
    Arm, Definition, Edge, Endpoint, Graph, Guard, Literal, Name, Node, NodeId, NodeKind, PortDecl,
    Sheet, Switch,
};
use shawline_vm::bytecode::{Instr, Module, Value};

// ---- building sheets ----

const P: fn(u32) -> Endpoint = Endpoint::Param;
const RES: fn(u32) -> Endpoint = Endpoint::Result;
const IN: fn(NodeId, u32) -> Endpoint = Endpoint::Input;
const OUT: fn(NodeId, u32) -> Endpoint = Endpoint::Output;

fn pos(row: usize, col: usize) -> Pos {
    Pos::new(row, col)
}

fn name(text: &str, row: usize, col: usize) -> Name {
    Name {
        text: text.into(),
        at: pos(row, col),
    }
}

struct G(Graph);

impl Default for G {
    fn default() -> Self {
        G(Graph {
            nodes: Vec::new(),
            edges: Vec::new(),
        })
    }
}

impl G {
    fn push(&mut self, kind: NodeKind, rect: Rect, inputs: Vec<Pos>, outputs: Vec<Pos>) -> NodeId {
        self.0.nodes.push(Node {
            kind,
            rect,
            inputs,
            outputs,
        });
        self.0.nodes.len() - 1
    }

    /// A boxed call whose word starts at `(row, col)`; the box's top-left
    /// corner is one row up and two columns left.
    fn call(&mut self, word: &str, row: usize, col: usize, ins: usize, outs: usize) -> NodeId {
        let right = col + word.chars().count() + 1;
        let rect = Rect {
            top: row - 1,
            left: col - 2,
            bottom: row + ins.max(outs),
            right,
        };
        let inputs = (0..ins).map(|i| pos(row + i, col - 2)).collect();
        let outputs = (0..outs).map(|i| pos(row + i, right)).collect();
        self.push(NodeKind::Call(name(word, row, col)), rect, inputs, outputs)
    }

    fn inline(&mut self, word: &str, row: usize, col: usize) -> NodeId {
        let right = col + word.chars().count() - 1;
        let rect = Rect {
            top: row,
            left: col,
            bottom: row,
            right,
        };
        let (input, output) = (pos(row, col - 2), pos(row, right + 2));
        self.push(
            NodeKind::Inline(name(word, row, col)),
            rect,
            vec![input],
            vec![output],
        )
    }

    fn lit(&mut self, lit: Literal, row: usize, col: usize) -> NodeId {
        let rect = Rect {
            top: row,
            left: col,
            bottom: row,
            right: col,
        };
        self.push(NodeKind::Const(lit), rect, vec![], vec![pos(row, col + 2)])
    }

    fn int(&mut self, n: i64, row: usize, col: usize) -> NodeId {
        self.lit(Literal::Int(n), row, col)
    }

    /// A switch whose selector `▼` is at `(row, col)`.
    fn switch(&mut self, row: usize, col: usize, arms: Vec<Arm>, outs: usize) -> NodeId {
        let ins = 1 + arms.iter().map(|a| a.params.len()).sum::<usize>();
        let bottom = arms.last().map_or(row + 1, |a| a.rows.1);
        let rect = Rect {
            top: row,
            left: col - 2,
            bottom,
            right: col + 20,
        };
        let mut inputs = vec![pos(row, col)];
        inputs.extend(arms.iter().flat_map(|a| a.params.clone()));
        let outputs = arms.first().map_or_else(Vec::new, |a| a.results.clone());
        assert_eq!((inputs.len(), outputs.len()), (ins, outs));
        let sw = Switch {
            selector: pos(row, col),
            arms,
        };
        self.push(NodeKind::Switch(sw), rect, inputs, outputs)
    }

    fn wire(&mut self, from: Endpoint, to: &[Endpoint]) {
        let wire = self.0.edges.len();
        self.0.edges.push(Edge {
            wire,
            from,
            to: to.to_vec(),
        });
    }
}

/// An arm on interior rows `start..start + 2`.
fn arm(guard: Option<Literal>, start: usize, params: usize, results: usize, body: G) -> Arm {
    Arm {
        guard: guard.map(|value| Guard {
            value,
            at: pos(start - 1, 5),
        }),
        rows: (start, start + 2),
        params: (0..params).map(|i| pos(start + i, 2)).collect(),
        results: (0..results).map(|i| pos(start + i, 22)).collect(),
        body: body.0,
    }
}

/// A definition whose namer dot is at `(row, col + 3)`.
fn def(text: &str, row: usize, ins: usize, outs: usize, body: G) -> Definition {
    let port = |i: usize, col: usize| PortDecl {
        arrow: pos(row + 2 + i, col),
        name: name("𐑯", row + 2 + i, col + 2),
    };
    Definition {
        name: name(text, row, 3),
        rect: Rect {
            top: row,
            left: 0,
            bottom: row + 20,
            right: 40,
        },
        inputs: (0..ins).map(|i| port(i, 0)).collect(),
        outputs: (0..outs).map(|i| port(i, 40)).collect(),
        body: body.0,
    }
}

/// `main` calling the unary definition `callee` once per argument, one call
/// per output.
fn main_calling(callee: &str, args: &[Literal], row: usize) -> Definition {
    let mut g = G::default();
    for (i, a) in args.iter().enumerate() {
        let c = g.lit(a.clone(), row + 3 + 4 * i, 3);
        let n = g.call(callee, row + 3 + 4 * i, 10, 1, 1);
        g.wire(OUT(c, 0), &[IN(n, 0)]);
        g.wire(OUT(n, 0), &[RES(i as u32)]);
    }
    def("𐑥𐑱𐑯", row, 0, args.len(), g)
}

/// `𐑦(n, 0)` feeding a fresh switch's selector: the shape of 08, 10 and 11.
fn is_zero(g: &mut G) -> NodeId {
    let zero = g.int(0, 4, 11);
    let eq = g.call("𐑦", 3, 16, 2, 1);
    g.wire(OUT(zero, 0), &[IN(eq, 1)]);
    eq
}

// ---- running ----

fn compiled(sheet: &Sheet) -> Module {
    let (program, diags) = lower(sheet);
    assert!(diags.iter().all(Diagnostic::is_warning), "{diags:?}");
    compile(&program.expect("a program"))
}

fn run(sheet: &Sheet) -> Result<Vec<Value>, Diagnostic> {
    shawline_vm::run(&compiled(sheet))
}

fn assert_values(sheet: &Sheet, expected: &[Value]) {
    let got = run(sheet).expect("runs");
    assert_eq!(got.len(), expected.len(), "{got:?}");
    for (g, e) in got.iter().zip(expected) {
        assert!(shawline_vm::equal(g, e), "got {g}, expected {e}");
    }
}

fn codes(sheet: &Sheet) -> Vec<(Pos, &'static str)> {
    let mut d: Vec<_> = lower(sheet).1.into_iter().map(|d| (d.at, d.code)).collect();
    d.sort();
    d
}

fn has(m: &Module, func: &str, pred: fn(&Instr) -> bool) -> bool {
    m.functions
        .iter()
        .find(|f| f.name == func)
        .expect("function")
        .code
        .iter()
        .any(pred)
}

fn int(n: i64) -> Value {
    Value::Int(n)
}

fn text(t: &str) -> Value {
    Value::Text(t.into())
}

// ---- corpus programs ----

/// corpus/04-abs
fn abs_sheet() -> Sheet {
    let mut g = G::default();
    let lt = g.call("𐑤", 3, 16, 2, 1);
    let zero = g.int(0, 4, 11);
    let mut keep = G::default();
    keep.wire(P(0), &[RES(0)]);
    let mut negate = G::default();
    let z = negate.int(0, 11, 15);
    let sub = negate.call("𐑚", 11, 20, 2, 1);
    negate.wire(OUT(z, 0), &[IN(sub, 0)]);
    negate.wire(P(0), &[IN(sub, 1)]);
    negate.wire(OUT(sub, 0), &[RES(0)]);
    let arms = vec![
        arm(Some(Literal::Int(0)), 8, 1, 1, keep),
        arm(None, 10, 1, 1, negate),
    ];
    let sw = g.switch(7, 20, arms, 1);
    g.wire(P(0), &[IN(lt, 0), IN(sw, 1), IN(sw, 2)]);
    g.wire(OUT(zero, 0), &[IN(lt, 1)]);
    g.wire(OUT(lt, 0), &[IN(sw, 0)]);
    g.wire(OUT(sw, 0), &[RES(0)]);
    let abs = def("𐑨𐑚𐑕", 2, 1, 1, g);

    let mut m = G::default();
    let sub = m.call("𐑚", 23, 8, 2, 1);
    let (a, b) = (m.int(0, 23, 3), m.int(7, 24, 3));
    let abs1 = m.call("𐑨𐑚𐑕", 23, 18, 1, 1);
    let five = m.int(5, 26, 3);
    let abs2 = m.call("𐑨𐑚𐑕", 26, 18, 1, 1);
    m.wire(OUT(a, 0), &[IN(sub, 0)]);
    m.wire(OUT(b, 0), &[IN(sub, 1)]);
    m.wire(OUT(sub, 0), &[IN(abs1, 0)]);
    m.wire(OUT(abs1, 0), &[RES(0)]);
    m.wire(OUT(five, 0), &[IN(abs2, 0)]);
    m.wire(OUT(abs2, 0), &[RES(1)]);
    Sheet {
        definitions: vec![abs, def("𐑥𐑱𐑯", 20, 0, 2, m)],
    }
}

#[test]
fn abs() {
    assert_values(&abs_sheet(), &[int(7), int(5)]);
}

/// corpus/08-factorial: `𐑑(𐑓𐑨𐑒𐑑(n 𐑚 1), n)` is not a tail call.
#[test]
fn factorial() {
    let mut g = G::default();
    let eq = is_zero(&mut g);
    let mut base = G::default();
    let one = base.int(1, 8, 13);
    base.wire(OUT(one, 0), &[RES(0)]);
    let mut step = G::default();
    let sub = step.call("𐑚", 11, 16, 2, 1);
    let rec = step.call("𐑓𐑨𐑒𐑑", 11, 24, 1, 1);
    let one = step.int(1, 12, 11);
    let mul = step.call("𐑑", 11, 36, 2, 1);
    step.wire(P(0), &[IN(sub, 0), IN(mul, 1)]);
    step.wire(OUT(one, 0), &[IN(sub, 1)]);
    step.wire(OUT(sub, 0), &[IN(rec, 0)]);
    step.wire(OUT(rec, 0), &[IN(mul, 0)]);
    step.wire(OUT(mul, 0), &[RES(0)]);
    let arms = vec![
        arm(Some(Literal::Int(1)), 8, 0, 1, base),
        arm(None, 10, 1, 1, step),
    ];
    let sw = g.switch(7, 20, arms, 1);
    g.wire(P(0), &[IN(eq, 0), IN(sw, 1)]);
    g.wire(OUT(eq, 0), &[IN(sw, 0)]);
    g.wire(OUT(sw, 0), &[RES(0)]);
    let fact = def("𐑓𐑨𐑒𐑑", 2, 1, 1, g);
    let main = main_calling("𐑓𐑨𐑒𐑑", &[Literal::Int(10), Literal::Int(20)], 20);
    let sheet = Sheet {
        definitions: vec![fact, main],
    };

    assert_values(&sheet, &[int(3_628_800), int(2_432_902_008_176_640_000)]);
    let m = compiled(&sheet);
    assert!(!has(&m, "𐑓𐑨𐑒𐑑", |i| matches!(
        i,
        Instr::TailCall { .. }
    )));
    assert!(has(&m, "𐑓𐑨𐑒𐑑", |i| matches!(
        i,
        Instr::Call { .. }
    )));
}

/// `·𐑒𐑬𐑯𐑑`: 0 when `n` is 0, else itself on `n 𐑚 1`, through a switch arm.
fn countdown(name: &str, callee: &str, base_value: Option<i64>) -> Definition {
    let mut g = G::default();
    let eq = is_zero(&mut g);
    let mut base = G::default();
    let base_params = match base_value {
        Some(v) => {
            let c = base.int(v, 8, 13);
            base.wire(OUT(c, 0), &[RES(0)]);
            0
        }
        None => {
            base.wire(P(0), &[RES(0)]);
            1
        }
    };
    let mut step = G::default();
    let sub = step.call("𐑚", 11, 16, 2, 1);
    let one = step.int(1, 12, 13);
    let rec = step.call(callee, 11, 26, 1, 1);
    step.wire(P(0), &[IN(sub, 0)]);
    step.wire(OUT(one, 0), &[IN(sub, 1)]);
    step.wire(OUT(sub, 0), &[IN(rec, 0)]);
    step.wire(OUT(rec, 0), &[RES(0)]);
    let arms = vec![
        arm(Some(Literal::Int(1)), 8, base_params, 1, base),
        arm(None, 10, 1, 1, step),
    ];
    let sw = g.switch(7, 20, arms, 1);
    let mut sinks = vec![IN(eq, 0)];
    sinks.extend((1..=base_params + 1).map(|k| IN(sw, k as u32)));
    g.wire(P(0), &sinks);
    g.wire(OUT(eq, 0), &[IN(sw, 0)]);
    g.wire(OUT(sw, 0), &[RES(0)]);
    def(name, 2, 1, 1, g)
}

/// corpus/10-countdown: 100 000 deep, so the self call must be a tail call.
#[test]
fn countdown_tail_calls() {
    let main = main_calling("𐑒𐑬𐑯𐑑", &[Literal::Int(100_000)], 20);
    let sheet = Sheet {
        definitions: vec![countdown("𐑒𐑬𐑯𐑑", "𐑒𐑬𐑯𐑑", None), main],
    };
    assert_values(&sheet, &[int(0)]);
    let m = compiled(&sheet);
    assert!(has(&m, "𐑒𐑬𐑯𐑑", |i| matches!(
        i,
        Instr::TailCall { .. }
    )));
    assert!(!has(&m, "𐑒𐑬𐑯𐑑", |i| matches!(
        i,
        Instr::Call { .. }
    )));
}

/// corpus/11-even-odd: mutual tail calls.
#[test]
fn even_odd() {
    let even = countdown("𐑰𐑝𐑩𐑯", "𐑪𐑛", Some(1));
    let odd = countdown("𐑪𐑛", "𐑰𐑝𐑩𐑯", Some(0));
    let mut main = G::default();
    for (i, f) in ["𐑰𐑝𐑩𐑯", "𐑪𐑛"].into_iter().enumerate() {
        let seven = main.int(7, 3 + 4 * i, 3);
        let call = main.call(f, 3 + 4 * i, 10, 1, 1);
        main.wire(OUT(seven, 0), &[IN(call, 0)]);
        main.wire(OUT(call, 0), &[RES(i as u32)]);
    }
    let sheet = Sheet {
        definitions: vec![even, odd, def("𐑥𐑱𐑯", 40, 0, 2, main)],
    };
    assert_values(&sheet, &[int(0), int(1)]);
    let m = compiled(&sheet);
    assert!(has(&m, "𐑰𐑝𐑩𐑯", |i| matches!(
        i,
        Instr::TailCall { .. }
    )));
}

/// corpus/12-guards: text constants, two guards and a default arm.
#[test]
fn guards() {
    let mut g = G::default();
    let mut arms = Vec::new();
    for (i, (guard, word)) in [(Some(1), "𐑢𐑳𐑯"), (Some(2), "𐑑𐑵"), (None, "𐑥𐑧𐑯𐑦")]
        .into_iter()
        .enumerate()
    {
        let mut body = G::default();
        let t = body.lit(Literal::Text(word.into()), 5 + 2 * i, 4);
        body.wire(OUT(t, 0), &[RES(0)]);
        arms.push(arm(guard.map(Literal::Int), 5 + 2 * i, 0, 1, body));
    }
    let sw = g.switch(4, 9, arms, 1);
    g.wire(P(0), &[IN(sw, 0)]);
    g.wire(OUT(sw, 0), &[RES(0)]);
    let nam = def("𐑯𐑱𐑥", 2, 1, 1, g);
    let args = [Literal::Int(1), Literal::Int(2), Literal::Int(9)];
    let sheet = Sheet {
        definitions: vec![nam, main_calling("𐑯𐑱𐑥", &args, 20)],
    };
    assert_values(&sheet, &[text("𐑢𐑳𐑯"), text("𐑑𐑵"), text("𐑥𐑧𐑯𐑦")]);
}

/// corpus/13-fanout plus an inline chain and a two-output primitive whose
/// second output is unused (09-length's `𐑕`).
#[test]
fn fanout_inline_and_unused_output() {
    let mut g = G::default();
    let seven = g.int(7, 5, 2);
    let add = g.call("𐑐", 5, 12, 2, 1);
    let mul = g.call("𐑑", 10, 12, 2, 1);
    let show = g.inline("𐑖", 14, 10);
    let read = g.inline("𐑠", 14, 15);
    let nil = g.lit(Literal::Nil, 18, 2);
    let pre = g.call("𐑟", 18, 12, 2, 1);
    let split = g.call("𐑕", 18, 20, 1, 2);
    g.wire(
        OUT(seven, 0),
        &[
            IN(add, 0),
            IN(add, 1),
            IN(mul, 0),
            IN(mul, 1),
            IN(show, 0),
            IN(pre, 0),
        ],
    );
    g.wire(OUT(add, 0), &[RES(0)]);
    g.wire(OUT(mul, 0), &[RES(1)]);
    g.wire(OUT(show, 0), &[IN(read, 0)]);
    g.wire(OUT(read, 0), &[RES(2)]);
    g.wire(OUT(nil, 0), &[IN(pre, 1)]);
    g.wire(OUT(pre, 0), &[IN(split, 0)]);
    g.wire(OUT(split, 0), &[RES(3)]);
    let sheet = Sheet {
        definitions: vec![def("𐑥𐑱𐑯", 2, 0, 4, g)],
    };
    assert_values(&sheet, &[int(14), int(49), int(7), int(7)]);

    // Unused primitive slots are 0, so the module round-trips through text.
    let m = compiled(&sheet);
    let asm = shawline_vm::disassemble(&m);
    let again = shawline_vm::assemble(&asm).expect("reassembles");
    assert_eq!(shawline_vm::disassemble(&again), asm);
}

/// A call whose results reach the definition's outputs swapped can't reuse
/// the frame, and a definition that just forwards a call can.
#[test]
fn tail_call_needs_results_in_order() {
    let pair = |g: &mut G| {
        let p = g.call("𐑒", 3, 10, 2, 1);
        let u = g.call("𐑜", 3, 20, 1, 2);
        g.wire(P(0), &[IN(p, 0)]);
        g.wire(P(1), &[IN(p, 1)]);
        g.wire(OUT(p, 0), &[IN(u, 0)]);
        u
    };
    let mut id = G::default();
    let u = pair(&mut id);
    id.wire(OUT(u, 0), &[RES(0)]);
    id.wire(OUT(u, 1), &[RES(1)]);
    let mut fwd = G::default();
    let c = fwd.call("𐑦𐑛", 3, 10, 2, 2);
    fwd.wire(P(0), &[IN(c, 0)]);
    fwd.wire(P(1), &[IN(c, 1)]);
    fwd.wire(OUT(c, 0), &[RES(0)]);
    fwd.wire(OUT(c, 1), &[RES(1)]);
    let mut swap = G::default();
    let c = swap.call("𐑦𐑛", 3, 10, 2, 2);
    swap.wire(P(0), &[IN(c, 0)]);
    swap.wire(P(1), &[IN(c, 1)]);
    swap.wire(OUT(c, 0), &[RES(1)]);
    swap.wire(OUT(c, 1), &[RES(0)]);
    let mut main = G::default();
    let (a, b) = (main.int(1, 3, 3), main.int(2, 4, 3));
    let f = main.call("𐑓𐑢𐑛", 3, 10, 2, 2);
    let s = main.call("𐑕𐑢𐑪𐑐", 8, 10, 2, 2);
    main.wire(OUT(a, 0), &[IN(f, 0), IN(s, 0)]);
    main.wire(OUT(b, 0), &[IN(f, 1), IN(s, 1)]);
    for (k, src) in [OUT(f, 0), OUT(f, 1), OUT(s, 0), OUT(s, 1)]
        .into_iter()
        .enumerate()
    {
        main.wire(src, &[RES(k as u32)]);
    }
    let sheet = Sheet {
        definitions: vec![
            def("𐑦𐑛", 2, 2, 2, id),
            def("𐑓𐑢𐑛", 30, 2, 2, fwd),
            def("𐑕𐑢𐑪𐑐", 60, 2, 2, swap),
            def("𐑥𐑱𐑯", 90, 0, 4, main),
        ],
    };
    assert_values(&sheet, &[int(1), int(2), int(2), int(1)]);
    let m = compiled(&sheet);
    assert!(has(&m, "𐑓𐑢𐑛", |i| matches!(
        i,
        Instr::TailCall { .. }
    )));
    assert!(!has(&m, "𐑕𐑢𐑪𐑐", |i| matches!(
        i,
        Instr::TailCall { .. }
    )));
}

// ---- runtime spans ----

/// corpus/80-div-zero: the error points at the node's word.
#[test]
fn runtime_error_at_word() {
    let mut g = G::default();
    let div = g.call("𐑛", 5, 8, 2, 1);
    let (one, zero) = (g.int(1, 5, 3), g.int(0, 6, 3));
    g.wire(OUT(one, 0), &[IN(div, 0)]);
    g.wire(OUT(zero, 0), &[IN(div, 1)]);
    g.wire(OUT(div, 0), &[RES(0)]);
    let err = run(&Sheet {
        definitions: vec![def("𐑥𐑱𐑯", 2, 0, 1, g)],
    })
    .unwrap_err();
    assert_eq!((err.at, err.code), (pos(5, 8), "R002"));
}

/// corpus/82-no-arm: `W001` and `R005` both point at the selector, and a
/// warning still yields a program.
#[test]
fn no_default_arm() {
    let mut g = G::default();
    let two = g.int(2, 4, 2);
    let mut body = G::default();
    let five = body.int(5, 7, 4);
    body.wire(OUT(five, 0), &[RES(0)]);
    let sw = g.switch(6, 8, vec![arm(Some(Literal::Int(1)), 7, 0, 1, body)], 1);
    g.wire(OUT(two, 0), &[IN(sw, 0)]);
    g.wire(OUT(sw, 0), &[RES(0)]);
    let sheet = Sheet {
        definitions: vec![def("𐑥𐑱𐑯", 2, 0, 1, g)],
    };
    let (program, diags) = lower(&sheet);
    assert_eq!(
        diags.iter().map(|d| (d.at, d.code)).collect::<Vec<_>>(),
        [(pos(6, 8), "W001")]
    );
    let err = shawline_vm::run(&compile(&program.expect("warnings don't block"))).unwrap_err();
    assert_eq!((err.at, err.code), (pos(6, 8), "R005"));
}

// ---- checker diagnostics ----

/// `main` feeding `1` through one call node to its output.
fn main_with_node(word: &str, ins: usize, outs: usize) -> Sheet {
    let mut g = G::default();
    let n = g.call(word, 5, 8, ins, outs);
    let one = g.int(1, 5, 3);
    g.wire(OUT(one, 0), &[IN(n, 0)]);
    g.wire(OUT(n, 0), &[RES(0)]);
    Sheet {
        definitions: vec![def("𐑥𐑱𐑯", 2, 0, 1, g)],
    }
}

/// corpus/95-unknown, and a single letter that isn't a primitive.
#[test]
fn c001_unknown_name() {
    assert_eq!(codes(&main_with_node("𐑓𐑵", 1, 1)), [(pos(5, 8), "C001")]);
    assert_eq!(codes(&main_with_node("𐑓", 1, 1)), [(pos(5, 8), "C001")]);
    assert!(lower(&main_with_node("𐑓𐑵", 1, 1)).0.is_none());
}

/// corpus/93-arity, and an inline node calling a binary primitive.
#[test]
fn c002_arity() {
    assert_eq!(codes(&main_with_node("𐑐", 1, 1)), [(pos(5, 8), "C002")]);
    let mut g = G::default();
    let one = g.int(1, 5, 3);
    let add = g.inline("𐑐", 5, 8);
    g.wire(OUT(one, 0), &[IN(add, 0)]);
    g.wire(OUT(add, 0), &[RES(0)]);
    let sheet = Sheet {
        definitions: vec![def("𐑥𐑱𐑯", 2, 0, 1, g)],
    };
    assert_eq!(codes(&sheet), [(pos(5, 8), "C002")]);
}

/// corpus/94-cycle: on the word of the cycle's first node in row-major order.
#[test]
fn c003_cycle() {
    let mut g = G::default();
    let a = g.call("𐑐", 5, 9, 2, 1);
    let b = g.call("𐑐", 5, 21, 2, 1);
    let one = g.int(1, 5, 4);
    let two = g.int(2, 6, 16);
    g.wire(OUT(one, 0), &[IN(a, 0)]);
    g.wire(OUT(a, 0), &[IN(b, 0)]);
    g.wire(OUT(two, 0), &[IN(b, 1)]);
    g.wire(OUT(b, 0), &[RES(0), IN(a, 1)]);
    let sheet = Sheet {
        definitions: vec![def("𐑥𐑱𐑯", 2, 0, 1, g)],
    };
    assert_eq!(codes(&sheet), [(pos(5, 9), "C003")]);
    assert!(lower(&sheet).0.is_none());
}

/// The second definition of a name is the duplicate.
#[test]
fn c004_duplicate_definition() {
    let double = |row| {
        let mut g = G::default();
        let add = g.call("𐑐", row + 3, 12, 2, 1);
        g.wire(P(0), &[IN(add, 0), IN(add, 1)]);
        g.wire(OUT(add, 0), &[RES(0)]);
        def("𐑛𐑳𐑚𐑩𐑤", row, 1, 1, g)
    };
    let mut sheet = main_with_node("𐑛𐑳𐑚𐑩𐑤", 1, 1);
    sheet.definitions.extend([double(10), double(20)]);
    assert_eq!(codes(&sheet), [(pos(20, 3), "C004")]);
}

/// corpus/98-no-main.
#[test]
fn c005_no_main() {
    let mut sheet = main_with_node("𐑖", 1, 1);
    sheet.definitions[0].name.text = "𐑛𐑳𐑚𐑩𐑤".into();
    assert_eq!(codes(&sheet), [(pos(0, 0), "C005")]);
    assert!(lower(&sheet).0.is_none());
}

#[test]
fn c006_main_with_inputs() {
    let mut g = G::default();
    g.wire(P(0), &[RES(0)]);
    let sheet = Sheet {
        definitions: vec![def("𐑥𐑱𐑯", 2, 1, 1, g)],
    };
    assert_eq!(codes(&sheet), [(pos(2, 3), "C006")]);
}

#[test]
fn w001_switch_without_default() {
    let mut sheet = abs_sheet();
    let NodeKind::Switch(sw) = &mut sheet.definitions[0].body.nodes[2].kind else {
        panic!("abs has its switch third");
    };
    sw.arms[1].guard = Some(Guard {
        value: Literal::Int(1),
        at: pos(9, 15),
    });
    assert_eq!(codes(&sheet), [(pos(7, 20), "W001")]);
    assert_values(&sheet, &[int(7), int(5)]);
}

/// On the unused port's arrowhead; the program still runs.
#[test]
fn w002_unused_input() {
    let mut g = G::default();
    let c = g.int(3, 4, 6);
    g.wire(OUT(c, 0), &[RES(0)]);
    let three = def("𐑔𐑮𐑰", 10, 2, 1, g);
    let mut sheet = main_with_node("𐑔𐑮𐑰", 2, 1);
    let two = sheet.definitions[0].body.nodes.len();
    let m = &mut sheet.definitions[0].body;
    m.nodes.push(Node {
        kind: NodeKind::Const(Literal::Int(2)),
        ..m.nodes[1].clone()
    });
    m.edges.push(Edge {
        wire: 2,
        from: OUT(two, 0),
        to: vec![IN(0, 1)],
    });
    sheet.definitions.push(three);
    // Ports of `def` sit at row + 2 + i, column 0.
    assert_eq!(codes(&sheet), [(pos(12, 0), "W002"), (pos(13, 0), "W002")]);
    assert_values(&sheet, &[int(3)]);
}
