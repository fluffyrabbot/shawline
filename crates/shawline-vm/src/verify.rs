//! Static checks that make every register, constant, label and call in a
//! module safe for the dispatch loop to index without bounds failures.

use crate::bytecode::{Function, Instr, Module};

/// Why a module is malformed, and where: a function index and, when the fault
/// is in one instruction, its index.
pub(crate) struct Invalid {
    pub func: Option<usize>,
    pub instr: Option<usize>,
    pub message: String,
}

pub(crate) fn verify(m: &Module) -> Result<(), Invalid> {
    let fail = |func, instr, message: String| Invalid { func, instr, message };
    let Some(main) = m.functions.get(m.main as usize) else {
        return Err(fail(None, None, format!("main is function {}, which doesn't exist", m.main)));
    };
    if main.params != 0 {
        let at = Some(m.main as usize);
        return Err(fail(
            at,
            None,
            format!("main takes {} arguments; it must take none", main.params),
        ));
    }
    for (i, f) in m.functions.iter().enumerate() {
        check_function(m, f).map_err(|(instr, message)| fail(Some(i), instr, message))?;
    }
    Ok(())
}

fn check_function(m: &Module, f: &Function) -> Result<(), (Option<usize>, String)> {
    if f.spans.len() != f.code.len() {
        let msg = format!("{} spans for {} instructions", f.spans.len(), f.code.len());
        return Err((None, msg));
    }
    if f.params > f.registers || f.results > f.registers {
        return Err((
            None,
            format!("{} registers can't hold its arguments or results", f.registers),
        ));
    }
    // Every instruction but the terminators falls through, so a terminator
    // last means execution never runs off the end.
    match f.code.last() {
        Some(
            Instr::Return { .. } | Instr::TailCall { .. } | Instr::Jump { .. } | Instr::NoMatch,
        ) => {}
        _ => return Err((None, "the last instruction must be ret, tail, jump or nomatch".into())),
    }
    for (pc, instr) in f.code.iter().enumerate() {
        check_instr(m, f, instr).map_err(|msg| (Some(pc), msg))?;
    }
    Ok(())
}

fn check_instr(m: &Module, f: &Function, instr: &Instr) -> Result<(), String> {
    let regs = |base: u16, count: u16| {
        if u32::from(base) + u32::from(count) <= u32::from(f.registers) {
            Ok(())
        } else if count == 1 {
            Err(format!("register r{base} is outside the frame of {}", f.registers))
        } else {
            Err(format!("registers r{base}..+{count} run outside the frame of {}", f.registers))
        }
    };
    let constant = |idx: u32| match f.consts.get(idx as usize) {
        Some(_) => Ok(()),
        None => Err(format!("constant k{idx} doesn't exist")),
    };
    let label = |target: u32| match (target as usize) < f.code.len() {
        true => Ok(()),
        false => Err(format!("jump target {target} is past the end")),
    };
    let callee = |func: u32, argc: u16| match m.functions.get(func as usize) {
        None => Err(format!("function {func} doesn't exist")),
        Some(g) if g.params != argc => {
            Err(format!("{} takes {} arguments, not {argc}", g.name, g.params))
        }
        Some(g) => Ok(g),
    };
    match *instr {
        Instr::Const { dst, idx } => {
            regs(dst, 1)?;
            constant(idx)
        }
        Instr::Move { dst, src } => {
            regs(dst, 1)?;
            regs(src, 1)
        }
        Instr::Prim { prim, dst, src } => {
            let (ins, outs) = prim.arity();
            src[..ins as usize].iter().try_for_each(|&r| regs(r, 1))?;
            dst[..outs as usize].iter().try_for_each(|&r| regs(r, 1))
        }
        Instr::Call { func, base, argc, dst } => {
            let g = callee(func, argc)?;
            regs(base, argc)?;
            regs(dst, g.results)
        }
        Instr::TailCall { func, base, argc } => {
            let g = callee(func, argc)?;
            regs(base, argc)?;
            if g.results != f.results {
                return Err(format!(
                    "tail call to {}, which returns {}, from a function returning {}",
                    g.name, g.results, f.results
                ));
            }
            Ok(())
        }
        Instr::Return { base, count } => {
            regs(base, count)?;
            if count != f.results {
                return Err(format!("returns {count} values; {} declares {}", f.name, f.results));
            }
            Ok(())
        }
        Instr::Jump { target } => label(target),
        Instr::JumpIfNe { src, idx, target } => {
            regs(src, 1)?;
            constant(idx)?;
            label(target)
        }
        Instr::NoMatch => Ok(()),
    }
}
