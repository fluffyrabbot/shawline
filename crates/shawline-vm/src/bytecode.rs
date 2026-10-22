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
//!   const k0 21                ; an integer, «text», nil, (a b) or [a b c]
//!   prim 𐑐 r1 r0,r0            ; prim <letter> <dst>[,<dst>] <src>[,<src>]
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
    /// `arity().1` of `dst`. Unused slots are 0, so `.shawasm` round-trips.
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

    /// The short English name the gloss strip and the palette show (SPEC §9):
    /// at most 6 lowercase letters, and each pair of duals named as a pair.
    pub const fn gloss(self) -> &'static str {
        match self {
            Prim::Add => "add",
            Prim::Sub => "sub",
            Prim::Mul => "mul",
            Prim::Div => "div",
            Prim::Pair => "pair",
            Prim::Unpair => "unpair",
            Prim::Prepend => "cons",
            Prim::Split => "uncons",
            Prim::Less => "less",
            Prim::Greater => "more",
            Prim::Equal => "same",
            Prim::NotEqual => "differ",
            Prim::Min => "min",
            Prim::Max => "max",
            Prim::Show => "show",
            Prim::Read => "read",
        }
    }

    /// SPEC §9's description, for hover.
    pub const fn meaning(self) -> &'static str {
        match self {
            Prim::Add => "add",
            Prim::Sub => "subtract",
            Prim::Mul => "multiply",
            Prim::Div => "divide (truncating; ÷0 is an error)",
            Prim::Pair => "pair",
            Prim::Unpair => "unpair",
            Prim::Prepend => "prepend (head, tail → list)",
            Prim::Split => "split (list → head, tail; empty is an error)",
            Prim::Less => "less than",
            Prim::Greater => "greater than",
            Prim::Equal => "equal",
            Prim::NotEqual => "not equal",
            Prim::Min => "minimum",
            Prim::Max => "maximum",
            Prim::Show => "show (Int → Text)",
            Prim::Read => "read (Text → Int)",
        }
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
///
/// The derived `==` recurses, so it can overflow the stack on long lists.
/// Compare runtime values with [`crate::equal`] instead.
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
///
/// Values nest as deep as memory allows, so this walks an explicit worklist
/// rather than recursing on the host stack.
impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        enum Task<'a> {
            Value(&'a Value),
            Str(&'static str),
            /// The rest of a list after an item: more items, or the `]`.
            Rest(&'a Value),
        }
        if let Value::Text(t) = self {
            return f.write_str(t);
        }
        let mut tasks = vec![Task::Value(self)];
        while let Some(task) = tasks.pop() {
            match task {
                Task::Str(s) => f.write_str(s)?,
                Task::Value(Value::Int(n)) => write!(f, "{n}")?,
                Task::Value(Value::Text(t)) => write!(f, "«{t}»")?,
                Task::Value(Value::Pair(p)) => {
                    f.write_str("(")?;
                    tasks.extend([Task::Str(")"), Task::Value(&p.1), Task::Str(" ")]);
                    tasks.push(Task::Value(&p.0));
                }
                Task::Value(Value::List(None)) => f.write_str("[]")?,
                Task::Value(Value::List(Some(c))) => {
                    f.write_str("[")?;
                    tasks.extend([Task::Rest(&c.1), Task::Value(&c.0)]);
                }
                Task::Rest(Value::List(Some(c))) => {
                    f.write_str(" ")?;
                    tasks.extend([Task::Rest(&c.1), Task::Value(&c.0)]);
                }
                Task::Rest(_) => f.write_str("]")?,
            }
        }
        Ok(())
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
    fn short_names_fit_a_node_box() {
        let mut seen = std::collections::HashSet::new();
        for p in Prim::ALL {
            let g = p.gloss();
            assert!(g.len() <= 6 && g.chars().all(|c| c.is_ascii_lowercase()), "{g:?}");
            assert!(seen.insert(g), "{g:?} names two primitives");
        }
    }

    /// SPEC §9's primitive table, row by row: letter, short name, meaning,
    /// ports, then the same for its mirror.
    #[test]
    fn spec_table_matches() {
        let spec = include_str!("../../../docs/SPEC.md");
        let rows = spec
            .lines()
            .skip_while(|l| !l.starts_with("| Letter | Short name |"))
            .skip(2)
            .take_while(|l| l.starts_with('|'));
        let mut documented = Vec::new();
        for line in rows {
            let cells: Vec<&str> = line.trim_matches('|').split('|').map(str::trim).collect();
            for half in cells.chunks(4) {
                let [letter, short, meaning, ports] = half else { panic!("{line}") };
                documented.push((letter.chars().next().unwrap(), *short, *meaning, ports.to_string()));
            }
        }
        let mut table: Vec<_> = Prim::ALL
            .into_iter()
            .map(|p| {
                let (i, o) = p.arity();
                (p.letter(), p.gloss(), p.meaning(), format!("{i}→{o}"))
            })
            .collect();
        table.sort();
        documented.sort();
        assert_eq!(documented, table);
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
