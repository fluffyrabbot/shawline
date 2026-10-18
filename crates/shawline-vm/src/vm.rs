//! The dispatch loop and the primitives of SPEC §9.

use std::mem;

use shawline_grid::{Code, Diagnostic, Pos};

use crate::MALFORMED;
use crate::bytecode::{FuncId, Instr, Module, Prim, Reg, Value};
use crate::record::Recorder;
use crate::value::{equal, heap, printed_len};
use crate::verify::verify;

/// The most calls that may nest below `main` (SPEC §10). One more stops the
/// program with `R007` instead of growing until the machine runs out of
/// memory.
pub const MAX_FRAMES: usize = 1_000_000;

/// The most values the live frames may hold together. A value is 24 bytes,
/// so that is about 400 MB of registers. It bounds memory for definitions
/// with many values, which reach it in fewer than [`MAX_FRAMES`] calls.
pub const MAX_REGISTERS: usize = 1 << 24;

/// The most pairs and list cells that may be alive at once (SPEC §10). One
/// more stops the program with `R008`.
pub const MAX_CELLS: usize = 1 << 23;

/// The most characters a result of `main` may print as (SPEC §10). A longer
/// one stops the program with `R009`.
pub const MAX_OUTPUT: usize = 1_000_000;

/// The bounds a run enforces; tests shrink them.
#[derive(Clone, Copy)]
pub(crate) struct Limits {
    pub frames: usize,
    pub registers: usize,
    pub cells: usize,
    pub output: usize,
}

impl Limits {
    pub const SPEC: Limits = Limits {
        frames: MAX_FRAMES,
        registers: MAX_REGISTERS,
        cells: MAX_CELLS,
        output: MAX_OUTPUT,
    };
}

/// An activation record. Its registers are `regs[base..base + registers]` in
/// the shared register file, so a call costs a resize, not an allocation.
struct Frame {
    func: FuncId,
    /// Where to resume once a callee returns; only stale for the running frame.
    pc: usize,
    base: usize,
    /// Where this frame's results land in its caller's registers.
    dst: Reg,
}

/// Runs `main` with no arguments and returns its results.
///
/// The module is verified first; a malformed one is reported as `A001`.
/// Runtime errors are `R001`–`R009`, positioned at the failing instruction's
/// span. `R009` is at the `Return` that ends `main`'s frame.
pub fn run(module: &Module) -> Result<Vec<Value>, Diagnostic> {
    execute(module, Limits::SPEC).map(|(results, _)| results)
}

/// [`run`] under `limits`, also reporting the deepest the frame stack grew.
pub(crate) fn execute(
    module: &Module,
    limits: Limits,
) -> Result<(Vec<Value>, usize), Diagnostic> {
    match dispatch::<false>(module, limits, &mut Recorder::default()) {
        Ok(done) => Ok(done),
        Err(Halt::Error(e)) => Err(e),
        Err(Halt::Stopped) => unreachable!("an untraced run has no step budget"),
    }
}

/// Why a run ended without results.
pub(crate) enum Halt {
    Error(Diagnostic),
    /// A traced run used up its steps.
    Stopped,
}

impl From<Diagnostic> for Halt {
    fn from(e: Diagnostic) -> Halt {
        Halt::Error(e)
    }
}

/// The one dispatch loop behind [`run`] and [`crate::run_traced`]. With
/// `TRACE` false every recording branch is a constant and compiles away, so
/// `rec` is never touched.
pub(crate) fn dispatch<const TRACE: bool>(
    module: &Module,
    limits: Limits,
    rec: &mut Recorder,
) -> Result<(Vec<Value>, usize), Halt> {
    verify(module).map_err(|e| {
        let f = e.func.map(|i| &module.functions[i]);
        let at = f.zip(e.instr).and_then(|(f, pc)| f.spans.get(pc).copied());
        Diagnostic::new(at.unwrap_or(Pos::new(0, 0)), MALFORMED, e.message)
    })?;
    // Declared before anything holding values, so it stops counting last.
    let _heap = heap::start();

    let mut f = &module.functions[module.main as usize];
    let mut regs = vec![Value::Int(0); f.registers as usize];
    let mut frames = vec![Frame { func: module.main, pc: 0, base: 0, dst: 0 }];
    let mut peak = 1;
    // Holds arguments and results while frames are torn down.
    let mut scratch: Vec<Value> = Vec::new();
    let mut base = 0;
    let mut pc = 0;
    if TRACE {
        rec.start(module);
    }
    let running = |frames: &[Frame]| frames.last().expect("a running frame").func;

    // Verification guarantees every index below is in bounds.
    loop {
        if TRACE && !rec.tick() {
            return Err(Halt::Stopped);
        }
        let r = |reg: Reg| base + reg as usize;
        match f.code[pc] {
            Instr::Const { dst, idx } => {
                regs[r(dst)] = f.consts[idx as usize].clone();
                if TRACE {
                    rec.write(running(&frames), pc, 0, &regs[r(dst)]);
                }
                pc += 1;
            }
            Instr::Move { dst, src } => {
                regs[r(dst)] = regs[r(src)].clone();
                if TRACE {
                    rec.write(running(&frames), pc, 0, &regs[r(dst)]);
                }
                pc += 1;
            }
            Instr::Prim { prim, dst, src } => {
                let (a, b) = (&regs[r(src[0])], &regs[r(src[1])]);
                let (x, y) = apply(prim, a, b)
                    .map_err(|(code, msg)| Diagnostic::new(f.spans[pc], code, msg))?;
                if matches!(prim, Prim::Pair | Prim::Prepend) && heap::live() > limits.cells {
                    let message = format!("heap exhausted: {} cells alive", heap::live());
                    return Err(Diagnostic::new(f.spans[pc], "R008", message).into());
                }
                regs[r(dst[0])] = x;
                let two = prim.arity().1 == 2;
                if two {
                    regs[r(dst[1])] = y;
                }
                if TRACE {
                    let func = running(&frames);
                    rec.write(func, pc, 0, &regs[r(dst[0])]);
                    if two {
                        rec.write(func, pc, 1, &regs[r(dst[1])]);
                    }
                }
                pc += 1;
            }
            Instr::Call { func, base: args, argc, dst } => {
                let needed = module.functions[func as usize].registers as usize;
                // `frames` holds main, so its length is the depth this call
                // would reach below main.
                if frames.len() > limits.frames || regs.len() + needed > limits.registers {
                    let message = format!("call stack exhausted {} calls deep", frames.len());
                    return Err(Diagnostic::new(f.spans[pc], "R007", message).into());
                }
                frames.last_mut().expect("a running frame").pc = pc + 1;
                f = &module.functions[func as usize];
                let callee = regs.len();
                regs.extend_from_within(r(args)..r(args) + argc as usize);
                if TRACE {
                    rec.call(func, &regs[callee..]);
                }
                regs.resize(callee + f.registers as usize, Value::Int(0));
                frames.push(Frame { func, pc: 0, base: callee, dst });
                peak = peak.max(frames.len());
                base = callee;
                pc = 0;
            }
            Instr::TailCall { func, base: args, argc } => {
                let needed = module.functions[func as usize].registers as usize;
                if base + needed > limits.registers {
                    let message = format!("call stack exhausted {} calls deep", frames.len() - 1);
                    return Err(Diagnostic::new(f.spans[pc], "R007", message).into());
                }
                scratch.extend(regs.drain(r(args)..r(args) + argc as usize));
                if TRACE {
                    rec.tail(frames.len(), running(&frames), pc, func, &scratch);
                }
                regs.truncate(base);
                regs.append(&mut scratch);
                f = &module.functions[func as usize];
                regs.resize(base + f.registers as usize, Value::Int(0));
                frames.last_mut().expect("a running frame").func = func;
                pc = 0;
            }
            Instr::Return { base: from, count } => {
                scratch.extend(regs.drain(r(from)..r(from) + count as usize));
                regs.truncate(base);
                let done = frames.pop().expect("a running frame");
                let Some(caller) = frames.last() else {
                    if scratch.iter().any(|v| printed_len(v, limits.output).is_none()) {
                        let message = format!("a result prints longer than {}", limits.output);
                        return Err(Diagnostic::new(f.spans[pc], "R009", message).into());
                    }
                    if TRACE {
                        rec.ret(1, &scratch, None);
                    }
                    return Ok((mem::take(&mut scratch), peak));
                };
                if TRACE {
                    rec.ret(frames.len() + 1, &scratch, Some((caller.func, caller.pc - 1)));
                }
                f = &module.functions[caller.func as usize];
                base = caller.base;
                pc = caller.pc;
                let dst = base + done.dst as usize;
                for (slot, v) in regs[dst..].iter_mut().zip(scratch.drain(..)) {
                    *slot = v;
                }
            }
            Instr::Jump { target } => pc = target as usize,
            Instr::JumpIfNe { src, idx, target } => {
                pc = if equal(&regs[r(src)], &f.consts[idx as usize]) {
                    pc + 1
                } else {
                    target as usize
                };
            }
            Instr::NoMatch => {
                return Err(Diagnostic::new(f.spans[pc], "R005", "no arm matches").into());
            }
        }
    }
}

type Fault = (Code, String);

/// Runs one primitive. Primitives with one output leave the second unused.
/// Pairs and list cells are made through [`heap::cell`], so they count.
fn apply(p: Prim, a: &Value, b: &Value) -> Result<(Value, Value), Fault> {
    let one = |v| Ok((v, Value::Int(0)));
    let truth = |t: bool| one(Value::Int(t.into()));
    let overflow = || ("R001", format!("{} overflows", p.letter()));
    match p {
        Prim::Add | Prim::Sub | Prim::Mul | Prim::Div => {
            let (x, y) = (int(p, a)?, int(p, b)?);
            let n = match p {
                Prim::Add => x.checked_add(y),
                Prim::Sub => x.checked_sub(y),
                Prim::Mul => x.checked_mul(y),
                _ if y == 0 => return Err(("R002", format!("{} by zero", p.letter()))),
                // Rust's `/` truncates toward zero, as SPEC §9 asks.
                _ => x.checked_div(y),
            };
            one(Value::Int(n.ok_or_else(overflow)?))
        }
        Prim::Less => truth(int(p, a)? < int(p, b)?),
        Prim::Greater => truth(int(p, a)? > int(p, b)?),
        Prim::Min => one(Value::Int(int(p, a)?.min(int(p, b)?))),
        Prim::Max => one(Value::Int(int(p, a)?.max(int(p, b)?))),
        Prim::Equal => truth(equal(a, b)),
        Prim::NotEqual => truth(!equal(a, b)),
        Prim::Pair => one(Value::Pair(heap::cell(a.clone(), b.clone()))),
        Prim::Unpair => match a {
            Value::Pair(cell) => Ok((cell.0.clone(), cell.1.clone())),
            other => Err(mismatch(p, "a pair", other)),
        },
        Prim::Prepend => match b {
            Value::List(_) => one(Value::List(Some(heap::cell(a.clone(), b.clone())))),
            other => Err(mismatch(p, "a list tail", other)),
        },
        Prim::Split => match a {
            Value::List(Some(cell)) => Ok((cell.0.clone(), cell.1.clone())),
            Value::List(None) => Err(("R003", format!("{} of the empty list", p.letter()))),
            other => Err(mismatch(p, "a list", other)),
        },
        Prim::Show => one(Value::Text(int(p, a)?.to_string().into())),
        Prim::Read => match a {
            Value::Text(t) => one(Value::Int(read(t)?)),
            other => Err(mismatch(p, "text", other)),
        },
    }
}

fn int(p: Prim, v: &Value) -> Result<i64, Fault> {
    match v {
        Value::Int(n) => Ok(*n),
        other => Err(mismatch(p, "an integer", other)),
    }
}

/// Parses an optional `-` and ASCII digits. A well-formed number too large
/// for an Int is an overflow, not malformed text.
fn read(t: &str) -> Result<i64, Fault> {
    let digits = t.strip_prefix('-').unwrap_or(t);
    if digits.is_empty() || !digits.bytes().all(|c| c.is_ascii_digit()) {
        return Err(("R006", format!("«{t}» isn't a number")));
    }
    t.parse().map_err(|_| ("R001", format!("{t} overflows an integer")))
}

fn mismatch(p: Prim, wanted: &str, got: &Value) -> Fault {
    let got = match got {
        Value::Int(_) => "an integer",
        Value::Text(_) => "text",
        Value::Pair(_) => "a pair",
        Value::List(_) => "a list",
    };
    ("R004", format!("{} wants {wanted}, not {got}", p.letter()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assemble;

    #[test]
    fn tail_calls_run_in_constant_stack() {
        let src = "
fn 𐑒𐑬𐑯𐑑 params 2 results 1 registers 3   ; count(n, acc) = count(n-1, acc+1)
  const k0 0
  const k1 1
  jne r0 k0 2
  ret r1 1
  const r2 k1
  prim 𐑚 r0 r0,r2
  prim 𐑐 r1 r1,r2
  tail 𐑒𐑬𐑯𐑑 r0 2
end
fn 𐑥𐑱𐑯 params 0 results 1 registers 3
  const k0 1000000
  const k1 0
  const r0 k0
  const r1 k1
  call 𐑒𐑬𐑯𐑑 r0 2 r2
  ret r2 1
end
main 𐑥𐑱𐑯
";
        let (results, peak) = execute(&assemble(src).unwrap(), Limits::SPEC).unwrap();
        assert_eq!(results, [Value::Int(1_000_000)]);
        assert_eq!(peak, 2, "main and one reused frame");
    }

    /// Runaway recursion stops at the call, long before memory runs out.
    #[test]
    fn runaway_recursion_is_r007() {
        let deep = |registers: u16| {
            format!(
                "
fn 𐑛𐑰𐑐 params 1 results 1 registers {registers}
  call 𐑛𐑰𐑐 r0 1 r1 at 3:4
  ret r1 1
end
fn 𐑥𐑱𐑯 params 0 results 1 registers 1
  const k0 0
  const r0 k0
  call 𐑛𐑰𐑐 r0 1 r0
  ret r0 1
end
main 𐑥𐑱𐑯
"
            )
        };
        for registers in [2, 60_000] {
            let err = run(&assemble(&deep(registers)).unwrap()).unwrap_err();
            assert_eq!((err.code, err.at), ("R007", Pos::new(3, 4)), "{registers} registers");
        }
    }

    /// `down(n)` nests n + 1 calls below main.
    const DOWN: &str = "
fn 𐑛𐑬𐑯 params 1 results 1 registers 2
  const k0 0
  const k1 1
  jne r0 k0 2
  ret r0 1
  const r1 k1
  prim 𐑚 r0 r0,r1
  call 𐑛𐑬𐑯 r0 1 r1 at 6:2
  ret r1 1
end
fn 𐑥𐑱𐑯 params 0 results 1 registers 1
  const k0 N
  const r0 k0
  call 𐑛𐑬𐑯 r0 1 r0
  ret r0 1
end
main 𐑥𐑱𐑯
";

    #[test]
    fn exactly_a_million_nested_calls_succeed() {
        let at_limit = assemble(&DOWN.replace(" N", " 999999")).unwrap();
        let (results, peak) = execute(&at_limit, Limits::SPEC).unwrap();
        assert_eq!((results, peak), (vec![Value::Int(0)], MAX_FRAMES + 1));
        let err = run(&assemble(&DOWN.replace(" N", " 1000000")).unwrap()).unwrap_err();
        assert_eq!((err.code, err.at), ("R007", Pos::new(6, 2)));
    }

    #[test]
    fn tail_calls_respect_the_register_cap() {
        let src = "
fn 𐑢𐑲𐑛 params 0 results 1 registers 100
  ret r0 1
end
fn 𐑥𐑱𐑯 params 0 results 1 registers 1
  tail 𐑢𐑲𐑛 r0 0 at 2:2
end
main 𐑥𐑱𐑯
";
        let limits = Limits { registers: 99, ..Limits::SPEC };
        let err = execute(&assemble(src).unwrap(), limits).unwrap_err();
        assert_eq!((err.code, err.at), ("R007", Pos::new(2, 2)));
    }

    const SMALL_HEAP: Limits = Limits { cells: 100, ..Limits::SPEC };

    /// `grow(n, l)` prepends n cells to `l` in a tail loop.
    const GROW: &str = "
fn 𐑜𐑮𐑴 params 2 results 1 registers 3
  const k0 0
  const k1 1
  jne r0 k0 2
  ret r1 1
  prim 𐑟 r1 r0,r1 at 5:13
  const r2 k1
  prim 𐑚 r0 r0,r2
  tail 𐑜𐑮𐑴 r0 2
end
fn 𐑥𐑱𐑯 params 0 results 1 registers 2
  const k0 N
  const k1 nil
  const r0 k0
  const r1 k1
  call 𐑜𐑮𐑴 r0 2 r0
  ret r0 1
end
main 𐑥𐑱𐑯
";

    #[test]
    fn a_growing_list_stops_at_the_heap_limit() {
        let ok = assemble(&GROW.replace(" N", " 100")).unwrap();
        let too_many = assemble(&GROW.replace(" N", " 101")).unwrap();
        // Each run counts afresh, even while an earlier run's list survives.
        let kept = execute(&ok, SMALL_HEAP).unwrap();
        let err = execute(&too_many, SMALL_HEAP).unwrap_err();
        assert_eq!((err.code, err.at), ("R008", Pos::new(5, 13)));
        assert!(execute(&ok, SMALL_HEAP).is_ok());
        drop(kept);
        assert!(execute(&ok, SMALL_HEAP).is_ok());
    }

    #[test]
    fn freed_cells_count_down() {
        // Ten thousand pairs, each dropped as soon as the next is made.
        let src = "
fn 𐑤𐑵𐑐 params 1 results 1 registers 3
  const k0 0
  const k1 1
  jne r0 k0 2
  ret r0 1
  prim 𐑒 r1 r0,r0
  const r2 k1
  prim 𐑚 r0 r0,r2
  tail 𐑤𐑵𐑐 r0 1
end
fn 𐑥𐑱𐑯 params 0 results 1 registers 1
  const k0 10000
  const r0 k0
  call 𐑤𐑵𐑐 r0 1 r0
  ret r0 1
end
main 𐑥𐑱𐑯
";
        assert!(execute(&assemble(src).unwrap(), SMALL_HEAP).is_ok());
    }

    #[test]
    fn a_result_too_long_to_print_is_r009() {
        // dag(n, x) doubles x n times by pairing it with itself: n cells that
        // print as 2^n leaves.
        let dag = |n: u32| {
            format!(
                "
fn 𐑛𐑨𐑜 params 2 results 1 registers 3
  const k0 0
  const k1 1
  jne r0 k0 2
  ret r1 1
  prim 𐑒 r1 r1,r1
  const r2 k1
  prim 𐑚 r0 r0,r2
  tail 𐑛𐑨𐑜 r0 2
end
fn 𐑥𐑱𐑯 params 0 results 1 registers 2
  const k0 {n}
  const k1 0
  const r0 k0
  const r1 k1
  call 𐑛𐑨𐑜 r0 2 r0
  ret r0 1 at 1:3
end
main 𐑥𐑱𐑯
"
            )
        };
        let err = run(&assemble(&dag(60)).unwrap()).unwrap_err();
        assert_eq!((err.code, err.at), ("R009", Pos::new(1, 3)));
        // 2^17 leaves print in under a million characters; 2^18 don't.
        assert!(run(&assemble(&dag(17)).unwrap()).is_ok());
        assert_eq!(run(&assemble(&dag(18)).unwrap()).unwrap_err().code, "R009");
    }
}
