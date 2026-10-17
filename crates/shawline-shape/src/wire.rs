//! Wires: the contract between stroke recognition and syntax.
//!
//! Lead-owned (see `docs/PLAN.md`). Agent A1 traces wires elsewhere in this
//! crate and fills [`Diagram::wires`](crate::Diagram::wires); this file fixes
//! their shape and the `.wires` dump the corpus checks.
//!
//! A wire is a connected set of stroke cells that belong to no box border and
//! no divider. Adjacent cells are connected when each connects toward the
//! other. A crossing (`┼`, or an ASCII `+` joined on all four sides) carries
//! two independent wires, one per axis, and appears in both.

use std::fmt::{self, Write};

use shawline_grid::{Dir, Pos};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Wire {
    /// Every cell of the wire, row-major. Crossings appear in both wires.
    pub cells: Vec<Pos>,
    /// Tees inside the wire: fan-out points, or merges of switch outputs.
    pub tees: Vec<Pos>,
    /// Crossings the wire passes through.
    pub crossings: Vec<Pos>,
    /// Where the wire stops, ordered by position then direction.
    pub ends: Vec<WireEnd>,
}

/// A place where a wire stops: a cell of the wire, and a direction leading
/// out of it that does not lead to more of the same wire.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct WireEnd {
    /// The wire's last cell at this end.
    pub pos: Pos,
    /// The direction leading out of the wire from `pos`.
    pub facing: Dir,
    pub kind: EndKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum EndKind {
    /// `pos` is a free-standing arrowhead pointing `facing`. What it points at
    /// (normally an inline node's word, after one space) is for syntax.
    Arrow,
    /// The neighbour in `facing` is the box port at this position. A switch
    /// through-port has one wire ending at it from each side.
    Port(Pos),
    /// `pos` connects toward `facing`, and the neighbour there is not a
    /// stroke: a space, text, or the grid edge. Syntax decides whether text
    /// there makes this end a source.
    Open,
    /// `pos` connects toward `facing`, but the stroke there doesn't connect
    /// back and isn't a port. Always diagnosed (`S010`).
    Blocked,
}

/// Writes the `.wires` dump: one fact per line, wires in order of their
/// first cell.
///
/// ```text
/// wire 0 cells 9
///   tee 5:6
///   cross 7:8
///   end 5:4 W open
///   end 5:9 E port 5:10
/// ```
pub fn dump(wires: &[Wire]) -> String {
    let mut out = String::new();
    for (i, w) in wires.iter().enumerate() {
        write_wire(&mut out, i, w).expect("writing to a String cannot fail");
    }
    out
}

fn write_wire(out: &mut String, i: usize, w: &Wire) -> fmt::Result {
    writeln!(out, "wire {i} cells {}", w.cells.len())?;
    for t in &w.tees {
        writeln!(out, "  tee {t}")?;
    }
    for c in &w.crossings {
        writeln!(out, "  cross {c}")?;
    }
    for e in &w.ends {
        write!(out, "  end {} {} ", e.pos, e.facing)?;
        match e.kind {
            EndKind::Arrow => writeln!(out, "arrow")?,
            EndKind::Port(p) => writeln!(out, "port {p}")?,
            EndKind::Open => writeln!(out, "open")?,
            EndKind::Blocked => writeln!(out, "blocked")?,
        }
    }
    Ok(())
}
