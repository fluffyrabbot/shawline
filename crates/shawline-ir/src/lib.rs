//! AST to dataflow IR, the checker, and lowering from IR to bytecode.
//!
//! Contract: `ir.rs`, written by the lead at Gate P1 and read-only to agents.
//! Built in Wave B (agent B5). See `docs/PLAN.md`.

mod compile;
pub mod ir;
mod lower;

pub use compile::compile;
pub use lower::lower;
