//! Grid cells versus LSP positions, over `shawline_doc::position`, which
//! Studio shares.
//!
//! A grid column counts characters (SPEC §1.4), but an LSP `character`
//! counts code units of the negotiated encoding. Shavian letters lie outside
//! the BMP, so under UTF-16 each one is two units wide while still one cell.

use lsp_types::{Position, PositionEncodingKind, Range};
use shawline_doc::position as cells;
pub use shawline_doc::position::{Encoding, line};
use shawline_grid::Pos;

/// Picks UTF-32 when the client offers it, else the mandatory UTF-16.
pub fn negotiate(offered: Option<&[PositionEncodingKind]>) -> Encoding {
    match offered {
        Some(kinds) if kinds.contains(&PositionEncodingKind::UTF32) => Encoding::Utf32,
        _ => Encoding::Utf16,
    }
}

/// The LSP name of `enc`.
pub fn kind(enc: Encoding) -> PositionEncodingKind {
    match enc {
        Encoding::Utf32 => PositionEncodingKind::UTF32,
        Encoding::Utf16 => PositionEncodingKind::UTF16,
    }
}

/// The LSP position of character index `col` on line `row`. Columns past the
/// end of the line count one unit each, like the grid's implicit spaces.
pub fn to_lsp(text: &str, enc: Encoding, row: usize, col: usize) -> Position {
    Position::new(row as u32, cells::units_before(text, enc, row, col))
}

/// The LSP position of grid cell `p`.
pub fn cell_to_lsp(text: &str, enc: Encoding, p: Pos) -> Position {
    Position::new(p.row as u32, cells::cell_units(text, enc, p))
}

/// The range covering `width` cells from grid cell `p`.
pub fn cells_range(text: &str, enc: Encoding, p: Pos, width: usize) -> Range {
    let col = cells::raw_col(text, p);
    Range::new(to_lsp(text, enc, p.row, col), to_lsp(text, enc, p.row, col + width))
}

/// The character index on its raw line of an LSP position. A UTF-16 offset
/// that splits a surrogate pair rounds down to the start of that character.
pub fn from_lsp(text: &str, enc: Encoding, pos: Position) -> usize {
    cells::col_at(text, enc, pos.line as usize, pos.character)
}

/// The grid cell of an LSP position.
pub fn lsp_to_cell(text: &str, enc: Encoding, pos: Position) -> Pos {
    cells::cell_at(text, enc, pos.line as usize, pos.character)
}

/// The position just past the last character of `text`.
pub fn end_of(text: &str, enc: Encoding) -> Position {
    let (row, units) = cells::end(text, enc);
    Position::new(row as u32, units)
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
        assert_eq!(negotiate(None), Encoding::Utf16);
        assert_eq!(
            negotiate(Some(&[K::UTF8, K::UTF16])),
            Encoding::Utf16
        );
        assert_eq!(
            negotiate(Some(&[K::UTF16, K::UTF32])),
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
