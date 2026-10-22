//! The gloss layer's contract (PLAN P9): what [`render_with`] draws and what
//! [`glosses`] reports. English goes in a strip under every row, never in
//! the grid, and never changes what the sheet means.
//!
//! [`render_with`]: crate::render_with
//! [`glosses`]: crate::glosses

use std::fmt;

use shawline_grid::Pos;

use crate::Theme;

/// How to draw a sheet.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Options {
    pub theme: Theme,
    /// Open a strip under every row and show each word's gloss in it.
    pub gloss: bool,
}

/// One Shavian word on the sheet, and what the strip under its row shows
/// for it.
#[derive(Clone, Debug, PartialEq)]
pub struct Gloss {
    /// The word's first cell.
    pub at: Pos,
    /// The word as written: a maximal run of Shavian letters, namer dots and
    /// digits that isn't all digits. Words inside `«»` and comments count;
    /// numbers don't.
    pub word: String,
    /// How the word is typed: [`shawline_word::reading`].
    pub reading: String,
    /// The primitive's short name ([`Prim::gloss`]) when the word names a
    /// primitive node, boxed or inline.
    ///
    /// [`Prim::gloss`]: shawline_vm::bytecode::Prim::gloss
    pub gloss: Option<&'static str>,
    /// Where the strip shows [`Gloss::text`], as `(left, right)` in columns
    /// from the grid's left edge, or `None` when it is dropped.
    ///
    /// The text is centred under the word, one gloss advance per character.
    /// It is shown only if every character has a gloss skeleton, it lies
    /// within the grid's width, it covers no column a vertical stroke blocks
    /// in this strip, and it keeps at least one advance from every gloss
    /// already shown in row-major order. A column is blocked in a strip when
    /// a box's side runs through it there, or when the cell above the strip
    /// connects south or the cell below connects north. Anything else is
    /// dropped: never truncated, shifted or drawn over a stroke. Hover still
    /// has it.
    pub span: Option<(f64, f64)>,
}

impl Gloss {
    /// What the strip shows: the short name for a primitive, the reading for
    /// every other word.
    pub fn text(&self) -> &str {
        self.gloss.unwrap_or(&self.reading)
    }

    /// Whether the strip shows it.
    pub fn shown(&self) -> bool {
        self.span.is_some()
    }
}

/// The `.gloss` dump line: `ROW:COL WORD READING [GLOSS] shown|dropped`.
impl fmt::Display for Gloss {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {} {}", self.at, self.word, self.reading)?;
        if let Some(gloss) = self.gloss {
            write!(f, " {gloss}")?;
        }
        f.write_str(if self.shown() { " shown" } else { " dropped" })
    }
}
