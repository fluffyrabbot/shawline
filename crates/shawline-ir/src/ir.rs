//! The dataflow IR: the contract between lowering from the AST and code
//! generation to bytecode.
//!
//! Lead-owned (see `docs/PLAN.md`). Agent B5 builds both sides in this crate.
//! By the time a [`Program`] exists, names are resolved, arities match, and
//! every block is acyclic and in evaluation order.

use shawline_grid::Pos;
use shawline_syntax::ast::Literal;
use shawline_vm::bytecode::Prim;

/// An index into [`Program::functions`].
pub type FuncId = u32;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Program {
    /// In the order of the sheet's definitions.
    pub functions: Vec<Function>,
    pub main: FuncId,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Function {
    /// The definition's name without its namer dot.
    pub name: String,
    /// `body.params` are the function's parameters; `body.results` its results.
    pub body: Block,
    /// The definition's namer dot.
    pub at: Pos,
}

/// A scope in SSA form: every operand is a parameter or an output of an
/// earlier instruction in the same block.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Block {
    pub params: u32,
    pub insts: Vec<Inst>,
    pub results: Vec<Value>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Value {
    /// A parameter of the enclosing block.
    Param(u32),
    /// Output `output` of instruction `inst` in the enclosing block.
    Out { inst: u32, output: u32 },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Inst {
    pub op: Op,
    /// Where runtime errors from this instruction point: the node's word, or
    /// the switch's selector.
    pub at: Pos,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Op {
    /// One output.
    Const(Literal),
    /// `Prim::arity().1` outputs.
    Prim { prim: Prim, args: Vec<Value> },
    /// As many outputs as the callee has results.
    Call { func: FuncId, args: Vec<Value> },
    /// Evaluates the first arm whose guard equals `selector`, else the default
    /// arm, else fails with `R005`. The chosen arm's block gets its `args` as
    /// parameters. Every arm has `outputs` results, which become this
    /// instruction's outputs.
    Switch { selector: Value, arms: Vec<Arm>, outputs: u32 },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Arm {
    /// `None` for the default arm, which is last.
    pub guard: Option<Literal>,
    pub args: Vec<Value>,
    pub body: Block,
}
