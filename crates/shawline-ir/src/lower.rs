//! The checker and lowering from the AST to the dataflow IR.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, VecDeque};

use shawline_grid::{Diagnostic, Pos};
use shawline_syntax::ast::{self, Endpoint, Graph, Literal, Node, NodeKind, Sheet};
use shawline_syntax::message::{self, Port};
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
                let input = Port::DefinitionInput {
                    definition: Some(&d.name.text),
                    index: i,
                };
                let message = match port.name.text.as_str() {
                    "" => format!("{input} is unused"),
                    name => format!("{input}, {name}, is unused"),
                };
                cx.diags.push(Diagnostic::new(port.arrow, "W002", message));
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
        let owner = Owner::Definition(&d.name.text);
        match cx.lower_graph(&d.body, d.inputs.len(), &results, owner, d.name.at) {
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
fn unfed(at: Pos, port: Port) -> Diagnostic {
    Diagnostic::new(at, "Y019", message::unfed(port))
}

/// The value a slot reads, given the endpoint feeding it (`None` if no wire
/// does), its arrowhead and its port, which `Y019` names.
type SlotValue<'f> = dyn Fn(Option<Endpoint>, Pos, Port<'_>) -> Result<Value, Diagnostic> + 'f;

/// What a scope's results are: a definition's outputs, or an arm's right
/// through-ports.
#[derive(Clone, Copy)]
enum Owner<'a> {
    /// The definition's name, without its namer dot.
    Definition(&'a str),
    /// The arm's index in its switch.
    Arm(usize),
}

impl<'a> Owner<'a> {
    /// The port that result `index` arrives at.
    fn result(self, index: usize) -> Port<'a> {
        match self {
            Owner::Definition(name) => Port::DefinitionOutput {
                definition: Some(name),
                index,
            },
            Owner::Arm(arm) => Port::RightThroughPort { arm, index },
        }
    }
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
                        let inputs = |n| message::quantity(n, "input");
                        let outputs = |n| message::quantity(n, "output");
                        let node = if inline { "inline node" } else { "node" };
                        let message = format!(
                            "{} takes {} and gives {}; this {node} has {} and {}",
                            name.text,
                            inputs(want.0),
                            outputs(want.1),
                            inputs(have.0),
                            outputs(have.1)
                        );
                        self.diags.push(Diagnostic::new(name.at, "C002", message));
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
        for (at, message) in cycles_of(g, &mut succ) {
            self.diags.push(Diagnostic::new(at, "C003", message));
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
        owner: Owner,
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
            let message = cycles_of(g, &mut succ)
                .into_iter()
                .next()
                .map_or_else(|| "cycle in the wires".to_owned(), |(_, m)| m);
            return Err(Diagnostic::new(at, "C003", message));
        }
        let mut pos = vec![0u32; n];
        for (i, &a) in order.iter().enumerate() {
            pos[a] = i as u32;
        }

        // `at` is the arrowhead of the slot being read, and `port` names it.
        let value = |ep: Option<Endpoint>, at: Pos, port: Port<'_>| match ep {
            None => Err(unfed(at, port)),
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
            .enumerate()
            .map(|(j, (ep, &at))| value(ep, at, owner.result(j)))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Block {
            params: params as u32,
            insts,
            results,
        })
    }

    fn lower_node<'n>(
        &self,
        node: &'n Node,
        slots: &[Option<Endpoint>],
        value: &SlotValue,
        fallback: Pos,
    ) -> Result<Inst, Diagnostic> {
        // Slot `i` arrives at `node.inputs[i]`, its arrowhead. `port(k)`
        // names the `k`th of the slots read.
        let args = |first: usize, n: usize, port: &dyn Fn(usize) -> Port<'n>| {
            (first..first + n)
                .map(|i| value(slots[i], node.inputs[i], port(i - first)))
                .collect::<Result<Vec<_>, _>>()
        };
        let op = match &node.kind {
            NodeKind::Const(lit) => Op::Const(lit.clone()),
            NodeKind::Call(name) | NodeKind::Inline(name) => {
                let unknown = || Diagnostic::new(name.at, "C001", format!("unknown name {}", name.text));
                let input = |index| Port::NodeInput {
                    word: Some(&name.text),
                    index,
                };
                match self.resolve(name).ok_or_else(unknown)? {
                    Callee::Prim(prim) => Op::Prim {
                        prim,
                        args: args(0, slots.len(), &input)?,
                    },
                    Callee::Nil => Op::Const(Literal::Nil),
                    Callee::Func(func) => Op::Call {
                        func,
                        args: args(0, slots.len(), &input)?,
                    },
                }
            }
            NodeKind::Switch(sw) => {
                let outputs = node.outputs.len();
                if sw.arms.iter().any(|a| a.results.len() != outputs) {
                    let results: Vec<usize> = sw.arms.iter().map(|a| a.results.len()).collect();
                    let message = message::unequal_arms(&results);
                    return Err(Diagnostic::new(sw.selector, "Y011", message));
                }
                // The selector, then every arm's through-ports.
                if slots.len() != 1 + sw.arms.iter().map(|a| a.params.len()).sum::<usize>() {
                    return Err(Diagnostic::new(sw.selector, "Y005", "switch slots out of step"));
                }
                let mut first = 1;
                let mut arms = Vec::with_capacity(sw.arms.len());
                for (a, arm) in sw.arms.iter().enumerate() {
                    let k = arm.params.len();
                    let through = |index| Port::LeftThroughPort { arm: a, index };
                    arms.push(ir::Arm {
                        guard: arm.guard.as_ref().map(|g| g.value.clone()),
                        args: args(first, k, &through)?,
                        body: self.lower_graph(&arm.body, k, &arm.results, Owner::Arm(a), fallback)?,
                    });
                    first += k;
                }
                Op::Switch {
                    selector: value(slots[0], sw.selector, Port::Selector)?,
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

/// For each strongly connected component of `succ` that holds a cycle, in
/// order of its first node: the first of its nodes' words in row-major
/// order, where `C003` points, and `C003`'s message, which follows a
/// shortest cycle from that node back to it.
fn cycles_of(g: &Graph, succ: &mut [Vec<usize>]) -> Vec<(Pos, String)> {
    // Successors in row-major order of their words, so that of two equally
    // short cycles the path takes the one a reader meets first.
    for next in succ.iter_mut() {
        next.sort_unstable_by_key(|&b| (word(&g.nodes[b]), b));
        next.dedup();
    }
    let mut component_of = vec![usize::MAX; succ.len()];
    let mut parent = vec![usize::MAX; succ.len()];
    let mut found = Vec::new();
    for (c, component) in components(succ).iter().enumerate() {
        let a = component[0];
        if component.len() == 1 && !succ[a].contains(&a) {
            continue;
        }
        for &b in component {
            component_of[b] = c;
        }
        let first = component
            .iter()
            .copied()
            .min_by_key(|&b| (word(&g.nodes[b]), b))
            .expect("a component has a node");
        let path = shortest_cycle(succ, first, |b| component_of[b] == c, &mut parent);
        found.push((word(&g.nodes[first]), cycle_message(g, &path)));
    }
    found
}

/// The nodes of a shortest cycle through `start`, which lies on one, among
/// the nodes `inside` admits: `start` first, then each node in the order the
/// wires run. A breadth-first search. `parent` holds one entry per node,
/// `usize::MAX` for each node `inside` admits, and is left marked.
fn shortest_cycle(
    succ: &[Vec<usize>],
    start: usize,
    inside: impl Fn(usize) -> bool,
    parent: &mut [usize],
) -> Vec<usize> {
    let mut queue = VecDeque::from([start]);
    while let Some(v) = queue.pop_front() {
        for &w in &succ[v] {
            if w == start {
                let mut path = vec![v];
                while let Some(&last) = path.last().filter(|&&u| u != start) {
                    path.push(parent[last]);
                }
                path.reverse();
                return path;
            }
            if inside(w) && parent[w] == usize::MAX {
                parent[w] = v;
                queue.push_back(w);
            }
        }
    }
    vec![start]
}

/// `C003`'s message: the words of the nodes on `path`, a cycle, then the
/// first again. A long cycle shows its first nodes and its last.
fn cycle_message(g: &Graph, path: &[usize]) -> String {
    /// Cycles through more nodes than this are shortened.
    const SHOWN: usize = 8;
    let phrase = |&a: &usize| node_phrase(&g.nodes[a]);
    let mut words: Vec<String> = if path.len() <= SHOWN {
        path.iter().map(phrase).collect()
    } else {
        let mut words: Vec<String> = path[..SHOWN - 2].iter().map(phrase).collect();
        words.push("…".to_owned());
        words.push(phrase(&path[path.len() - 1]));
        words
    };
    words.push(phrase(&path[0]));
    let through = match path.len() {
        n if n > SHOWN => format!(" through {n} nodes"),
        _ => String::new(),
    };
    format!("cycle in the wires{through}: {}", words.join(" → "))
}

/// A node as `C003` names it: by its word. A switch has none.
fn node_phrase(node: &Node) -> String {
    match &node.kind {
        NodeKind::Call(name) | NodeKind::Inline(name) => {
            message::node(Some(name.text.as_str()).filter(|t| !t.is_empty()))
        }
        NodeKind::Switch(_) => "a switch".to_owned(),
        NodeKind::Const(_) => "a constant".to_owned(),
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
