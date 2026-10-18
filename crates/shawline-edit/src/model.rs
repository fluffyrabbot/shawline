//! The sheet as Studio's wiring edits see it (PLAN P7): what kind each box
//! is, which cells are sources and sinks, and a position-free print of the
//! sheet that an edit may change only as it intends.
//!
//! Everything is read off the recognized [`Diagram`], not the syntax tree,
//! because edits run on half-built sheets: a constant whose wire was just
//! removed is stray text to the parser, but still a source here.

use std::collections::{HashMap, HashSet};

use shawline_grid::{Dir, Grid, Pos, Rect};
use shawline_shape::stroke::{self, Kind};
use shawline_shape::{BoxStyle, Diagram, EndKind, Port, PortKind, TextKind};

use crate::Refusal;
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
                    None => Err(Refusal::new(at, "this input has no name yet; type one first")),
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
                    "a switch's merged output can't be wired by mouse yet; draw it in the text",
                )),
                _ => Err(Refusal::new(at, "this arrowhead takes a wire in; drag from a source")),
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
                return Err(Refusal::new(
                    at,
                    "wires at an inline node can't be edited by mouse yet; edit the text",
                ));
            }
            return Err(Refusal::new(at, format!("{} can't feed a wire", self.content(t))));
        }
        Err(Refusal::new(
            at,
            "nothing here feeds a wire: start at an output tee, an input, a constant or an arm's input",
        ))
    }

    /// The sink named by cell `at`: an arrowhead on a box border.
    pub(crate) fn sink(&self, at: Pos) -> Result<Sink, Refusal> {
        let not = || {
            Refusal::new(at, "wires end at arrowheads: a node's input, an output or a switch port")
        };
        let (b, port) = self.port(at).ok_or_else(not)?;
        if port.kind != PortKind::Arrow {
            return Err(not());
        }
        let row = at.row;
        let scope = match (self.kind(b), port.side, port.dir) {
            (BoxKind::Node, Dir::W, Dir::E) => self.outside(b, row),
            (BoxKind::Definition, Dir::E, Dir::E) => Some(self.inside(b, row)),
            (BoxKind::Definition, Dir::W, _) => {
                return Err(Refusal::new(at, "this is an input: wires start here, they don't end"));
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
    /// touches an inline node, or carries a switch's merged output.
    pub(crate) fn editable(&self, w: usize) -> Result<(), &'static str> {
        for &term in &self.terms[w] {
            match term {
                Term::Arrow(_) => {
                    return Err("wires at an inline node can't be edited by mouse yet; edit the text");
                }
                Term::Text(p, Dir::W) if self.text_at(p).is_some_and(|t| self.after_free_arrow(t)) => {
                    return Err("wires at an inline node can't be edited by mouse yet; edit the text");
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

    /// The wire cells' own connections, as drawn. A crossing belongs to two
    /// wires; each owns one axis of it.
    pub(crate) fn own_mask(&self, w: usize, p: Pos) -> u8 {
        let Some(s) = self.diagram.strokes.get(p) else { return 0 };
        if s.kind != Kind::Crossing {
            return s.conn;
        }
        let cells = &self.diagram.wires[w].cells;
        let horizontal = [Dir::E, Dir::W].into_iter().any(|d| {
            p.step(d).is_some_and(|q| {
                cells.contains(&q)
                    && self.diagram.strokes.get(q).is_some_and(|s| s.connects(d.opposite()))
            })
        });
        if horizontal { stroke::E | stroke::W } else { stroke::N | stroke::S }
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
}

/// A box as an edit must keep it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct BoxPrint {
    pub rect: (usize, usize, usize, usize),
    pub style: BoxStyle,
    pub title_gap: Option<(usize, usize)>,
    pub ports: Vec<(Pos, Dir, bool, Dir)>,
    pub dividers: Vec<(usize, Option<(usize, usize)>)>,
    pub slips: usize,
    pub parent: Option<(usize, usize, usize, usize)>,
}

/// A text as an edit must keep it.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct TextPrint {
    pub pos: Pos,
    pub width: usize,
    pub kind: u8,
    pub content: String,
    pub owner: Option<(usize, usize, usize, usize)>,
    pub title: bool,
    pub divider: bool,
}

/// The whole sheet, position-free where wires are concerned.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Print {
    pub boxes: Vec<BoxPrint>,
    pub texts: Vec<TextPrint>,
    /// Each wire's sorted terms, the list sorted.
    pub wires: Vec<Vec<Term>>,
}

pub(crate) fn key(r: Rect) -> (usize, usize, usize, usize) {
    (r.top, r.left, r.bottom, r.right)
}

pub(crate) fn text_kind(kind: TextKind) -> u8 {
    match kind {
        TextKind::Word => 0,
        TextKind::Number => 1,
        TextKind::Literal => 2,
        TextKind::Comment => 3,
    }
}

impl Print {
    pub(crate) fn of(model: &Model) -> Self {
        let d = model.diagram;
        let mut boxes: Vec<BoxPrint> = d
            .boxes
            .iter()
            .map(|b| {
                let mut ports: Vec<_> = b
                    .ports
                    .iter()
                    .map(|p| (p.pos, p.side, p.kind == PortKind::Arrow, p.dir))
                    .collect();
                ports.sort();
                BoxPrint {
                    rect: key(b.rect),
                    style: b.style,
                    title_gap: b.title_gap,
                    ports,
                    dividers: b.dividers.iter().map(|v| (v.row, v.gap)).collect(),
                    slips: b.slips.len(),
                    parent: b.parent.map(|p| key(d.boxes[p].rect)),
                }
            })
            .collect();
        boxes.sort_by_key(|b| b.rect);
        let mut texts: Vec<TextPrint> = d
            .texts
            .iter()
            .map(|t| TextPrint {
                pos: t.pos,
                width: t.width,
                kind: text_kind(t.kind),
                content: t.content.clone(),
                owner: t.owner.map(|o| key(d.boxes[o].rect)),
                title: t.title_of.is_some(),
                divider: t.divider_of.is_some(),
            })
            .collect();
        texts.sort();
        let mut wires = model.terms.clone();
        wires.sort();
        Print { boxes, texts, wires }
    }

    pub(crate) fn sort(&mut self) {
        self.boxes.sort_by_key(|b| b.rect);
        self.texts.sort();
        for w in &mut self.wires {
            w.sort();
        }
        self.wires.retain(|w| !w.is_empty());
        self.wires.sort();
    }

    /// Takes `term` off its wire; a wire left with one end goes entirely.
    pub(crate) fn cut(&mut self, term: Term) {
        for w in &mut self.wires {
            w.retain(|&t| t != term);
            if w.len() < 2 {
                w.clear();
            }
        }
    }

    /// Removes the wire with an end meeting `term`.
    pub(crate) fn drop_wire(&mut self, term: Term) {
        self.wires.retain(|w| !w.contains(&term));
    }

    /// `Ok` if `self` (what the edit meant) equals `found` (what the result
    /// recognizes to); otherwise what differs.
    pub(crate) fn expect(&self, found: &Print) -> Result<(), &'static str> {
        if self.boxes != found.boxes {
            return Err("it would change a box");
        }
        if self.texts != found.texts {
            return Err("it would change the text on the sheet");
        }
        if self.wires != found.wires {
            return Err("it would change what another wire connects");
        }
        Ok(())
    }
}
