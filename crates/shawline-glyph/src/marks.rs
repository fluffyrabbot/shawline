//! Digits, the namer dot and the text marks `« » ※`.

use kurbo::Affine;

use crate::metrics::{AXIS, BASELINE, FIGURE_HEIGHT};
use crate::{FLIP_H, Skeleton, dot, ellipse, strokes_only};

pub(crate) fn skeleton(ch: char) -> Option<Skeleton> {
    Some(match ch {
        // Shavian's namer dot sits mid x-height, which is also the wire axis.
        '·' => Skeleton { strokes: vec![], fills: vec![dot(300.0, AXIS)] },
        '«' => strokes_only(&["M275,460 L165,600 L275,740", "M435,460 L325,600 L435,740"]),
        '»' => skeleton('«')?.transformed(FLIP_H),
        '※' => Skeleton {
            strokes: crate::paths(&["M195,495 L405,705", "M405,495 L195,705"]),
            fills: vec![dot(300.0, 390.0), dot(300.0, 810.0), dot(100.0, 600.0), dot(500.0, 600.0)],
        },
        '0'..='9' => digit(ch),
        _ => return None,
    })
}

/// Lining figures from the baseline up to `FIGURE_HEIGHT`, between x-height
/// and ascender so numbers stand out from short letters without towering.
fn digit(ch: char) -> Skeleton {
    let mid = (FIGURE_HEIGHT + BASELINE) / 2.0;
    let oval = |cy: f64, rx: f64, ry: f64| Skeleton { strokes: vec![ellipse(300.0, cy, rx, ry)], fills: vec![] };
    match ch {
        '0' => oval(mid, 125.0, (BASELINE - FIGURE_HEIGHT) / 2.0),
        '1' => strokes_only(&["M195,375 L320,280 L320,800"]),
        '2' => strokes_only(&["M185,370 C195,315 240,280 300,280 C370,280 415,325 415,395 C415,475 355,545 185,800 L425,800"]),
        '3' => strokes_only(&[
            "M185,345 C205,302 250,280 300,280 C365,280 410,320 410,390 C410,460 360,510 280,510 \
             C375,510 425,565 425,650 C425,745 365,800 295,800 C240,800 195,775 178,735",
        ]),
        '4' => strokes_only(&["M370,800 L370,280 L170,620 L440,620"]),
        '5' => strokes_only(&[
            "M415,280 L215,280 L200,495 C235,475 265,468 300,468 C378,468 425,530 425,630 \
             C425,740 368,800 295,800 C240,800 198,778 178,740",
        ]),
        '6' => strokes_only(&[
            "M395,310 C370,290 340,280 305,280 C220,280 175,380 175,540 L175,640 C175,740 230,800 300,800 \
             C375,800 425,745 425,660 C425,575 375,520 300,520 C230,520 180,565 175,630",
        ]),
        '7' => strokes_only(&["M175,280 L425,280 L250,800"]),
        '8' => Skeleton {
            strokes: vec![ellipse(300.0, 402.0, 108.0, 122.0), ellipse(300.0, 662.0, 128.0, 138.0)],
            fills: vec![],
        },
        // A 9 is a 6 turned about the figure's centre.
        '9' => digit('6').transformed(Affine::new([-1.0, 0.0, 0.0, -1.0, 600.0, 2.0 * mid])),
        _ => unreachable!("not a digit: {ch:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digits_stand_on_the_baseline_at_figure_height() {
        for ch in '0'..='9' {
            let b = skeleton(ch).unwrap().bounds().unwrap();
            assert!((b.y0 - FIGURE_HEIGHT).abs() < 0.5 && (b.y1 - BASELINE).abs() < 0.5, "{ch:?} {b:?}");
        }
    }
}
