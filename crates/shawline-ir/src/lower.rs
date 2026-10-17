//! The checker and lowering from the AST to the dataflow IR.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};

use shawline_grid::{Diagnostic, Pos};
use shawline_syntax::ast::{self, Endpoint, Graph, Node, NodeKind, Sheet};
use shawline_vm::bytecode::Prim;

use crate::ir::{self, Block, FuncId, Function, Inst, Op, Program, Value};

/// `·𐑥𐑱𐑯`, the entry point, without its namer dot.
const MAIN: &str = "𐑥𐑱𐑯";

/// Checks a sheet and lowers it to IR.
///
/// Reports `C001`–`C006`, `W001` and `W002` at the positions of SPEC §14.
/// The program is `None` when any diagnostic is an error. The sheet is assumed
/// to have passed syntax; a structural inconsistency that syntax reports
/// (an unfed slot, an edge to a missing node) also yields `None`, without a
/// further diagnostic.
pub fn lower(sheet: &Sheet) -> (Option<Program>, Vec<Diagnostic>) {
    let mut diags = Vec::new();
    let mut defs = HashMap::new();
    for (i, d) in sheet.definitions.iter().enumerate() {
        if defs.contains_key(d.name.text.as_str()) {
            diags.push(Diagnostic::new(
                d.name.at,
                "C004",
                format!("·{} is defined more than once", d.name.text),
            ));
        } else {
            defs.insert(d.name.text.as_str(), i as FuncId);
        }
    }

    let main = defs.get(MAIN).copied();
    match main {
        None => diags.push(Diagnostic::new(Pos::new(0, 0), "C005", "no ·𐑥𐑱𐑯")),
        Some(m) => {
            let d = &sheet.definitions[m as usize];
            if !d.inputs.is_empty() {
                diags.push(Diagnostic::new(
                    d.name.at,
                    "C006",
                    "·𐑥𐑱𐑯 must have no inputs",
                ));
            }
        }
    }

    let mut cx = Checker { sheet, defs, diags };
    for d in &sheet.definitions {
        for (i, port) in d.inputs.iter().enumerate() {
            let param = Endpoint::Param(i as u32);
            if !d.body.edges.iter().any(|e| e.from == param) {
                cx.diags.push(Diagnostic::new(
                    port.arrow,
                    "W002",
                    format!("input {} is unused", port.name.text),
                ));
            }
        }
        cx.check(&d.body);
    }

    let Some(main) = main else {
        return (None, cx.diags);
    };
    if cx.diags.iter().any(|d| !d.is_warning()) {
        return (None, cx.diags);
    }
    let functions = sheet
        .definitions
        .iter()
        .map(|d| {
            Some(Function {
                name: d.name.text.clone(),
                body: cx.lower_graph(&d.body, d.inputs.len(), d.outputs.len())?,
                at: d.name.at,
            })
        })
        .collect::<Option<Vec<_>>>();
    (
        functions.map(|functions| Program { functions, main }),
        cx.diags,
    )
}

#[derive(Clone, Copy)]
enum Callee {
    Prim(Prim),
    Func(FuncId),
}

struct Checker<'a> {
    sheet: &'a Sheet,
    /// The first definition with each name.
    defs: HashMap<&'a str, FuncId>,
    diags: Vec<Diagnostic>,
}

impl Checker<'_> {
    /// Single letters name primitives only (SPEC §4), so they never reach the
    /// definition table.
    fn resolve(&self, name: &ast::Name) -> Option<Callee> {
        let mut chars = name.text.chars();
        match (chars.next(), chars.next()) {
            (Some(ch), None) => Prim::from_letter(ch).map(Callee::Prim),
            _ => self.defs.get(name.text.as_str()).copied().map(Callee::Func),
        }
    }

    fn arity(&self, callee: Callee) -> (usize, usize) {
        match callee {
            Callee::Prim(p) => {
                let (i, o) = p.arity();
                (i.into(), o.into())
            }
            Callee::Func(f) => {
                let d = &self.sheet.definitions[f as usize];
                (d.inputs.len(), d.outputs.len())
            }
        }
    }

    /// Names, arities, default arms and cycles in one scope and its arms.
    fn check(&mut self, g: &Graph) {
        for node in &g.nodes {
            match &node.kind {
                NodeKind::Call(name) | NodeKind::Inline(name) => {
                    let Some(callee) = self.resolve(name) else {
                        self.diags.push(Diagnostic::new(
                            name.at,
                            "C001",
                            format!("unknown name {}", name.text),
                        ));
                        continue;
                    };
                    let want = self.arity(callee);
                    let inline = matches!(node.kind, NodeKind::Inline(_));
                    let have = (node.inputs.len(), node.outputs.len());
                    if have != want || (inline && want != (1, 1)) {
                        self.diags.push(Diagnostic::new(
                            name.at,
                            "C002",
                            format!(
                                "{} takes {} and gives {}; this node has {} and {}",
                                name.text, want.0, want.1, have.0, have.1
                            ),
                        ));
                    }
                }
                NodeKind::Switch(sw) => {
                    if sw.arms.last().is_none_or(|a| a.guard.is_some()) {
                        self.diags.push(Diagnostic::new(
                            sw.selector,
                            "W001",
                            "switch has no default arm",
                        ));
                    }
                    for arm in &sw.arms {
                        self.check(&arm.body);
                    }
                }
                NodeKind::Const(_) => {}
            }
        }
        self.cycles(g);
    }

    /// One `C003` per strongly connected component that contains a cycle, at
    /// the word of its first node. Nodes are already in row-major order.
    fn cycles(&mut self, g: &Graph) {
        let n = g.nodes.len();
        let mut succ = vec![Vec::new(); n];
        for e in &g.edges {
            let Endpoint::Output(a, _) = e.from else {
                continue;
            };
            for &sink in &e.to {
                match sink {
                    Endpoint::Input(b, _) if a < n && b < n => succ[a].push(b),
                    _ => {}
                }
            }
        }
        // Drawings are small, so plain reachability is simpler than Tarjan.
        let reach: Vec<Vec<bool>> = (0..n).map(|a| reachable(&succ, a)).collect();
        let mut seen = vec![false; n];
        for a in 0..n {
            if seen[a] || !reach[a][a] {
                continue;
            }
            for b in a..n {
                if reach[a][b] && reach[b][a] {
                    seen[b] = true;
                }
            }
            self.diags.push(Diagnostic::new(
                word(&g.nodes[a]),
                "C003",
                "cycle in the wires",
            ));
        }
    }

    /// Lowers one scope with `params` inputs and `results` outputs to a block
    /// in topological order, breaking ties by row-major node order.
    fn lower_graph(&self, g: &Graph, params: usize, results: usize) -> Option<Block> {
        let n = g.nodes.len();
        let mut inputs: Vec<Vec<Option<Endpoint>>> = g
            .nodes
            .iter()
            .map(|nd| vec![None; nd.inputs.len()])
            .collect();
        let mut outs = vec![None; results];
        let mut succ = vec![Vec::new(); n];
        let mut indegree = vec![0usize; n];
        for e in &g.edges {
            for &sink in &e.to {
                let slot = match sink {
                    Endpoint::Input(b, k) => inputs.get_mut(b)?.get_mut(k as usize)?,
                    Endpoint::Result(j) => outs.get_mut(j as usize)?,
                    Endpoint::Param(_) | Endpoint::Output(..) => return None,
                };
                *slot = Some(e.from);
                if let (Endpoint::Output(a, _), Endpoint::Input(b, _)) = (e.from, sink) {
                    succ.get_mut(a)?.push(b);
                    indegree[b] += 1;
                }
            }
        }

        let mut ready: BinaryHeap<Reverse<usize>> =
            (0..n).filter(|&a| indegree[a] == 0).map(Reverse).collect();
        let mut order = Vec::with_capacity(n);
        while let Some(Reverse(a)) = ready.pop() {
            order.push(a);
            for &b in &succ[a] {
                indegree[b] -= 1;
                if indegree[b] == 0 {
                    ready.push(Reverse(b));
                }
            }
        }
        if order.len() != n {
            return None;
        }
        let mut pos = vec![0u32; n];
        for (i, &a) in order.iter().enumerate() {
            pos[a] = i as u32;
        }

        let value = |ep: Option<Endpoint>| match ep? {
            Endpoint::Param(i) if (i as usize) < params => Some(Value::Param(i)),
            Endpoint::Output(a, k) if (k as usize) < g.nodes.get(a)?.outputs.len() => {
                Some(Value::Out {
                    inst: pos[a],
                    output: k,
                })
            }
            _ => None,
        };
        let insts = order
            .iter()
            .map(|&a| self.lower_node(&g.nodes[a], &inputs[a], &value))
            .collect::<Option<Vec<_>>>()?;
        let results = outs.into_iter().map(value).collect::<Option<Vec<_>>>()?;
        Some(Block {
            params: params as u32,
            insts,
            results,
        })
    }

    fn lower_node(
        &self,
        node: &Node,
        slots: &[Option<Endpoint>],
        value: &dyn Fn(Option<Endpoint>) -> Option<Value>,
    ) -> Option<Inst> {
        let args = |slots: &[Option<Endpoint>]| {
            slots.iter().map(|&s| value(s)).collect::<Option<Vec<_>>>()
        };
        let op = match &node.kind {
            NodeKind::Const(lit) => Op::Const(lit.clone()),
            NodeKind::Call(name) | NodeKind::Inline(name) => match self.resolve(name)? {
                Callee::Prim(prim) => Op::Prim {
                    prim,
                    args: args(slots)?,
                },
                Callee::Func(func) => Op::Call {
                    func,
                    args: args(slots)?,
                },
            },
            NodeKind::Switch(sw) => {
                let (&selector, mut rest) = slots.split_first()?;
                let outputs = node.outputs.len();
                let mut arms = Vec::with_capacity(sw.arms.len());
                for arm in &sw.arms {
                    let k = arm.params.len();
                    if rest.len() < k {
                        return None;
                    }
                    let (mine, tail) = rest.split_at(k);
                    rest = tail;
                    arms.push(ir::Arm {
                        guard: arm.guard.as_ref().map(|g| g.value.clone()),
                        args: args(mine)?,
                        body: self.lower_graph(&arm.body, k, outputs)?,
                    });
                }
                if !rest.is_empty() {
                    return None;
                }
                Op::Switch {
                    selector: value(selector)?,
                    arms,
                    outputs: outputs as u32,
                }
            }
        };
        Some(Inst { op, at: word(node) })
    }
}

/// Where diagnostics about a node point: its word, a switch's selector, or a
/// literal's first cell.
fn word(node: &Node) -> Pos {
    match &node.kind {
        NodeKind::Call(name) | NodeKind::Inline(name) => name.at,
        NodeKind::Switch(sw) => sw.selector,
        NodeKind::Const(_) => Pos::new(node.rect.top, node.rect.left),
    }
}

/// Nodes reachable from `start` by one or more edges.
fn reachable(succ: &[Vec<usize>], start: usize) -> Vec<bool> {
    let mut seen = vec![false; succ.len()];
    let mut stack = succ[start].clone();
    while let Some(a) = stack.pop() {
        if !seen[a] {
            seen[a] = true;
            stack.extend(&succ[a]);
        }
    }
    seen
}
