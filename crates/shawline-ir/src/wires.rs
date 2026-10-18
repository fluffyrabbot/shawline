//! A traced run mapped back onto the drawing: which value each wire carried.
//!
//! The chain is wire → AST edge → IR value → bytecode instruction. An edge's
//! source is a scope parameter or a node output. A node is the IR
//! instruction with the same word position, and [`compile_mapped`] says
//! which bytecode instructions carry out each IR instruction, so a recorded
//! write names its IR value directly. Two kinds of value have no instruction
//! of their own and are derived instead:
//!
//! - a parameter is the first call's argument, or for a switch arm, the value
//!   the switch passed in;
//! - a switch output is the matching result of the arm the selector chose.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use shawline_grid::Pos;
use shawline_syntax::ast::{Endpoint, Graph, Literal, NodeKind, Sheet};
use shawline_vm::Traced;
use shawline_vm::bytecode::Value;

use crate::compile::{Path, compile_mapped};
use crate::ir::{self, Block, Op, Program};
use crate::lower::word;
use crate::trace::SheetTrace;

pub(crate) fn trace(sheet: &Sheet, program: &Program, steps: u64) -> SheetTrace {
    let (module, sites) = compile_mapped(program);
    let traced = shawline_vm::run_traced(&module, steps);
    let mut cx = Recorded::new(&traced, &sites);
    // R009 is found at main's final return, after everything has run.
    cx.complete = match &traced.error {
        None => !traced.stopped,
        Some(e) => e.code == "R009",
    };
    let mut wires = Vec::new();
    for (f, (def, func)) in sheet.definitions.iter().zip(&program.functions).enumerate() {
        let scope = Scope {
            func: f as u32,
            path: Rc::from([]),
            block: &func.body,
            parent: None,
        };
        cx.walk(&scope, &def.body, &mut wires);
    }
    wires.sort_by_key(|&(w, _)| w);
    wires.dedup_by_key(|&mut (w, _)| w);
    SheetTrace {
        wires,
        error: traced.error,
        stopped: traced.stopped,
    }
}

/// A scope that ran during its definition's first call: a body, or the arm a
/// switch chose.
struct Scope<'a> {
    func: u32,
    path: Path,
    block: &'a Block,
    /// For an arm: the enclosing scope and the values the switch passes in.
    parent: Option<(&'a Scope<'a>, &'a [ir::Value])>,
}

impl<'a> Scope<'a> {
    fn arm(&'a self, switch: u32, a: usize, arm: &'a ir::Arm) -> Scope<'a> {
        Scope {
            func: self.func,
            path: self
                .path
                .iter()
                .copied()
                .chain([(switch, a as u32)])
                .collect(),
            block: &arm.body,
            parent: Some((self, &arm.args)),
        }
    }
}

/// Where a recorded write landed: function, block, instruction, output.
type Key = (u32, Path, u32, u32);

struct Recorded {
    /// The first value each IR instruction output took in its function's
    /// first activation.
    values: HashMap<Key, Value>,
    /// Each called function's first arguments.
    args: HashMap<u32, Vec<Value>>,
    /// Functions whose first activation ended in a recorded tail call, and so
    /// ran to its end.
    finished: Vec<bool>,
    /// The run finished, so every scope that started also ended.
    complete: bool,
    /// The arm each switch chose, by `(func, path, inst)`.
    chosen: RefCell<HashMap<(u32, Path, u32), Option<usize>>>,
}

impl Recorded {
    fn new(traced: &Traced, sites: &[Vec<Option<crate::compile::Site>>]) -> Recorded {
        let mut values = HashMap::new();
        let mut finished = vec![false; sites.len()];
        for out in &traced.outputs {
            let Some(site) = &sites[out.func as usize][out.instr as usize] else {
                continue;
            };
            finished[out.func as usize] |= site.tail;
            let key = (
                out.func,
                site.path.clone(),
                site.inst,
                u32::from(out.output),
            );
            values.entry(key).or_insert_with(|| out.value.clone());
        }
        Recorded {
            values,
            args: traced.calls.iter().cloned().collect(),
            finished,
            complete: false,
            chosen: RefCell::default(),
        }
    }

    /// Records the value of every wire in `graph`, the drawing of `scope`,
    /// and of the arms that ran inside it.
    fn walk(&self, scope: &Scope, graph: &Graph, wires: &mut Vec<(usize, Value)>) {
        let insts: HashMap<Pos, u32> = scope
            .block
            .insts
            .iter()
            .enumerate()
            .map(|(i, inst)| (inst.at, i as u32))
            .collect();
        let inst = |n: usize| insts[&word(&graph.nodes[n])];
        for edge in &graph.edges {
            let v = match edge.from {
                Endpoint::Param(i) => ir::Value::Param(i),
                Endpoint::Output(n, k) => ir::Value::Out {
                    inst: inst(n),
                    output: k,
                },
                Endpoint::Result(_) | Endpoint::Input(..) => continue,
            };
            if let Some(value) = self.value(scope, v) {
                wires.push((edge.wire, value));
            }
        }
        for (n, node) in graph.nodes.iter().enumerate() {
            let NodeKind::Switch(switch) = &node.kind else {
                continue;
            };
            let i = inst(n);
            let Op::Switch { arms, .. } = &scope.block.insts[i as usize].op else {
                unreachable!("a switch node lowers to a switch");
            };
            if let Some(a) = self.choice(scope, i) {
                self.walk(&scope.arm(i, a, &arms[a]), &switch.arms[a].body, wires);
            }
        }
    }

    /// The value `v` took in `scope`, if it was computed.
    fn value(&self, scope: &Scope, v: ir::Value) -> Option<Value> {
        match v {
            ir::Value::Param(i) => match scope.parent {
                None => self.args.get(&scope.func)?.get(i as usize).cloned(),
                Some((outer, args)) => self.value(outer, args[i as usize]),
            },
            ir::Value::Out { inst, output } => match &scope.block.insts[inst as usize].op {
                Op::Switch { arms, .. } => {
                    let a = self.choice(scope, inst)?;
                    let arm = scope.arm(inst, a, &arms[a]);
                    self.value(&arm, arms[a].body.results[output as usize])
                }
                _ => {
                    let key = (scope.func, scope.path.clone(), inst, output);
                    self.values.get(&key).cloned()
                }
            },
        }
    }

    /// The arm the switch at `inst` ran, if it ran: the first whose guard
    /// equals the selector, else the default.
    fn choice(&self, scope: &Scope, inst: u32) -> Option<usize> {
        let key = (scope.func, scope.path.clone(), inst);
        if let Some(&known) = self.chosen.borrow().get(&key) {
            return known;
        }
        let Op::Switch { selector, arms, .. } = &scope.block.insts[inst as usize].op else {
            unreachable!("only switches choose");
        };
        let choice = self.value(scope, *selector).and_then(|sel| {
            let a = arms.iter().position(|arm| {
                arm.guard
                    .as_ref()
                    .is_none_or(|g| shawline_vm::equal(&literal(g), &sel))
            })?;
            self.reached(scope, inst).then_some(a)
        });
        self.chosen.borrow_mut().insert(key, choice);
        choice
    }

    /// Whether the run got as far as the switch at `inst` of `scope`, which
    /// is known to have started. A run that halted early reached it only if
    /// something inside or after it was recorded. Within an activation code
    /// runs forward and in IR order, except that a tail call runs last, and
    /// its outputs are recorded only once its frame returns.
    fn reached(&self, scope: &Scope, inst: u32) -> bool {
        if self.complete || self.finished[scope.func as usize] {
            return true;
        }
        let here: Vec<(u32, u32)> = scope.path.iter().copied().chain([(inst, 0)]).collect();
        self.values.keys().any(|(func, path, at, _)| {
            if *func != scope.func {
                return false;
            }
            // Compare positions level by level: the first level where the
            // two differ decides which came first.
            for (level, &(pivot, arm)) in here.iter().enumerate() {
                let theirs = path.get(level).map(|&(i, _)| i).unwrap_or(*at);
                let inside = path.get(level).is_some_and(|&(_, a)| a == arm);
                if theirs != pivot {
                    return theirs > pivot;
                }
                if level + 1 == here.len() {
                    // At the switch itself: anything recorded in one of its
                    // arms ran inside it.
                    return path.len() > level;
                }
                if !inside {
                    return false;
                }
            }
            false
        })
    }
}

fn literal(lit: &Literal) -> Value {
    match lit {
        Literal::Int(n) => Value::Int(*n),
        Literal::Text(t) => Value::Text(Rc::from(t.as_str())),
        Literal::Nil => Value::List(None),
    }
}

#[cfg(test)]
mod tests {
    use shawline_grid::Grid;
    use shawline_syntax::ast::{Definition, Endpoint, Graph, NodeKind, Sheet};

    use crate::trace::SheetTrace;

    fn traced(src: &str) -> (Sheet, SheetTrace) {
        let grid = Grid::parse(src).unwrap();
        let diagram = shawline_shape::recognize(&grid);
        let (sheet, diags) = shawline_syntax::parse(&grid, &diagram);
        assert!(
            diagram.diagnostics.is_empty() && diags.is_empty(),
            "{diags:?}"
        );
        let program = crate::lower(&sheet).0.expect("a valid program");
        let trace = crate::trace(&sheet, &program, 1_000_000);
        assert!(trace.error.is_none() && !trace.stopped);
        (sheet, trace)
    }

    fn def<'a>(sheet: &'a Sheet, name: &str) -> &'a Definition {
        sheet
            .definitions
            .iter()
            .find(|d| d.name.text == name)
            .unwrap()
    }

    /// The printed value on the wire leaving `from` in `graph`, if any.
    fn on(trace: &SheetTrace, graph: &Graph, from: Endpoint) -> Option<String> {
        let wire = graph
            .edges
            .iter()
            .find(|e| e.from == from)
            .expect("a wire from there")
            .wire;
        trace
            .wires
            .iter()
            .find(|(w, _)| *w == wire)
            .map(|(_, v)| v.to_string())
    }

    fn switch(graph: &Graph) -> usize {
        graph
            .nodes
            .iter()
            .position(|n| matches!(n.kind, NodeKind::Switch(_)))
            .unwrap()
    }

    fn arms(graph: &Graph) -> Vec<&Graph> {
        let NodeKind::Switch(s) = &graph.nodes[switch(graph)].kind else {
            unreachable!()
        };
        s.arms.iter().map(|a| &a.body).collect()
    }

    /// A switch in tail position returns from inside its arms; its output
    /// wire still carries the taken arm's result, and the untaken arm's
    /// wires carry nothing.
    #[test]
    fn tail_switch_output_is_the_taken_arms_result() {
        let (sheet, trace) = traced(include_str!("../../../corpus/04-abs.shaw"));
        let abs = &def(&sheet, "𐑨𐑚𐑕").body;
        // The first call is `0 - 7`.
        assert_eq!(on(&trace, abs, Endpoint::Param(0)).as_deref(), Some("-7"));
        assert_eq!(
            on(&trace, abs, Endpoint::Output(switch(abs), 0)).as_deref(),
            Some("7")
        );
        let [kept, negated] = arms(abs)[..] else {
            panic!("two arms")
        };
        assert_eq!(on(&trace, kept, Endpoint::Param(0)), None);
        assert_eq!(
            on(&trace, negated, Endpoint::Param(0)).as_deref(),
            Some("-7")
        );
    }

    /// A switch whose output feeds another node joins its arms' results.
    #[test]
    fn join_switch_output_is_the_taken_arms_result() {
        let src = "\
╭─ ·𐑯𐑱𐑥 ──────────────────────────╮
│                                 │
▶ 𐑯 ─────┐                        │
│        │                        │
│ ┌─ 1 ──▼────────┐               │
│ │ 10 ───────────▶──┐            │
│ ├┄ 2 ┄┄┄┄┄┄┄┄┄┄┄┤  │            │
│ │ 20 ───────────▶──┤            │
│ ├┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┤  │            │
│ │ 30 ───────────▶──┴──▶ 𐑖 ──────▶ 𐑬
│ └───────────────┘               │
│                                 │
╰─────────────────────────────────╯

╭─ ·𐑥𐑱𐑯 ──────────────╮
│                     │
│        ┌─────┐      │
│   2 ───▶ 𐑯𐑱𐑥 ├──────▶ 𐑢𐑳𐑯
│        └─────┘      │
│                     │
│        ┌─────┐      │
│   9 ───▶ 𐑯𐑱𐑥 ├──────▶ 𐑑𐑵
│        └─────┘      │
│                     │
╰─────────────────────╯
";
        let (sheet, trace) = traced(src);
        let num = &def(&sheet, "𐑯𐑱𐑥").body;
        assert_eq!(
            on(&trace, num, Endpoint::Output(switch(num), 0)).as_deref(),
            Some("20")
        );
        let shown = num
            .nodes
            .iter()
            .position(|n| matches!(n.kind, NodeKind::Inline(_)))
            .unwrap();
        assert_eq!(
            on(&trace, num, Endpoint::Output(shown, 0)).as_deref(),
            Some("20")
        );
        let carried: Vec<_> = arms(num)
            .iter()
            .map(|a| on(&trace, a, Endpoint::Output(0, 0)))
            .collect();
        assert_eq!(carried, [None, Some("20".into()), None]);
        let main = &def(&sheet, "𐑥𐑱𐑯").body;
        let calls = main
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, n)| matches!(n.kind, NodeKind::Call(_)));
        let results: Vec<_> = calls
            .map(|(i, _)| on(&trace, main, Endpoint::Output(i, 0)))
            .collect();
        assert_eq!(results, [Some("20".into()), Some("30".into())]);
    }

    /// Only the first call is shown: the recursive call's wire carries
    /// `9!`, the value that call returned to the first one.
    #[test]
    fn recursion_shows_the_first_call() {
        let (sheet, trace) = traced(include_str!("../../../corpus/08-factorial.shaw"));
        let fact = &def(&sheet, "𐑓𐑨𐑒𐑑").body;
        assert_eq!(on(&trace, fact, Endpoint::Param(0)).as_deref(), Some("10"));
        let out = on(&trace, fact, Endpoint::Output(switch(fact), 0));
        assert_eq!(out.as_deref(), Some("3628800"));
        let step = arms(fact)[1];
        let call = step
            .nodes
            .iter()
            .position(|n| matches!(&n.kind, NodeKind::Call(w) if w.text == "𐑓𐑨𐑒𐑑"));
        assert_eq!(
            on(&trace, step, Endpoint::Output(call.unwrap(), 0)).as_deref(),
            Some("362880")
        );
    }
}
