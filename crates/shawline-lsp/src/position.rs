//! Grid cells versus LSP positions.
//!
//! A grid column counts characters (SPEC §1.4), but an LSP `character`
//! counts code units of the negotiated encoding. Shavian letters lie outside
//! the BMP, so under UTF-16 each one is two units wide while still one cell.

use lsp_types::{Position, PositionEncodingKind, Range};
use shawline_grid::Pos;

const BOM: char = '\u{feff}';

/// The position encoding agreed with the client.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Encoding {
    /// Columns are code points: one unit per grid cell.
    Utf32,
    /// The LSP default.
    Utf16,
}

impl Encoding {
    /// Picks UTF-32 when the client offers it, else the mandatory UTF-16.
    pub fn negotiate(offered: Option<&[PositionEncodingKind]>) -> Encoding {
        match offered {
            Some(kinds) if kinds.contains(&PositionEncodingKind::UTF32) => Encoding::Utf32,
            _ => Encoding::Utf16,
        }
    }

    pub fn kind(self) -> PositionEncodingKind {
        match self {
            Encoding::Utf32 => PositionEncodingKind::UTF32,
            Encoding::Utf16 => PositionEncodingKind::UTF16,
        }
    }

    fn units(self, c: char) -> u32 {
        match self {
            Encoding::Utf32 => 1,
            Encoding::Utf16 => c.len_utf16() as u32,
        }
    }
}

/// Line `row` of `text` without its line ending, as the grid splits it.
pub fn line(text: &str, row: usize) -> &str {
    let l = text.split('\n').nth(row).unwrap_or("");
    l.strip_suffix('\r').unwrap_or(l)
}

/// The character index on the raw line of grid cell `p`. They differ only
/// on a first line that starts with a byte-order mark, which the grid drops.
pub fn raw_col(text: &str, p: Pos) -> usize {
    p.col + usize::from(p.row == 0 && text.starts_with(BOM))
}

/// The LSP position of character index `col` on line `row`. Columns past the
/// end of the line count one unit each, like the grid's implicit spaces.
pub fn to_lsp(text: &str, enc: Encoding, row: usize, col: usize) -> Position {
    let (mut units, mut seen) = (0u32, 0usize);
    for c in line(text, row).chars().take(col) {
        units += enc.units(c);
        seen += 1;
    }
    let past = u32::try_from(col - seen).unwrap_or(u32::MAX);
    Position::new(row as u32, units.saturating_add(past))
}

/// The LSP position of grid cell `p`.
pub fn cell_to_lsp(text: &str, enc: Encoding, p: Pos) -> Position {
    to_lsp(text, enc, p.row, raw_col(text, p))
}

/// The range covering `width` cells from grid cell `p`.
pub fn cells_range(text: &str, enc: Encoding, p: Pos, width: usize) -> Range {
    let col = raw_col(text, p);
    Range::new(
        to_lsp(text, enc, p.row, col),
        to_lsp(text, enc, p.row, col + width),
    )
}

/// The character index on its raw line of an LSP position. A UTF-16 offset
/// that splits a surrogate pair rounds down to the start of that character.
/// Past the end of the line each unit is one column, counted in one step:
/// a client may send any `u32`.
pub fn from_lsp(text: &str, enc: Encoding, pos: Position) -> usize {
    let mut units = 0u32;
    let mut col = 0usize;
    for c in line(text, pos.line as usize).chars() {
        let next = units.saturating_add(enc.units(c));
        if next > pos.character {
            return col;
        }
        units = next;
        col += 1;
    }
    col.saturating_add((pos.character - units) as usize)
}

/// The grid cell of an LSP position.
pub fn lsp_to_cell(text: &str, enc: Encoding, pos: Position) -> Pos {
    let row = pos.line as usize;
    let col = from_lsp(text, enc, pos);
    let bom = usize::from(row == 0 && text.starts_with(BOM));
    Pos::new(row, col.saturating_sub(bom))
}

/// The position just past the last character of `text`.
pub fn end_of(text: &str, enc: Encoding) -> Position {
    let row = text.split('\n').count() - 1;
    let last = text.rsplit('\n').next().unwrap_or("");
    Position::new(row as u32, last.chars().map(|c| enc.units(c)).sum())
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT: &str = "※ 𐑛𐑳𐑚\n▶ 𐑯 ──┬─▶ 𐑐 ├\r\nx";

    #[test]
    fn utf16_counts_shavian_as_two_units() {
        let enc = Encoding::Utf16;
        // `▶`, space, `𐑯`, space: the `─` after them is cell 4, unit 5.
        assert_eq!(cell_to_lsp(TEXT, enc, Pos::new(1, 4)), Position::new(1, 5));
        assert_eq!(
            cell_to_lsp(TEXT, enc, Pos::new(1, 10)),
            Position::new(1, 11)
        );
        assert_eq!(
            cell_to_lsp(TEXT, enc, Pos::new(1, 11)),
            Position::new(1, 13)
        );
        assert_eq!(
            lsp_to_cell(TEXT, enc, Position::new(1, 13)),
            Pos::new(1, 11)
        );
        // Halfway through `𐑐` rounds down to `𐑐` itself.
        assert_eq!(
            lsp_to_cell(TEXT, enc, Position::new(1, 12)),
            Pos::new(1, 10)
        );
        assert_eq!(
            cells_range(TEXT, enc, Pos::new(0, 2), 3),
            Range::new(Position::new(0, 2), Position::new(0, 8))
        );
    }

    #[test]
    fn utf32_is_the_grid_column() {
        let enc = Encoding::Utf32;
        for col in [0, 4, 10, 11, 14] {
            let p = Pos::new(1, col);
            assert_eq!(cell_to_lsp(TEXT, enc, p), Position::new(1, col as u32));
            assert_eq!(lsp_to_cell(TEXT, enc, Position::new(1, col as u32)), p);
        }
    }

    #[test]
    fn negotiation_prefers_utf32() {
        use PositionEncodingKind as K;
        assert_eq!(Encoding::negotiate(None), Encoding::Utf16);
        assert_eq!(
            Encoding::negotiate(Some(&[K::UTF8, K::UTF16])),
            Encoding::Utf16
        );
        assert_eq!(
            Encoding::negotiate(Some(&[K::UTF16, K::UTF32])),
            Encoding::Utf32
        );
    }

    #[test]
    fn bom_shifts_only_the_first_line() {
        let text = "\u{feff}𐑐 x\n𐑐 x";
        let enc = Encoding::Utf16;
        assert_eq!(cell_to_lsp(text, enc, Pos::new(0, 1)), Position::new(0, 3));
        assert_eq!(cell_to_lsp(text, enc, Pos::new(1, 1)), Position::new(1, 2));
        assert_eq!(lsp_to_cell(text, enc, Position::new(0, 3)), Pos::new(0, 1));
    }

    #[test]
    fn far_past_the_end_takes_no_time() {
        // Line 1 is 13 characters: 15 UTF-16 units, with its two letters.
        let far = Position::new(1, u32::MAX);
        let max = u32::MAX as usize;
        assert_eq!(from_lsp(TEXT, Encoding::Utf16, far), 13 + (max - 15));
        assert_eq!(from_lsp(TEXT, Encoding::Utf32, far), max);
        assert_eq!(
            lsp_to_cell(TEXT, Encoding::Utf16, far),
            Pos::new(1, max - 2)
        );
        assert_eq!(
            to_lsp(TEXT, Encoding::Utf16, 1, usize::MAX),
            Position::new(1, u32::MAX)
        );
        // Just past the end, both ways agree.
        assert_eq!(to_lsp(TEXT, Encoding::Utf16, 1, 20), Position::new(1, 22));
        assert_eq!(from_lsp(TEXT, Encoding::Utf16, Position::new(1, 22)), 20);
    }

    #[test]
    fn end_of_text() {
        assert_eq!(end_of(TEXT, Encoding::Utf16), Position::new(2, 1));
        assert_eq!(end_of("𐑐\n", Encoding::Utf16), Position::new(1, 0));
        assert_eq!(end_of("𐑐", Encoding::Utf16), Position::new(0, 2));
    }
}
