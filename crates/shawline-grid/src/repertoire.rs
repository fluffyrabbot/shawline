//! The closed character repertoire.
//!
//! Every character a Shawline source may contain is listed here; anything else
//! is an error. Each one occupies exactly one grid cell, which Shawline Mono
//! guarantees, so column arithmetic is code-point arithmetic.

use std::ops::RangeInclusive;

/// The Shavian block: 48 letters.
pub const SHAVIAN: RangeInclusive<char> = '\u{10450}'..='\u{1047F}';

/// Marks a definition name, as it marks proper names in Shavian text.
pub const NAMER_DOT: char = '·';

/// Characters that draw: box drawing, arrowheads, and their ASCII fallbacks.
/// `shawline-shape` gives each one its connectivity.
pub const STROKES: &[char] = &[
    '─', '│', '┌', '┐', '└', '┘', '├', '┤', '┬', '┴', '┼', '╭', '╮', '╯', '╰', '┄', '┆', '▶', '◀',
    '▲', '▼', // canonical
    '-', '|', '+', '.', '\'', '~', ':', '>', '<', '^', 'v', // ASCII fallback
];

/// Text delimiters: `«literal»` and `※ comment`.
pub const MARKS: &[char] = &['«', '»', '※'];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Class {
    Space,
    Shavian,
    NamerDot,
    Digit,
    Stroke,
    Mark,
}

/// The repertoire class of `ch`, or `None` if Shawline source may not contain it.
pub fn classify(ch: char) -> Option<Class> {
    Some(match ch {
        ' ' => Class::Space,
        '0'..='9' => Class::Digit,
        NAMER_DOT => Class::NamerDot,
        _ if SHAVIAN.contains(&ch) => Class::Shavian,
        _ if STROKES.contains(&ch) => Class::Stroke,
        _ if MARKS.contains(&ch) => Class::Mark,
        _ => return None,
    })
}

/// True for characters that may appear inside a word.
pub fn is_word_char(ch: char) -> bool {
    matches!(classify(ch), Some(Class::Shavian | Class::NamerDot | Class::Digit))
}
