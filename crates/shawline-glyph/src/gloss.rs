//! Gloss glyphs: the Latin letters the gloss layer writes readings and
//! primitive names in (PLAN P9). They are drawn with the same [`pen`] as the
//! repertoire but are not part of it, so Shawline Mono doesn't carry them.
//!
//! The signatures are a contract. The letters are a monoline geometric
//! lowercase in the Shavian letters' construction: bowls are ellipses, stems
//! are straight, and a letter that is another one turned or mirrored is
//! derived by that transform. At half a cell the full stroke weight is heavy,
//! so counters and apertures are kept as open as the box allows.
//!
//! [`pen`]: crate::pen

use kurbo::{Affine, BezPath};

use crate::{CELL_HEIGHT, CELL_WIDTH, STROKE_WEIGHT, Skeleton, ellipse, paths, strokes_only};

/// Width of one gloss glyph's box, in glyph units: half a cell.
pub const GLOSS_WIDTH: f64 = CELL_WIDTH / 2.0;
/// Height of one gloss glyph's box, in glyph units: half a cell. Its guides
/// are half of [`metrics`](crate::metrics), so a gloss letter is half the
/// width and height of a Shavian one.
pub const GLOSS_HEIGHT: f64 = CELL_HEIGHT / 2.0;
/// Every character with a gloss skeleton: what readings and primitive names
/// are spelled in.
pub const GLOSS_CHARS: &str = "abcdefghijklmnopqrstuvwxyzə-";

/// Ink radius of the dots on `i` and `j`: a little over the stroke, as a
/// half-size [`DOT_RADIUS`](crate::DOT_RADIUS) would be thinner than the pen.
const DOT: f64 = 44.0;

/// Mirrored left to right within the gloss box.
const FLIP_H: Affine = Affine::new([-1.0, 0.0, 0.0, 1.0, GLOSS_WIDTH, 0.0]);
/// Mirrored about the gloss axis: ascender ↔ descender, x-height stays.
const FLIP_V: Affine = Affine::new([1.0, 0.0, 0.0, -1.0, 0.0, GLOSS_HEIGHT]);
/// 180° about the box centre.
const ROTATE: Affine = Affine::new([-1.0, 0.0, 0.0, -1.0, GLOSS_WIDTH, GLOSS_HEIGHT]);

/// The skeleton of `ch` in a [`GLOSS_WIDTH`] × [`GLOSS_HEIGHT`] box (origin
/// top-left, y down), or `None` if `ch` is not in [`GLOSS_CHARS`].
pub fn gloss_skeleton(ch: char) -> Option<Skeleton> {
    // Guides: ascender 55, x-height 200, axis 300, baseline 400, descender
    // 545. Stems stand at x = 75 and 225 and bowls span the same width, so
    // counters and the gaps between letters are both about a stroke wide.
    let bowl_and = |bowl: BezPath, stem: &str| Skeleton { strokes: [vec![bowl], paths(&[stem])].concat(), fills: vec![] };
    let with_bowl = |stem: &str| bowl_and(ellipse(150.0, 300.0, 75.0, 100.0), stem);
    // The dot's ink tops out with an ascender's.
    let r = DOT - STROKE_WEIGHT / 2.0;
    let dotted = |x: f64, stem: &[&str]| Skeleton { strokes: paths(stem), fills: vec![ellipse(x, 55.0 + r, r, r)] };
    let arch = "M75,290 C75,235 106,200 150,200 C194,200 225,235 225,290 L225,400";
    Some(match ch {
        'a' => with_bowl("M225,200 L225,400"),
        'b' => with_bowl("M75,55 L75,400"),
        'c' => strokes_only(&[
            "M219,240 C202,214 179,200 150,200 C107,200 72,245 72,300 C72,355 107,400 150,400 \
             C179,400 202,386 219,360",
        ]),
        'd' => gloss_skeleton('b')?.transformed(FLIP_H),
        // Three strokes stacked in the x-height leave little white, and the
        // bar sits low to give most of it to the closed eye: the aperture
        // below is open to the right and reads anyway.
        'e' => strokes_only(&[
            "M72,318 L228,318 C228,251 193,200 150,200 C107,200 72,245 72,300 C72,355 107,400 150,400 \
             C178,400 202,392 220,374",
        ]),
        'f' => strokes_only(&[
            "M232,88 C216,66 196,55 172,55 C135,55 118,80 118,125 L118,400",
            "M58,200 L228,200",
        ]),
        'g' => with_bowl("M225,200 L225,455 C225,514 192,545 146,545 C112,545 88,533 72,512"),
        'h' => strokes_only(&["M75,55 L75,400", arch]),
        'i' => dotted(150.0, &["M82,200 L150,200 L150,400", "M72,400 L228,400"]),
        'j' => dotted(172.0, &["M92,200 L172,200 L172,465 C172,516 144,545 104,545 C84,545 68,538 56,526"]),
        'k' => strokes_only(&["M80,55 L80,400", "M226,200 L80,320", "M140,271 L230,400"]),
        // A tail and a serif, so it can't pass for `i` or a vertical wire.
        'l' => strokes_only(&["M82,55 L150,55 L150,328 C150,377 176,400 222,400"]),
        'm' => strokes_only(&[
            "M42,200 L42,400",
            "M42,282 C42,231 64,200 96,200 C128,200 150,231 150,282 L150,400",
            "M150,282 C150,231 172,200 204,200 C236,200 258,231 258,282 L258,400",
        ]),
        'n' => strokes_only(&["M75,200 L75,400", arch]),
        'o' => Skeleton { strokes: vec![ellipse(150.0, 300.0, 78.0, 100.0)], fills: vec![] },
        'p' => gloss_skeleton('b')?.transformed(FLIP_V),
        'q' => gloss_skeleton('b')?.transformed(ROTATE),
        'r' => strokes_only(&["M92,200 L92,400", "M92,290 C92,234 126,200 180,200 C201,200 219,205 234,215"]),
        's' => strokes_only(&[
            "M220,234 C205,212 180,200 150,200 C106,200 80,222 80,251 C80,281 108,292 150,300 \
             C192,308 220,319 220,349 C220,378 194,400 150,400 C116,400 92,389 76,366",
        ]),
        't' => strokes_only(&[
            "M118,95 L118,335 C118,381 142,400 182,400 C202,400 219,395 232,386",
            "M58,200 L228,200",
        ]),
        'u' => gloss_skeleton('n')?.transformed(ROTATE),
        'v' => strokes_only(&["M66,200 L150,400 L234,200"]),
        'w' => strokes_only(&["M40,200 L95,400 L150,250 L205,400 L260,200"]),
        'x' => strokes_only(&["M70,200 L230,400", "M230,200 L70,400"]),
        'y' => strokes_only(&["M66,200 L150,400", "M234,200 L90,545"]),
        'z' => strokes_only(&["M75,200 L225,200 L75,400 L225,400"]),
        // The schwa is `e` turned, as its IPA form is.
        'ə' => gloss_skeleton('e')?.transformed(ROTATE),
        // On the axis, so it reads as a break between letters, not a dash.
        '-' => strokes_only(&["M96,300 L204,300"]),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use kurbo::PathEl;

    fn all() -> impl Iterator<Item = (char, Skeleton)> {
        GLOSS_CHARS.chars().map(|ch| (ch, gloss_skeleton(ch).unwrap_or_else(|| panic!("{ch:?} has no skeleton"))))
    }

    #[test]
    fn exactly_the_gloss_characters_have_skeletons() {
        for (ch, s) in all() {
            assert!(!s.is_empty(), "{ch:?} is empty");
        }
        let others = (0..=0x7f).chain(0x10450..=0x1047f).filter_map(char::from_u32).chain(['·', 'é', '─']);
        for ch in others.filter(|c| !GLOSS_CHARS.contains(*c)) {
            assert_eq!(gloss_skeleton(ch), None, "{ch:?}");
        }
    }

    #[test]
    fn no_two_glyphs_are_drawn_alike() {
        let glyphs: Vec<_> = all().collect();
        for (i, (a, sa)) in glyphs.iter().enumerate() {
            for (b, sb) in &glyphs[i + 1..] {
                assert_ne!(sa, sb, "{a:?} and {b:?} are drawn alike");
            }
        }
    }

    #[test]
    fn ink_stays_inside_the_box() {
        let half = STROKE_WEIGHT / 2.0;
        for (ch, s) in all() {
            let b = s.bounds().unwrap().inflate(half, half);
            assert!(
                b.x0 >= 0.0 && b.y0 >= 0.0 && b.x1 <= GLOSS_WIDTH && b.y1 <= GLOSS_HEIGHT,
                "{ch:?} ink {b:?} leaves the box"
            );
        }
    }

    #[test]
    fn strokes_are_single_open_subpaths() {
        for (ch, s) in all() {
            for p in &s.strokes {
                let moves = p.elements().iter().filter(|e| matches!(e, PathEl::MoveTo(_))).count();
                let closed = p.elements().iter().any(|e| matches!(e, PathEl::ClosePath));
                assert!(moves == 1 && !closed, "{ch:?} has a stroke that is not one open subpath");
            }
        }
    }

    #[test]
    fn letters_occupy_their_height_class() {
        let (ascender, x_height, baseline, descender) = (55.0, 200.0, 400.0, 545.0);
        for (ch, s) in all().filter(|(c, _)| c.is_ascii_lowercase()) {
            let b = s.bounds().unwrap();
            let top = if "bdfhijkl".contains(ch) {
                ascender
            } else if ch == 't' {
                95.0
            } else {
                x_height
            };
            let bottom = if "gjpqy".contains(ch) { descender } else { baseline };
            assert!((b.y0 - top).abs() < 0.5 && (b.y1 - bottom).abs() < 0.5, "{ch:?} spans {b:?}");
        }
    }
}
