//! Wire tracing: groups the stroke cells outside box borders into [`Wire`]s,
//! as defined in [`crate::wire`].

use std::collections::{BTreeMap, HashSet};

use shawline_grid::{Diagnostic, Dir, Grid, Pos, Rect};

use crate::boxes::BoxShape;
use crate::stroke::{E, Kind, N, S, StrokeMap, W};
use crate::wire::{EndKind, Wire, WireEnd};

/// One traversable part of a wire cell: the cell and the directions it owns.
/// A crossing is two nodes, one per axis; every other cell is one node.
type Node = (Pos, u8);

struct Tracer<'a> {
    strokes: &'a StrokeMap,
    /// Box perimeters, slipped border cells and divider rows: strokes that
    /// are never wire.
    border: HashSet<Pos>,
    ports: HashSet<Pos>,
}

impl Tracer<'_> {
    fn is_wire(&self, p: Pos) -> bool {
        self.strokes.get(p).is_some() && !self.border.contains(&p)
    }

    /// The node of wire cell `p` that owns direction `d`.
    fn node(&self, p: Pos, d: Dir) -> Node {
        let s = self.strokes.get(p).expect("wire cells are strokes");
        let mask = match (s.kind, d.is_horizontal()) {
            (Kind::Crossing, true) => E | W,
            (Kind::Crossing, false) => N | S,
            _ => s.conn,
        };
        (p, mask & s.conn)
    }

    /// The node joined to `node` in direction `d`: both sides must connect.
    fn joined(&self, (p, mask): Node, d: Dir) -> Option<Node> {
        let q = p.step(d).filter(|_| mask & d.bit() != 0)?;
        let back = self.strokes.get(q)?.connects(d.opposite());
        (back && self.is_wire(q)).then(|| self.node(q, d.opposite()))
    }

    fn wire(&self, start: Node, seen: &mut HashSet<Node>) -> Wire {
        let mut nodes = vec![start];
        let mut pending = vec![start];
        seen.insert(start);
        while let Some(n) = pending.pop() {
            for d in Dir::ALL {
                if let Some(m) = self.joined(n, d).filter(|&m| seen.insert(m)) {
                    nodes.push(m);
                    pending.push(m);
                }
            }
        }

        // A crossing the wire passes both ways is two nodes of one cell.
        let mut joins: BTreeMap<Pos, u8> = BTreeMap::new();
        for &(p, mask) in &nodes {
            *joins.entry(p).or_default() |= mask;
        }
        let (cells, links): (Vec<Pos>, Vec<u8>) = joins.into_iter().unzip();
        let of_kind = |want: fn(Kind) -> bool| -> Vec<Pos> {
            let has = |p: &&Pos| self.strokes.get(**p).is_some_and(|s| want(s.kind));
            cells.iter().filter(has).copied().collect()
        };
        let tees = of_kind(|k| k == Kind::Tee);
        let crossings = of_kind(|k| k == Kind::Crossing);

        let mut ends = Vec::new();
        for &(p, mask) in &nodes {
            for d in Dir::ALL.into_iter().filter(|d| mask & d.bit() != 0) {
                if self.joined((p, mask), d).is_some() {
                    continue;
                }
                let kind = match p.step(d) {
                    Some(q) if self.ports.contains(&q) => EndKind::Port(q),
                    Some(q) if self.strokes.get(q).is_some() => EndKind::Blocked,
                    _ => EndKind::Open,
                };
                ends.push(WireEnd { pos: p, facing: d, kind });
            }
            // Wire cells are off every border, so any arrowhead here is free.
            if let Some(Kind::Arrow(points)) = self.strokes.get(p).map(|s| s.kind) {
                ends.push(WireEnd { pos: p, facing: points, kind: EndKind::Arrow });
            }
        }
        ends.sort();
        ends.dedup();
        Wire { cells, links, tees, crossings, ends }
    }
}

/// Every cell of `r`'s border, once each.
fn perimeter(r: Rect) -> impl Iterator<Item = Pos> {
    let sides =
        (r.top..=r.bottom).flat_map(move |row| [Pos::new(row, r.left), Pos::new(row, r.right)]);
    let ends =
        (r.left + 1..r.right).flat_map(move |col| [Pos::new(r.top, col), Pos::new(r.bottom, col)]);
    sides.chain(ends)
}

/// Traces every wire, appending `S010` for blocked ends and `S011` for
/// crossings on box borders. Wires come out ordered by their cells.
pub(crate) fn trace(
    grid: &Grid,
    strokes: &StrokeMap,
    boxes: &[BoxShape],
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<Wire> {
    let mut border = HashSet::new();
    for b in boxes {
        border.extend(perimeter(b.rect));
        // A slipped cell is border drawn out of place, not a wire.
        border.extend(b.slips.iter().map(|s| s.at));
        for d in &b.dividers {
            border.extend((b.rect.left..=b.rect.right).map(|col| Pos::new(d.row, col)));
        }
    }
    let ports = boxes.iter().flat_map(|b| b.ports.iter().map(|p| p.pos)).collect();
    let tracer = Tracer { strokes, border, ports };

    // Where two borders cross, the boxes overlap; that is `S001`, not a wire.
    let mut on_border: BTreeMap<Pos, usize> = BTreeMap::new();
    for p in boxes.iter().flat_map(|b| perimeter(b.rect)) {
        if strokes.get(p).is_some_and(|s| s.kind == Kind::Crossing) {
            *on_border.entry(p).or_default() += 1;
        }
    }
    for (&p, _) in on_border.iter().filter(|&(_, &n)| n == 1) {
        diagnostics.push(Diagnostic::new(p, "S011", "crossing on a box border"));
    }

    let mut seen = HashSet::new();
    let mut wires = Vec::new();
    for row in 0..grid.height() {
        for col in 0..grid.row(row).len() {
            let p = Pos::new(row, col);
            if !tracer.is_wire(p) {
                continue;
            }
            for d in [Dir::E, Dir::S] {
                let start = tracer.node(p, d);
                if !seen.contains(&start) {
                    wires.push(tracer.wire(start, &mut seen));
                }
            }
        }
    }
    wires.sort_by(|a, b| a.cells.cmp(&b.cells));

    for e in wires.iter().flat_map(|w| &w.ends).filter(|e| e.kind == EndKind::Blocked) {
        let message = format!("wire runs {} into a stroke that doesn't join it", e.facing);
        diagnostics.push(Diagnostic::new(e.pos, "S010", message));
    }
    wires
}

#[cfg(test)]
mod tests {
    use shawline_grid::Grid;

    use crate::wire::{EndKind, dump};
    use crate::{Diagram, recognize};

    use super::*;

    fn diagram(src: &str) -> Diagram {
        recognize(&Grid::parse(src).unwrap())
    }

    fn end(row: usize, col: usize, facing: Dir, kind: EndKind) -> WireEnd {
        WireEnd { pos: Pos::new(row, col), facing, kind }
    }

    fn codes(d: &Diagram) -> Vec<(Pos, &str)> {
        d.diagnostics.iter().map(|d| (d.at, d.code)).collect()
    }

    #[test]
    fn crossing_carries_one_wire_per_axis() {
        let d = diagram(" │\n─┼─\n │\n");
        assert_eq!(d.wires.len(), 2);
        let (vertical, horizontal) = (&d.wires[0], &d.wires[1]);
        assert_eq!(vertical.cells, [Pos::new(0, 1), Pos::new(1, 1), Pos::new(2, 1)]);
        assert_eq!(horizontal.cells, [Pos::new(1, 0), Pos::new(1, 1), Pos::new(1, 2)]);
        for w in &d.wires {
            assert_eq!(w.crossings, [Pos::new(1, 1)]);
            assert!(w.ends.iter().all(|e| e.kind == EndKind::Open && e.pos != Pos::new(1, 1)));
        }
    }

    #[test]
    fn a_crossing_links_only_its_own_axis() {
        let d = diagram(" │\n─┼─\n │\n");
        let at = |w: &Wire, p: Pos| w.links[w.cells.iter().position(|&c| c == p).unwrap()];
        assert_eq!(at(&d.wires[0], Pos::new(1, 1)), N | S);
        assert_eq!(at(&d.wires[1], Pos::new(1, 1)), E | W);
        assert_eq!(at(&d.wires[1], Pos::new(1, 0)), E | W);
    }

    #[test]
    fn a_wire_crossing_itself_links_both_axes() {
        // One wire loops round and crosses itself at 1:1.
        let d = diagram(" ┌─┐\n─┼─┘\n └─\n");
        assert_eq!(d.wires.len(), 1, "{}", dump(&d.wires));
        let w = &d.wires[0];
        assert_eq!(w.links.len(), w.cells.len());
        assert_eq!(w.links[w.cells.iter().position(|&c| c == Pos::new(1, 1)).unwrap()], N | E | S | W);
        assert_eq!(w.links[w.cells.iter().position(|&c| c == Pos::new(0, 1)).unwrap()], E | S);
    }

    #[test]
    fn tee_fans_out_one_wire() {
        let d = diagram("─┬─\n │\n");
        assert_eq!(d.wires.len(), 1);
        assert_eq!(d.wires[0].tees, [Pos::new(0, 1)]);
        assert_eq!(d.wires[0].ends.len(), 3);
    }

    #[test]
    fn ends_at_ports_from_both_sides() {
        let d = diagram(" ┌───┐\n─▶ 𐑐 ├─\n └───┘\n");
        assert_eq!(d.wires.len(), 2);
        assert_eq!(
            d.wires[0].ends,
            [end(1, 0, Dir::E, EndKind::Port(Pos::new(1, 1))), end(1, 0, Dir::W, EndKind::Open)]
        );
        assert_eq!(
            d.wires[1].ends,
            [end(1, 6, Dir::E, EndKind::Open), end(1, 6, Dir::W, EndKind::Port(Pos::new(1, 5)))]
        );
    }

    #[test]
    fn stroke_that_does_not_join_blocks() {
        let d = diagram("─│\n");
        assert_eq!(d.wires[0].ends[0], end(0, 0, Dir::E, EndKind::Blocked));
        assert_eq!(codes(&d), [(Pos::new(0, 0), "S010")]);
    }

    #[test]
    fn free_arrowhead_ends_where_it_points() {
        let d = diagram("─▶ 𐑖\n");
        assert_eq!(
            d.wires[0].ends,
            [end(0, 0, Dir::W, EndKind::Open), end(0, 1, Dir::E, EndKind::Arrow)]
        );
    }

    #[test]
    fn ascii_crossing_matches_canonical() {
        let canonical = diagram(" │\n─┼─\n │\n");
        let ascii = diagram(" |\n-+-\n |\n");
        assert_eq!(dump(&ascii.wires), dump(&canonical.wires));
    }

    #[test]
    fn crossing_on_border_is_diagnosed() {
        let d = diagram("┌─┐\n┼ │\n└─┘\n");
        assert!(codes(&d).contains(&(Pos::new(1, 0), "S011")), "{:?}", d.diagnostics);
    }
}
