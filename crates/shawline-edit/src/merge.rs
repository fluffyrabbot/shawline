//! A switch's merged output (PLAN P10, "Switch outputs"; SPEC §8): a stub
//! east from each arm's right through-port at one ordinal, the stubs joined
//! on one vertical run with tees, as the corpus draws it.
//!
//! The merge is drawn first as a wire with no sink, then wired on to the
//! sink as any wired source is. That sheet is never returned: it would
//! read as a wire with no sink.

use shawline_doc::Doc;
use shawline_grid::{Dir, Grid, Pos};
use shawline_shape::structure::{Mapping, Part, Routing, Structure, WirePart};

use crate::canon::tidy;
use crate::model::{Model, Source, Term};
use crate::wiring::{Canvas, read};
use crate::{Blockage, Refusal, RefusalKind};

/// How far east of the switch's border the run goes at least: two stub
/// cells, then the run.
const STUB: usize = 3;

/// The sheet with `source`'s merge drawn, checked to be the old one with
/// exactly that wire added. `source` is unwired, with two or more ports.
pub(crate) fn draw(grid: &Grid, model: &Model, source: &Source) -> Result<Doc, Refusal> {
    let ports = &source.merged;
    let (first, last) = (ports[0], *ports.last().expect("a merge has two or more ports"));
    let border = first.col;
    let region = model.region(source.scope);
    let no_room = || {
        let why = "there's no room right of the switch to join its arms' outputs; try Make room";
        Refusal::new(first, RefusalKind::Blocked(Blockage::NoRoom), why)
    };
    // The nearest run whose cells, and the stubs to it, are all blank.
    let cells = |run: usize| -> Vec<(Pos, char)> {
        let mut cells = Vec::new();
        for (i, p) in ports.iter().enumerate() {
            cells.extend((border + 1..run).map(|col| (Pos::new(p.row, col), '─')));
            let joint = match i {
                0 => '┐',
                i if i + 1 == ports.len() => '┘',
                _ => '┤',
            };
            cells.push((Pos::new(p.row, run), joint));
        }
        for row in first.row + 1..last.row {
            if !ports.iter().any(|p| p.row == row) {
                cells.push((Pos::new(row, run), '│'));
            }
        }
        cells
    };
    let run = (border + STUB..=region.right)
        .find(|&run| cells(run).iter().all(|&(p, _)| grid.get(p) == ' '))
        .ok_or_else(no_room)?;
    let mut canvas = Canvas::new(grid);
    for (p, ch) in cells(run) {
        canvas.set(p, ch);
    }
    let changed = |why: &str| {
        let why = format!("the switch's output can't be joined here: {why}; try Make room");
        Refusal::new(first, RefusalKind::Changed, why)
    };
    let doc = tidy(Doc::new(canvas.render())).map_err(changed)?;
    let (new_grid, new) = read(&doc);
    let map = Mapping::by_place(model.diagram, new, |_, r| r, |_, p| p);
    let mut expected = Structure::of(grid, model.diagram).relabel(&map);
    let mut terms: Vec<_> = ports
        .iter()
        .map(|&p| model.named(Term::Port(p, Dir::W)).expect("a through-port is a port"))
        .collect();
    terms.sort();
    expected.wires.push(WirePart { terms, tees: 0, crossings: 0 });
    expected.wires.sort();
    expected.expect(&Structure::of(new_grid, new), Routing::Ignored).map_err(|part| {
        changed(match part {
            Part::Wires => "it would meet another wire",
            _ => "it would disturb what is beside it",
        })
    })?;
    Ok(doc)
}
