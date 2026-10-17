//! Diagram and words to AST: resolves boxes into definitions, nodes,
//! switches and ports, and wires into edges.
//!
//! Contract: `ast.rs`, written by the lead at Gate P1 and read-only to agents.
//! Built in Wave B (agent B1). See `docs/PLAN.md`.

pub mod ast;
mod dump;
mod parse;

pub use parse::parse;

#[cfg(test)]
mod tests;
