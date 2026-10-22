//! Gloss glyphs: the Latin letters the gloss layer writes readings and
//! primitive names in (PLAN P9). They are drawn with the same [`pen`] as the
//! repertoire but are not part of it, so Shawline Mono doesn't carry them.
//!
//! The signatures are a contract; the skeletons here are placeholders until
//! the real ones land.
//!
//! [`pen`]: crate::pen

use kurbo::BezPath;

use crate::{CELL_HEIGHT, CELL_WIDTH, Skeleton};

/// Width of one gloss glyph's box, in glyph units: half a cell.
pub const GLOSS_WIDTH: f64 = CELL_WIDTH / 2.0;
/// Height of one gloss glyph's box, in glyph units: half a cell. Its guides
/// are half of [`metrics`](crate::metrics), so a gloss letter is half the
/// width and height of a Shavian one.
pub const GLOSS_HEIGHT: f64 = CELL_HEIGHT / 2.0;
/// Every character with a gloss skeleton: what readings and primitive names
/// are spelled in.
pub const GLOSS_CHARS: &str = "abcdefghijklmnopqrstuvwxyz@=/";

/// The skeleton of `ch` in a [`GLOSS_WIDTH`] × [`GLOSS_HEIGHT`] box (origin
/// top-left, y down), or `None` if `ch` is not in [`GLOSS_CHARS`].
pub fn gloss_skeleton(ch: char) -> Option<Skeleton> {
    if !GLOSS_CHARS.contains(ch) {
        return None;
    }
    // Placeholder: an x-height box, so a strip can be drawn and measured.
    let mut path = BezPath::new();
    path.move_to((75.0, 200.0));
    path.line_to((225.0, 200.0));
    path.line_to((225.0, 400.0));
    path.line_to((75.0, 400.0));
    path.line_to((75.0, 200.0));
    Some(Skeleton { strokes: vec![path], fills: vec![] })
}
