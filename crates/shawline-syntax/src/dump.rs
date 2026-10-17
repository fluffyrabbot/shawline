//! The `.ast` dump: one fact per line, nested by indentation.
//!
//! ```text
//! def ·𐑛𐑳𐑚𐑩𐑤 at 2:3 box 2:0-9:22
//!   param 0 𐑯 at 5:2 arrow 5:0
//!   result 0 𐑬 at 5:24 arrow 5:22
//!   node 0 call 𐑐 at 5:12 box 4:10-7:14 in 5:10 6:10 out 5:14
//!   edge wire 0: param 0 -> node 0 in 0, node 0 in 1
//!   edge wire 1: node 0 out 0 -> result 0
//! ```
//!
//! A switch's arms follow its node line, one level deeper, each with its own
//! nodes and edges. Order is the AST's: definitions, then per scope its ports,
//! nodes and edges.

use std::fmt::{self, Display, Formatter, Write};

use shawline_grid::Pos;

use crate::ast::{Endpoint, Graph, Literal, Name, Node, NodeKind, Sheet};

impl Display for Sheet {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        for def in &self.definitions {
            writeln!(f, "def ·{} at {} box {}", NameText(&def.name), def.name.at, def.rect)?;
            for (i, p) in def.inputs.iter().enumerate() {
                writeln!(
                    f,
                    "  param {i} {} at {} arrow {}",
                    NameText(&p.name),
                    p.name.at,
                    p.arrow
                )?;
            }
            for (i, p) in def.outputs.iter().enumerate() {
                writeln!(
                    f,
                    "  result {i} {} at {} arrow {}",
                    NameText(&p.name),
                    p.name.at,
                    p.arrow
                )?;
            }
            write_graph(f, &def.body, 1)?;
        }
        Ok(())
    }
}

/// A name, or `?` where it is missing.
struct NameText<'a>(&'a Name);

impl Display for NameText<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        if self.0.text.is_empty() { f.write_str("?") } else { f.write_str(&self.0.text) }
    }
}

struct Lit<'a>(&'a Literal);

impl Display for Lit<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self.0 {
            Literal::Int(n) => write!(f, "{n}"),
            Literal::Text(s) => write!(f, "«{s}»"),
            Literal::Nil => f.write_str("𐑯𐑦𐑤"),
        }
    }
}

struct End(Endpoint);

impl Display for End {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self.0 {
            Endpoint::Param(i) => write!(f, "param {i}"),
            Endpoint::Result(i) => write!(f, "result {i}"),
            Endpoint::Input(n, i) => write!(f, "node {n} in {i}"),
            Endpoint::Output(n, i) => write!(f, "node {n} out {i}"),
        }
    }
}

/// ` label a b c`, or nothing for an empty list.
fn positions(f: &mut Formatter<'_>, label: &str, cells: &[Pos]) -> fmt::Result {
    if !cells.is_empty() {
        write!(f, " {label}")?;
        for p in cells {
            write!(f, " {p}")?;
        }
    }
    Ok(())
}

fn write_graph(f: &mut Formatter<'_>, g: &Graph, depth: usize) -> fmt::Result {
    let pad = "  ".repeat(depth);
    for (i, node) in g.nodes.iter().enumerate() {
        write!(f, "{pad}node {i} ")?;
        write_node(f, node, depth)?;
    }
    for e in &g.edges {
        let mut sinks = String::new();
        for (k, s) in e.to.iter().enumerate() {
            let sep = if k == 0 { "" } else { ", " };
            write!(sinks, "{sep}{}", End(*s))?;
        }
        writeln!(f, "{pad}edge wire {}: {} -> {sinks}", e.wire, End(e.from))?;
    }
    Ok(())
}

fn write_node(f: &mut Formatter<'_>, node: &Node, depth: usize) -> fmt::Result {
    let ports = |f: &mut Formatter<'_>| {
        positions(f, "in", &node.inputs)?;
        positions(f, "out", &node.outputs)?;
        writeln!(f)
    };
    match &node.kind {
        NodeKind::Call(name) => {
            write!(f, "call {} at {} box {}", NameText(name), name.at, node.rect)?;
            ports(f)
        }
        NodeKind::Inline(name) => {
            write!(f, "inline {} at {} cells {}", NameText(name), name.at, node.rect)?;
            ports(f)
        }
        NodeKind::Const(lit) => {
            write!(f, "const {} cells {}", Lit(lit), node.rect)?;
            ports(f)
        }
        NodeKind::Switch(sw) => {
            write!(f, "switch box {} selector {}", node.rect, sw.selector)?;
            ports(f)?;
            let pad = "  ".repeat(depth + 1);
            for (k, arm) in sw.arms.iter().enumerate() {
                write!(f, "{pad}arm {k} ")?;
                match &arm.guard {
                    Some(g) => write!(f, "guard {} at {}", Lit(&g.value), g.at)?,
                    None => f.write_str("default")?,
                }
                write!(f, " rows {}..{}", arm.rows.0, arm.rows.1)?;
                positions(f, "params", &arm.params)?;
                positions(f, "results", &arm.results)?;
                writeln!(f)?;
                write_graph(f, &arm.body, depth + 2)?;
            }
            Ok(())
        }
    }
}
