//! What a traced run remembers: the writes of each function's first
//! activation, and its step budget.
//!
//! Recorded values keep their cells alive, so a traced run can count more
//! live cells than [`crate::run`] would and reach `R008` sooner. Only a value
//! a first activation saw can be kept, so it takes a sheet that builds and
//! drops millions of cells early to notice.

use crate::bytecode::{FuncId, Module, Value};
use crate::trace::{Traced, TracedOutput};

/// Recording state for [`crate::vm::dispatch`]. An untraced run holds the
/// default, which is never touched.
#[derive(Default)]
pub(crate) struct Recorder {
    /// Instructions left to execute.
    steps: u64,
    /// Which functions have started an activation.
    seen: Vec<bool>,
    /// One per live frame: whether its current activation is its function's
    /// first. A tail call starts a new activation in the same frame.
    first: Vec<bool>,
    /// Tail calls made by first activations, as `(depth, func, instr)`. Their
    /// outputs are the results their frame eventually returns. Each first
    /// activation makes at most one, so this stays as short as the module.
    pending: Vec<(usize, FuncId, u32)>,
    pub traced: Traced,
}

impl Recorder {
    pub(crate) fn new(steps: u64) -> Recorder {
        Recorder { steps, ..Recorder::default() }
    }

    /// `main` starts its first activation with no arguments.
    pub(crate) fn start(&mut self, module: &Module) {
        self.seen = vec![false; module.functions.len()];
        self.call(module.main, &[]);
    }

    /// Spends a step, or reports that none are left.
    #[inline]
    pub(crate) fn tick(&mut self) -> bool {
        if self.steps == 0 {
            return false;
        }
        self.steps -= 1;
        true
    }

    /// Instruction `instr` of `func`, running in the top frame, wrote `value`.
    #[inline]
    pub(crate) fn write(&mut self, func: FuncId, instr: usize, output: u16, value: &Value) {
        if self.top() {
            self.record(func, instr, output, value);
        }
    }

    /// A new frame starts an activation of `func`.
    pub(crate) fn call(&mut self, func: FuncId, args: &[Value]) {
        let first = self.enter(func, args);
        self.first.push(first);
    }

    /// The frame at `depth`, running instruction `instr` of `from`, becomes
    /// an activation of `func`.
    pub(crate) fn tail(&mut self, depth: usize, from: FuncId, instr: usize, func: FuncId, args: &[Value]) {
        if self.top() {
            self.pending.push((depth, from, instr as u32));
        }
        let first = self.enter(func, args);
        *self.first.last_mut().expect("a running frame") = first;
    }

    /// The frame at `depth` returned `results`, into the caller's `Call` at
    /// `caller` if there is one.
    pub(crate) fn ret(&mut self, depth: usize, results: &[Value], caller: Option<(FuncId, usize)>) {
        while let Some(&(d, func, instr)) = self.pending.last()
            && d == depth
        {
            self.pending.pop();
            for (k, v) in results.iter().enumerate() {
                self.record(func, instr as usize, k as u16, v);
            }
        }
        self.first.pop();
        if let Some((func, instr)) = caller
            && self.top()
        {
            for (k, v) in results.iter().enumerate() {
                self.record(func, instr, k as u16, v);
            }
        }
    }

    fn top(&self) -> bool {
        self.first.last().copied().unwrap_or(false)
    }

    /// Marks `func` seen, keeping its arguments if this is its first
    /// activation.
    fn enter(&mut self, func: FuncId, args: &[Value]) -> bool {
        let seen = &mut self.seen[func as usize];
        let first = !*seen;
        *seen = true;
        if first {
            self.traced.calls.push((func, args.to_vec()));
        }
        first
    }

    fn record(&mut self, func: FuncId, instr: usize, output: u16, value: &Value) {
        self.traced.outputs.push(TracedOutput { func, instr: instr as u32, output, value: value.clone() });
    }
}

#[cfg(test)]
mod tests {
    use crate::bytecode::Value;
    use crate::{Traced, assemble, run, run_traced};

    /// `down(n)` recurses n times below main, then returns 0.
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

    /// `count(n, acc)` tail-calls itself n times.
    const COUNT: &str = "
fn 𐑒𐑬𐑯𐑑 params 2 results 1 registers 3
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
  const k0 N
  const k1 0
  const r0 k0
  const r1 k1
  call 𐑒𐑬𐑯𐑑 r0 2 r2
  ret r2 1
end
main 𐑥𐑱𐑯
";

    fn traced(src: &str, n: u64, steps: u64) -> Traced {
        run_traced(&assemble(&src.replace(" N", &format!(" {n}"))).unwrap(), steps)
    }

    fn written(t: &Traced, func: u32, instr: u32) -> Vec<String> {
        let at = t.outputs.iter().filter(|o| (o.func, o.instr) == (func, instr));
        at.map(|o| format!("{}={}", o.output, o.value)).collect()
    }

    #[test]
    fn only_the_first_activation_is_recorded() {
        let t = traced(DOWN, 50, u64::MAX);
        assert_eq!(t.results, Some(vec![Value::Int(0)]));
        let calls: Vec<String> = t.calls.iter().map(|(f, a)| format!("{f}{a:?}")).collect();
        assert_eq!(calls, ["1[]", "0[Int(50)]"]);
        // The first activation's subtraction and the call it made, once each.
        assert_eq!(written(&t, 0, 3), ["0=49"]);
        assert_eq!(written(&t, 0, 4), ["0=0"]);
        assert_eq!(written(&t, 1, 1), ["0=0"]);
        // As few outputs as for a single level.
        assert_eq!(traced(DOWN, 50_000, u64::MAX).outputs.len(), t.outputs.len());
    }

    #[test]
    fn a_tail_call_outputs_its_frames_results() {
        let t = traced(COUNT, 1000, u64::MAX);
        assert_eq!(written(&t, 0, 5), ["0=1000"]);
        assert_eq!(written(&t, 1, 2), ["0=1000"]);
    }

    #[test]
    fn the_step_budget_stops_cleanly() {
        let t = traced(COUNT, 1_000_000, 1000);
        assert!(t.stopped && t.error.is_none() && t.results.is_none());
        assert_eq!(t.calls.len(), 2);
        assert_eq!(written(&t, 0, 3), ["0=999999"]);
        // Main's 3 up to the call, 5 per pass, 2 for the last and main's return.
        let exact = 3 + 5 * 10 + 2 + 1;
        assert!(traced(COUNT, 10, exact).results.is_some());
        assert!(traced(COUNT, 10, exact - 1).stopped);
    }

    #[test]
    fn errors_match_run() {
        let src = DOWN.replace(" N", " 1000000");
        let module = assemble(&src).unwrap();
        let (want, got) = (run(&module).unwrap_err(), run_traced(&module, u64::MAX).error.unwrap());
        assert_eq!((got.code, got.at), (want.code, want.at));
        assert_eq!(got.code, "R007");
    }
}
