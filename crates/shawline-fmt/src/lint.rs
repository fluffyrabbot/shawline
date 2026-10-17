//! SPEC §12: the aesthetic lints `L001`–`L006`.

use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};

use shawline_grid::{Diagnostic, Dir, Grid, Pos};
use shawline_shape::stroke::{E, Kind, N, S, W};
use shawline_shape::{BoxShape, BoxStyle, Diagram, EndKind, PortKind, TextKind, Wire, WireEnd};

use crate::THRESHOLDS;

pub(crate) fn lint(grid: &Grid, diagram: &Diagram) -> Vec<Diagnostic> {
    let mut found = Vec::new();
    mixed_style(diagram, &mut found);
    short_wires(diagram, &mut found);
    bends(diagram, &mut found);
    definitions(grid, diagram, &mut found);
    padding(diagram, &mut found);
    found.sort_by(|a, b| (a.at, a.code).cmp(&(b.at, b.code)));
    found
}

/// L001: a box with some rounded and some square corners.
fn mixed_style(diagram: &Diagram, found: &mut Vec<Diagnostic>) {
    for b in diagram.boxes.iter().filter(|b| b.style == BoxStyle::Mixed) {
        let at = Pos::new(b.rect.top, b.rect.left);
        found.push(Diagnostic::new(at, "L001", "box mixes rounded and square corners"));
    }
}

/// L002: an arrowhead with no line cell before it. A definition's inputs
/// have nothing before them by design.
fn short_wires(diagram: &Diagram, found: &mut Vec<Diagnostic>) {
    let ports = diagram.boxes.iter().flat_map(|b| {
        b.ports
            .iter()
            .filter(move |p| p.kind == PortKind::Arrow && !(b.parent.is_none() && p.is_inward()))
            .map(|p| (p.pos, p.dir))
    });
    let free = diagram
        .wires
        .iter()
        .flat_map(|w| &w.ends)
        .filter(|e| e.kind == EndKind::Arrow)
        .map(|e| (e.pos, e.facing));
    let arrows: BTreeSet<(Pos, Dir)> = ports.chain(free).collect();
    for (pos, dir) in arrows {
        let before = pos.step(dir.opposite()).and_then(|p| diagram.strokes.get(p));
        let line = before
            .is_some_and(|s| s.kind == Kind::Crossing || (s.kind == Kind::Line && s.runs(dir)));
        if !line {
            found.push(Diagnostic::new(pos, "L002", "arrowhead with no line before it"));
        }
    }
}

/// L003: a wire that bends too often between a source and a sink.
fn bends(diagram: &Diagram, found: &mut Vec<Diagnostic>) {
    let arrow_dir: HashMap<Pos, (PortKind, Dir)> = diagram
        .boxes
        .iter()
        .flat_map(|b| b.ports.iter().map(|p| (p.pos, (p.kind, p.dir))))
        .collect();
    for w in &diagram.wires {
        let (mut sources, mut sinks) = (Vec::new(), Vec::new());
        for e in &w.ends {
            match e.kind {
                EndKind::Arrow => sinks.push(*e),
                EndKind::Port(p) => match arrow_dir.get(&p) {
                    Some(&(PortKind::Arrow, d)) if d == e.facing => sinks.push(*e),
                    Some(&(PortKind::Arrow, d)) if d == e.facing.opposite() => sources.push(*e),
                    Some(&(PortKind::Tee, _)) => sources.push(*e),
                    _ => {}
                },
                EndKind::Open if e.facing == Dir::W => sources.push(*e),
                _ => {}
            }
        }
        let most = sources
            .iter()
            .flat_map(|s| sinks.iter().filter_map(move |k| path_bends(diagram, w, s, k)))
            .max();
        if let Some(n) = most.filter(|&n| n > THRESHOLDS.max_bends) {
            let message = format!("wire bends {n} times between its source and a sink");
            found.push(Diagnostic::new(w.cells[0], "L003", message));
        }
    }
}

/// The bends on the path through `w` from `source` to `sink`, counting a
/// turn into or out of the ports at either end.
fn path_bends(diagram: &Diagram, w: &Wire, source: &WireEnd, sink: &WireEnd) -> Option<usize> {
    let cells: HashSet<Pos> = w.cells.iter().copied().collect();
    // A crossing is two nodes, one per axis, as in the tracer.
    let node = |p: Pos, d: Dir| -> Option<(Pos, u8)> {
        let s = diagram.strokes.get(p)?;
        let mask = match (s.kind, d.is_horizontal()) {
            (Kind::Crossing, true) => E | W,
            (Kind::Crossing, false) => N | S,
            _ => s.conn,
        };
        Some((p, mask))
    };
    let start = node(source.pos, source.facing)?;
    let goal = node(sink.pos, sink.facing)?;
    let mut came: HashMap<(Pos, u8), ((Pos, u8), Dir)> = HashMap::new();
    let mut queue = VecDeque::from([start]);
    let mut seen = HashSet::from([start]);
    while let Some(n @ (p, mask)) = queue.pop_front() {
        if n == goal {
            break;
        }
        for d in Dir::ALL.into_iter().filter(|d| mask & d.bit() != 0) {
            let Some(q) = p.step(d).filter(|q| cells.contains(q)) else {
                continue;
            };
            if !diagram.strokes.get(q).is_some_and(|s| s.connects(d.opposite())) {
                continue;
            }
            let Some(m) = node(q, d.opposite()) else {
                continue;
            };
            if seen.insert(m) {
                came.insert(m, (n, d));
                queue.push_back(m);
            }
        }
    }
    if !seen.contains(&goal) {
        return None;
    }
    let mut moves = vec![sink.facing];
    let mut at = goal;
    while let Some(&(prev, d)) = came.get(&at) {
        moves.push(d);
        at = prev;
    }
    moves.push(source.facing.opposite());
    Some(moves.windows(2).filter(|m| m[0] != m[1]).count())
}

/// L004 and L005, per definition: crossings and ink density.
fn definitions(grid: &Grid, diagram: &Diagram, found: &mut Vec<Diagnostic>) {
    let crossings: BTreeSet<Pos> =
        diagram.wires.iter().flat_map(|w| w.crossings.iter().copied()).collect();
    for (i, b) in diagram.boxes.iter().enumerate().filter(|(_, b)| b.parent.is_none()) {
        let r = b.rect;
        let at = name_pos(diagram, i, b);
        let crossed = crossings.iter().filter(|&&p| r.interior_contains(p)).count();
        if crossed > THRESHOLDS.max_crossings {
            let message = format!("{crossed} crossings in one definition");
            found.push(Diagnostic::new(at, "L004", message));
        }
        if r.bottom - r.top < 2 || r.right - r.left < 2 {
            continue;
        }
        let area = (r.bottom - r.top - 1) * (r.right - r.left - 1);
        let ink = (r.top + 1..r.bottom)
            .flat_map(|row| (r.left + 1..r.right).map(move |col| Pos::new(row, col)))
            .filter(|&p| grid.get(p) != ' ')
            .count();
        if ink * 100 > THRESHOLDS.max_ink_percent * area {
            let message = format!("interior is {}% ink", ink * 100 / area);
            found.push(Diagnostic::new(at, "L005", message));
        }
    }
}

/// L006: a node whose word is off-centre by more than centring requires.
fn padding(diagram: &Diagram, found: &mut Vec<Diagnostic>) {
    for (i, b) in diagram.boxes.iter().enumerate() {
        let is_switch = !b.dividers.is_empty() || b.ports.iter().any(|p| p.side == Dir::N);
        if b.parent.is_none() || is_switch {
            continue;
        }
        let held: Vec<_> = diagram.texts.iter().filter(|t| t.owner == Some(i)).collect();
        let [t] = held[..] else { continue };
        if t.kind == TextKind::Comment {
            continue;
        }
        let left = t.pos.col - b.rect.left - 1;
        let right = b.rect.right - (t.pos.col + t.width);
        if left.abs_diff(right) > THRESHOLDS.max_padding_skew {
            let message = format!("node padding is {left} left and {right} right");
            found.push(Diagnostic::new(t.pos, "L006", message));
        }
    }
}

/// Where a definition-wide lint points: the title's namer dot, or the
/// top-left corner of an untitled box.
fn name_pos(diagram: &Diagram, i: usize, b: &BoxShape) -> Pos {
    diagram
        .texts
        .iter()
        .find(|t| t.title_of == Some(i))
        .map_or(Pos::new(b.rect.top, b.rect.left), |t| t.pos)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn codes(src: &str) -> Vec<(Pos, &'static str)> {
        let grid = Grid::parse(src).unwrap();
        let d = shawline_shape::recognize(&grid);
        lint(&grid, &d).into_iter().map(|d| (d.at, d.code)).collect()
    }

    #[test]
    fn clean_definition() {
        assert_eq!(
            codes(
                "╭─ ·𐑨𐑛 ──────╮\n│            │\n▶ 𐑯 ─────────▶ 𐑚\n│            │\n╰────────────╯\n"
            ),
            []
        );
    }

    #[test]
    fn mixed_box() {
        let src =
            "╭─ ·𐑨𐑛 ──────┐\n│            │\n▶ 𐑯 ─────────▶ 𐑚\n│            │\n╰────────────╯\n";
        assert_eq!(codes(src), [(Pos::new(0, 0), "L001")]);
    }

    #[test]
    fn arrowhead_without_line() {
        let src =
            "╭─ ·𐑨𐑛 ──────╮\n│            │\n│ 1 ▶ 𐑖 ──────▶ 𐑚\n│            │\n╰────────────╯\n";
        assert_eq!(codes(src), [(Pos::new(2, 4), "L002")]);
    }

    #[test]
    fn too_many_bends() {
        let src = concat!(
            "╭─ ·𐑨𐑛 ─────────╮\n",
            "│               │\n",
            "▶ 𐑯 ─┐ ┌─┐      │\n",
            "│    └─┘ └──────▶ 𐑚\n",
            "│               │\n",
            "╰───────────────╯\n",
        );
        assert_eq!(codes(src), [(Pos::new(2, 4), "L003")]);
    }

    #[test]
    fn too_many_crossings() {
        let src = concat!(
            "╭─ ·𐑨𐑛 ──────────╮\n",
            "│    1  2  3     │\n",
            "│    │  │  │     │\n",
            "▶ 𐑯 ─┼──┼──┼─────▶ 𐑚\n",
            "│    │  │  │     │\n",
            "│    ▼  ▼  ▼     │\n",
            "│                │\n",
            "╰────────────────╯\n",
        );
        let found = codes(src);
        assert!(found.contains(&(Pos::new(0, 3), "L004")), "{found:?}");
    }

    #[test]
    fn dense_ink() {
        let src = "╭─ ·𐑨𐑛 ─╮\n│1 ─────▶ 𐑚\n╰───────╯\n";
        assert_eq!(codes(src), [(Pos::new(0, 3), "L005")]);
    }

    #[test]
    fn asymmetric_padding() {
        let node = |row: &str| {
            format!(
                "╭─ ·𐑨𐑛 ────────────╮\n│     ┌─────┐      │\n▶ 𐑯 ──▶{row}├──────▶ 𐑚\n│     └─────┘      │\n╰──────────────────╯\n"
            )
        };
        assert_eq!(codes(&node("  𐑖  ")), []);
        assert_eq!(codes(&node(" 𐑖   ")), [(Pos::new(2, 8), "L006")]);
        let wide = "╭─ ·𐑨𐑛 ────────────╮\n│     ┌────┐       │\n▶ 𐑯 ──▶ 𐑖  ├───────▶ 𐑚\n│     └────┘       │\n╰──────────────────╯\n";
        assert_eq!(codes(wide), [], "centring is impossible in an even gap");
    }
}
