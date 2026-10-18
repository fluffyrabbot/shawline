//! Code generation from the dataflow IR to register bytecode.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use shawline_grid::Pos;
use shawline_syntax::ast::Literal;
use shawline_vm::bytecode::{self as bc, ConstId, Instr, Reg};

use crate::ir::{Block, Function, Op, Program, Value};

/// Compiles a checked program to a bytecode module.
///
/// Registers are reused once a value's last use has passed. A call whose
/// results are exactly its definition's results, in order, becomes a
/// `TailCall`, including through the arms of a switch whose results are, in
/// turn, exactly the enclosing results (SPEC §10).
///
/// Only a tail call runs out of row-major order. A switch whose outputs are the
/// results stays in place, and the nodes after it run inside each arm, before
/// the arm's own tail call or return. Main has no tail calls, so its frame
/// survives to its own `Return`. Every `Return` sits at its definition's namer
/// dot, which is where `R009` points (SPEC §14).
pub fn compile(program: &Program) -> bc::Module {
    compile_mapped(program).0
}

/// A block of the IR, named by the switch instructions and arms leading to
/// it from its function's body: `[]` for the body itself.
pub(crate) type Path = Rc<[(u32, u32)]>;

/// The IR instruction a bytecode instruction carries out. Its outputs are
/// that instruction's outputs, ordinal for ordinal.
#[derive(Clone, Debug)]
pub(crate) struct Site {
    pub path: Path,
    pub inst: u32,
    /// A `TailCall`, whose outputs are its frame's eventual results.
    pub tail: bool,
}

/// [`compile`], also giving, for each function and each of its bytecode
/// instructions, the IR instruction it carries out. Moves, jumps and returns
/// carry out none. An IR instruction after a tail-position switch is emitted
/// once per arm, so several bytecode instructions may share a site.
pub(crate) fn compile_mapped(program: &Program) -> (bc::Module, Vec<Vec<Option<Site>>>) {
    let (functions, sites) = program
        .functions
        .iter()
        .enumerate()
        .map(|(i, f)| function(program, f, i as u32 != program.main))
        .unzip();
    let module = bc::Module {
        functions,
        main: program.main,
    };
    (module, sites)
}

fn function(
    program: &Program,
    f: &Function,
    tail_calls: bool,
) -> (bc::Function, Vec<Option<Site>>) {
    let mut e = Emitter {
        program,
        tail_calls,
        ret_at: f.at,
        consts: Vec::new(),
        const_ids: HashMap::new(),
        code: Vec::new(),
        spans: Vec::new(),
        sites: Vec::new(),
        busy: Vec::new(),
        high: 0,
    };
    let params: Vec<Reg> = (0..f.body.params).map(|_| e.alloc()).collect();
    let scope = Scope {
        path: Rc::from([]),
        origin: None,
    };
    e.block(&f.body, &params, true, Ctx::Tail(&[]), f.at, &scope);
    let results = f.body.results.len();
    let registers = e.high.max(params.len()).max(results);
    let function = bc::Function {
        name: f.name.clone(),
        params: reg(params.len()),
        results: reg(results),
        registers: reg(registers),
        consts: e.consts,
        code: e.code,
        spans: e.spans,
    };
    (function, e.sites)
}

/// Where the block being emitted sits in the IR.
#[derive(Clone)]
struct Scope {
    path: Path,
    /// For a deferred block, each instruction's index in the block it was
    /// taken from; otherwise instructions keep their own indices.
    origin: Option<Vec<u32>>,
}

impl Scope {
    fn site(&self, i: usize, tail: bool) -> Site {
        let inst = self.origin.as_ref().map_or(i as u32, |o| o[i]);
        Site {
            path: self.path.clone(),
            inst,
            tail,
        }
    }

    /// The scope of arm `arm` of the switch at index `i`.
    fn arm(&self, i: usize, arm: usize) -> Scope {
        let switch = self.site(i, false).inst;
        let path = self
            .path
            .iter()
            .copied()
            .chain([(switch, arm as u32)])
            .collect();
        Scope { path, origin: None }
    }
}

fn reg(n: usize) -> Reg {
    Reg::try_from(n).expect("a frame fits in u16 registers")
}

/// Where a block's results go.
#[derive(Clone, Copy)]
enum Ctx<'a> {
    /// They are the function's results: return them, or tail-call for them.
    /// These deferred nodes run just before the tail call or return.
    Tail(&'a [Deferred]),
    /// They are a switch's outputs: move them into these registers. The
    /// switch emits the jump to its end.
    Join(&'a [Reg]),
}

/// Nodes that row-major order puts after a tail-position switch, rebuilt as a
/// block that reads the enclosing values it needs as parameters.
#[derive(Clone)]
struct Deferred {
    block: Block,
    params: Vec<Reg>,
    scope: Scope,
}

struct Emitter<'p> {
    program: &'p Program,
    /// Whether calls in tail position become `TailCall`s.
    tail_calls: bool,
    /// Where every `Return` points: the definition's namer dot.
    ret_at: Pos,
    consts: Vec<bc::Value>,
    const_ids: HashMap<Literal, ConstId>,
    code: Vec<Instr>,
    spans: Vec<Pos>,
    /// Parallel to `code`.
    sites: Vec<Option<Site>>,
    /// Which registers hold a live value.
    busy: Vec<bool>,
    high: usize,
}

impl Emitter<'_> {
    fn emit(&mut self, instr: Instr, at: Pos) -> usize {
        self.code.push(instr);
        self.spans.push(at);
        self.sites.push(None);
        self.code.len() - 1
    }

    fn constant(&mut self, lit: &Literal) -> ConstId {
        if let Some(&id) = self.const_ids.get(lit) {
            return id;
        }
        let id = self.consts.len() as ConstId;
        self.consts.push(match lit {
            Literal::Int(n) => bc::Value::Int(*n),
            Literal::Text(t) => bc::Value::Text(Rc::from(t.as_str())),
            Literal::Nil => bc::Value::List(None),
        });
        self.const_ids.insert(lit.clone(), id);
        id
    }

    fn alloc(&mut self) -> Reg {
        self.alloc_run(1)
    }

    /// The lowest `k` consecutive free registers, marked busy. Calls and
    /// returns need their values contiguous.
    fn alloc_run(&mut self, k: usize) -> Reg {
        let start = (0..=self.busy.len())
            .find(|&s| (s..s + k).all(|r| !self.is_busy(r)))
            .expect("past the end is always free");
        self.claim(start, k);
        reg(start)
    }

    fn is_busy(&self, r: usize) -> bool {
        self.busy.get(r).copied().unwrap_or(false)
    }

    /// Marks `base..base + k` busy.
    fn claim(&mut self, base: usize, k: usize) {
        if self.busy.len() < base + k {
            self.busy.resize(base + k, false);
        }
        self.busy[base..base + k].fill(true);
        self.high = self.high.max(self.busy.len());
    }

    /// A run of `k` registers, the first holding copies of `srcs`. The run is
    /// left free: the caller claims what it keeps before allocating again.
    fn moved_run(&mut self, srcs: &[Reg], k: usize, at: Pos) -> Reg {
        let base = self.alloc_run(k);
        for (j, &src) in srcs.iter().enumerate() {
            self.emit(
                Instr::Move {
                    dst: base + reg(j),
                    src,
                },
                at,
            );
        }
        for j in 0..k {
            self.free(base + reg(j));
        }
        base
    }

    fn free(&mut self, r: Reg) {
        self.busy[r as usize] = false;
    }

    /// Emits `b` with its parameters in `params`. `owned` says whether the
    /// block may reuse its parameters' registers: an arm's parameters belong
    /// to the enclosing block. `scope` is where `b` sits in the IR.
    fn block(&mut self, b: &Block, params: &[Reg], owned: bool, ctx: Ctx, at: Pos, scope: &Scope) {
        let (direct, deferred) = match ctx {
            Ctx::Tail(deferred) => (self.direct_inst(b), deferred),
            Ctx::Join(_) => (None, &[][..]),
        };
        // Only a tail call may run out of row-major order (SPEC §10). Nothing
        // reads its outputs, so it can go last.
        let moved = direct.filter(|&i| self.tail_calls && matches!(b.insts[i].op, Op::Call { .. }));
        let order: Vec<usize> = (0..b.insts.len())
            .filter(|&i| Some(i) != moved)
            .chain(moved)
            .collect();
        let mut last: HashMap<Value, usize> = HashMap::new();
        for (p, &i) in order.iter().enumerate() {
            for v in operands(&b.insts[i].op) {
                last.insert(v, p);
            }
        }
        for &v in &b.results {
            last.insert(v, order.len());
        }

        let mut regs: HashMap<Value, Reg> = params
            .iter()
            .enumerate()
            .map(|(i, &r)| (Value::Param(i as u32), r))
            .collect();
        if owned {
            for (i, &r) in params.iter().enumerate() {
                if !last.contains_key(&Value::Param(i as u32)) {
                    self.free(r);
                }
            }
        }

        for (p, &i) in order.iter().enumerate() {
            let inst = &b.insts[i];
            let src = |v: &Value| regs[v];
            let dying: HashSet<Reg> = operands(&inst.op)
                .into_iter()
                .filter(|v| last[v] == p && (owned || !matches!(v, Value::Param(_))))
                .map(|v| regs[&v])
                .collect();
            let outs: Vec<Reg> = match &inst.op {
                Op::Const(lit) => {
                    let idx = self.constant(lit);
                    let dst = self.alloc();
                    let j = self.emit(Instr::Const { dst, idx }, inst.at);
                    self.sites[j] = Some(scope.site(i, false));
                    vec![dst]
                }
                Op::Prim { prim, args } => {
                    let mut srcs = [0; 2];
                    for (slot, a) in srcs.iter_mut().zip(args) {
                        *slot = src(a);
                    }
                    // The VM reads every source before writing, so a dying
                    // source's register can take an output.
                    dying.iter().for_each(|&r| self.free(r));
                    let mut dst = [0; 2];
                    let n = prim.arity().1 as usize;
                    for slot in &mut dst[..n] {
                        *slot = self.alloc();
                    }
                    let j = self.emit(
                        Instr::Prim {
                            prim: *prim,
                            dst,
                            src: srcs,
                        },
                        inst.at,
                    );
                    self.sites[j] = Some(scope.site(i, false));
                    dst[..n].to_vec()
                }
                Op::Call { func, args } => {
                    let argc = reg(args.len());
                    let nres = self.program.functions[*func as usize].body.results.len();
                    let k = args.len().max(nres);
                    let srcs: Vec<Reg> = args.iter().map(src).collect();
                    let in_run = srcs
                        .first()
                        .copied()
                        .filter(|&r0| srcs.iter().enumerate().all(|(j, &r)| r == r0 + reg(j)));
                    if moved == Some(i) {
                        self.run_deferred(deferred, inst.at);
                        // The frame is replaced, so arguments already in a
                        // run can stay put.
                        let base = in_run.unwrap_or_else(|| self.moved_run(&srcs, k, inst.at));
                        let j = self.emit(
                            Instr::TailCall {
                                func: *func,
                                base,
                                argc,
                            },
                            inst.at,
                        );
                        self.sites[j] = Some(scope.site(i, true));
                        return;
                    }
                    let reuse = in_run.filter(|&r0| {
                        srcs.iter().all(|r| dying.contains(r))
                            && (args.len()..k).all(|j| !self.is_busy(r0 as usize + j))
                    });
                    // The moves happen into a fresh run before any dying
                    // operand is freed, so none clobbers a later move's source.
                    let base = reuse.unwrap_or_else(|| self.moved_run(&srcs, k, inst.at));
                    dying.iter().for_each(|&r| self.free(r));
                    self.claim(base as usize, nres);
                    let j = self.emit(
                        Instr::Call {
                            func: *func,
                            base,
                            argc,
                            dst: base,
                        },
                        inst.at,
                    );
                    self.sites[j] = Some(scope.site(i, false));
                    (0..nres).map(|j| base + reg(j)).collect()
                }
                Op::Switch {
                    selector,
                    arms,
                    outputs,
                } => {
                    let sel = src(selector);
                    let arm_params: Vec<Vec<Reg>> = arms
                        .iter()
                        .map(|a| a.args.iter().map(src).collect())
                        .collect();
                    if direct == Some(i) {
                        // The arms return, so whatever follows the switch
                        // runs in each arm, ahead of what outer blocks defer.
                        let rest = &order[p + 1..];
                        let mut chain = Vec::with_capacity(deferred.len() + 1);
                        if !rest.is_empty() {
                            chain.push(defer(b, rest, &regs, scope));
                        }
                        chain.extend_from_slice(deferred);
                        let arm = |a| scope.arm(i, a);
                        self.switch(sel, arms, &arm_params, Ctx::Tail(&chain), inst.at, &arm);
                        return;
                    }
                    let outs: Vec<Reg> = (0..*outputs).map(|_| self.alloc()).collect();
                    let arm = |a| scope.arm(i, a);
                    self.switch(sel, arms, &arm_params, Ctx::Join(&outs), inst.at, &arm);
                    dying.iter().for_each(|&r| self.free(r));
                    outs
                }
            };
            for (j, &r) in outs.iter().enumerate() {
                let v = Value::Out {
                    inst: i as u32,
                    output: j as u32,
                };
                if last.contains_key(&v) {
                    regs.insert(v, r);
                } else {
                    self.free(r);
                }
            }
        }

        let results: Vec<Reg> = b.results.iter().map(|v| regs[v]).collect();
        match ctx {
            Ctx::Join(outs) => {
                for (&dst, &src) in outs.iter().zip(&results) {
                    if dst != src {
                        self.emit(Instr::Move { dst, src }, at);
                    }
                }
            }
            Ctx::Tail(_) => {
                self.run_deferred(deferred, at);
                let count = reg(results.len());
                let contiguous = results.windows(2).all(|w| w[1] == w[0] + 1);
                let base = match results.first() {
                    None => 0,
                    Some(&r) if contiguous => r,
                    Some(_) => {
                        let base = self.alloc_run(results.len());
                        for (j, &src) in results.iter().enumerate() {
                            self.emit(
                                Instr::Move {
                                    dst: base + reg(j),
                                    src,
                                },
                                at,
                            );
                        }
                        base
                    }
                };
                self.emit(Instr::Return { base, count }, self.ret_at);
            }
        }
    }

    /// Guarded arms test the selector in order; a failed test jumps to the
    /// next arm. The default arm, or `NoMatch`, comes last.
    fn switch(
        &mut self,
        sel: Reg,
        arms: &[crate::ir::Arm],
        params: &[Vec<Reg>],
        ctx: Ctx,
        at: Pos,
        scope: &dyn Fn(usize) -> Scope,
    ) {
        let mut to_end = Vec::new();
        for (a, (arm, params)) in arms.iter().zip(params).enumerate() {
            let skip = arm.guard.as_ref().map(|g| {
                let idx = self.constant(g);
                self.emit(
                    Instr::JumpIfNe {
                        src: sel,
                        idx,
                        target: 0,
                    },
                    at,
                )
            });
            // Arms are exclusive, so each starts from the same registers.
            let saved = self.busy.clone();
            self.block(&arm.body, params, false, ctx, at, &scope(a));
            if matches!(ctx, Ctx::Join(_)) && skip.is_some() {
                to_end.push(self.emit(Instr::Jump { target: 0 }, at));
            }
            self.busy = saved;
            self.busy.resize(self.high, false);
            if let Some(j) = skip {
                self.patch(j);
            }
        }
        if arms.last().is_none_or(|a| a.guard.is_some()) {
            self.emit(Instr::NoMatch, at);
        }
        for j in to_end {
            self.patch(j);
        }
    }

    /// Points the jump at `j` to the next instruction to be emitted.
    fn patch(&mut self, j: usize) {
        let here = self.code.len() as bc::Label;
        match &mut self.code[j] {
            Instr::Jump { target } | Instr::JumpIfNe { target, .. } => *target = here,
            _ => unreachable!("only jumps are patched"),
        }
    }

    /// Emits deferred nodes in order. Their outputs feed only each other.
    fn run_deferred(&mut self, deferred: &[Deferred], at: Pos) {
        for d in deferred {
            self.block(&d.block, &d.params, false, Ctx::Join(&[]), at, &d.scope);
        }
    }

    /// The call or switch whose outputs are exactly `b`'s results, in order,
    /// and feed nothing else in `b`: its results can be returned directly.
    /// Whether it may also run last is the caller's decision.
    fn direct_inst(&self, b: &Block) -> Option<usize> {
        let Value::Out { inst, .. } = *b.results.first()? else {
            return None;
        };
        let outputs = match &b.insts[inst as usize].op {
            Op::Call { func, .. } => self.program.functions[*func as usize].body.results.len(),
            Op::Switch { outputs, .. } => *outputs as usize,
            Op::Const(_) | Op::Prim { .. } => return None,
        };
        let exact = outputs == b.results.len()
            && b.results.iter().enumerate().all(|(j, &v)| {
                v == Value::Out {
                    inst,
                    output: j as u32,
                }
            });
        let read_elsewhere = b
            .insts
            .iter()
            .flat_map(|i| operands(&i.op))
            .any(|v| matches!(v, Value::Out { inst: k, .. } if k == inst));
        (exact && !read_elsewhere).then_some(inst as usize)
    }
}

/// Rebuilds instructions `idxs` of `b` as a block of their own. Values they
/// read from the rest of `b` become its parameters, held in `regs`.
fn defer(b: &Block, idxs: &[usize], regs: &HashMap<Value, Reg>, scope: &Scope) -> Deferred {
    let renumbered: HashMap<u32, u32> = idxs
        .iter()
        .enumerate()
        .map(|(j, &i)| (i as u32, j as u32))
        .collect();
    let mut outer: Vec<Value> = Vec::new();
    let mut map = |v: Value| match v {
        Value::Out { inst, output } if renumbered.contains_key(&inst) => Value::Out {
            inst: renumbered[&inst],
            output,
        },
        _ => {
            let k = outer.iter().position(|&o| o == v).unwrap_or_else(|| {
                outer.push(v);
                outer.len() - 1
            });
            Value::Param(k as u32)
        }
    };
    let insts = idxs
        .iter()
        .map(|&i| {
            let inst = &b.insts[i];
            let op = match &inst.op {
                Op::Const(lit) => Op::Const(lit.clone()),
                Op::Prim { prim, args } => Op::Prim {
                    prim: *prim,
                    args: args.iter().map(|&v| map(v)).collect(),
                },
                Op::Call { func, args } => Op::Call {
                    func: *func,
                    args: args.iter().map(|&v| map(v)).collect(),
                },
                Op::Switch {
                    selector,
                    arms,
                    outputs,
                } => Op::Switch {
                    selector: map(*selector),
                    arms: arms
                        .iter()
                        .map(|a| crate::ir::Arm {
                            guard: a.guard.clone(),
                            args: a.args.iter().map(|&v| map(v)).collect(),
                            body: a.body.clone(),
                        })
                        .collect(),
                    outputs: *outputs,
                },
            };
            crate::ir::Inst { op, at: inst.at }
        })
        .collect();
    Deferred {
        block: Block {
            params: outer.len() as u32,
            insts,
            results: Vec::new(),
        },
        params: outer.iter().map(|v| regs[v]).collect(),
        scope: Scope {
            path: scope.path.clone(),
            origin: Some(idxs.iter().map(|&i| scope.site(i, false).inst).collect()),
        },
    }
}

/// The values an instruction reads in its own block. A switch reads its
/// selector and its arms' arguments; arm bodies see only their parameters.
fn operands(op: &Op) -> Vec<Value> {
    match op {
        Op::Const(_) => Vec::new(),
        Op::Prim { args, .. } | Op::Call { args, .. } => args.clone(),
        Op::Switch { selector, arms, .. } => std::iter::once(*selector)
            .chain(arms.iter().flat_map(|a| a.args.iter().copied()))
            .collect(),
    }
}
