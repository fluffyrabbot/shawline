//! The gloss strip: which words it glosses, where each gloss goes, and how
//! vertical strokes cross it.

use std::collections::HashMap;

use kurbo::{Affine, BezPath, Point};
use shawline_glyph::{CELL_HEIGHT, CELL_WIDTH, GLOSS_HEIGHT, GLOSS_WIDTH, gloss_skeleton};
use shawline_grid::repertoire::is_word_char;
use shawline_grid::{Class, Dir, Grid, Pos, classify};
use shawline_shape::Diagram;
use shawline_syntax::ast::{Graph, NodeKind};
use shawline_vm::bytecode::Prim;

use crate::cells::{Fill, Ink, Stroke, ends};
use crate::chain::{Key, key};
use crate::{GLOSS_SCALE, Gloss, STRIP};

/// One gloss character's advance, in columns.
const ADVANCE: f64 = GLOSS_SCALE * GLOSS_WIDTH / CELL_WIDTH;
/// How far a gloss glyph's box sits below its strip's top: centred. Shavian
/// ink stops 74 units short of both edges of a cell (the ascender and the
/// descender are each 110 from an edge, less half a stroke), and the gloss
/// guides are symmetric too, so a centred gloss is as clear of the word it
/// glosses as of the row below: 93 units each way at the defaults. Height
/// the strip gains, or the gloss loses, at tuning is split evenly, so a
/// gloss never presses against its own word's descenders.
const DROP: f64 = (STRIP - GLOSS_SCALE * GLOSS_HEIGHT) / 2.0;
/// Slack when spacing glosses, so a tuned scale whose advance isn't exact in
/// binary doesn't drop a gloss that fits exactly. It is tighter than
/// `gloss_clear`'s, so nothing shown here fails there.
const SLACK: f64 = 1e-10;

/// Every Shavian word in row-major order, with where the strip under its row
/// shows its gloss (see [`Gloss::span`]).
pub(crate) fn glosses(grid: &Grid, diagram: &Diagram, blocked: &[Vec<bool>]) -> Vec<Gloss> {
    let primitives = primitives(grid, diagram);
    let mut out = Vec::new();
    for (row, columns) in blocked.iter().enumerate() {
        let mut strip = Strip { blocked: columns, shown: Vec::new() };
        let cells = grid.row(row);
        let mut col = 0;
        while col < cells.len() {
            if !is_word_char(cells[col]) {
                col += 1;
                continue;
            }
            let start = col;
            while col < cells.len() && is_word_char(cells[col]) {
                col += 1;
            }
            let word: String = cells[start..col].iter().collect();
            if word.chars().all(|c| classify(c) == Some(Class::Digit)) {
                continue;
            }
            let at = Pos::new(row, start);
            let mut gloss = Gloss {
                at,
                reading: shawline_word::reading(&word),
                word,
                gloss: primitives.get(&at).map(|p| p.gloss()),
                span: None,
            };
            gloss.span = strip.place(gloss.text(), (start + col) as f64 / 2.0);
            out.push(gloss);
        }
    }
    out
}

/// The strip under one row, as glosses are placed in it left to right.
struct Strip<'a> {
    /// Whether a vertical stroke crosses each column.
    blocked: &'a [bool],
    /// The spans shown so far.
    shown: Vec<(f64, f64)>,
}

impl Strip<'_> {
    /// Shows `text` centred on column `centre` if it fits, and returns where.
    fn place(&mut self, text: &str, centre: f64) -> Option<(f64, f64)> {
        let n = text.chars().count();
        if n == 0 || !text.chars().all(|c| gloss_skeleton(c).is_some()) {
            return None;
        }
        let half = n as f64 * ADVANCE / 2.0;
        let (left, right) = (centre - half, centre + half);
        if left < 0.0 || right > self.blocked.len() as f64 {
            return None;
        }
        // Every column the span touches, counted as `gloss_clear` counts them.
        let mut touched = (left.floor() as usize..).take_while(|&c| (c as f64) < right);
        if touched.any(|c| self.blocked[c]) {
            return None;
        }
        let crowded = self.shown.iter().any(|&(l, r)| {
            let gap = if l < left { left - r } else { l - right };
            gap < ADVANCE - SLACK
        });
        if crowded {
            return None;
        }
        self.shown.push((left, right));
        Some((left, right))
    }
}

/// For the strip under each row, whether a vertical stroke crosses each
/// column: a box's side runs through it, or the cell above connects south or
/// the cell below connects north. Port arrowheads stacked on a side connect
/// only across it, so the cells alone would miss the side between them.
pub(crate) fn blocked(grid: &Grid, diagram: &Diagram) -> Vec<Vec<bool>> {
    let connects = |p: Pos, d: Dir| diagram.strokes.get(p).is_some_and(|s| s.connects(d));
    let mut out: Vec<Vec<bool>> = (0..grid.height())
        .map(|row| {
            (0..grid.width())
                .map(|col| {
                    connects(Pos::new(row, col), Dir::S) || connects(Pos::new(row + 1, col), Dir::N)
                })
                .collect()
        })
        .collect();
    for b in &diagram.boxes {
        let r = b.rect;
        for row in r.top..r.bottom {
            for col in [r.left, r.right] {
                if let Some(cell) = out.get_mut(row).and_then(|cols| cols.get_mut(col)) {
                    *cell = true;
                }
            }
        }
    }
    out
}

/// Lines through the strips wherever the plain drawing joins strokes across
/// a row boundary: a stroke end on the middle of a cell's bottom edge, and
/// one of the same ink on the top edge of the cell below. Chaining then makes
/// each box side and wire one unbroken line. A stroke that reaches a strip
/// from one side only keeps its free end, as in the plain drawing. Only
/// blocked columns are bridged, so no gloss is ever drawn across one.
pub(crate) fn bridges(strokes: &[Stroke], blocked: &[Vec<bool>], pitch: f64) -> Vec<Stroke> {
    let mut count: HashMap<Key, usize> = HashMap::new();
    for s in strokes {
        let (a, b) = ends(&s.path);
        for (p, on_edge) in [a, b].into_iter().zip(s.on_edge) {
            if let (Some(p), true) = (p, on_edge) {
                *count.entry(key(p, s.ink)).or_default() += 1;
            }
        }
    }
    let one = |p: Point, ink: Ink| count.get(&key(p, ink)) == Some(&1);
    let mut out = Vec::new();
    for (row, cols) in blocked.iter().enumerate() {
        for (col, _) in cols.iter().enumerate().filter(|&(_, &b)| b) {
            let x = (col as f64 + 0.5) * CELL_WIDTH;
            let top = Point::new(x, row as f64 * pitch + CELL_HEIGHT);
            let bottom = Point::new(x, (row + 1) as f64 * pitch);
            for ink in Ink::ALL.into_iter().filter(|&ink| one(top, ink) && one(bottom, ink)) {
                let mut path = BezPath::new();
                path.move_to(top);
                path.line_to(bottom);
                out.push(Stroke { path, ink, on_edge: [true, true] });
            }
        }
    }
    out
}

/// Every shown gloss drawn in its strip with the gloss skeletons, in the
/// comment ink. Gloss strokes never join anything.
pub(crate) fn pieces(glosses: &[Gloss], pitch: f64) -> (Vec<Stroke>, Vec<Fill>) {
    let (mut strokes, mut fills) = (Vec::new(), Vec::new());
    for g in glosses {
        let Some((left, _)) = g.span else { continue };
        let y = g.at.row as f64 * pitch + CELL_HEIGHT + DROP;
        for (i, ch) in g.text().chars().enumerate() {
            let Some(s) = gloss_skeleton(ch) else { continue };
            let x = (left + i as f64 * ADVANCE) * CELL_WIDTH;
            let s = s.transformed(Affine::translate((x, y)) * Affine::scale(GLOSS_SCALE));
            strokes.extend(s.strokes.into_iter().map(|path| Stroke {
                path,
                ink: Ink::Comment,
                on_edge: [false; 2],
            }));
            fills.extend(s.fills.into_iter().map(|path| Fill { path, ink: Ink::Comment }));
        }
    }
    (strokes, fills)
}

/// The primitive each primitive node's word names, by the word's cell.
fn primitives(grid: &Grid, diagram: &Diagram) -> HashMap<Pos, Prim> {
    fn walk(graph: &Graph, out: &mut HashMap<Pos, Prim>) {
        for node in &graph.nodes {
            match &node.kind {
                NodeKind::Call(name) | NodeKind::Inline(name) => {
                    let mut letters = name.text.chars();
                    if let (Some(c), None) = (letters.next(), letters.next())
                        && let Some(p) = Prim::from_letter(c)
                    {
                        out.insert(name.at, p);
                    }
                }
                NodeKind::Switch(switch) => {
                    for arm in &switch.arms {
                        walk(&arm.body, out);
                    }
                }
                NodeKind::Const(_) => {}
            }
        }
    }
    let (sheet, _) = shawline_syntax::parse(grid, diagram);
    let mut out = HashMap::new();
    for def in &sheet.definitions {
        walk(&def.body, &mut out);
    }
    out
}
