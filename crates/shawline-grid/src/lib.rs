//! Source text as a grid of one-cell characters.
//!
//! Shawline source is two-dimensional: meaning comes from where characters sit
//! relative to each other. This crate turns text into a [`Grid`], enforcing the
//! closed [repertoire] so every cell is exactly one column wide.

pub mod repertoire;

use std::fmt;

pub use repertoire::{Class, classify};

/// A cell position, zero-based.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Pos {
    pub row: usize,
    pub col: usize,
}

impl Pos {
    pub const fn new(row: usize, col: usize) -> Self {
        Self { row, col }
    }

    /// The neighbouring position in `dir`, or `None` off the top or left edge.
    pub fn step(self, dir: Dir) -> Option<Pos> {
        Some(match dir {
            Dir::N => Pos::new(self.row.checked_sub(1)?, self.col),
            Dir::E => Pos::new(self.row, self.col + 1),
            Dir::S => Pos::new(self.row + 1, self.col),
            Dir::W => Pos::new(self.row, self.col.checked_sub(1)?),
        })
    }
}

impl fmt::Display for Pos {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.row, self.col)
    }
}

/// A compass direction on the grid. North is up (lower row numbers).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Dir {
    N,
    E,
    S,
    W,
}

impl Dir {
    pub const ALL: [Dir; 4] = [Dir::N, Dir::E, Dir::S, Dir::W];

    pub const fn opposite(self) -> Dir {
        match self {
            Dir::N => Dir::S,
            Dir::E => Dir::W,
            Dir::S => Dir::N,
            Dir::W => Dir::E,
        }
    }

    /// This direction as a one-bit mask, for connectivity sets.
    pub const fn bit(self) -> u8 {
        1 << self as u8
    }

    pub const fn is_horizontal(self) -> bool {
        matches!(self, Dir::E | Dir::W)
    }
}

impl fmt::Display for Dir {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Dir::N => "N",
            Dir::E => "E",
            Dir::S => "S",
            Dir::W => "W",
        })
    }
}

/// An inclusive rectangle of cells.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Rect {
    pub top: usize,
    pub left: usize,
    pub bottom: usize,
    pub right: usize,
}

impl Rect {
    pub fn contains(&self, p: Pos) -> bool {
        (self.top..=self.bottom).contains(&p.row) && (self.left..=self.right).contains(&p.col)
    }

    /// True if `p` is strictly inside the border.
    pub fn interior_contains(&self, p: Pos) -> bool {
        p.row > self.top && p.row < self.bottom && p.col > self.left && p.col < self.right
    }

    /// True if `other` lies entirely within this rectangle's interior.
    pub fn encloses(&self, other: &Rect) -> bool {
        other.top > self.top
            && other.bottom < self.bottom
            && other.left > self.left
            && other.right < self.right
    }

    pub fn intersects(&self, other: &Rect) -> bool {
        self.top <= other.bottom
            && other.top <= self.bottom
            && self.left <= other.right
            && other.left <= self.right
    }

    pub fn area(&self) -> usize {
        (self.bottom - self.top + 1) * (self.right - self.left + 1)
    }
}

impl fmt::Display for Rect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}-{}:{}", self.top, self.left, self.bottom, self.right)
    }
}

/// Source text laid out as rows of cells.
#[derive(Clone, Debug)]
pub struct Grid {
    rows: Vec<Vec<char>>,
    line_starts: Vec<usize>,
    width: usize,
    len: usize,
}

impl Grid {
    /// Lays out `src`, reporting every character outside the repertoire.
    ///
    /// Accepts `\n` or `\r\n` line endings and a leading byte-order mark.
    pub fn parse(src: &str) -> Result<Grid, Vec<GridError>> {
        let body = src.strip_prefix('\u{feff}').unwrap_or(src);
        let mut start = src.len() - body.len();
        let mut rows = Vec::new();
        let mut line_starts = Vec::new();
        let mut errors = Vec::new();

        for (r, line) in body.split('\n').enumerate() {
            let text = line.strip_suffix('\r').unwrap_or(line);
            let mut row = Vec::with_capacity(text.len());
            for (c, (b, ch)) in text.char_indices().enumerate() {
                let kind = match ch {
                    '\t' => Some(GridErrorKind::Tab),
                    '\r' => Some(GridErrorKind::StrayCarriageReturn),
                    _ if classify(ch).is_none() => Some(GridErrorKind::OutsideRepertoire(ch)),
                    _ => None,
                };
                if let Some(kind) = kind {
                    errors.push(GridError { pos: Pos::new(r, c), byte: start + b, kind });
                }
                row.push(ch);
            }
            line_starts.push(start);
            rows.push(row);
            start += line.len() + 1;
        }
        // A trailing newline (or empty input) leaves an empty final row.
        if body.is_empty() || body.ends_with('\n') {
            rows.pop();
            line_starts.pop();
        }

        if !errors.is_empty() {
            return Err(errors);
        }
        let width = rows.iter().map(Vec::len).max().unwrap_or(0);
        Ok(Grid { rows, line_starts, width, len: src.len() })
    }

    pub fn height(&self) -> usize {
        self.rows.len()
    }

    /// The length of the longest row.
    pub fn width(&self) -> usize {
        self.width
    }

    /// The character at `p`; cells past the end of a row are spaces.
    pub fn get(&self, p: Pos) -> char {
        self.rows.get(p.row).and_then(|row| row.get(p.col)).copied().unwrap_or(' ')
    }

    pub fn row(&self, row: usize) -> &[char] {
        self.rows.get(row).map_or(&[], Vec::as_slice)
    }

    /// The byte offset of `p` in the original source, clamped to its row's end.
    pub fn byte_offset(&self, p: Pos) -> usize {
        let Some(&start) = self.line_starts.get(p.row) else {
            return self.len;
        };
        start + self.rows[p.row].iter().take(p.col).map(|c| c.len_utf8()).sum::<usize>()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GridError {
    pub pos: Pos,
    pub byte: usize,
    pub kind: GridErrorKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GridErrorKind {
    Tab,
    StrayCarriageReturn,
    OutsideRepertoire(char),
}

impl fmt::Display for GridError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}: {}", self.pos, self.kind.code(), self.kind)
    }
}

impl GridErrorKind {
    pub fn code(&self) -> Code {
        match self {
            GridErrorKind::Tab => "G001",
            GridErrorKind::StrayCarriageReturn => "G002",
            GridErrorKind::OutsideRepertoire(_) => "G003",
        }
    }
}

impl fmt::Display for GridErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            GridErrorKind::Tab => f.write_str("tab; use spaces"),
            GridErrorKind::StrayCarriageReturn => f.write_str("stray carriage return"),
            GridErrorKind::OutsideRepertoire(ch) => {
                write!(f, "{ch:?} (U+{:04X}) is outside the repertoire", ch as u32)
            }
        }
    }
}

/// A stable diagnostic code such as `S001`. The corpus checks codes, never
/// message wording. The registry is SPEC §14; the first letter names the stage
/// (`G`rid, `S`hape, s`Y`ntax, `C`heck, `R`untime) or marks a `W`arning or `L`int.
pub type Code = &'static str;

/// A located message from any stage of the pipeline.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub at: Pos,
    pub code: Code,
    pub message: String,
}

impl Diagnostic {
    pub fn new(at: Pos, code: Code, message: impl Into<String>) -> Self {
        Self { at, code, message: message.into() }
    }

    /// Warnings and lints don't stop a program from running.
    pub fn is_warning(&self) -> bool {
        self.code.starts_with(['W', 'L'])
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}: {}", self.at, self.code, self.message)
    }
}

impl From<GridError> for Diagnostic {
    fn from(e: GridError) -> Self {
        Diagnostic::new(e.pos, e.kind.code(), e.kind.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shavian_is_one_cell() {
        let grid = Grid::parse("·𐑛𐑳𐑚 ─▶ 2\n").unwrap();
        assert_eq!(grid.width(), 9);
        assert_eq!(grid.get(Pos::new(0, 1)), '𐑛');
        assert_eq!(grid.byte_offset(Pos::new(0, 2)), '·'.len_utf8() + 4);
    }

    #[test]
    fn rejects_outside_repertoire() {
        let errors = Grid::parse("𐑐\n\tab").unwrap_err();
        let kinds: Vec<_> = errors.iter().map(|e| (e.pos, e.kind.clone())).collect();
        assert_eq!(kinds[0], (Pos::new(1, 0), GridErrorKind::Tab));
        assert_eq!(kinds[1], (Pos::new(1, 1), GridErrorKind::OutsideRepertoire('a')));
        assert_eq!(errors[0].byte, "𐑐\n".len());
    }

    #[test]
    fn crlf_and_trailing_newline() {
        let grid = Grid::parse("│\r\n│\r\n").unwrap();
        assert_eq!(grid.height(), 2);
        assert_eq!(grid.byte_offset(Pos::new(1, 0)), "│\r\n".len());
    }
}
