//! Values on wires: a traced run mapped back onto the drawing, for Studio's
//! live value badges (PLAN P7).
//!
//! Lead-owned contract: the types and signatures here are frozen; agent T
//! implements behind them.

use shawline_grid::Diagnostic;
use shawline_syntax::ast::Sheet;
use shawline_vm::bytecode::Value;

use crate::ir::Program;

/// The value each wire carried, as far as the run got.
#[derive(Clone, Debug, Default)]
pub struct SheetTrace {
    /// `(wire, value)` pairs: `wire` indexes `Diagram::wires`, as
    /// `ast::Edge::wire` does. Each wire carries the value its source
    /// produced during the first call of its definition. A definition's
    /// parameter wires carry the arguments that first call received.
    /// Sorted by wire, at most one entry per wire. A wire is missing only
    /// when its source never ran: an untaken switch arm, a definition never
    /// called, or a run that stopped first.
    pub wires: Vec<(usize, Value)>,
    /// The runtime error that ended the run, as `run` would report it.
    pub error: Option<Diagnostic>,
    /// The run used up its step budget before finishing.
    pub stopped: bool,
}

/// Compiles `program`, runs it traced for at most `steps` instructions, and
/// maps the recorded values onto `sheet`'s wires. `program` must be what
/// [`crate::lower`] made from `sheet`.
pub fn trace(sheet: &Sheet, program: &Program, steps: u64) -> SheetTrace {
    crate::wires::trace(sheet, program, steps)
}
