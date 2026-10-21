//! The checker and lowering from the AST to the dataflow IR.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};

use shawline_grid::{Diagnostic, Pos};
use shawline_syntax::ast::{self, Endpoint, Graph, Literal, Node, NodeKind, Sheet};
use shawline_vm::bytecode::Prim;

use crate::compile::{MAX_REGISTERS, frame_bound};
use crate::ir::{self, Block, FuncId, Function, Inst, Op, Program, Value};

/// `·𐑥𐑱𐑯`, the entry point, without its namer dot.
const MAIN: &str = "𐑥𐑱𐑯";

/// Checks a sheet and lowers it to IR.
///
/// Reports `C001`–`C007`, `W001` and `W002` at the positions of SPEC §14.
/// The program is `None` exactly when some diagnostic is an error. The sheet
/// is assumed to have passed syntax; should a structural fault that syntax
/// reports slip through anyway, lowering reports it again rather than
/// failing silently (`Y019` for an unfed slot, for instance).
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
    let mut functions = Vec::with_capacity(sheet.definitions.len());
    for d in &sheet.definitions {
        let results: Vec<Pos> = d.outputs.iter().map(|p| p.arrow).collect();
        match cx.lower_graph(&d.body, d.inputs.len(), &results, d.name.at) {
            Ok(body) => functions.push(Function {
                name: d.name.text.clone(),
                body,
                at: d.name.at,
            }),
            Err(diag) => cx.diags.push(diag),
        }
    }
    if functions.len() < sheet.definitions.len() {
        return (None, cx.diags);
    }
    let program = Program { functions, main };
    // Code generation can't fail, so a frame it couldn't number is caught here.
    let mut too_large = false;
    for f in &program.functions {
        let size = frame_bound(&program, f);
        if size > MAX_REGISTERS {
            too_large = true;
            let message = format!(
                "·{} is too large to compile: it counts {size} values, more than {MAX_REGISTERS}",
                f.name
            );
            cx.diags.push(Diagnostic::new(f.at, "C007", message));
        }
    }
    if too_large {
        return (None, cx.diags);
    }
    (Some(program), cx.diags)
}

#[derive(Clone, Copy)]
enum Callee {
    Prim(Prim),
    /// `𐑯𐑦𐑤` in a box: the empty list, 0→1 (SPEC §9).
    Nil,
    Func(FuncId),
}

/// `𐑯𐑦𐑤`, reserved like the single letters (SPEC §4).
const NIL: &str = "𐑯𐑦𐑤";

/// A slot no wire feeds, which syntax should already have reported.
fn unfed(at: Pos) -> Diagnostic {
    Diagnostic::new(at, "Y019", "port arrowhead with no wire reaching it")
}

struct Checker<'a> {
    sheet: &'a Sheet,
    /// The first definition with each name.
    defs: HashMap<&'a str, FuncId>,
    diags: Vec<Diagnostic>,
}

impl Checker<'_> {
    /// Single letters and `𐑯𐑦𐑤` name primitives only (SPEC §4), so they
    /// never reach the definition table.
    fn resolve(&self, name: &ast::Name) -> Option<Callee> {
        if name.text == NIL {
            return Some(Callee::Nil);
        }
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
            Callee::Nil => (0, 1),
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
    /// the first of its nodes' words in row-major order (SPEC §14).
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
        for component in components(&succ) {
            let a = component[0];
            if component.len() > 1 || succ[a].contains(&a) {
                let words = component.iter().map(|&b| word(&g.nodes[b]));
                let first = words.fold(word(&g.nodes[a]), Pos::min);
                self.diags.push(Diagnostic::new(first, "C003", "cycle in the wires"));
            }
        }
    }

    /// Lowers one scope with `params` inputs and outputs at `result_at` to a
    /// block in dependency order, breaking ties by the row-major order of the
    /// nodes' words (SPEC §10). `fallback`, the definition's namer dot,
    /// locates faults with no better position.
    fn lower_graph(
        &self,
        g: &Graph,
        params: usize,
        result_at: &[Pos],
        fallback: Pos,
    ) -> Result<Block, Diagnostic> {
        let malformed = || Diagnostic::new(fallback, "Y005", "a wire syntax left inconsistent");
        let n = g.nodes.len();
        let mut inputs: Vec<Vec<Option<Endpoint>>> = g
            .nodes
            .iter()
            .map(|nd| vec![None; nd.inputs.len()])
            .collect();
        let mut outs = vec![None; result_at.len()];
        let mut succ = vec![Vec::new(); n];
        let mut indegree = vec![0usize; n];
        for e in &g.edges {
            for &sink in &e.to {
                let slot = match sink {
                    Endpoint::Input(b, k) => inputs.get_mut(b).and_then(|s| s.get_mut(k as usize)),
                    Endpoint::Result(j) => outs.get_mut(j as usize),
                    Endpoint::Param(_) | Endpoint::Output(..) => None,
                };
                *slot.ok_or_else(malformed)? = Some(e.from);
                if let (Endpoint::Output(a, _), Endpoint::Input(b, _)) = (e.from, sink) {
                    succ.get_mut(a).ok_or_else(malformed)?.push(b);
                    indegree[b] += 1;
                }
            }
        }

        let key = |a: usize| Reverse((word(&g.nodes[a]), a));
        let mut ready: BinaryHeap<_> = (0..n).filter(|&a| indegree[a] == 0).map(key).collect();
        let mut order = Vec::with_capacity(n);
        while let Some(Reverse((_, a))) = ready.pop() {
            order.push(a);
            for &b in &succ[a] {
                indegree[b] -= 1;
                if indegree[b] == 0 {
                    ready.push(key(b));
                }
            }
        }
        if order.len() != n {
            let stuck = (0..n).filter(|&a| indegree[a] > 0).map(|a| word(&g.nodes[a])).min();
            let at = stuck.unwrap_or(fallback);
            return Err(Diagnostic::new(at, "C003", "cycle in the wires"));
        }
        let mut pos = vec![0u32; n];
        for (i, &a) in order.iter().enumerate() {
            pos[a] = i as u32;
        }

        // `at` is the arrowhead of the slot being read.
        let value = |ep: Option<Endpoint>, at: Pos| match ep {
            None => Err(unfed(at)),
            Some(Endpoint::Param(i)) if (i as usize) < params => Ok(Value::Param(i)),
            Some(Endpoint::Output(a, k))
                if g.nodes.get(a).is_some_and(|nd| (k as usize) < nd.outputs.len()) =>
            {
                Ok(Value::Out {
                    inst: pos[a],
                    output: k,
                })
            }
            Some(_) => Err(malformed()),
        };
        let insts = order
            .iter()
            .map(|&a| self.lower_node(&g.nodes[a], &inputs[a], &value, fallback))
            .collect::<Result<Vec<_>, _>>()?;
        let results = outs
            .into_iter()
            .zip(result_at)
            .map(|(ep, &at)| value(ep, at))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Block {
            params: params as u32,
            insts,
            results,
        })
    }

    fn lower_node(
        &self,
        node: &Node,
        slots: &[Option<Endpoint>],
        value: &dyn Fn(Option<Endpoint>, Pos) -> Result<Value, Diagnostic>,
        fallback: Pos,
    ) -> Result<Inst, Diagnostic> {
        // Slot `i` arrives at `node.inputs[i]`, its arrowhead.
        let args = |first: usize, n: usize| {
            (first..first + n)
                .map(|i| value(slots[i], node.inputs[i]))
                .collect::<Result<Vec<_>, _>>()
        };
        let op = match &node.kind {
            NodeKind::Const(lit) => Op::Const(lit.clone()),
            NodeKind::Call(name) | NodeKind::Inline(name) => {
                let unknown = || Diagnostic::new(name.at, "C001", "unknown name");
                match self.resolve(name).ok_or_else(unknown)? {
                    Callee::Prim(prim) => Op::Prim {
                        prim,
                        args: args(0, slots.len())?,
                    },
                    Callee::Nil => Op::Const(Literal::Nil),
                    Callee::Func(func) => Op::Call {
                        func,
                        args: args(0, slots.len())?,
                    },
                }
            }
            NodeKind::Switch(sw) => {
                let outputs = node.outputs.len();
                if sw.arms.iter().any(|a| a.results.len() != outputs) {
                    return Err(Diagnostic::new(sw.selector, "Y011", "arms with different results"));
                }
                // The selector, then every arm's through-ports.
                if slots.len() != 1 + sw.arms.iter().map(|a| a.params.len()).sum::<usize>() {
                    return Err(Diagnostic::new(sw.selector, "Y005", "switch slots out of step"));
                }
                let mut first = 1;
                let mut arms = Vec::with_capacity(sw.arms.len());
                for arm in &sw.arms {
                    let k = arm.params.len();
                    arms.push(ir::Arm {
                        guard: arm.guard.as_ref().map(|g| g.value.clone()),
                        args: args(first, k)?,
                        body: self.lower_graph(&arm.body, k, &arm.results, fallback)?,
                    });
                    first += k;
                }
                Op::Switch {
                    selector: value(slots[0], sw.selector)?,
                    arms,
                    outputs: outputs as u32,
                }
            }
        };
        Ok(Inst { op, at: word(node) })
    }
}

/// Where diagnostics about a node point: its word, a switch's selector, or a
/// literal's first cell.
pub(crate) fn word(node: &Node) -> Pos {
    match &node.kind {
        NodeKind::Call(name) | NodeKind::Inline(name) => name.at,
        NodeKind::Switch(sw) => sw.selector,
        NodeKind::Const(_) => Pos::new(node.rect.top, node.rect.left),
    }
}

/// The strongly connected components of a graph, each listing its nodes in
/// ascending order, in ascending order of their first node.
///
/// Tarjan's algorithm with an explicit stack, since a drawing can hold more
/// nodes than the call stack holds frames. Storing reachability from every
/// node instead takes memory quadratic in the nodes, even with no cycle.
fn components(succ: &[Vec<usize>]) -> Vec<Vec<usize>> {
    const UNSEEN: usize = usize::MAX;
    let n = succ.len();
    let mut index = vec![UNSEEN; n];
    let mut low = vec![0; n];
    let mut on_stack = vec![false; n];
    let mut stack = Vec::new();
    let mut found = Vec::new();
    let mut next = 0;
    // Nodes being visited, each with how many of its successors it has tried.
    let mut path: Vec<(usize, usize)> = Vec::new();
    for root in 0..n {
        if index[root] != UNSEEN {
            continue;
        }
        let mut entering = Some(root);
        loop {
            if let Some(v) = entering.take() {
                index[v] = next;
                low[v] = next;
                next += 1;
                on_stack[v] = true;
                stack.push(v);
                path.push((v, 0));
            }
            let Some((v, tried)) = path.last_mut() else {
                break;
            };
            let v = *v;
            if let Some(&w) = succ[v].get(*tried) {
                *tried += 1;
                if index[w] == UNSEEN {
                    entering = Some(w);
                } else if on_stack[w] {
                    low[v] = low[v].min(index[w]);
                }
                continue;
            }
            path.pop();
            if let Some(&(u, _)) = path.last() {
                low[u] = low[u].min(low[v]);
            }
            if low[v] == index[v] {
                let mut component = Vec::new();
                while let Some(w) = stack.pop() {
                    on_stack[w] = false;
                    component.push(w);
                    if w == v {
                        break;
                    }
                }
                component.sort_unstable();
                found.push(component);
            }
        }
    }
    found.sort_unstable_by_key(|c| c[0]);
    found
}
