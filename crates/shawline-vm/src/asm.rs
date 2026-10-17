//! The `.shawasm` text form of a [`Module`]; the grammar is in `bytecode.rs`.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::rc::Rc;
use std::str::FromStr;

use shawline_grid::{Diagnostic, Pos};

use crate::MALFORMED;
use crate::bytecode::{ConstId, FuncId, Function, Instr, Module, Prim, Reg, Value};
use crate::verify::verify;

/// Parses `.shawasm` text into a verified module.
///
/// Errors are `A001`, at the zero-based row and column (in chars) of the
/// offending token. Beyond the grammar, constants may also be pairs `(a b)`
/// and lists `[a b c]` of constants, so every module round-trips.
pub fn assemble(src: &str) -> Result<Module, Diagnostic> {
    let mut asm = Assembler::default();
    for (row, text) in src.lines().enumerate() {
        let mut line = Line::new(text, row)?;
        if !line.is_empty() {
            asm.line(&mut line)?;
        }
    }
    asm.finish(src.lines().count())
}

/// How deep constants may nest. Deeper ones are rejected as malformed (`A001`).
const MAX_NESTING: usize = 10_000;

fn error(at: Pos, message: impl Into<String>) -> Diagnostic {
    Diagnostic::new(at, MALFORMED, message)
}

#[derive(Default)]
struct Assembler {
    functions: Vec<Function>,
    /// Where each function's header is, and each of its instructions.
    headers: Vec<Pos>,
    instr_at: Vec<Vec<Pos>>,
    names: HashMap<String, FuncId>,
    /// Calls by name, resolved once every function is known.
    fixups: Vec<(usize, usize, String, Pos)>,
    main: Option<(String, Pos)>,
    open: bool,
}

impl Assembler {
    fn line(&mut self, line: &mut Line) -> Result<(), Diagnostic> {
        let op = line.next("an item")?;
        match (op.text, self.open) {
            ("fn", false) => self.header(line),
            ("end", true) => {
                self.open = false;
                line.finish()
            }
            ("main", false) => {
                let name = line.next("main's name")?;
                if self.main.is_some() {
                    return Err(error(op.at, "a second main"));
                }
                self.main = Some((name.text.to_owned(), name.at));
                line.finish()
            }
            ("fn" | "main", true) => {
                Err(error(op.at, format!("{} before the open fn's end", op.text)))
            }
            (_, false) => Err(error(op.at, format!("expected fn or main, found {}", op.text))),
            (_, true) => self.body(op, line),
        }
    }

    fn header(&mut self, line: &mut Line) -> Result<(), Diagnostic> {
        let name = line.next("a function name")?;
        let id = self.functions.len() as FuncId;
        if self.names.insert(name.text.to_owned(), id).is_some() {
            return Err(error(name.at, format!("a second function named {}", name.text)));
        }
        line.keyword("params")?;
        let params = line.num("a parameter count")?;
        line.keyword("results")?;
        let results = line.num("a result count")?;
        line.keyword("registers")?;
        let registers = line.num("a register count")?;
        line.finish()?;
        self.functions.push(Function {
            name: name.text.to_owned(),
            params,
            results,
            registers,
            consts: Vec::new(),
            code: Vec::new(),
            spans: Vec::new(),
        });
        self.headers.push(Pos::new(line.row, 0));
        self.instr_at.push(Vec::new());
        self.open = true;
        Ok(())
    }

    fn body(&mut self, op: Tok, line: &mut Line) -> Result<(), Diagnostic> {
        let fi = self.functions.len() - 1;
        let instr = match op.text {
            "const" if line.peek().is_some_and(|t| t.text.starts_with('k')) => {
                let k = line.next("a constant")?;
                let idx: ConstId = parse_prefixed(k, 'k', "a constant")?;
                let f = &mut self.functions[fi];
                if idx as usize != f.consts.len() {
                    return Err(error(k.at, format!("expected k{} next", f.consts.len())));
                }
                let value = line.value()?;
                line.finish()?;
                f.consts.push(value);
                return Ok(());
            }
            "const" => Instr::Const { dst: line.reg()?, idx: line.konst()? },
            "move" => Instr::Move { dst: line.reg()?, src: line.reg()? },
            "prim" => line.prim()?,
            "call" | "tail" => {
                let name = line.next("a function name")?;
                let fixup = (fi, self.functions[fi].code.len(), name.text.to_owned(), name.at);
                self.fixups.push(fixup);
                let (base, argc) = (line.reg()?, line.num("an argument count")?);
                match op.text {
                    "call" => Instr::Call { func: 0, base, argc, dst: line.reg()? },
                    _ => Instr::TailCall { func: 0, base, argc },
                }
            }
            "ret" => Instr::Return { base: line.reg()?, count: line.num("a result count")? },
            "jump" => Instr::Jump { target: line.num("an instruction index")? },
            "jne" => Instr::JumpIfNe {
                src: line.reg()?,
                idx: line.konst()?,
                target: line.num("an instruction index")?,
            },
            "nomatch" => Instr::NoMatch,
            other => return Err(error(op.at, format!("unknown instruction {other}"))),
        };
        let span = line.span()?;
        line.finish()?;
        let f = &mut self.functions[fi];
        f.code.push(instr);
        f.spans.push(span);
        self.instr_at[fi].push(op.at);
        Ok(())
    }

    fn finish(mut self, rows: usize) -> Result<Module, Diagnostic> {
        if self.open {
            let at = *self.headers.last().expect("an open function");
            return Err(error(at, "fn without end"));
        }
        for (fi, pc, name, at) in std::mem::take(&mut self.fixups) {
            let id = self.resolve(&name, at)?;
            if let Instr::Call { func, .. } | Instr::TailCall { func, .. } =
                &mut self.functions[fi].code[pc]
            {
                *func = id;
            }
        }
        let Some((name, at)) = self.main.take() else {
            return Err(error(Pos::new(rows, 0), "no main"));
        };
        let module = Module { main: self.resolve(&name, at)?, functions: self.functions };
        verify(&module).map_err(|e| {
            let at = match (e.func, e.instr) {
                (Some(f), Some(pc)) => self.instr_at[f][pc],
                (Some(f), None) => self.headers[f],
                (None, _) => at,
            };
            error(at, e.message)
        })?;
        Ok(module)
    }

    fn resolve(&self, name: &str, at: Pos) -> Result<FuncId, Diagnostic> {
        self.names.get(name).copied().ok_or_else(|| error(at, format!("no function named {name}")))
    }
}

#[derive(Clone, Copy)]
struct Tok<'a> {
    text: &'a str,
    at: Pos,
}

/// One line's tokens and a cursor over them.
struct Line<'a> {
    toks: Vec<Tok<'a>>,
    next: usize,
    row: usize,
    /// Where "expected …" errors point once the tokens run out.
    end: Pos,
}

impl<'a> Line<'a> {
    /// Splits a line into tokens: `«text»` (spaces and all), each bracket on
    /// its own, and runs of anything else. `;` outside text starts a comment.
    fn new(text: &'a str, row: usize) -> Result<Self, Diagnostic> {
        let mut toks = Vec::new();
        let mut chars = text.char_indices().enumerate().peekable();
        let mut end = Pos::new(row, 0);
        while let Some((col, (start, ch))) = chars.next() {
            let at = Pos::new(row, col);
            let stop = match ch {
                ';' => break,
                c if c.is_whitespace() => continue,
                '(' | ')' | '[' | ']' => start + ch.len_utf8(),
                '«' => loop {
                    match chars.next() {
                        Some((_, (i, '»'))) => break i + '»'.len_utf8(),
                        Some(_) => {}
                        None => return Err(error(at, "« without »")),
                    }
                },
                _ => loop {
                    match chars.peek() {
                        Some(&(_, (i, c))) if c.is_whitespace() || ";()[]«".contains(c) => {
                            break i;
                        }
                        Some(_) => {
                            chars.next();
                        }
                        None => break text.len(),
                    }
                },
            };
            toks.push(Tok { text: &text[start..stop], at });
            end = Pos::new(row, col + text[start..stop].chars().count());
        }
        Ok(Line { toks, next: 0, row, end })
    }

    fn is_empty(&self) -> bool {
        self.toks.is_empty()
    }

    fn peek(&self) -> Option<Tok<'a>> {
        self.toks.get(self.next).copied()
    }

    fn next(&mut self, what: &str) -> Result<Tok<'a>, Diagnostic> {
        let tok = self.peek().ok_or_else(|| error(self.end, format!("expected {what}")))?;
        self.next += 1;
        Ok(tok)
    }

    fn finish(&self) -> Result<(), Diagnostic> {
        match self.peek() {
            Some(t) => Err(error(t.at, format!("unexpected {}", t.text))),
            None => Ok(()),
        }
    }

    fn keyword(&mut self, kw: &str) -> Result<(), Diagnostic> {
        let tok = self.next(kw)?;
        match tok.text == kw {
            true => Ok(()),
            false => Err(error(tok.at, format!("expected {kw}, found {}", tok.text))),
        }
    }

    fn num<T: FromStr>(&mut self, what: &str) -> Result<T, Diagnostic> {
        let tok = self.next(what)?;
        digits(tok.text)
            .ok_or_else(|| error(tok.at, format!("expected {what}, found {}", tok.text)))
    }

    fn reg(&mut self) -> Result<Reg, Diagnostic> {
        parse_prefixed(self.next("a register")?, 'r', "a register")
    }

    fn konst(&mut self) -> Result<ConstId, Diagnostic> {
        parse_prefixed(self.next("a constant")?, 'k', "a constant")
    }

    /// `prim L rD[,rD] rS[,rS]`. The registers may also be listed with spaces
    /// alone, as in the example in `bytecode.rs`, since the letter fixes how
    /// many of each there are.
    fn prim(&mut self) -> Result<Instr, Diagnostic> {
        let tok = self.next("a primitive letter")?;
        let mut letter = tok.text.chars();
        let prim = match (letter.next().and_then(Prim::from_letter), letter.next()) {
            (Some(p), None) => p,
            _ => return Err(error(tok.at, format!("{} isn't a primitive", tok.text))),
        };
        let (ins, outs) = prim.arity();
        let (ins, outs) = (ins as usize, outs as usize);
        let mut groups = Vec::new();
        while let Some(t) = self.peek().filter(|t| t.text != "at") {
            self.next += 1;
            let regs = t.text.split(',').map(|r| {
                let r = Tok { text: r, at: t.at };
                parse_prefixed(r, 'r', "a register")
            });
            groups.push(regs.collect::<Result<Vec<_>, _>>()?);
        }
        let sizes: Vec<_> = groups.iter().map(Vec::len).collect();
        let flat = vec![1; ins + outs];
        if sizes != [outs, ins] && sizes != flat {
            let at = self.toks.get(self.next - groups.len()).map_or(self.end, |t| t.at);
            let shape = |n| (0..n).map(|_| "r").collect::<Vec<_>>().join(",");
            return Err(error(
                at,
                format!("{} wants {} {}", prim.letter(), shape(outs), shape(ins)),
            ));
        }
        let regs: Vec<Reg> = groups.concat();
        let mut dst = [0; 2];
        let mut src = [0; 2];
        dst[..outs].copy_from_slice(&regs[..outs]);
        src[..ins].copy_from_slice(&regs[outs..]);
        Ok(Instr::Prim { prim, dst, src })
    }

    /// An optional trailing `at ROW:COL`.
    fn span(&mut self) -> Result<Pos, Diagnostic> {
        if self.peek().is_none_or(|t| t.text != "at") {
            return Ok(Pos::new(0, 0));
        }
        self.next += 1;
        let tok = self.next("ROW:COL")?;
        let pos =
            tok.text.split_once(':').and_then(|(r, c)| Some(Pos::new(digits(r)?, digits(c)?)));
        pos.ok_or_else(|| error(tok.at, format!("expected ROW:COL, found {}", tok.text)))
    }

    /// A constant: an integer, `«text»`, `nil`, `(a b)` or `[a b c]`, nested
    /// at most [`MAX_NESTING`] deep. Open brackets wait on a stack, not the
    /// host's, however deep the input goes.
    fn value(&mut self) -> Result<Value, Diagnostic> {
        // Each open bracket and the items it has so far.
        let mut open: Vec<(bool, Vec<Value>)> = Vec::new();
        loop {
            let in_list = open.last().is_some_and(|(list, _)| *list);
            let tok = self.next("a constant value")?;
            let mut done = match tok.text {
                "(" | "[" => {
                    if open.len() == MAX_NESTING {
                        let msg = format!("constant nested deeper than {MAX_NESTING}");
                        return Err(error(tok.at, msg));
                    }
                    open.push((tok.text == "[", Vec::new()));
                    continue;
                }
                "]" if in_list => {
                    let (_, items) = open.pop().expect("an open list");
                    let cons = |tail, head| Value::List(Some(Rc::new((head, tail))));
                    items.into_iter().rev().fold(Value::List(None), cons)
                }
                "nil" => Value::List(None),
                t if t.starts_with('«') => {
                    Value::Text(t['«'.len_utf8()..t.len() - '»'.len_utf8()].into())
                }
                t => Value::Int(
                    int(t).ok_or_else(|| error(tok.at, format!("{t} isn't a constant")))?,
                ),
            };
            // Hand the finished value to its bracket, closing every pair it
            // completes.
            loop {
                match open.last_mut() {
                    None => return Ok(done),
                    Some((true, items)) => {
                        items.push(done);
                        break;
                    }
                    Some((false, items)) if items.is_empty() => {
                        items.push(done);
                        break;
                    }
                    Some((false, items)) => {
                        let first = items.pop().expect("a pair's first item");
                        open.pop();
                        self.keyword(")")?;
                        done = Value::Pair(Rc::new((first, done)));
                    }
                }
            }
        }
    }
}

/// An optional `-` and ASCII digits.
fn int(t: &str) -> Option<i64> {
    let d = t.strip_prefix('-').unwrap_or(t);
    (!d.is_empty() && d.bytes().all(|c| c.is_ascii_digit())).then(|| t.parse().ok()).flatten()
}

/// Parses ASCII digits only: no sign, no `+`, no spaces.
fn digits<T: FromStr>(s: &str) -> Option<T> {
    let ok = !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit());
    ok.then(|| s.parse().ok()).flatten()
}

fn parse_prefixed<T: FromStr>(tok: Tok, prefix: char, what: &str) -> Result<T, Diagnostic> {
    let n = tok.text.strip_prefix(prefix).and_then(digits);
    n.ok_or_else(|| error(tok.at, format!("expected {what}, found {}", tok.text)))
}

/// Writes a module as `.shawasm` that [`assemble`] reads back to an equal
/// module, as long as unused primitive register slots are zero.
pub fn disassemble(m: &Module) -> String {
    let mut out = String::new();
    let name = |id: FuncId| m.functions.get(id as usize).map_or("?", |f| f.name.as_str());
    for f in &m.functions {
        let _ = writeln!(
            out,
            "fn {} params {} results {} registers {}",
            f.name, f.params, f.results, f.registers
        );
        for (k, v) in f.consts.iter().enumerate() {
            let _ = write!(out, "  const k{k} ");
            write_const(&mut out, v);
            out.push('\n');
        }
        for (i, instr) in f.code.iter().enumerate() {
            out.push_str("  ");
            let _ = match *instr {
                Instr::Const { dst, idx } => write!(out, "const r{dst} k{idx}"),
                Instr::Move { dst, src } => write!(out, "move r{dst} r{src}"),
                Instr::Prim { prim, dst, src } => {
                    let (ins, outs) = prim.arity();
                    let list = |rs: &[Reg]| {
                        rs.iter().map(|r| format!("r{r}")).collect::<Vec<_>>().join(",")
                    };
                    let (d, s) = (list(&dst[..outs as usize]), list(&src[..ins as usize]));
                    write!(out, "prim {} {d} {s}", prim.letter())
                }
                Instr::Call { func, base, argc, dst } => {
                    write!(out, "call {} r{base} {argc} r{dst}", name(func))
                }
                Instr::TailCall { func, base, argc } => {
                    write!(out, "tail {} r{base} {argc}", name(func))
                }
                Instr::Return { base, count } => write!(out, "ret r{base} {count}"),
                Instr::Jump { target } => write!(out, "jump {target}"),
                Instr::JumpIfNe { src, idx, target } => write!(out, "jne r{src} k{idx} {target}"),
                Instr::NoMatch => write!(out, "nomatch"),
            };
            match f.spans.get(i) {
                Some(&at) if at != Pos::new(0, 0) => {
                    let _ = write!(out, " at {at}");
                }
                _ => {}
            }
            out.push('\n');
        }
        out.push_str("end\n");
    }
    let _ = writeln!(out, "main {}", name(m.main));
    out
}

/// Writes a constant in the form [`Line::value`] reads, from a worklist
/// rather than by recursion.
fn write_const(out: &mut String, v: &Value) {
    enum Task<'a> {
        Value(&'a Value),
        Str(&'static str),
        /// The rest of a list after an item: more items, or the `]`.
        Rest(&'a Value),
    }
    let mut tasks = vec![Task::Value(v)];
    while let Some(task) = tasks.pop() {
        match task {
            Task::Str(s) => out.push_str(s),
            Task::Value(Value::Int(n)) => {
                let _ = write!(out, "{n}");
            }
            Task::Value(Value::Text(t)) => {
                let _ = write!(out, "«{t}»");
            }
            Task::Value(Value::Pair(cell)) => {
                out.push('(');
                tasks.extend([Task::Str(")"), Task::Value(&cell.1), Task::Str(" ")]);
                tasks.push(Task::Value(&cell.0));
            }
            Task::Value(Value::List(None)) => out.push_str("nil"),
            Task::Value(Value::List(Some(c))) => {
                out.push('[');
                tasks.extend([Task::Rest(&c.1), Task::Value(&c.0)]);
            }
            Task::Rest(Value::List(Some(c))) => {
                out.push(' ');
                tasks.extend([Task::Rest(&c.1), Task::Value(&c.0)]);
            }
            Task::Rest(_) => out.push(']'),
        }
    }
}
