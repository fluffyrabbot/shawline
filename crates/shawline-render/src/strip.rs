//! The gloss strip: which words it glosses, and where each gloss goes.

use std::collections::HashMap;

use shawline_grid::repertoire::is_word_char;
use shawline_grid::{Class, Grid, Pos, classify};
use shawline_syntax::ast::{Graph, NodeKind};
use shawline_vm::bytecode::Prim;

use crate::Gloss;

/// Every Shavian word in row-major order. Placement is not done yet, so
/// every gloss is dropped.
pub(crate) fn glosses(grid: &Grid) -> Vec<Gloss> {
    let primitives = primitives(grid);
    let mut out = Vec::new();
    for row in 0..grid.height() {
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
            out.push(Gloss {
                at,
                reading: shawline_word::reading(&word),
                word,
                gloss: primitives.get(&at).map(|p| p.gloss()),
                span: None,
            });
        }
    }
    out
}

/// The primitive each primitive node's word names, by the word's cell.
fn primitives(grid: &Grid) -> HashMap<Pos, Prim> {
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
    let diagram = shawline_shape::recognize(grid);
    let (sheet, _) = shawline_syntax::parse(grid, &diagram);
    let mut out = HashMap::new();
    for def in &sheet.definitions {
        walk(&def.body, &mut out);
    }
    out
}
