//! The bytecode virtual machine and its textual assembler/disassembler.
//!
//! Contract: `bytecode.rs`, written by the lead at Gate P1 and read-only to
//! agents. Built in Wave A (agent A4). See `docs/PLAN.md`.

mod asm;
pub mod bytecode;
mod value;
mod verify;
mod vm;

pub use asm::{assemble, disassemble};
pub use value::equal;
pub use vm::run;

/// The code for malformed bytecode or `.shawasm`, which SPEC §14 leaves
/// unassigned: `A` for assembler.
pub const MALFORMED: shawline_grid::Code = "A001";
