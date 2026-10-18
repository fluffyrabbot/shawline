//! Tracing: the values a run computes, for Studio's live value badges
//! (PLAN P7).
//!
//! Lead-owned contract: the types and signatures here are frozen; agent T
//! implements behind them, adding private modules as needed.

use shawline_grid::Diagnostic;

use crate::bytecode::{FuncId, Module, Value};
use crate::record::Recorder;
use crate::vm::{Halt, Limits, dispatch};

/// One value an instruction wrote during the first activation of its
/// function.
#[derive(Clone, Debug)]
pub struct TracedOutput {
    pub func: FuncId,
    /// An index into that function's `code`.
    pub instr: u32,
    /// Which of the instruction's outputs: 0 for most, 0 or 1 for the
    /// two-output primitives, the result ordinal for a call.
    pub output: u16,
    pub value: Value,
}

/// A traced run. Compare its values with [`crate::equal`], never `==`.
#[derive(Clone, Debug, Default)]
pub struct Traced {
    /// Every output written during the first activation of each function,
    /// in execution order. Later activations aren't recorded, so a deep
    /// recursion costs no more to trace than a shallow one.
    pub outputs: Vec<TracedOutput>,
    /// The arguments each function's first activation received, in the order
    /// the functions were first called.
    pub calls: Vec<(FuncId, Vec<Value>)>,
    /// `main`'s results, when the run finished.
    pub results: Option<Vec<Value>>,
    /// The runtime error that ended the run, with the code and position
    /// [`crate::run`] would report.
    pub error: Option<Diagnostic>,
    /// The run used up its `steps` before finishing. Not an error: a live
    /// editor shows what was computed so far.
    pub stopped: bool,
}

/// Runs `main` exactly as [`crate::run`] does, with the same limits and
/// errors, recording as it goes, and executes at most `steps` instructions.
pub fn run_traced(module: &Module, steps: u64) -> Traced {
    let mut rec = Recorder::new(steps);
    match dispatch::<true>(module, Limits::SPEC, &mut rec) {
        Ok((results, _)) => rec.traced.results = Some(results),
        Err(Halt::Error(e)) => rec.traced.error = Some(e),
        Err(Halt::Stopped) => rec.traced.stopped = true,
    }
    rec.traced
}

/// `value` printed as SPEC §9 prints it, cut to at most `max_chars`
/// characters, the last of which is `…` when it was cut. Never builds the
/// whole printed form, so a vast shared value is as cheap as a small one.
pub fn show(value: &Value, max_chars: usize) -> String {
    crate::show::show(value, max_chars)
}
