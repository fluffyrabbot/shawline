//! The structure of a recognized sheet with every position left out: what
//! each box, text and wire is, and what it meets (PLAN P8, wave 2).
//!
//! Lead-owned: the one answer to "did this step change only what it meant
//! to?".
//!
//! - **A formatting step** must leave the structure equal, routing included
//!   ([`Routing::Kept`]).
//! - **A move, or inserting rows or columns,** must leave it equal once box
//!   and text indices are relabelled through a [`Mapping`], because no
//!   position is part of it. A move re-routes the stretches of wire that
//!   follow the box, so crossings may come and go ([`Routing::Rerouted`]).
//! - **A wiring edit** must leave the old structure plus the graph change it
//!   declares ([`Structure::connect`], [`Structure::remove_wire`],
//!   [`Structure::disconnect`], and a mapping that drops a removed or not
//!   yet placed node), however its wires run ([`Routing::Ignored`]).
//!
//! Syntax diagnostics aren't structure: a check that needs them compares
//! them beside it.

use std::collections::HashMap;

use shawline_grid::{Code, Dir, Grid, Pos, Rect};

use crate::{BoxShape, BoxStyle, Diagram, EndKind, PortKind, TextKind};

/// A sheet's structure. Its indices are `Diagram` indices.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Structure {
    /// In `Diagram::boxes` order.
    pub boxes: Vec<BoxPart>,
    /// In `Diagram::texts` order.
    pub texts: Vec<TextPart>,
    /// Sorted, since `Diagram::wires` is in order of position.
    pub wires: Vec<WirePart>,
    /// Shape diagnostics by code, sorted. `S012` and `S010` are left out:
    /// each box counts its own slipped cells, and each wire names its own
    /// blocked ends ([`End::Blocked`]).
    pub codes: Vec<Code>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BoxPart {
    pub style: BoxStyle,
    pub titled: bool,
    /// Slipped border cells, which only `fmt --fix` may move.
    pub slips: usize,
    pub parent: Option<usize>,
    /// In `BoxShape::ports` order, which is row-major.
    pub ports: Vec<PortPart>,
    /// One per divider, top to bottom: whether it has a guard gap.
    pub dividers: Vec<bool>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PortPart {
    pub side: Dir,
    pub kind: PortKind,
    pub dir: Dir,
    /// What an arrowhead points at across spaces; `Nothing` for a tee.
    pub meets: Across,
}

/// What lies across spaces from a cell, along its row (SPEC §6: text meets
/// wires only horizontally, across spaces).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Across {
    Text(usize),
    Stroke,
    Nothing,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextPart {
    pub kind: TextKind,
    pub content: String,
    pub owner: Option<usize>,
    /// The box whose title this text is.
    pub title_of: Option<usize>,
    /// The box one of whose dividers holds this text as a guard.
    pub divider_of: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct WirePart {
    /// Where the wire stops, sorted.
    pub terms: Vec<Term>,
    pub tees: usize,
    pub crossings: usize,
}

/// One place a wire stops, named by what it meets rather than where it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Term {
    /// The direction leading out of the wire there.
    pub facing: Dir,
    pub end: End,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum End {
    /// A box port, as box index and port index within the box.
    Port(usize, usize),
    /// A free arrowhead, and what it points at.
    Arrow(Across),
    /// A loose end, and what lies across spaces beyond it.
    Open(Across),
    /// A stroke that doesn't join back (`S010`).
    Blocked,
}

/// How far a step may change the way wires run, beyond what they connect.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Routing {
    /// Not at all: tee and crossing counts must match. Formatting, and
    /// inserting rows or columns.
    Kept,
    /// Crossings may come and go, tees may not: a move re-routes the
    /// stretches that follow the box and keeps the author's tees.
    Rerouted,
    /// Only what each wire connects counts: a wiring edit adds and removes
    /// tees and crossings.
    Ignored,
}

/// The first part in which a structure differs from what was expected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    Boxes,
    Texts,
    Wires,
    Codes,
}

/// Where each box and text of one diagram went in another: an index in the
/// other diagram, or `None` when it has none there. Distinct entries map
/// to distinct indices.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mapping {
    pub boxes: Vec<Option<usize>>,
    pub texts: Vec<Option<usize>>,
}

impl Mapping {
    /// Every box and text of `d` to itself.
    pub fn identity(d: &Diagram) -> Self {
        Self { boxes: (0..d.boxes.len()).map(Some).collect(), texts: (0..d.texts.len()).map(Some).collect() }
    }

    /// Box `i` of `from` to the box of `to` whose rectangle is `rect(i, r)`,
    /// where `r` is its rectangle in `from`; text `i` to the text of `to`
    /// that starts at `pos(i, p)`. `None` where nothing is there.
    pub fn by_place(
        from: &Diagram,
        to: &Diagram,
        rect: impl Fn(usize, Rect) -> Rect,
        pos: impl Fn(usize, Pos) -> Pos,
    ) -> Self {
        let boxes: HashMap<Rect, usize> = to.boxes.iter().enumerate().map(|(i, b)| (b.rect, i)).collect();
        let texts: HashMap<Pos, usize> = to.texts.iter().enumerate().map(|(i, t)| (t.pos, i)).collect();
        Self {
            boxes: from.boxes.iter().enumerate().map(|(i, b)| boxes.get(&rect(i, b.rect)).copied()).collect(),
            texts: from.texts.iter().enumerate().map(|(i, t)| texts.get(&pos(i, t.pos)).copied()).collect(),
        }
    }

    /// Whether every box and text has a place.
    pub fn is_total(&self) -> bool {
        self.boxes.iter().chain(&self.texts).all(Option::is_some)
    }
}

impl Structure {
    /// The structure of `diagram`, which must be `recognize(grid)`.
    pub fn of(grid: &Grid, diagram: &Diagram) -> Self {
        let text_at = text_cells(diagram);
        let across = |p: Pos, d: Dir| across(grid, diagram, &text_at, p, d);

        let boxes = diagram
            .boxes
            .iter()
            .map(|b| BoxPart {
                style: b.style,
                titled: b.title_gap.is_some(),
                slips: b.slips.len(),
                parent: b.parent,
                ports: b
                    .ports
                    .iter()
                    .map(|p| {
                        let meets = match p.kind {
                            PortKind::Arrow => across(p.pos, p.dir),
                            PortKind::Tee => Across::Nothing,
                        };
                        PortPart { side: p.side, kind: p.kind, dir: p.dir, meets }
                    })
                    .collect(),
                dividers: b.dividers.iter().map(|d| d.gap.is_some()).collect(),
            })
            .collect();

        let texts = diagram
            .texts
            .iter()
            .map(|t| TextPart {
                kind: t.kind,
                content: t.content.clone(),
                owner: t.owner,
                title_of: t.title_of,
                divider_of: t.divider_of,
            })
            .collect();

        let mut wires: Vec<WirePart> =
            (0..diagram.wires.len()).map(|w| wire_part(grid, diagram, &text_at, w)).collect();
        wires.sort();

        let mut codes: Vec<Code> =
            diagram.diagnostics.iter().map(|d| d.code).filter(|&c| c != "S012" && c != "S010").collect();
        codes.sort_unstable();
        Structure { boxes, texts, wires, codes }
    }

    /// `Ok` when `found` is what `self` expects, up to `routing`; otherwise
    /// the first part that differs, in the order boxes, texts, wires, codes.
    pub fn expect(&self, found: &Structure, routing: Routing) -> Result<(), Part> {
        if self.boxes != found.boxes {
            return Err(Part::Boxes);
        }
        if self.texts != found.texts {
            return Err(Part::Texts);
        }
        if routed(&self.wires, routing) != routed(&found.wires, routing) {
            return Err(Part::Wires);
        }
        if self.codes != found.codes {
            return Err(Part::Codes);
        }
        Ok(())
    }

    /// This structure in the indices `map` gives, as it should read after a
    /// step that keeps structure but moves things.
    ///
    /// Boxes and texts mapped to `None` are dropped. A reference to a
    /// dropped box becomes `None`, and what a port or wire end meets across
    /// a dropped text becomes [`Across::Nothing`]. A wire loses its terms at
    /// a dropped box's ports, and a wire left with fewer than two terms by
    /// that goes too, as when a node and its wires are removed.
    pub fn relabel(&self, map: &Mapping) -> Structure {
        let to_box = |i: usize| map.boxes.get(i).copied().flatten();
        let to_text = |i: usize| map.texts.get(i).copied().flatten();
        let across = |a: Across| match a {
            Across::Text(i) => to_text(i).map_or(Across::Nothing, Across::Text),
            other => other,
        };

        let mut boxes: Vec<(usize, BoxPart)> = self
            .boxes
            .iter()
            .enumerate()
            .filter_map(|(i, b)| {
                let ports = b.ports.iter().map(|p| PortPart { meets: across(p.meets), ..*p }).collect();
                Some((to_box(i)?, BoxPart { parent: b.parent.and_then(to_box), ports, ..b.clone() }))
            })
            .collect();
        boxes.sort_by_key(|&(i, _)| i);

        let mut texts: Vec<(usize, TextPart)> = self
            .texts
            .iter()
            .enumerate()
            .filter_map(|(i, t)| {
                let part = TextPart {
                    owner: t.owner.and_then(to_box),
                    title_of: t.title_of.and_then(to_box),
                    divider_of: t.divider_of.and_then(to_box),
                    ..t.clone()
                };
                Some((to_text(i)?, part))
            })
            .collect();
        texts.sort_by_key(|&(i, _)| i);

        let mut wires: Vec<WirePart> = self
            .wires
            .iter()
            .filter_map(|w| {
                let terms: Vec<Term> = w
                    .terms
                    .iter()
                    .filter_map(|t| {
                        let end = match t.end {
                            End::Port(b, k) => End::Port(to_box(b)?, k),
                            End::Arrow(a) => End::Arrow(across(a)),
                            End::Open(a) => End::Open(across(a)),
                            End::Blocked => End::Blocked,
                        };
                        Some(Term { facing: t.facing, end })
                    })
                    .collect();
                let cut = terms.len() < w.terms.len();
                (!cut || terms.len() >= 2).then(|| WirePart { terms, ..w.clone() })
            })
            .collect();
        for w in &mut wires {
            w.terms.sort();
        }
        wires.sort();

        Structure {
            boxes: boxes.into_iter().map(|(_, b)| b).collect(),
            texts: texts.into_iter().map(|(_, t)| t).collect(),
            wires,
            codes: self.codes.clone(),
        }
    }

    /// Declares a connection: `to` joins the wire with term `from`, or the
    /// two start a new wire. Tee and crossing counts are left as they were,
    /// for [`Routing::Ignored`].
    pub fn connect(&mut self, from: Term, to: Term) {
        match self.wires.iter_mut().find(|w| w.terms.contains(&from)) {
            Some(w) => w.terms.push(to),
            None => self.wires.push(WirePart { terms: vec![from, to], tees: 0, crossings: 0 }),
        }
        self.settle();
    }

    /// Declares that the wire with term `at` is removed, every branch of it.
    pub fn remove_wire(&mut self, at: Term) {
        self.wires.retain(|w| !w.terms.contains(&at));
    }

    /// Declares that one wire equal to `part` is removed: for a wire that
    /// no single term names, such as a stray stroke with a look-alike,
    /// whose terms are all loose ends. `part` is from [`Structure::wire`].
    pub fn remove_part(&mut self, part: &WirePart) {
        if let Some(i) = self.wires.iter().position(|w| w == part) {
            self.wires.remove(i);
        }
    }

    /// Wire `w` of `diagram` as [`Structure::of`] records it.
    pub fn wire(grid: &Grid, diagram: &Diagram, w: usize) -> WirePart {
        wire_part(grid, diagram, &text_cells(diagram), w)
    }

    /// Declares that the branch ending at term `at` is removed. A wire left
    /// with fewer than two terms goes entirely.
    pub fn disconnect(&mut self, at: Term) {
        for w in &mut self.wires {
            if let Some(k) = w.terms.iter().position(|&t| t == at) {
                w.terms.remove(k);
                if w.terms.len() < 2 {
                    w.terms.clear();
                }
            }
        }
        self.wires.retain(|w| !w.terms.is_empty());
        self.settle();
    }

    fn settle(&mut self) {
        for w in &mut self.wires {
            w.terms.sort();
        }
        self.wires.sort();
    }
}

/// The box and port index of the port at cell `p`.
pub fn port_index(boxes: &[BoxShape], p: Pos) -> Option<(usize, usize)> {
    boxes.iter().enumerate().find_map(|(b, shape)| shape.ports.iter().position(|q| q.pos == p).map(|k| (b, k)))
}

fn text_cells(diagram: &Diagram) -> HashMap<Pos, usize> {
    let mut text_at = HashMap::new();
    for (i, t) in diagram.texts.iter().enumerate() {
        text_at.extend(t.cells().map(|p| (p, i)));
    }
    text_at
}

fn wire_part(grid: &Grid, diagram: &Diagram, text_at: &HashMap<Pos, usize>, w: usize) -> WirePart {
    let wire = &diagram.wires[w];
    let mut terms: Vec<Term> = wire
        .ends
        .iter()
        .map(|e| {
            let end = match e.kind {
                EndKind::Port(p) => port_index(&diagram.boxes, p).map_or(End::Blocked, |(b, k)| End::Port(b, k)),
                EndKind::Arrow => End::Arrow(across(grid, diagram, text_at, e.pos, e.facing)),
                EndKind::Open => End::Open(across(grid, diagram, text_at, e.pos, e.facing)),
                EndKind::Blocked => End::Blocked,
            };
            Term { facing: e.facing, end }
        })
        .collect();
    terms.sort();
    WirePart { terms, tees: wire.tees.len(), crossings: wire.crossings.len() }
}

/// Each wire's terms, with as much of its routing as `routing` keeps,
/// sorted.
fn routed(wires: &[WirePart], routing: Routing) -> Vec<(&[Term], usize, usize)> {
    let mut out: Vec<(&[Term], usize, usize)> = wires
        .iter()
        .map(|w| match routing {
            Routing::Kept => (&w.terms[..], w.tees, w.crossings),
            Routing::Rerouted => (&w.terms[..], w.tees, 0),
            Routing::Ignored => (&w.terms[..], 0, 0),
        })
        .collect();
    out.sort();
    out
}

fn across(grid: &Grid, diagram: &Diagram, text_at: &HashMap<Pos, usize>, from: Pos, dir: Dir) -> Across {
    if !dir.is_horizontal() {
        return Across::Nothing;
    }
    let row_len = grid.row(from.row).len();
    let mut p = from;
    loop {
        p = match p.step(dir) {
            Some(q) if q.col < row_len => q,
            _ => return Across::Nothing,
        };
        if grid.get(p) == ' ' {
            continue;
        }
        if let Some(&i) = text_at.get(&p) {
            return Across::Text(i);
        }
        if diagram.strokes.get(p).is_some() {
            return Across::Stroke;
        }
        return Across::Nothing;
    }
}

#[cfg(test)]
mod tests {
    use shawline_grid::Grid;

    use super::*;
    use crate::recognize;

    const DOUBLE: &str = include_str!("../../../corpus/03-double.shaw");

    fn read(src: &str) -> (Grid, Diagram) {
        let grid = Grid::parse(src).unwrap();
        let diagram = recognize(&grid);
        (grid, diagram)
    }

    fn structure(src: &str) -> Structure {
        let (grid, diagram) = read(src);
        Structure::of(&grid, &diagram)
    }

    /// `src` with each `(old, new)` text replaced, checked to change it.
    fn edit(src: &str, rows: &[(&str, &str)]) -> String {
        let mut out = src.to_owned();
        for (old, new) in rows {
            let next = out.replacen(old, new, 1);
            assert_ne!(next, out, "{old} not found");
            out = next;
        }
        out
    }

    /// The term of the port at cell `p`, met facing `facing`.
    fn port(d: &Diagram, p: Pos, facing: Dir) -> Term {
        let (b, k) = port_index(&d.boxes, p).unwrap();
        Term { facing, end: End::Port(b, k) }
    }

    const NODE: [&str; 4] = [
        "│         ┌───┐       │",
        "▶ 𐑯 ──┬───▶ 𐑐 ├───────▶ 𐑬",
        "│     └───▶   │       │",
        "│         └───┘       │",
    ];

    #[test]
    fn positions_are_not_structure() {
        let a = structure(DOUBLE);
        // A constant moved along its row, with its wire one cell shorter.
        let nearer = edit(DOUBLE, &[("│ 21 ───▶", "│  21 ──▶")]);
        assert_eq!(a.expect(&structure(&nearer), Routing::Kept), Ok(()), "{nearer}");
        // Everything one row lower, and a comment moved right.
        let lower = edit(DOUBLE, &[("※ 𐑛𐑳𐑚𐑩𐑤\n", "\n  ※ 𐑛𐑳𐑚𐑩𐑤")]);
        assert_eq!(a.expect(&structure(&lower), Routing::Kept), Ok(()), "{lower}");
    }

    #[test]
    fn what_a_wire_meets_is_structure() {
        // 𐑐's output wire stops short of the definition's output.
        let short = edit(DOUBLE, &[(NODE[1], "▶ 𐑯 ──┬───▶ 𐑐 ├────   ▶ 𐑬")]);
        assert_eq!(structure(DOUBLE).expect(&structure(&short), Routing::Ignored), Err(Part::Wires));
    }

    #[test]
    fn a_box_style_is_structure() {
        let mixed = DOUBLE.replacen('╭', "┌", 1);
        assert_eq!(structure(DOUBLE).expect(&structure(&mixed), Routing::Kept), Err(Part::Boxes));
    }

    #[test]
    fn removing_a_wire_is_declared_by_any_of_its_terms() {
        let (grid, d) = read(DOUBLE);
        let mut want = Structure::of(&grid, &d);
        // The wire leaving 𐑐's output tee faces west into it.
        want.remove_wire(port(&d, Pos::new(5, 14), Dir::W));
        let gone = edit(DOUBLE, &[(NODE[1], "▶ 𐑯 ──┬───▶ 𐑐 ├       ▶ 𐑬")]);
        let found = structure(&gone);
        assert_eq!(want.expect(&found, Routing::Ignored), Ok(()), "{gone}");
        assert_eq!(structure(DOUBLE).expect(&found, Routing::Ignored), Err(Part::Wires));
    }

    #[test]
    fn a_branch_is_declared_by_its_sink() {
        let (grid, d) = read(DOUBLE);
        let whole = Structure::of(&grid, &d);
        // 𐑯's fan-out loses the branch into 𐑐's second input.
        let sink = port(&d, Pos::new(6, 10), Dir::E);
        let mut want = whole.clone();
        want.disconnect(sink);
        let one = edit(DOUBLE, &[(NODE[1], "▶ 𐑯 ──────▶ 𐑐 ├───────▶ 𐑬"), (NODE[2], "│         ▶   │       │")]);
        let found = structure(&one);
        assert_eq!(want.expect(&found, Routing::Ignored), Ok(()), "{one}");
        // Its tee became a plain stroke, which only routing notices.
        assert_eq!(want.expect(&found, Routing::Kept), Err(Part::Wires));
        // Connecting it again from the wire's source gives the whole back.
        let fan = whole.wires.iter().find(|w| w.terms.contains(&sink)).unwrap();
        let from = *fan.terms.iter().find(|t| t.facing == Dir::W).unwrap();
        want.connect(from, sink);
        assert_eq!(want.expect(&whole, Routing::Ignored), Ok(()));
    }

    #[test]
    fn a_move_keeps_structure_once_relabelled() {
        // main's node, the constant feeding it and the output it feeds, one
        // row lower.
        let moved = edit(
            DOUBLE,
            &[(
                "│       ┌───────┐     │\n│ 21 ───▶ 𐑛𐑳𐑚𐑩𐑤 ├─────▶ 𐑬\n│       └───────┘     │\n│                     │",
                "│                     │\n│       ┌───────┐     │\n│ 21 ───▶ 𐑛𐑳𐑚𐑩𐑤 ├─────▶ 𐑬\n│       └───────┘     │",
            )],
        );
        let ((og, od), (ng, nd)) = (read(DOUBLE), read(&moved));
        let node = od.boxes.iter().position(|b| b.rect.top == 13).unwrap();
        let down = |p: Pos| Pos::new(p.row + 1, p.col);
        let rect = od.boxes[node].rect;
        let map = Mapping::by_place(
            &od,
            &nd,
            |i, r| if i == node { Rect { top: r.top + 1, bottom: r.bottom + 1, ..r } } else { r },
            |_, p| if rect.contains(p) || p.row == 14 { down(p) } else { p },
        );
        assert!(map.is_total(), "{map:?}");
        let want = Structure::of(&og, &od).relabel(&map);
        assert_eq!(want.expect(&Structure::of(&ng, &nd), Routing::Rerouted), Ok(()));
        // Without the mapping, the same text still matches: nothing reordered.
        assert_eq!(Structure::of(&og, &od).expect(&Structure::of(&ng, &nd), Routing::Kept), Ok(()));
    }

    #[test]
    fn relabelling_permutes_every_reference() {
        let s = structure(DOUBLE);
        let reverse = |n: usize| (0..n).rev().map(Some).collect::<Vec<_>>();
        let map = Mapping { boxes: reverse(s.boxes.len()), texts: reverse(s.texts.len()) };
        let turned = s.relabel(&map);
        assert_ne!(turned, s);
        // The first box comes last, and the boxes inside it point at it.
        let last = s.boxes.len() - 1;
        assert_eq!(turned.boxes[last].ports.len(), s.boxes[0].ports.len());
        let inside: Vec<usize> = (0..s.boxes.len()).filter(|&b| s.boxes[b].parent == Some(0)).collect();
        assert!(!inside.is_empty());
        assert!(inside.iter().all(|&b| turned.boxes[last - b].parent == Some(last)));
        assert_eq!(turned.relabel(&map), s);
    }

    #[test]
    fn removing_a_node_is_dropping_it_and_its_output() {
        let (grid, d) = read(DOUBLE);
        let gone = edit(
            DOUBLE,
            &[
                (NODE[0], "│                     │"),
                (NODE[1], "▶ 𐑯                   ▶ 𐑬"),
                (NODE[2], "│                     │"),
                (NODE[3], "│                     │"),
            ],
        );
        let (ng, nd) = read(&gone);
        // Nothing else moved, so places match: the node and its word don't.
        let map = Mapping::by_place(&d, &nd, |_, r| r, |_, p| p);
        assert_eq!(map.boxes.iter().filter(|b| b.is_none()).count(), 1);
        assert_eq!(map.texts.iter().filter(|t| t.is_none()).count(), 1);
        let mut want = Structure::of(&grid, &d);
        want.remove_wire(port(&d, Pos::new(5, 14), Dir::W));
        // The fan-out into its inputs is cut to one end, so it goes too.
        let want = want.relabel(&map);
        assert_eq!(want.expect(&Structure::of(&ng, &nd), Routing::Ignored), Ok(()), "{gone}");
    }

    /// Stray strokes: a blocked end, loose ends across strokes and across
    /// nothing, and a look-alike pair.
    const STRAYS: &str = "\
╭─ ·𐑛𐑳𐑚𐑩𐑤 ──────────╮
│                   │
▶ 𐑯 ────────────────▶ 𐑬
│                   │
│  ───   ───│       │
│                   │
╰───────────────────╯
  ───
";

    #[test]
    fn each_wire_reads_as_the_whole_does() {
        let sheets = [
            DOUBLE,
            include_str!("../../../corpus/04-abs.shaw"),
            include_str!("../../../corpus/13-fanout.shaw"),
            include_str!("../../../corpus/15-fanout-cross.shaw"),
            STRAYS,
        ];
        let mut seen = std::collections::HashSet::new();
        for src in sheets {
            let (grid, d) = read(src);
            let mut parts: Vec<WirePart> = (0..d.wires.len()).map(|w| Structure::wire(&grid, &d, w)).collect();
            parts.sort();
            assert_eq!(parts, Structure::of(&grid, &d).wires, "{src}");
            seen.extend(parts.iter().flat_map(|p| &p.terms).map(|t| match t.end {
                End::Port(..) => End::Port(0, 0),
                End::Arrow(_) => End::Arrow(Across::Nothing),
                End::Open(Across::Text(_)) => End::Open(Across::Text(0)),
                other => other,
            }));
        }
        // Every kind of end occurs, so every branch of the reading is compared.
        let kinds = [
            End::Port(0, 0),
            End::Arrow(Across::Nothing),
            End::Open(Across::Text(0)),
            End::Open(Across::Stroke),
            End::Open(Across::Nothing),
            End::Blocked,
        ];
        assert!(kinds.iter().all(|k| seen.contains(k)), "{seen:?}");
    }

    #[test]
    fn a_look_alike_is_removed_one_at_a_time() {
        let pair = edit(STRAYS, &[("│  ───   ───│       │", "│  ───       ───    │")]);
        let (grid, d) = read(&pair);
        let w = d.wires.iter().position(|w| w.cells.contains(&Pos::new(4, 3))).unwrap();
        let part = Structure::wire(&grid, &d, w);
        let mut s = Structure::of(&grid, &d);
        assert_eq!(s.wires.iter().filter(|p| **p == part).count(), 2);
        // No term names just one of them: removing by term takes both.
        let mut by_term = s.clone();
        by_term.remove_wire(part.terms[0]);
        assert_eq!(by_term.wires.iter().filter(|p| **p == part).count(), 0);
        s.remove_part(&part);
        assert_eq!(s.wires.iter().filter(|p| **p == part).count(), 1);
    }

    #[test]
    fn a_blocked_end_is_a_term_not_a_code() {
        let (grid, d) = read(STRAYS);
        assert!(d.diagnostics.iter().any(|x| x.code == "S010"), "{:?}", d.diagnostics);
        let s = Structure::of(&grid, &d);
        assert!(!s.codes.contains(&"S010"));
        assert!(s.wires.iter().flat_map(|w| &w.terms).any(|t| t.end == End::Blocked));
    }

    #[test]
    fn a_wire_crossing_itself_is_one_wire() {
        let s = structure(include_str!("../../../corpus/15-fanout-cross.shaw"));
        let crossing: Vec<&WirePart> = s.wires.iter().filter(|w| w.crossings > 0).collect();
        assert_eq!(crossing.len(), 1, "{:?}", s.wires);
        assert!(crossing[0].terms.len() >= 3, "{:?}", crossing[0]);
    }
}
