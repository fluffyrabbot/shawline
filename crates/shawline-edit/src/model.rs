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
use shawline_shape::structure::{self, Across, End};
use shawline_shape::{Diagram, EndKind, Port, PortKind, TextKind};

use crate::{BoxKind, Blockage, Refusal, RefusalKind};
use crate::sheet::{across, borders, text_cells};

/// The empty list's word (SPEC §9), the one constant written as a word.
pub(crate) const NIL: &str = "𐑯𐑦𐑤";


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
    /// The text a constant, input name or inline node source is.
    pub text: Option<usize>,
    /// What to call it in a refusal.
    pub name: String,
    /// A switch's merged output of two or more arms: each arm's right
    /// through-port at its ordinal, top to bottom. Empty for any other.
    pub merged: Vec<Pos>,
}

impl Source {
    /// The cell a new wire's first stroke takes.
    pub(crate) fn first(&self) -> Pos {
        Pos::new(self.goal.row, self.goal.col + 1)
    }
}

/// Where a wire can end: an arrowhead on a box border, or an inline
/// node's free arrowhead.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Sink {
    pub at: Pos,
    /// The way the arrowhead points.
    pub dir: Dir,
    pub scope: Scope,
    /// An inline node's input: a free arrowhead, not a box port.
    pub free: bool,
}

impl Sink {
    pub(crate) fn term(&self) -> Term {
        if self.free { Term::Arrow(self.at) } else { Term::Port(self.at, self.dir) }
    }

    /// The cell before the arrowhead, which the wire's last stroke takes.
    pub(crate) fn target(&self) -> Option<Pos> {
        self.at.step(self.dir.opposite())
    }
}

/// The refusal for a cell that is neither a port nor text, as a source.
pub(crate) fn no_source(at: Pos) -> Refusal {
    Refusal::new(
        at,
        RefusalKind::Target,
        "nothing here starts a wire: a source is a node's ├, an input, a constant or an arm's input",
    )
}

/// The refusal for a cell that isn't an arrowhead, as a sink.
pub(crate) fn no_sink(at: Pos) -> Refusal {
    let why = "wires end at arrowheads: a node's input, an output, a selector or a through-port";
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

    pub(crate) fn text_at(&self, p: Pos) -> Option<usize> {
        self.texts.get(&p).copied()
    }

    fn content(&self, t: usize) -> &str {
        &self.diagram.texts[t].content
    }

    /// True if a free arrowhead points at text `t` across spaces: `t` is an
    /// inline node's word, or the literal after one (SPEC §6).
    pub(crate) fn after_free_arrow(&self, t: usize) -> bool {
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
    pub(crate) fn constant(&self, t: usize) -> Option<Scope> {
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
    pub(crate) fn input_named(&self, t: usize) -> Option<(usize, Port)> {
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
                RefusalKind::Blocked(Blockage::NoRoom),
                format!("{} needs a space after it before a wire can start; try Make room", text.content),
            ));
        }
        Ok(Source {
            term: Term::Text(text.pos, Dir::W),
            scope,
            goal,
            text: Some(t),
            name: text.content.clone(),
            merged: Vec::new(),
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
                    merged: Vec::new(),
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
                    merged: Vec::new(),
                }),
                (BoxKind::Switch, PortKind::Arrow, Dir::E) => {
                    let Some(scope) = self.outside(b, row) else {
                        let why = "this switch has no definition around it";
                        return Err(Refusal::new(at, RefusalKind::Target, why));
                    };
                    let ports = self.merged(b, at);
                    // Whichever arm's port a wire leaves names that wire.
                    let wired = ports.iter().copied().find(|&p| self.wire_of(Term::Port(p, Dir::W)).is_some());
                    let named = wired.unwrap_or(at);
                    Ok(Source {
                        term: Term::Port(named, Dir::W),
                        scope,
                        goal: named,
                        text: None,
                        name: "the switch's output".to_owned(),
                        merged: if ports.len() > 1 { ports } else { Vec::new() },
                    })
                }
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
            if self.inline_word(t) {
                let text = &self.diagram.texts[t];
                let Some(owner) = text.owner else {
                    let why = "an inline node goes inside a definition";
                    return Err(Refusal::new(at, RefusalKind::Target, why));
                };
                return self.text_source(t, self.inside(owner, text.pos.row));
            }
            let why = format!("{} can't feed a wire", self.content(t));
            return Err(Refusal::new(at, RefusalKind::Target, why));
        }
        Err(no_source(at))
    }

    /// The sink named by cell `at`: an arrowhead on a box border, or an
    /// inline node's free arrowhead.
    pub(crate) fn sink(&self, at: Pos) -> Result<Sink, Refusal> {
        let not = || no_sink(at);
        if let Some(t) = self.inline_after(at) {
            let text = &self.diagram.texts[t];
            let owner = text.owner.ok_or_else(not)?;
            return Ok(Sink { at, dir: Dir::E, scope: self.inside(owner, text.pos.row), free: true });
        }
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
        Ok(Sink { at, dir: port.dir, scope, free: false })
    }

    /// The text of the word written in box `b`, if it holds one.
    pub(crate) fn word(&self, b: usize) -> Option<usize> {
        let rect = self.diagram.boxes[b].rect;
        self.diagram.texts.iter().position(|t| t.owner == Some(b) && rect.contains(t.pos) && t.kind == TextKind::Word)
    }

    /// Whether box `b` is a switch, or the square box [`crate::place_switch`]
    /// draws before its selector: a box in a definition with no word.
    pub(crate) fn switch_like(&self, b: usize) -> bool {
        match self.kind(b) {
            BoxKind::Switch => true,
            BoxKind::Node => self.word(b).is_none(),
            BoxKind::Definition => false,
        }
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

    /// Whether text `t` is an inline node's word: a word that a free
    /// arrowhead points at across spaces (SPEC §6).
    pub(crate) fn inline_word(&self, t: usize) -> bool {
        self.diagram.texts[t].kind == TextKind::Word && self.after_free_arrow(t)
    }

    /// The inline node's word that a free arrowhead at `p` points at.
    pub(crate) fn inline_after(&self, p: Pos) -> Option<usize> {
        if self.borders.contains(&p) || self.diagram.strokes.get(p)?.kind != Kind::Arrow(Dir::E) {
            return None;
        }
        let t = across(self.grid, &self.texts, p, Dir::E).1?;
        self.inline_word(t).then_some(t)
    }

    /// The words of the inline nodes wire `w` feeds and leaves: the node
    /// whose input it is, and the node whose output it is.
    pub(crate) fn inline_ends(&self, w: usize) -> (Option<usize>, Option<usize>) {
        let (mut into, mut from) = (None, None);
        for &term in &self.terms[w] {
            match term {
                Term::Arrow(p) => into = into.or(self.inline_after(p)),
                Term::Text(p, Dir::W) => from = from.or(self.text_at(p).filter(|&t| self.inline_word(t))),
                _ => {}
            }
        }
        (into, from)
    }

    /// The refusal for an edit that would leave the inline node whose word
    /// is text `t` without its input wire or its last output branch.
    pub(crate) fn strands(&self, at: Pos, t: usize) -> Refusal {
        let word = self.content(t);
        let why = format!(
            "the inline node {word} needs a wire in and a wire out; delete {word} first to join its wire up"
        );
        Refusal::new(at, RefusalKind::Target, why)
    }

    /// How many sinks wire `w` reaches: arrowheads, on a border or free.
    pub(crate) fn sink_count(&self, w: usize) -> usize {
        self.terms[w]
            .iter()
            .filter(|t| matches!(t, Term::Arrow(_)) || matches!(t, Term::Port(_, d) if *d != Dir::W))
            .count()
    }

    /// How many sources wire `w` leaves: more than one only for a merged
    /// output, whose stubs and run go with its last sink.
    pub(crate) fn source_count(&self, w: usize) -> usize {
        self.terms[w].iter().filter(|t| matches!(t, Term::Port(_, Dir::W) | Term::Text(_, Dir::W))).count()
    }

    /// Each arm's right through-port at the ordinal of switch `b`'s right
    /// through-port `at`, top to bottom: the ports of one merged output.
    pub(crate) fn merged(&self, b: usize, at: Pos) -> Vec<Pos> {
        let shape = &self.diagram.boxes[b];
        let right: Vec<Pos> = shape
            .ports
            .iter()
            .filter(|p| p.side == Dir::E && p.kind == PortKind::Arrow)
            .map(|p| p.pos)
            .collect();
        let arm = self.arm(b, at.row);
        let Some(k) = right.iter().filter(|p| self.arm(b, p.row) == arm).position(|&p| p == at) else {
            return vec![at];
        };
        (0..=shape.dividers.len())
            .filter_map(|a| right.iter().copied().filter(|p| self.arm(b, p.row) == a).nth(k))
            .collect()
    }

    /// The first cell of wire `w`'s source as [`Model::source`] names it:
    /// a merged output by its first arm's port, which sorts first.
    pub(crate) fn source_cell(&self, w: usize) -> Option<Pos> {
        self.terms[w].iter().find_map(|&term| {
            let cell = match term {
                Term::Port(p, Dir::W) | Term::Text(p, Dir::W) => p,
                _ => return None,
            };
            let source = self.source(cell).ok()?;
            let first = source.merged.first().copied().unwrap_or(cell);
            (source.term == term || source.merged.contains(&cell)).then_some(first)
        })
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
                // A merged output is named once, by its first arm's port.
                if let Ok(s) = self.source(p.pos)
                    && s.merged.first().is_none_or(|&first| first == p.pos)
                {
                    let wired = self.wire_of(s.term).is_some();
                    sources.push((p.pos, s, wired));
                }
                if let Ok(s) = self.sink(p.pos) {
                    sinks.push((s, self.wire_of(s.term()).is_some()));
                }
            }
        }
        for w in &self.diagram.wires {
            for e in w.ends.iter().filter(|e| e.kind == EndKind::Arrow) {
                if let Ok(s) = self.sink(e.pos) {
                    sinks.push((s, true));
                }
            }
        }
        for t in 0..self.diagram.texts.len() {
            if self.constant(t).is_some() || self.inline_word(t) {
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
            Term::Arrow(p) => (Dir::E, End::Arrow(Across::Text(self.inline_after(p)?))),
            Term::Loose(..) => return None,
        };
        Some(structure::Term { facing, end })
    }

}
