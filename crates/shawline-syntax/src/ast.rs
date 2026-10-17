//! The AST: the contract between `shawline-syntax` and its consumers
//! (`shawline-ir`, `shawline-fmt`, `shawline-lsp`).
//!
//! Lead-owned (see `docs/PLAN.md`). Agent B1 builds it from a
//! [`Diagram`](shawline_shape::Diagram). Every item keeps its source geometry,
//! because the formatter preserves placement and diagnostics point at cells.
//!
//! # Scopes
//!
//! A [`Graph`] is one scope: a definition body or a switch arm. Inside a
//! scope, the scope's own inputs are [`Endpoint::Param`] sources and its
//! outputs are [`Endpoint::Result`] sinks. For a definition those are its
//! ports; for an arm, its through-ports. Values cross into an arm only
//! through its through-ports (SPEC §8).
//!
//! # Wires and edges
//!
//! Every wire in a scope becomes exactly one [`Edge`]: one source, one or more
//! sinks. The edges of a scope, together with its definition or arm ports,
//! account for every wire inside it.

use shawline_grid::{Pos, Rect};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sheet {
    /// Definitions in row-major order of their top-left corners.
    pub definitions: Vec<Definition>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Definition {
    /// The name without its namer dot.
    pub name: Name,
    /// The rounded box.
    pub rect: Rect,
    /// Left-border ports, top to bottom.
    pub inputs: Vec<PortDecl>,
    /// Right-border ports, top to bottom.
    pub outputs: Vec<PortDecl>,
    pub body: Graph,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Name {
    pub text: String,
    /// The first cell of the word (the namer dot, for definition names).
    pub at: Pos,
}

/// A definition port: its arrowhead on the border and the name after it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PortDecl {
    pub arrow: Pos,
    pub name: Name,
}

/// One scope: a definition body or a switch arm.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Graph {
    /// Nodes in row-major order of their top-left cell.
    pub nodes: Vec<Node>,
    /// One per wire, in the order of `Diagram::wires`.
    pub edges: Vec<Edge>,
}

/// An index into [`Graph::nodes`] of the enclosing scope.
pub type NodeId = usize;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Node {
    pub kind: NodeKind,
    /// The box; for inline nodes and constants, the text's cells.
    pub rect: Rect,
    /// Input slots, top to bottom: the cell each input arrives at.
    pub inputs: Vec<Pos>,
    /// Output slots, top to bottom: the cell each output leaves from.
    pub outputs: Vec<Pos>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NodeKind {
    /// A boxed call of a definition or primitive.
    Call(Name),
    /// A word on a wire (`─▶ 𐑖 ──`): one input, one output.
    Inline(Name),
    /// A literal or `𐑯𐑦𐑤` feeding a wire: no inputs, one output.
    Const(Literal),
    /// Input slots: the selector first, then every arm's through-ports, arm by
    /// arm. Output slots: one per result ordinal, shared by all arms.
    Switch(Switch),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Switch {
    /// The `▼` on the top border.
    pub selector: Pos,
    /// Arms top to bottom; a default arm, if any, is last.
    pub arms: Vec<Arm>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Arm {
    /// `None` for the default arm.
    pub guard: Option<Guard>,
    /// Interior rows `start..end`.
    pub rows: (usize, usize),
    /// Left-border through-ports, top to bottom.
    pub params: Vec<Pos>,
    /// Right-border through-ports, top to bottom.
    pub results: Vec<Pos>,
    pub body: Graph,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Guard {
    pub value: Literal,
    pub at: Pos,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Literal {
    Int(i64),
    Text(String),
    /// `𐑯𐑦𐑤`, the empty list.
    Nil,
}

/// One wire: a single source feeding one or more sinks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Edge {
    /// An index into `Diagram::wires`.
    pub wire: usize,
    pub from: Endpoint,
    pub to: Vec<Endpoint>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Endpoint {
    /// The scope's input with this ordinal (a source).
    Param(u32),
    /// The scope's output with this ordinal (a sink).
    Result(u32),
    /// A node's input slot (a sink).
    Input(NodeId, u32),
    /// A node's output slot (a source).
    Output(NodeId, u32),
}
