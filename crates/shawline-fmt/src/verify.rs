//! The safety net: a structural fingerprint that every formatting step must
//! leave unchanged.
//!
//! It records what connects to what and ignores where things sit, so a step
//! may move text along its row, lengthen a wire or redraw a stroke in its
//! canonical character, but may not change which port, word or arrowhead any
//! wire end meets, a box's style or slipped cells, or the syntax errors.

use std::collections::HashMap;

use shawline_grid::{Code, Dir, Grid, Pos};
use shawline_shape::{BoxShape, BoxStyle, Diagram, EndKind, PortKind, TextKind};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Fingerprint {
    boxes: Vec<BoxPrint>,
    texts: Vec<TextPrint>,
    /// Sorted, because wire order follows cell positions.
    wires: Vec<WirePrint>,
    /// Shape diagnostics by code, sorted. `S012` is left out: each box
    /// counts its own slips.
    codes: Vec<Code>,
    /// Syntax diagnostics by code, sorted.
    syntax: Vec<Code>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct BoxPrint {
    style: BoxStyle,
    /// Slipped border cells: only `fix` may move them.
    slips: usize,
    parent: Option<usize>,
    titled: bool,
    ports: Vec<(Dir, PortKind, Dir, Target)>,
    /// One entry per divider: whether it has a guard gap.
    dividers: Vec<bool>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct TextPrint {
    kind: TextKind,
    content: String,
    owner: Option<usize>,
    title_of: Option<usize>,
    divider_of: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct WirePrint {
    ends: Vec<(Dir, EndPrint)>,
    tees: usize,
    crossings: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum EndPrint {
    /// Box index and port index within it.
    Port(usize, usize),
    Arrow(Target),
    Open(Target),
    Blocked,
}

/// What lies across spaces from a cell, on its row (SPEC §6: text meets
/// wires only horizontally, across spaces).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Target {
    Text(usize),
    Stroke,
    Nothing,
}

impl Fingerprint {
    pub(crate) fn of(grid: &Grid, diagram: &Diagram) -> Self {
        let mut text_at = HashMap::new();
        for (i, t) in diagram.texts.iter().enumerate() {
            for col in t.pos.col..t.pos.col + t.width {
                text_at.insert(Pos::new(t.pos.row, col), i);
            }
        }
        let target = |p: Pos, d: Dir| target(grid, diagram, &text_at, p, d);

        let boxes = diagram
            .boxes
            .iter()
            .map(|b| BoxPrint {
                style: b.style,
                slips: b.slips.len(),
                parent: b.parent,
                titled: b.title_gap.is_some(),
                ports: b
                    .ports
                    .iter()
                    .map(|p| {
                        let t = match p.kind {
                            PortKind::Arrow => target(p.pos, p.dir),
                            PortKind::Tee => Target::Nothing,
                        };
                        (p.side, p.kind, p.dir, t)
                    })
                    .collect(),
                dividers: b.dividers.iter().map(|d| d.gap.is_some()).collect(),
            })
            .collect();

        let texts = diagram
            .texts
            .iter()
            .map(|t| TextPrint {
                kind: t.kind,
                content: t.content.clone(),
                owner: t.owner,
                title_of: t.title_of,
                divider_of: t.divider_of,
            })
            .collect();

        let mut wires: Vec<WirePrint> = diagram
            .wires
            .iter()
            .map(|w| {
                let mut ends: Vec<(Dir, EndPrint)> = w
                    .ends
                    .iter()
                    .map(|e| {
                        let print = match e.kind {
                            EndKind::Port(p) => port_index(&diagram.boxes, p)
                                .map_or(EndPrint::Blocked, |(b, i)| EndPrint::Port(b, i)),
                            EndKind::Arrow => EndPrint::Arrow(target(e.pos, e.facing)),
                            EndKind::Open => EndPrint::Open(target(e.pos, e.facing)),
                            EndKind::Blocked => EndPrint::Blocked,
                        };
                        (e.facing, print)
                    })
                    .collect();
                ends.sort();
                WirePrint { ends, tees: w.tees.len(), crossings: w.crossings.len() }
            })
            .collect();
        wires.sort();

        let mut codes: Vec<Code> =
            diagram.diagnostics.iter().map(|d| d.code).filter(|&c| c != "S012").collect();
        codes.sort_unstable();
        let mut syntax: Vec<Code> =
            shawline_syntax::parse(grid, diagram).1.iter().map(|d| d.code).collect();
        syntax.sort_unstable();
        Fingerprint { boxes, texts, wires, codes, syntax }
    }

    /// The fingerprint once box `i`'s slipped cells are back in place.
    pub(crate) fn repaired(&self, i: usize) -> Self {
        let mut want = self.clone();
        if let Some(b) = want.boxes.get_mut(i) {
            b.slips = 0;
        }
        want
    }
}

/// The first box port at `p`, as (box index, port index).
pub(crate) fn port_index(boxes: &[BoxShape], p: Pos) -> Option<(usize, usize)> {
    boxes
        .iter()
        .enumerate()
        .find_map(|(b, shape)| shape.ports.iter().position(|q| q.pos == p).map(|i| (b, i)))
}

fn target(
    grid: &Grid,
    diagram: &Diagram,
    text_at: &HashMap<Pos, usize>,
    from: Pos,
    dir: Dir,
) -> Target {
    if !dir.is_horizontal() {
        return Target::Nothing;
    }
    let row_len = grid.row(from.row).len();
    let mut p = from;
    loop {
        p = match p.step(dir) {
            Some(q) if q.col < row_len => q,
            _ => return Target::Nothing,
        };
        if grid.get(p) == ' ' {
            continue;
        }
        if let Some(&i) = text_at.get(&p) {
            return Target::Text(i);
        }
        if diagram.strokes.get(p).is_some() {
            return Target::Stroke;
        }
        return Target::Nothing;
    }
}
