//! A switch's default arm, added whole: the repair for `W001` (PLAN P10,
//! "Wave 3 contracts landed").
//!
//! Rows go in above the switch's bottom border, as [`crate::insert`] puts
//! them in, for a divider and the arm. The arm takes a right through-port
//! at each of the switch's result ordinals, so its arms still have equal
//! results, and each port's stub is routed east to join that ordinal's
//! merge with a tee, as a branch joins a wired source. Inside, each port is
//! unwired until the author wires the arm's value.

use shawline_doc::Doc;
use shawline_grid::{Dir, Pos};
use shawline_shape::PortKind;
use shawline_shape::structure::{End, PortPart, Routing, Structure, Term as Part};

use crate::canon::{Sheets, tidy};
use crate::cut::{self, render};
use crate::drawing::{put, rows};
use crate::mapped::{self, compose};
use crate::model::{Model, Sink, Term};
use crate::wiring::{Canvas, join, read};
use crate::{Axis, BoxKind, Edited, Insertion, Refusal, RefusalKind};

pub(crate) fn add_default_arm(doc: &Doc, corner: Pos) -> Result<Edited, Refusal> {
    let refuse = |kind, why: &str| Refusal::new(corner, kind, why);
    let sheets = Sheets::read(doc, corner)?;
    let (g, d) = sheets.given();
    let not = || refuse(RefusalKind::Target, "there is no switch with its top-left corner here");
    let b = Model::new(g, d).box_at(corner).ok_or_else(not)?;
    let (grid, diagram) = sheets.canon();
    let model = Model::new(grid, diagram);
    if model.kind(b) != BoxKind::Switch {
        return Err(not());
    }
    let shape = &diagram.boxes[b];
    if shape.title_gap.is_none() || shape.dividers.iter().any(|d| d.gap.is_none()) {
        return Err(refuse(RefusalKind::Target, "this switch has a default arm already"));
    }
    let rect = shape.rect;
    // The first arm's right through-ports name the result ordinals.
    let firsts: Vec<Pos> = shape
        .ports
        .iter()
        .filter(|p| p.side == Dir::E && p.kind == PortKind::Arrow && model.arm(b, p.pos.row) == 0)
        .map(|p| p.pos)
        .collect();
    let results = firsts.len();

    // Room: a divider row, then one row per port.
    let insertion = Insertion { axis: Axis::Row, at: Pos::new(rect.bottom, rect.left), count: 1 + results.max(1) };
    let (roomed, cut) = cut::cut(sheets.doc(), insertion, false).map_err(|r| Refusal { at: corner, ..r })?;
    let (grid1, diagram1) = read(&roomed.doc);
    let nb = roomed.map.boxes[b].expect("an insertion keeps every box");
    let firsts: Vec<Pos> = firsts.iter().map(|&p| cut.cell(p).expect("an insertion keeps every cell")).collect();
    let divider = rect.bottom;
    let ports: Vec<Pos> = (0..results).map(|k| Pos::new(divider + 1 + k, rect.right)).collect();
    let mut lines = rows(grid1);
    let width = rect.right - rect.left + 1;
    let rule: String = std::iter::once('├').chain(std::iter::repeat_n('┄', width - 2)).chain(std::iter::once('┤')).collect();
    for (j, ch) in rule.chars().enumerate() {
        put(&mut lines, Pos::new(divider, rect.left + j), ch);
    }
    for &p in &ports {
        put(&mut lines, p, '▶');
    }
    let mut current = Doc::new(render(&lines));

    // Each port's stub joins its ordinal's merge, one at a time, each on the
    // sheet the one before it left. An ordinal no wire leaves stays so.
    let mut joined: Vec<(usize, Pos)> = Vec::new();
    for (k, (&first, &port)) in firsts.iter().zip(&ports).enumerate() {
        let (g2, d2) = read(&current);
        let m2 = Model::new(g2, d2);
        let source = m2.source(first).map_err(|r| Refusal { at: corner, ..r })?;
        if m2.wire_of(source.term).is_none() {
            continue;
        }
        // The stub leaves the port eastward, as a route leaves a sink.
        let stub = Sink { at: port, dir: Dir::W, scope: source.scope, free: false };
        let mut canvas = Canvas::new(g2);
        join(&mut canvas, &m2, &source, &stub).map_err(|r| {
            let why = "the switch's output has no room for another arm's stub; try Make room";
            Refusal { at: corner, reason: why.to_owned(), ..r }
        })?;
        current = Doc::new(canvas.render());
        joined.push((k, first));
    }

    let changed = |why: &str| refuse(RefusalKind::Changed, &format!("the default arm can't go here: {why}"));
    let done = tidy(current).map_err(changed)?;
    let new = mapped::read(&done);
    let second = mapped::map((grid1, diagram1), new, Some);
    let nb2 = second.boxes[nb].ok_or_else(|| changed("the switch would move"))?;
    let mut expected = Structure::of(grid1, diagram1).relabel(&second);
    let found = Structure::of(new.0, new.1);
    let old_ports = expected.boxes[nb2].ports.len();
    expected.boxes[nb2].dividers.push(false);
    for k in 0..results {
        let meets = found.boxes[nb2].ports.get(old_ports + k).map(|p| p.meets).ok_or_else(|| changed("a port is missing"))?;
        expected.boxes[nb2].ports.push(PortPart { side: Dir::E, kind: PortKind::Arrow, dir: Dir::E, meets });
    }
    let first_model = Model::new(grid1, diagram1);
    for &(k, first) in &joined {
        let named = first_model.named(Term::Port(first, Dir::W)).expect("a through-port is a port");
        let term = Part { facing: named.facing, end: carried(named.end, &second) };
        let new_term = Part { facing: Dir::W, end: End::Port(nb2, old_ports + k) };
        if let Some(wire) = expected.wires.iter_mut().find(|w| w.terms.contains(&term)) {
            wire.terms.push(new_term);
            wire.terms.sort();
        }
    }
    expected.wires.sort();
    expected.expect(&found, Routing::Ignored).map_err(|_| changed("it would change what the switch's wires connect"))?;
    let mut map = compose(&roomed.map, &second);
    map.wires = mapped::wires_by_terms(mapped::before(doc), new, &map);
    Ok(Edited { doc: done, map, tidied: sheets.tidied() })
}

/// A port end of one sheet in the indices of the next.
fn carried(end: End, map: &shawline_shape::structure::Mapping) -> End {
    match end {
        End::Port(b, k) => End::Port(map.boxes[b].unwrap_or(b), k),
        other => other,
    }
}
