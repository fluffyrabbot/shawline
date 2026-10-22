//! The sheet as Studio's wiring edits see it (PLAN P7): what kind each box
//! is, which cells are sources and sinks, and what a change an edit declares
//! calls them in a [`Structure`](structure::Structure).
//!
//! Everything is read off the recognized [`Diagram`], not the syntax tree,
//! because edits run on half-built sheets: a constant whose wire was just
//! removed is stray text to the parser, but still a source here.

use std::collections::{HashMap, HashSet};

use shawline_grid::{Dir, Grid, Pos, Rect};
use shawline_shape::stroke::Kind;
use shawline_shape::structure::{self, Across, End, WirePart};
use shawline_shape::{Diagram, EndKind, Port, PortKind, TextKind};

use crate::{Refusal, RefusalKind};
use crate::sheet::{across, borders, text_cells};

/// The empty list's word (SPEC §9), the one constant written as a word.
pub(crate) const NIL: &str = "𐑯𐑦𐑤";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BoxKind {
    Definition,
    Switch,
    Node,
}

/// Where a wire may run: a box's interior (a definition) or one arm of a
/// switch, as (box index, arm index).
pub(crate) type Scope = (usize, usize);

/// What a wire end meets, with no reference to the wire's own cells, so a
/// re-routed wire prints the same as the one it replaces.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum Term {
    /// The box port at this cell, met from the side the end faces.
    Port(Pos, Dir),
    /// The text starting at this cell, across spaces in that direction: a
    /// constant, an input's name or an inline node's word.
    Text(Pos, Dir),
    /// A free arrowhead at this cell: an inline node's input.
    Arrow(Pos),
    /// A dangling end (`false`) or a blocked one (`true`), kept by place.
    Loose(Pos, Dir, bool),
}

/// Where a new wire can start.
#[derive(Clone, Debug)]
pub(crate) struct Source {
    pub term: Term,
    pub scope: Scope,
    /// The cell a route arrives at, heading west, to start here: the tee or
    /// through-port itself, or the space after a text.
    pub goal: Pos,
    /// The text a constant or input name source is.
    pub text: Option<usize>,
    /// What to call it in a refusal.
    pub name: String,
}

impl Source {
    /// The cell a new wire's first stroke takes.
    pub(crate) fn first(&self) -> Pos {
        Pos::new(self.goal.row, self.goal.col + 1)
    }
}

/// Where a wire can end: an arrowhead on a box border.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Sink {
    pub at: Pos,
    /// The way the arrowhead points.
    pub dir: Dir,
    pub scope: Scope,
}

impl Sink {
    pub(crate) fn term(&self) -> Term {
        Term::Port(self.at, self.dir)
    }

    /// The cell before the arrowhead, which the wire's last stroke takes.
    pub(crate) fn target(&self) -> Option<Pos> {
        self.at.step(self.dir.opposite())
    }
}

/// Why a wire at an inline node is refused ([`RefusalKind::Unsupported`]).
const INLINE: &str = "wires at an inline node can't be edited by mouse yet; edit the text";

/// The refusal for a cell that is neither a port nor text, as a source.
pub(crate) fn no_source(at: Pos) -> Refusal {
    Refusal::new(
        at,
        RefusalKind::Target,
        "nothing here feeds a wire: start at an output tee, an input, a constant or an arm's input",
    )
}

/// The refusal for a cell that isn't an arrowhead, as a sink.
pub(crate) fn no_sink(at: Pos) -> Refusal {
    let why = "wires end at arrowheads: a node's input, an output or a switch port";
    Refusal::new(at, RefusalKind::Target, why)
}

/// A source with the cell naming it, and whether a wire already starts there.
pub(crate) type WiredSource = (Pos, Source, bool);

pub(crate) struct Model<'a> {
    pub grid: &'a Grid,
    pub diagram: &'a Diagram,
    pub texts: HashMap<Pos, usize>,
    pub borders: HashSet<Pos>,
    ports: HashMap<Pos, (usize, Port)>,
    /// Every wire's terms, in wire order.
    pub terms: Vec<Vec<Term>>,
}

impl<'a> Model<'a> {
    pub(crate) fn new(grid: &'a Grid, diagram: &'a Diagram) -> Self {
        let texts = text_cells(diagram);
        let ports = diagram
            .boxes
            .iter()
            .enumerate()
            .flat_map(|(i, b)| b.ports.iter().map(move |p| (p.pos, (i, *p))))
            .collect();
        let mut model =
            Model { grid, diagram, texts, borders: borders(diagram), ports, terms: Vec::new() };
        model.terms = diagram
            .wires
            .iter()
            .map(|w| {
                let mut terms: Vec<Term> = w.ends.iter().map(|e| model.term(e)).collect();
                terms.sort();
                terms
            })
            .collect();
        model
    }

    fn term(&self, e: &shawline_shape::WireEnd) -> Term {
        match e.kind {
            EndKind::Port(p) => Term::Port(p, e.facing),
            EndKind::Arrow => Term::Arrow(e.pos),
            EndKind::Open if e.facing.is_horizontal() => {
                match across(self.grid, &self.texts, e.pos, e.facing).1 {
                    Some(t) => Term::Text(self.diagram.texts[t].pos, e.facing),
                    None => Term::Loose(e.pos, e.facing, false),
                }
            }
            EndKind::Open => Term::Loose(e.pos, e.facing, false),
            EndKind::Blocked => Term::Loose(e.pos, e.facing, true),
        }
    }

    pub(crate) fn kind(&self, b: usize) -> BoxKind {
        let shape = &self.diagram.boxes[b];
        if shape.parent.is_none() {
            BoxKind::Definition
        } else if !shape.dividers.is_empty() || shape.ports.iter().any(|p| p.side == Dir::N) {
            BoxKind::Switch
        } else {
            BoxKind::Node
        }
    }

    /// The arm of box `b` that row `row` lies in; 0 for a box without arms.
    pub(crate) fn arm(&self, b: usize, row: usize) -> usize {
        self.diagram.boxes[b].dividers.iter().filter(|d| d.row < row).count()
    }

    /// The scope just inside box `b` at `row`.
    fn inside(&self, b: usize, row: usize) -> Scope {
        (b, self.arm(b, row))
    }

    /// The scope just outside box `b`, at `row`.
    fn outside(&self, b: usize, row: usize) -> Option<Scope> {
        self.diagram.boxes[b].parent.map(|p| self.inside(p, row))
    }

    /// The cells a scope's wires may use.
    pub(crate) fn region(&self, (b, arm): Scope) -> Rect {
        let shape = &self.diagram.boxes[b];
        let r = shape.rect;
        let top = if arm == 0 { r.top } else { shape.dividers[arm - 1].row };
        let bottom = shape.dividers.get(arm).map_or(r.bottom, |d| d.row);
        Rect { top: top + 1, left: r.left + 1, bottom: bottom.saturating_sub(1), right: r.right - 1 }
    }

    pub(crate) fn port(&self, p: Pos) -> Option<(usize, Port)> {
        self.ports.get(&p).copied()
    }

    pub(crate) fn box_at(&self, corner: Pos) -> Option<usize> {
        self.diagram.boxes.iter().position(|b| (b.rect.top, b.rect.left) == (corner.row, corner.col))
    }

    fn text_at(&self, p: Pos) -> Option<usize> {
        self.texts.get(&p).copied()
    }

    fn content(&self, t: usize) -> &str {
        &self.diagram.texts[t].content
    }

    /// True if a free arrowhead points at text `t` across spaces: `t` is an
    /// inline node's word, or the literal after one (SPEC §6).
    fn after_free_arrow(&self, t: usize) -> bool {
        let start = self.diagram.texts[t].pos;
        let (gap, text) = across(self.grid, &self.texts, start, Dir::W);
        if text.is_some() {
            return false;
        }
        let Some(q) = gap.last().copied().unwrap_or(start).step(Dir::W) else { return false };
        !self.borders.contains(&q)
            && matches!(self.diagram.strokes.get(q).map(|s| s.kind), Some(Kind::Arrow(Dir::E)))
    }

    /// The scope of text `t` if it can be a constant: a number, a literal
    /// or `𐑯𐑦𐑤`, loose in a definition or an arm.
    fn constant(&self, t: usize) -> Option<Scope> {
        let text = &self.diagram.texts[t];
        let literal = matches!(text.kind, TextKind::Number | TextKind::Literal)
            || (text.kind == TextKind::Word && text.content == NIL);
        if !literal || text.title_of.is_some() || text.divider_of.is_some() {
            return None;
        }
        let owner = text.owner?;
        if self.kind(owner) == BoxKind::Node || self.after_free_arrow(t) {
            return None;
        }
        Some(self.inside(owner, text.pos.row))
    }

    /// The definition input whose name text `t` is, if any.
    fn input_named(&self, t: usize) -> Option<(usize, Port)> {
        let start = self.diagram.texts[t].pos;
        let (gap, text) = across(self.grid, &self.texts, start, Dir::W);
        if text.is_some() {
            return None;
        }
        let q = gap.last().copied().unwrap_or(start).step(Dir::W)?;
        let (b, port) = self.port(q)?;
        (self.kind(b) == BoxKind::Definition && port.side == Dir::W && port.kind == PortKind::Arrow)
            .then_some((b, port))
    }

    /// The name text after a definition input's arrowhead.
    fn input_name(&self, port: Pos) -> Option<usize> {
        across(self.grid, &self.texts, port, Dir::E).1
    }

    fn text_source(&self, t: usize, scope: Scope) -> Result<Source, Refusal> {
        let text = &self.diagram.texts[t];
        let goal = Pos::new(text.pos.row, text.pos.col + text.width);
        if self.grid.get(goal) != ' ' {
            return Err(Refusal::new(
                text.pos,
                RefusalKind::Blocked,
                format!("{} needs a space after it before a wire can start", text.content),
            ));
        }
        Ok(Source {
            term: Term::Text(text.pos, Dir::W),
            scope,
            goal,
            text: Some(t),
            name: text.content.clone(),
        })
    }

    /// The source named by cell `at` (the P7 contract's naming).
    pub(crate) fn source(&self, at: Pos) -> Result<Source, Refusal> {
        if let Some((b, port)) = self.port(at) {
            let kind = self.kind(b);
            let row = at.row;
            return match (kind, port.kind, port.side) {
                (BoxKind::Node, PortKind::Tee, Dir::E) => Ok(Source {
                    term: Term::Port(at, Dir::W),
                    scope: self.outside(b, row).expect("a node has a parent"),
                    goal: at,
                    text: None,
                    name: format!("the output of {}", self.word_of(b)),
                }),
                (BoxKind::Definition, PortKind::Arrow, Dir::W) => match self.input_name(at) {
                    Some(t) => self.text_source(t, self.inside(b, row)),
                    None => Err(Refusal::new(
                        at,
                        RefusalKind::Target,
                        "this input has no name yet; type one first",
                    )),
                },
                (BoxKind::Switch, PortKind::Arrow, Dir::W) => Ok(Source {
                    term: Term::Port(at, Dir::W),
                    scope: self.inside(b, row),
                    goal: at,
                    text: None,
                    name: "this arm's input".to_owned(),
                }),
                (BoxKind::Switch, _, Dir::E) => Err(Refusal::new(
                    at,
                    RefusalKind::Unsupported,
                    "a switch's merged output can't be wired by mouse yet; draw it in the text",
                )),
                _ => Err(Refusal::new(
                    at,
                    RefusalKind::Target,
                    "this arrowhead takes a wire in; drag from a source",
                )),
            };
        }
        if let Some(t) = self.text_at(at) {
            if let Some(scope) = self.constant(t) {
                return self.text_source(t, scope);
            }
            if let Some((b, port)) = self.input_named(t) {
                return self.text_source(t, self.inside(b, port.pos.row));
            }
            if self.after_free_arrow(t) {
                return Err(Refusal::new(at, RefusalKind::Unsupported, INLINE));
            }
            let why = format!("{} can't feed a wire", self.content(t));
            return Err(Refusal::new(at, RefusalKind::Target, why));
        }
        Err(no_source(at))
    }

    /// The sink named by cell `at`: an arrowhead on a box border.
    pub(crate) fn sink(&self, at: Pos) -> Result<Sink, Refusal> {
        let not = || no_sink(at);
        let (b, port) = self.port(at).ok_or_else(not)?;
        if port.kind != PortKind::Arrow {
            return Err(not());
        }
        let row = at.row;
        let scope = match (self.kind(b), port.side, port.dir) {
            (BoxKind::Node, Dir::W, Dir::E) => self.outside(b, row),
            (BoxKind::Definition, Dir::E, Dir::E) => Some(self.inside(b, row)),
            (BoxKind::Definition, Dir::W, _) => {
                let why = "this is an input: wires start here, they don't end";
                return Err(Refusal::new(at, RefusalKind::Target, why));
            }
            (BoxKind::Switch, Dir::N, Dir::S) | (BoxKind::Switch, Dir::W, Dir::E) => {
                self.outside(b, row)
            }
            (BoxKind::Switch, Dir::E, Dir::E) => Some(self.inside(b, row)),
            _ => None,
        };
        let scope = scope.ok_or_else(not)?;
        Ok(Sink { at, dir: port.dir, scope })
    }

    /// The word written in node box `b`.
    pub(crate) fn word_of(&self, b: usize) -> String {
        let rect = self.diagram.boxes[b].rect;
        self.diagram
            .texts
            .iter()
            .find(|t| t.owner == Some(b) && rect.contains(t.pos))
            .map_or_else(|| "the node".to_owned(), |t| t.content.clone())
    }

    /// The wire with an end meeting `term`.
    pub(crate) fn wire_of(&self, term: Term) -> Option<usize> {
        self.terms.iter().position(|ts| ts.contains(&term))
    }

    /// `Err` with the reason when wire `w` isn't editable in Phase 1: it
    /// touches an inline node, or carries a switch's merged output. Either
    /// is [`RefusalKind::Unsupported`].
    pub(crate) fn editable(&self, w: usize) -> Result<(), &'static str> {
        for &term in &self.terms[w] {
            match term {
                Term::Arrow(_) => return Err(INLINE),
                Term::Text(p, Dir::W) if self.text_at(p).is_some_and(|t| self.after_free_arrow(t)) => {
                    return Err(INLINE);
                }
                Term::Port(p, Dir::W)
                    if self.port(p).is_some_and(|(b, port)| {
                        self.kind(b) == BoxKind::Switch && port.side == Dir::E
                    }) =>
                {
                    return Err("a switch's merged output can't be edited by mouse yet; edit the text");
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// The directions in which cell `p` joins wire `w` ([`Wire::links`]):
    /// its own strokes, or at a crossing the axes the wire owns. 0 for a
    /// cell off the wire.
    ///
    /// [`Wire::links`]: shawline_shape::Wire::links
    pub(crate) fn links(&self, w: usize, p: Pos) -> u8 {
        let wire = &self.diagram.wires[w];
        wire.cells.binary_search(&p).map_or(0, |i| wire.links[i])
    }

    /// Every potential text source in the sheet: constants and input names.
    pub(crate) fn text_sources(&self) -> Vec<usize> {
        (0..self.diagram.texts.len())
            .filter(|&t| self.constant(t).is_some() || self.input_named(t).is_some())
            .collect()
    }

    /// Every source and sink, for the UI: the cell naming it, its scope, and
    /// whether a wire already meets it.
    pub(crate) fn endpoints(&self) -> (Vec<WiredSource>, Vec<(Sink, bool)>) {
        let mut sources = Vec::new();
        let mut sinks = Vec::new();
        for b in &self.diagram.boxes {
            for p in &b.ports {
                if let Ok(s) = self.source(p.pos) {
                    let wired = self.wire_of(s.term).is_some();
                    sources.push((p.pos, s, wired));
                }
                if let Ok(s) = self.sink(p.pos) {
                    sinks.push((s, self.wire_of(s.term()).is_some()));
                }
            }
        }
        for t in 0..self.diagram.texts.len() {
            if self.constant(t).is_some() {
                let at = self.diagram.texts[t].pos;
                if let Ok(s) = self.source(at) {
                    let wired = self.wire_of(s.term).is_some();
                    sources.push((at, s, wired));
                }
            }
        }
        sources.sort_by_key(|(p, _, _)| *p);
        sinks.sort_by_key(|(s, _)| s.at);
        (sources, sinks)
    }

    /// What `term` is called in a [`Structure`](structure::Structure) when
    /// it names a port or a text, as a declared change names it. Only one
    /// wire stops at a port, or at a text from one side, so the name is
    /// that wire's alone.
    pub(crate) fn named(&self, term: Term) -> Option<structure::Term> {
        let (facing, end) = match term {
            Term::Port(p, facing) => {
                let (b, k) = structure::port_index(&self.diagram.boxes, p)?;
                (facing, End::Port(b, k))
            }
            Term::Text(p, facing) => (facing, End::Open(Across::Text(self.text_at(p)?))),
            Term::Arrow(_) | Term::Loose(..) => return None,
        };
        Some(structure::Term { facing, end })
    }

    /// Wire `w` as [`Structure::of`](structure::Structure::of) records it:
    /// what names a wire that no port or text does ([`Model::named`]).
    pub(crate) fn part(&self, w: usize) -> WirePart {
        let wire = &self.diagram.wires[w];
        let mut terms: Vec<structure::Term> = wire
            .ends
            .iter()
            .map(|e| {
                let end = match e.kind {
                    EndKind::Port(p) => structure::port_index(&self.diagram.boxes, p)
                        .map_or(End::Blocked, |(b, k)| End::Port(b, k)),
                    EndKind::Arrow => End::Arrow(self.meets(e.pos, e.facing)),
                    EndKind::Open => End::Open(self.meets(e.pos, e.facing)),
                    EndKind::Blocked => End::Blocked,
                };
                structure::Term { facing: e.facing, end }
            })
            .collect();
        terms.sort();
        WirePart { terms, tees: wire.tees.len(), crossings: wire.crossings.len() }
    }

    /// What lies across spaces from `from` in `dir`, as a structure names
    /// it. Text meets a wire only along a row (SPEC §6).
    fn meets(&self, from: Pos, dir: Dir) -> Across {
        if !dir.is_horizontal() {
            return Across::Nothing;
        }
        let (gap, text) = across(self.grid, &self.texts, from, dir);
        if let Some(t) = text {
            return Across::Text(t);
        }
        let next = gap.last().copied().unwrap_or(from).step(dir);
        if next.is_some_and(|q| self.diagram.strokes.get(q).is_some()) {
            Across::Stroke
        } else {
            Across::Nothing
        }
    }
}
