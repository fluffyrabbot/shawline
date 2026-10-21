//! The wire router: a best-first search over grid cells, costed by the
//! formatter's aesthetics (SPEC §11–§12).
//!
//! The search is generic over a [`Terrain`], which says what each cell is
//! and where the route may end. [`crate::route`] runs it over a plain sheet;
//! the move gesture runs it with goals on the wire it is reconnecting.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};

use shawline_grid::{Dir, Pos};

/// Every cost is in half cells, so clearance can cost less than a cell.
///
/// Each cell a wire takes.
pub(crate) const CELL: u32 = 2;
/// Each bend: three cells, so a straight run wins over a short jog.
pub(crate) const BEND: u32 = 6;
/// Each crossing of an existing wire: thirty cells. SPEC §12 lints more
/// than two crossings in a definition, so a detour is nearly always better.
pub(crate) const CROSS: u32 = 60;
/// Each neighbouring stroke (a box border or another wire): half a cell.
pub(crate) const CLEARANCE: u32 = 1;
/// A new fan-out point on an existing wire.
pub(crate) const TEE: u32 = 4;
/// Most search states one edit expands before giving up, over all its
/// searches. A sheet's interior is a few thousand cells, four headings
/// each, so this is several sheets' worth.
pub(crate) const BUDGET: usize = 40_000;

/// What a cell means to a new wire.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Cell {
    Free,
    Blocked,
    /// A straight wire line the route may cross at right angles; `true`
    /// when the line is horizontal.
    Line(bool),
}

/// The ground a search runs over.
pub(crate) trait Terrain {
    /// What `p` is. Anything outside the search area is [`Cell::Blocked`].
    fn cell(&self, p: Pos) -> Cell;

    /// Extra cost for taking free cell `p`, such as clearance.
    fn extra(&self, p: Pos) -> u32;

    /// The cost of ending the route by stepping into `p` while heading
    /// `heading`, or `None` if the route can't end there.
    fn goal(&self, p: Pos, heading: Dir) -> Option<u32>;

    /// Extra cost for bending at `p`.
    fn bend_extra(&self, _p: Pos) -> u32 {
        0
    }
}

/// Where a search starts.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Start {
    pub pos: Pos,
    /// The direction the route is already travelling, if any.
    pub heading: Option<Dir>,
    /// The first step must continue `heading`, as out of a port.
    pub straight: bool,
}

/// Why a search found no route.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Miss {
    /// Every reachable state was tried: no route exists.
    NoPath,
    /// The budget ran out first: a route may exist.
    GaveUp,
}

/// A route the search found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Found {
    /// From the start to the goal, both included.
    pub path: Vec<Pos>,
    pub cost: u32,
}

struct Node {
    pos: Pos,
    heading: Option<Dir>,
    /// The next step must go straight on: out of a port, or through a crossing.
    straight: bool,
    parent: Option<usize>,
}

type Key = (Pos, Option<Dir>, bool);

/// The cheapest route from `start` to a goal of `terrain`. Each state
/// expanded takes one from `budget`, which an edit shares between all its
/// searches. Ties go to the route found first, so the result is
/// deterministic.
pub(crate) fn search(
    terrain: &impl Terrain,
    start: Start,
    budget: &mut usize,
) -> Result<Found, Miss> {
    let mut nodes = vec![Node {
        pos: start.pos,
        heading: start.heading,
        straight: start.straight,
        parent: None,
    }];
    let mut best: HashMap<Key, u32> = HashMap::new();
    best.insert((start.pos, start.heading, start.straight), 0);
    // (cost, sequence, node, is a goal)
    let mut heap = BinaryHeap::new();
    heap.push(Reverse((0u32, 0usize, 0usize, false)));
    let mut seq = 0;

    while let Some(Reverse((cost, _, id, done))) = heap.pop() {
        if done {
            let mut path = Vec::new();
            let mut at = Some(id);
            while let Some(i) = at {
                path.push(nodes[i].pos);
                at = nodes[i].parent;
            }
            path.reverse();
            return Ok(Found { path, cost });
        }
        let (pos, heading, straight) = (nodes[id].pos, nodes[id].heading, nodes[id].straight);
        if best.get(&(pos, heading, straight)).is_some_and(|&b| b < cost) {
            continue;
        }
        if *budget == 0 {
            return Err(Miss::GaveUp);
        }
        *budget -= 1;
        for d in Dir::ALL {
            if let Some(h) = heading
                && (d == h.opposite() || (straight && d != h))
            {
                continue;
            }
            let Some(q) = pos.step(d) else { continue };
            let bend = match heading {
                Some(h) if h != d => BEND + terrain.bend_extra(pos),
                _ => 0,
            };
            if let Some(join) = terrain.goal(q, d) {
                seq += 1;
                nodes.push(Node { pos: q, heading: Some(d), straight: false, parent: Some(id) });
                heap.push(Reverse((cost + bend + join, seq, nodes.len() - 1, true)));
                continue;
            }
            let (step, through) = match terrain.cell(q) {
                Cell::Free => (CELL + terrain.extra(q), false),
                Cell::Line(horizontal) if horizontal != d.is_horizontal() => (CROSS, true),
                _ => continue,
            };
            let next = cost + bend + step;
            let key = (q, Some(d), through);
            if best.get(&key).is_some_and(|&b| b <= next) {
                continue;
            }
            best.insert(key, next);
            seq += 1;
            nodes.push(Node { pos: q, heading: Some(d), straight: through, parent: Some(id) });
            heap.push(Reverse((next, seq, nodes.len() - 1, false)));
        }
    }
    Err(Miss::NoPath)
}

/// The direction from `a` to its neighbour `b`.
pub(crate) fn toward(a: Pos, b: Pos) -> Dir {
    match (b.row.cmp(&a.row), b.col.cmp(&a.col)) {
        (std::cmp::Ordering::Less, _) => Dir::N,
        (std::cmp::Ordering::Greater, _) => Dir::S,
        (_, std::cmp::Ordering::Greater) => Dir::E,
        _ => Dir::W,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A field drawn as text: `.` free, `#` blocked, `-` and `|` crossable
    /// lines, `G` the goal.
    struct Field {
        rows: Vec<Vec<char>>,
    }

    impl Field {
        fn new(art: &str) -> Self {
            Self { rows: art.lines().map(|l| l.chars().collect()).collect() }
        }

        fn at(&self, p: Pos) -> char {
            self.rows.get(p.row).and_then(|r| r.get(p.col)).copied().unwrap_or('#')
        }
    }

    impl Terrain for Field {
        fn cell(&self, p: Pos) -> Cell {
            match self.at(p) {
                '.' => Cell::Free,
                '-' => Cell::Line(true),
                '|' => Cell::Line(false),
                _ => Cell::Blocked,
            }
        }

        fn extra(&self, _p: Pos) -> u32 {
            0
        }

        fn goal(&self, p: Pos, _heading: Dir) -> Option<u32> {
            (self.at(p) == 'G').then_some(CELL)
        }
    }

    fn from(row: usize, col: usize) -> Start {
        Start { pos: Pos::new(row, col), heading: None, straight: false }
    }

    #[test]
    fn crosses_when_the_detour_is_long() {
        let field = Field::new("#|##\n.|.G\n#|##\n");
        let found = search(&field, from(1, 0), &mut { BUDGET }).unwrap();
        assert_eq!(found.path, [Pos::new(1, 0), Pos::new(1, 1), Pos::new(1, 2), Pos::new(1, 3)]);
        assert_eq!(found.cost, CROSS + 2 * CELL);
    }

    #[test]
    fn no_bend_inside_a_crossing() {
        // Turning down inside the line would be a four-way junction.
        let field = Field::new(".|.\n#.#\n#G#\n");
        assert_eq!(search(&field, from(0, 0), &mut { BUDGET }), Err(Miss::NoPath));
    }

    #[test]
    fn searches_share_one_budget() {
        // Crossing a ten-cell corridor expands ten states.
        let field = Field::new("..........G\n");
        let mut budget = 15;
        assert!(search(&field, from(0, 0), &mut budget).is_ok());
        assert_eq!(budget, 5);
        assert_eq!(search(&field, from(0, 0), &mut budget), Err(Miss::GaveUp));
        // A walled-in start runs out of states, not budget.
        assert_eq!(search(&Field::new(".#G\n"), from(0, 0), &mut { BUDGET }), Err(Miss::NoPath));
    }

    #[test]
    fn straight_start_goes_straight_first() {
        let field = Field::new("...\n...\n..G\n");
        let start = Start { pos: Pos::new(0, 0), heading: Some(Dir::S), straight: true };
        let found = search(&field, start, &mut { BUDGET }).unwrap();
        assert_eq!(found.path[1], Pos::new(1, 0));
    }
}
