//! What each stroke character connects to.
//!
//! Canonical box-drawing characters have fixed connectivity. The ASCII joints
//! `+`, `.` and `'` take theirs from their neighbours, which is what lets a
//! rough ASCII sketch mean the same thing as its canonical form.

use shawline_grid::{Dir, Grid, Pos};

pub const N: u8 = Dir::N.bit();
pub const E: u8 = Dir::E.bit();
pub const S: u8 = Dir::S.bit();
pub const W: u8 = Dir::W.bit();

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Line,
    /// `┄ ┆`: switch dividers.
    Dashed,
    Corner { rounded: bool },
    Tee,
    /// `┼`: two wires crossing without connecting.
    Crossing,
    /// An arrowhead pointing in the given direction; it marks a wire's end.
    Arrow(Dir),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stroke {
    /// Directions this cell connects in, as [`Dir::bit`] flags.
    pub conn: u8,
    pub kind: Kind,
}

impl Stroke {
    pub fn connects(self, dir: Dir) -> bool {
        self.conn & dir.bit() != 0
    }

    /// True if the stroke runs straight through along `dir`'s axis.
    pub fn runs(self, dir: Dir) -> bool {
        self.connects(dir) && self.connects(dir.opposite())
    }
}

const fn stroke(conn: u8, kind: Kind) -> Option<Stroke> {
    Some(Stroke { conn, kind })
}

/// The fixed stroke for `ch`, or `None` for non-strokes and ASCII joints.
pub fn fixed(ch: char) -> Option<Stroke> {
    use Kind::*;
    let square = Corner { rounded: false };
    let round = Corner { rounded: true };
    match ch {
        '─' | '-' => stroke(E | W, Line),
        '│' | '|' => stroke(N | S, Line),
        '┄' | '~' => stroke(E | W, Dashed),
        '┆' | ':' => stroke(N | S, Dashed),
        '┌' => stroke(E | S, square),
        '┐' => stroke(W | S, square),
        '└' => stroke(N | E, square),
        '┘' => stroke(N | W, square),
        '╭' => stroke(E | S, round),
        '╮' => stroke(W | S, round),
        '╰' => stroke(N | E, round),
        '╯' => stroke(N | W, round),
        '├' => stroke(N | S | E, Tee),
        '┤' => stroke(N | S | W, Tee),
        '┬' => stroke(E | W | S, Tee),
        '┴' => stroke(E | W | N, Tee),
        '┼' => stroke(N | E | S | W, Crossing),
        '▶' | '>' => stroke(W, Arrow(Dir::E)),
        '◀' | '<' => stroke(E, Arrow(Dir::W)),
        '▲' | '^' => stroke(S, Arrow(Dir::N)),
        '▼' | 'v' => stroke(N, Arrow(Dir::S)),
        _ => None,
    }
}

/// An ASCII joint's possible connections, and whether it draws a round corner.
fn joint(ch: char) -> Option<(u8, bool)> {
    match ch {
        '+' => Some((N | E | S | W, false)),
        '.' => Some((E | S | W, true)),
        '\'' => Some((N | E | W, true)),
        _ => None,
    }
}

/// Connections a cell could make, before joints are resolved. Arrowheads
/// count as connecting everywhere because they sit inside box borders.
fn potential(ch: char) -> u8 {
    match (fixed(ch), joint(ch)) {
        (Some(Stroke { kind: Kind::Arrow(_), .. }), _) => N | E | S | W,
        (Some(s), _) => s.conn,
        (None, Some((conn, _))) => conn,
        (None, None) => 0,
    }
}

/// The resolved stroke of every cell in a grid.
#[derive(Clone, Debug)]
pub struct StrokeMap {
    cells: Vec<Vec<Option<Stroke>>>,
}

impl StrokeMap {
    pub fn new(grid: &Grid) -> Self {
        let cells = (0..grid.height())
            .map(|row| {
                (0..grid.row(row).len())
                    .map(|col| resolve(grid, Pos::new(row, col)))
                    .collect()
            })
            .collect();
        Self { cells }
    }

    pub fn get(&self, p: Pos) -> Option<Stroke> {
        self.cells.get(p.row)?.get(p.col).copied().flatten()
    }
}

fn resolve(grid: &Grid, p: Pos) -> Option<Stroke> {
    let ch = grid.get(p);
    if let Some(s) = fixed(ch) {
        return Some(s);
    }
    let (possible, rounded) = joint(ch)?;
    let conn = Dir::ALL
        .into_iter()
        .filter(|&d| possible & d.bit() != 0)
        .filter(|&d| p.step(d).is_some_and(|q| potential(grid.get(q)) & d.opposite().bit() != 0))
        .fold(0, |acc, d| acc | d.bit());
    let kind = match conn.count_ones() {
        4 => Kind::Crossing,
        3 => Kind::Tee,
        2 if conn != N | S && conn != E | W => Kind::Corner { rounded },
        _ => Kind::Line,
    };
    Some(Stroke { conn, kind })
}

#[cfg(test)]
mod tests {
    use super::*;
    use shawline_grid::repertoire::STROKES;

    #[test]
    fn every_repertoire_stroke_is_known() {
        for &ch in STROKES {
            assert!(fixed(ch).is_some() || joint(ch).is_some(), "{ch:?} has no connectivity");
        }
    }

    #[test]
    fn ascii_joints_follow_neighbours() {
        let grid = Grid::parse(".-+\n| |\n'-+-").unwrap();
        let map = StrokeMap::new(&grid);
        let at = |r, c| map.get(Pos::new(r, c)).unwrap();
        assert_eq!(at(0, 0), Stroke { conn: E | S, kind: Kind::Corner { rounded: true } });
        assert_eq!(at(0, 2), Stroke { conn: W | S, kind: Kind::Corner { rounded: false } });
        assert_eq!(at(2, 0).conn, N | E);
        assert_eq!(at(2, 2), Stroke { conn: N | E | W, kind: Kind::Tee });
    }
}
