//! Grid cells versus positions in the text as an editor counts them (PLAN
//! P10): the language server's line and character, and Studio's textarea
//! offsets.
//!
//! A grid column counts characters (SPEC §1.4), but an editor counts code
//! units of some encoding. Shavian letters lie outside the BMP, so under
//! UTF-16 each one is two units wide while still one cell. A leading
//! byte-order mark is in the text but not on the grid.

use shawline_grid::Pos;

const BOM: char = '\u{feff}';

/// What an editor counts positions in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Encoding {
    /// Code points: one unit per character.
    Utf32,
    /// UTF-16 code units: JavaScript strings, and the LSP default.
    Utf16,
}

impl Encoding {
    /// How many units `c` takes.
    pub fn units(self, c: char) -> u32 {
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

/// The units before character index `col` on line `row`. Columns past the
/// end of the line count one unit each, like the grid's implicit spaces.
pub fn units_before(text: &str, enc: Encoding, row: usize, col: usize) -> u32 {
    let (mut units, mut seen) = (0u32, 0usize);
    for c in line(text, row).chars().take(col) {
        units += enc.units(c);
        seen += 1;
    }
    let past = u32::try_from(col - seen).unwrap_or(u32::MAX);
    units.saturating_add(past)
}

/// The character index on line `row` that `units` units reach. An offset
/// inside a character (half a surrogate pair) rounds down to its start.
/// Past the end of the line each unit is one column, counted in one step,
/// since a caller may pass any `u32`.
pub fn col_at(text: &str, enc: Encoding, row: usize, units: u32) -> usize {
    let (mut seen, mut col) = (0u32, 0usize);
    for c in line(text, row).chars() {
        let next = seen.saturating_add(enc.units(c));
        if next > units {
            return col;
        }
        seen = next;
        col += 1;
    }
    col.saturating_add((units - seen) as usize)
}

/// The units before grid cell `p` on its line.
pub fn cell_units(text: &str, enc: Encoding, p: Pos) -> u32 {
    units_before(text, enc, p.row, raw_col(text, p))
}

/// The grid cell `units` units into line `row`.
pub fn cell_at(text: &str, enc: Encoding, row: usize, units: u32) -> Pos {
    let col = col_at(text, enc, row, units);
    let bom = usize::from(row == 0 && text.starts_with(BOM));
    Pos::new(row, col.saturating_sub(bom))
}

/// The line and units just past the last character of `text`.
pub fn end(text: &str, enc: Encoding) -> (usize, u32) {
    let row = text.split('\n').count() - 1;
    let last = text.rsplit('\n').next().unwrap_or("");
    (row, last.chars().map(|c| enc.units(c)).sum())
}

/// The offset into the whole of `text`, in units, of grid cell `p`: what a
/// textarea's selection holds. Every line ending counts.
pub fn offset_of_cell(text: &str, enc: Encoding, p: Pos) -> usize {
    let before: usize = text
        .split('\n')
        .take(p.row)
        .map(|l| l.chars().map(|c| enc.units(c) as usize).sum::<usize>() + 1)
        .sum();
    before + cell_units(text, enc, p) as usize
}

/// The grid cell at an offset into the whole of `text`, in units:
/// [`offset_of_cell`]'s inverse. An offset on a line ending is the cell
/// just past that line's last character; one past the end of the text is
/// on its last line.
pub fn cell_of_offset(text: &str, enc: Encoding, offset: usize) -> Pos {
    let mut start = 0usize;
    let rows = text.split('\n').count();
    for (row, l) in text.split('\n').enumerate() {
        let width: usize = l.chars().map(|c| enc.units(c) as usize).sum();
        if offset <= start + width || row + 1 == rows {
            let units = u32::try_from(offset - start).unwrap_or(u32::MAX);
            return cell_at(text, enc, row, units);
        }
        start += width + 1;
    }
    unreachable!("split always yields a last line")
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT: &str = "※ 𐑛𐑳𐑚\n▶ 𐑯 ──┬─▶ 𐑐 ├\r\nx";

    #[test]
    fn offsets_count_every_unit_and_line_ending() {
        let enc = Encoding::Utf16;
        // Line 0 is 8 units and its newline 1, so line 1 starts at 9.
        assert_eq!(offset_of_cell(TEXT, enc, Pos::new(1, 0)), 9);
        // `▶`, space, `𐑯` (two units), space: cell 4 is unit 5.
        assert_eq!(offset_of_cell(TEXT, enc, Pos::new(1, 4)), 14);
        assert_eq!(cell_of_offset(TEXT, enc, 14), Pos::new(1, 4));
        // Line 1 is 16 units with its `\r`, so line 2 starts at 26.
        assert_eq!(offset_of_cell(TEXT, enc, Pos::new(2, 0)), 26);
        assert_eq!(cell_of_offset(TEXT, enc, 26), Pos::new(2, 0));
        assert_eq!(cell_of_offset(TEXT, enc, 99), Pos::new(2, 73));
        for p in [Pos::new(0, 0), Pos::new(0, 3), Pos::new(1, 11), Pos::new(1, 14), Pos::new(2, 1)] {
            assert_eq!(cell_of_offset(TEXT, enc, offset_of_cell(TEXT, enc, p)), p);
            let utf32 = Encoding::Utf32;
            assert_eq!(cell_of_offset(TEXT, utf32, offset_of_cell(TEXT, utf32, p)), p);
        }
    }

    #[test]
    fn a_byte_order_mark_is_counted_but_not_a_cell() {
        let text = "\u{feff}𐑐 x\n𐑐 x";
        let enc = Encoding::Utf16;
        assert_eq!(offset_of_cell(text, enc, Pos::new(0, 0)), 1);
        assert_eq!(cell_of_offset(text, enc, 1), Pos::new(0, 0));
        assert_eq!(cell_of_offset(text, enc, 0), Pos::new(0, 0));
        assert_eq!(offset_of_cell(text, enc, Pos::new(1, 1)), 8);
    }
}
