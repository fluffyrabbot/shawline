//! Bytecode: the contract between `shawline-ir`, which emits it, and the VM.
//!
//! Lead-owned (see `docs/PLAN.md`). Agent A4 implements the VM and the
//! `.shawasm` text format elsewhere in this crate; agent B5 emits bytecode.
//!
//! # Machine
//!
//! A register machine. Each call gets a frame of `Function::registers`
//! registers, arguments in `0..params`. Frames live on a heap-allocated stack,
//! never the host stack, so recursion depth is bounded by memory (SPEC §10).
//!
//! # `.shawasm`
//!
//! The text form, one item per line; `;` starts a comment.
//!
//! ```text
//! fn 𐑛𐑳𐑚𐑩𐑤 params 1 results 1 registers 2
//!   const k0 21                ; consts: an integer, «text», or nil
//!   prim 𐑐 r1 r0 r0            ; prim <letter> <dst>[,<dst>] <src>[,<src>]
//!   ret r1 1                   ; ret <base> <count>
//! end
//! main 𐑥𐑱𐑯
//! ```
//!
//! Instructions: `const rD kI`, `move rD rS`, `prim L rD[,rD] rS[,rS]`,
//! `call NAME rBASE ARGC rDST`, `tail NAME rBASE ARGC`, `ret rBASE COUNT`,
//! `jump N`, `jne rS kI N`, `nomatch`. `N` is an instruction index within the
//! function (consts don't count). Any instruction may end with `at ROW:COL` to
//! set its span; the default is `0:0`. Functions are referred to by name and
//! numbered in order of appearance.

use std::fmt;
use std::rc::Rc;

use shawline_grid::Pos;

/// A register index within a frame.
pub type Reg = u16;
/// An index into [`Module::functions`].
pub type FuncId = u32;
/// An index into [`Function::consts`].
pub type ConstId = u32;
/// An index into [`Function::code`].
pub type Label = u32;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Module {
    pub functions: Vec<Function>,
    pub main: FuncId,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Function {
    /// The definition's name without its namer dot.
    pub name: String,
    /// Arguments arrive in registers `0..params`.
    pub params: u16,
    pub results: u16,
    /// Frame size. Every register an instruction names is below it.
    pub registers: u16,
    pub consts: Vec<Value>,
    pub code: Vec<Instr>,
    /// The source position of each instruction, parallel to `code`.
    pub spans: Vec<Pos>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Instr {
    /// `dst = consts[idx]`
    Const { dst: Reg, idx: ConstId },
    Move { dst: Reg, src: Reg },
    /// Runs a primitive on the first `arity().0` of `src`, writing the first
    /// `arity().1` of `dst`. Unused slots are ignored.
    Prim { prim: Prim, dst: [Reg; 2], src: [Reg; 2] },
    /// Calls `func` with arguments `base..base + argc`. Its results land in
    /// `dst..dst + results`.
    Call { func: FuncId, base: Reg, argc: u16, dst: Reg },
    /// A call that replaces the current frame. The callee's results become this
    /// function's results. Runs in constant stack.
    TailCall { func: FuncId, base: Reg, argc: u16 },
    /// Returns registers `base..base + count`.
    Return { base: Reg, count: u16 },
    Jump { target: Label },
    /// Jumps to `target` unless `src` equals `consts[idx]` structurally.
    /// Switch guards lower to this.
    JumpIfNe { src: Reg, idx: ConstId, target: Label },
    /// No switch arm matched: runtime error `R005`.
    NoMatch,
}

/// The primitives of SPEC §9. Each one's mirror is named by the mirrored letter.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Prim {
    Add,
    Sub,
    Mul,
    Div,
    Pair,
    Unpair,
    Prepend,
    Split,
    Less,
    Greater,
    Equal,
    NotEqual,
    Min,
    Max,
    Show,
    Read,
}

impl Prim {
    pub const ALL: [Prim; 16] = [
        Prim::Add,
        Prim::Sub,
        Prim::Mul,
        Prim::Div,
        Prim::Pair,
        Prim::Unpair,
        Prim::Prepend,
        Prim::Split,
        Prim::Less,
        Prim::Greater,
        Prim::Equal,
        Prim::NotEqual,
        Prim::Min,
        Prim::Max,
        Prim::Show,
        Prim::Read,
    ];

    /// The single Shavian letter that names this primitive.
    pub const fn letter(self) -> char {
        match self {
            Prim::Add => '𐑐',
            Prim::Sub => '𐑚',
            Prim::Mul => '𐑑',
            Prim::Div => '𐑛',
            Prim::Pair => '𐑒',
            Prim::Unpair => '𐑜',
            Prim::Prepend => '𐑟',
            Prim::Split => '𐑕',
            Prim::Less => '𐑤',
            Prim::Greater => '𐑮',
            Prim::Equal => '𐑦',
            Prim::NotEqual => '𐑰',
            Prim::Min => '𐑥',
            Prim::Max => '𐑯',
            Prim::Show => '𐑖',
            Prim::Read => '𐑠',
        }
    }

    pub fn from_letter(ch: char) -> Option<Prim> {
        Prim::ALL.into_iter().find(|p| p.letter() == ch)
    }

    /// `(inputs, outputs)`.
    pub const fn arity(self) -> (u8, u8) {
        match self {
            Prim::Unpair | Prim::Split => (1, 2),
            Prim::Show | Prim::Read => (1, 1),
            _ => (2, 1),
        }
    }

    /// The dual primitive, named by the mirrored letter.
    pub fn mirror(self) -> Prim {
        let ch = self.letter() as u32;
        let partner = if (ch - 0x10450) % 20 < 10 { ch + 10 } else { ch - 10 };
        Prim::from_letter(char::from_u32(partner).expect("Shavian letter"))
            .expect("every primitive's mirror is a primitive")
    }
}

/// A runtime value. Truth is `Int(1)` / `Int(0)`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    Int(i64),
    Text(Rc<str>),
    Pair(Rc<(Value, Value)>),
    /// `None` is the empty list; otherwise head and tail, where the tail is
    /// itself a `List`.
    List(Option<Rc<(Value, Value)>>),
}

/// The output format of SPEC §9: integers in decimal, text bare at top level
/// and `«quoted»` inside a pair or list, pairs as `(a b)`, lists as `[a b c]`.
impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Text(t) => f.write_str(t),
            _ => write_nested(f, self),
        }
    }
}

fn write_nested(f: &mut fmt::Formatter<'_>, v: &Value) -> fmt::Result {
    match v {
        Value::Int(n) => write!(f, "{n}"),
        Value::Text(t) => write!(f, "«{t}»"),
        Value::Pair(p) => {
            f.write_str("(")?;
            write_nested(f, &p.0)?;
            f.write_str(" ")?;
            write_nested(f, &p.1)?;
            f.write_str(")")
        }
        Value::List(head) => {
            f.write_str("[")?;
            let mut cell = head.clone();
            let mut first = true;
            while let Some(c) = cell {
                if !first {
                    f.write_str(" ")?;
                }
                first = false;
                write_nested(f, &c.0)?;
                cell = match &c.1 {
                    Value::List(next) => next.clone(),
                    _ => None,
                };
            }
            f.write_str("]")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mirrors_are_duals() {
        for p in Prim::ALL {
            assert_eq!(p.mirror().mirror(), p);
            assert_ne!(p.mirror(), p);
        }
        assert_eq!(Prim::Add.mirror(), Prim::Sub);
        assert_eq!(Prim::Split.mirror(), Prim::Prepend);
    }

    #[test]
    fn display() {
        let text = |s: &str| Value::Text(s.into());
        let cons = |h, t| Value::List(Some(Rc::new((h, t))));
        let list = cons(Value::Int(1), cons(text("𐑣𐑦"), Value::List(None)));
        let pair = Value::Pair(Rc::new((Value::Int(-3), list)));
        assert_eq!(pair.to_string(), "(-3 [1 «𐑣𐑦»])");
        assert_eq!(text("𐑣𐑦").to_string(), "𐑣𐑦");
    }
}
