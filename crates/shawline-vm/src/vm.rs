//! The dispatch loop and the primitives of SPEC §9.

use std::mem;
use std::rc::Rc;

use shawline_grid::{Code, Diagnostic, Pos};

use crate::MALFORMED;
use crate::bytecode::{FuncId, Instr, Module, Prim, Reg, Value};
use crate::value::equal;
use crate::verify::verify;

/// The deepest calls may nest (SPEC §10). Beyond it a program stops with
/// `R007` instead of growing until the machine runs out of memory.
pub const MAX_FRAMES: usize = 1_000_000;

/// The most values the live frames may hold together: 256 MB of registers.
/// It bounds memory for definitions with many values, which reach it in
/// fewer than [`MAX_FRAMES`] calls.
pub const MAX_REGISTERS: usize = 1 << 24;

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
/// Runtime errors are `R001`–`R007`, positioned at the failing instruction's
/// span.
pub fn run(module: &Module) -> Result<Vec<Value>, Diagnostic> {
    execute(module).map(|(results, _)| results)
}

/// [`run`], also reporting the deepest the frame stack grew.
pub(crate) fn execute(module: &Module) -> Result<(Vec<Value>, usize), Diagnostic> {
    verify(module).map_err(|e| {
        let f = e.func.map(|i| &module.functions[i]);
        let at = f.zip(e.instr).and_then(|(f, pc)| f.spans.get(pc).copied());
        Diagnostic::new(at.unwrap_or(Pos::new(0, 0)), MALFORMED, e.message)
    })?;

    let mut f = &module.functions[module.main as usize];
    let mut regs = vec![Value::Int(0); f.registers as usize];
    let mut frames = vec![Frame { func: module.main, pc: 0, base: 0, dst: 0 }];
    let mut peak = 1;
    // Holds arguments and results while frames are torn down.
    let mut scratch: Vec<Value> = Vec::new();
    let mut base = 0;
    let mut pc = 0;

    // Verification guarantees every index below is in bounds.
    loop {
        let r = |reg: Reg| base + reg as usize;
        match f.code[pc] {
            Instr::Const { dst, idx } => {
                regs[r(dst)] = f.consts[idx as usize].clone();
                pc += 1;
            }
            Instr::Move { dst, src } => {
                regs[r(dst)] = regs[r(src)].clone();
                pc += 1;
            }
            Instr::Prim { prim, dst, src } => {
                let (a, b) = (&regs[r(src[0])], &regs[r(src[1])]);
                let (x, y) = apply(prim, a, b)
                    .map_err(|(code, msg)| Diagnostic::new(f.spans[pc], code, msg))?;
                regs[r(dst[0])] = x;
                if prim.arity().1 == 2 {
                    regs[r(dst[1])] = y;
                }
                pc += 1;
            }
            Instr::Call { func, base: args, argc, dst } => {
                let needed = module.functions[func as usize].registers as usize;
                if frames.len() >= MAX_FRAMES || regs.len() + needed > MAX_REGISTERS {
                    let message = format!("call stack exhausted {} calls deep", frames.len());
                    return Err(Diagnostic::new(f.spans[pc], "R007", message));
                }
                frames.last_mut().expect("a running frame").pc = pc + 1;
                f = &module.functions[func as usize];
                let callee = regs.len();
                regs.extend_from_within(r(args)..r(args) + argc as usize);
                regs.resize(callee + f.registers as usize, Value::Int(0));
                frames.push(Frame { func, pc: 0, base: callee, dst });
                peak = peak.max(frames.len());
                base = callee;
                pc = 0;
            }
            Instr::TailCall { func, base: args, argc } => {
                scratch.extend(regs.drain(r(args)..r(args) + argc as usize));
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
                    return Ok((mem::take(&mut scratch), peak));
                };
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
                return Err(Diagnostic::new(f.spans[pc], "R005", "no arm matches"));
            }
        }
    }
}

type Fault = (Code, String);

/// Runs one primitive. Primitives with one output leave the second unused.
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
        Prim::Pair => one(Value::Pair(Rc::new((a.clone(), b.clone())))),
        Prim::Unpair => match a {
            Value::Pair(cell) => Ok((cell.0.clone(), cell.1.clone())),
            other => Err(mismatch(p, "a pair", other)),
        },
        Prim::Prepend => match b {
            Value::List(_) => one(Value::List(Some(Rc::new((a.clone(), b.clone()))))),
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
        let (results, peak) = execute(&assemble(src).unwrap()).unwrap();
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
}
