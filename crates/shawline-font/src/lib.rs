//! Builds Shawline Mono, the TrueType font that draws every repertoire
//! character at exactly one cell, from the `shawline-glyph` skeletons.
//!
//! Glyph units are font units: one cell is [`CELL_WIDTH`] × [`CELL_HEIGHT`]
//! and the em is the cell height ([`UNITS_PER_EM`]), so line-height 1.0 stacks
//! rows with no gap and box drawing joins vertically. Each glyph is its
//! skeleton's strokes expanded by kurbo with the glyph pen, flipped from the
//! skeleton's y-down cell space to y-up about [`metrics::BASELINE`].
//!
//! Overlapping contours are kept, not unioned: every contour of a glyph is
//! wound the same way, so the TrueType nonzero fill paints their union, and the
//! glyph carries the `OVERLAP_SIMPLE` flag for rasterizers that want the hint.

use kurbo::{Affine, BezPath, PathEl, Point, Rect, Shape, StrokeOpts};
use shawline_glyph::{CELL_HEIGHT, CELL_WIDTH, STROKE_WEIGHT, Skeleton, metrics, pen, pen_for, skeleton};
use shawline_grid::Dir;
use shawline_grid::repertoire::{MARKS, NAMER_DOT, SHAVIAN, STROKES};
use write_fonts::FontBuilder;
use write_fonts::tables::cmap::Cmap;
use write_fonts::tables::glyf::{Bbox, GlyfLocaBuilder, Glyph, SimpleGlyph};
use write_fonts::tables::head::{Flags, Head, MacStyle};
use write_fonts::tables::hhea::Hhea;
use write_fonts::tables::hmtx::{Hmtx, LongMetric};
use write_fonts::tables::maxp::Maxp;
use write_fonts::tables::name::{Name, NameRecord};
use write_fonts::tables::os2::{Os2, SelectionFlags};
use write_fonts::tables::post::Post;
use write_fonts::types::{FWord, Fixed, GlyphId, LongDateTime, NameId, Tag, UfWord};

/// The family name in the `name` table.
pub const FAMILY: &str = "Shawline Mono";

/// Font units per em: the cell height, so one em is one row. CSS and editor
/// line heights are multiples of the em, and at 1.0 a row is then exactly a
/// cell. Keeping glyph units as font units also means no scaling, so every
/// guide in `shawline-glyph` lands on an integer.
pub const UNITS_PER_EM: u16 = CELL_HEIGHT as u16;

/// Advance width of every glyph.
pub const ADVANCE: u16 = CELL_WIDTH as u16;

/// Distance from the baseline up to the cell top (hhea, typo and win ascent).
pub const ASCENT: i16 = metrics::BASELINE as i16;

/// Distance from the baseline down to the cell bottom, as a positive number.
pub const DESCENT: i16 = (CELL_HEIGHT - metrics::BASELINE) as i16;

/// Stroke expansion tolerance in font units, far below a pixel at any size.
const STROKE_TOLERANCE: f64 = 0.1;
/// Allowed error of the quadratic approximation, before rounding to integers.
const QUAD_ACCURACY: f64 = 0.5;

/// Every character Shawline Mono maps, in glyph order after `.notdef`.
/// Space comes first; the rest is the closed repertoire of `shawline-grid`.
pub fn characters() -> impl Iterator<Item = char> {
    std::iter::once(' ')
        .chain(SHAVIAN)
        .chain('0'..='9')
        .chain([NAMER_DOT])
        .chain(STROKES.iter().copied())
        .chain(MARKS.iter().copied())
}

/// The skeleton a font draws for `ch`. A font glyph can't see the rows above
/// and below it, so canonical arrowheads always carry the border running
/// through them: nearly every one sits in a box border, and an inline node's
/// input arrowhead shows a small tick instead (SPEC §3).
fn font_skeleton(ch: char) -> Skeleton {
    let border = |points: Dir, across: [Dir; 2]| {
        shawline_glyph::arrowhead(points, &[points.opposite(), across[0], across[1]])
    };
    match ch {
        '▶' => border(Dir::E, [Dir::N, Dir::S]),
        '◀' => border(Dir::W, [Dir::N, Dir::S]),
        '▲' => border(Dir::N, [Dir::E, Dir::W]),
        '▼' => border(Dir::S, [Dir::E, Dir::W]),
        _ => skeleton(ch).unwrap_or_else(|| panic!("{ch:?} is in the repertoire but has no skeleton")),
    }
}

/// The Shawline Mono TrueType font.
pub fn build() -> Vec<u8> {
    let mut glyphs = vec![notdef()];
    let mut names = vec![".notdef".to_string()];
    let mut mappings = Vec::new();
    for ch in characters() {
        let sk = font_skeleton(ch);
        mappings.push((ch, GlyphId::new(glyphs.len() as u32)));
        glyphs.push(glyph(&sk));
        names.push(match ch {
            ' ' => "space".into(),
            c if (c as u32) <= 0xFFFF => format!("uni{:04X}", c as u32),
            c => format!("u{:05X}", c as u32),
        });
    }

    let mut glyf = GlyfLocaBuilder::new();
    for g in &glyphs {
        glyf.add_glyph(g).expect("glyph validates");
    }
    let (glyf, loca, loca_format) = glyf.build();

    let boxes: Vec<Bbox> = glyphs.iter().filter_map(Glyph::bbox).collect();
    let font_box = boxes.iter().copied().reduce(Bbox::union).unwrap_or_default();
    let lsb = |g: &Glyph| g.bbox().map_or(0, |b| b.x_min);
    let num_glyphs = glyphs.len() as u16;

    let head = Head {
        font_revision: Fixed::from_f64(0.001),
        flags: Flags::BASELINE_AT_Y_0 | Flags::LSB_AT_X_0 | Flags::FORCE_INTEGER_PPEM,
        units_per_em: UNITS_PER_EM,
        // A fixed timestamp keeps the build reproducible.
        created: LongDateTime::new(0),
        modified: LongDateTime::new(0),
        x_min: font_box.x_min,
        y_min: font_box.y_min,
        x_max: font_box.x_max,
        y_max: font_box.y_max,
        mac_style: MacStyle::empty(),
        lowest_rec_ppem: 8,
        index_to_loc_format: loca_format as i16,
        ..Default::default()
    };

    // One long metric covers every glyph: a monospaced font repeats the last
    // advance, and only the side bearings differ.
    let hmtx = Hmtx::new(
        vec![LongMetric::new(ADVANCE, lsb(&glyphs[0]))],
        glyphs[1..].iter().map(lsb).collect(),
    );
    let hhea = Hhea {
        ascender: FWord::new(ASCENT),
        descender: FWord::new(-DESCENT),
        line_gap: FWord::new(0),
        advance_width_max: UfWord::new(ADVANCE),
        min_left_side_bearing: FWord::new(boxes.iter().map(|b| b.x_min).min().unwrap_or(0)),
        min_right_side_bearing: FWord::new(boxes.iter().map(|b| ADVANCE as i16 - b.x_max).min().unwrap_or(0)),
        x_max_extent: FWord::new(boxes.iter().map(|b| b.x_max).max().unwrap_or(0)),
        caret_slope_rise: 1,
        caret_slope_run: 0,
        caret_offset: 0,
        number_of_h_metrics: 1,
    };

    let mut maxp = Maxp::new(num_glyphs);
    let simple = || glyphs.iter().filter_map(|g| if let Glyph::Simple(s) = g { Some(s) } else { None });
    maxp.max_points = Some(simple().map(|s| s.contours.iter().map(|c| c.len()).sum::<usize>()).max().unwrap_or(0) as u16);
    maxp.max_contours = Some(simple().map(|s| s.contours.len()).max().unwrap_or(0) as u16);
    maxp.max_composite_points = Some(0);
    maxp.max_composite_contours = Some(0);
    maxp.max_zones = Some(1);
    maxp.max_twilight_points = Some(0);
    maxp.max_storage = Some(0);
    maxp.max_function_defs = Some(0);
    maxp.max_instruction_defs = Some(0);
    maxp.max_stack_elements = Some(0);
    maxp.max_size_of_instructions = Some(0);
    maxp.max_component_elements = Some(0);
    maxp.max_component_depth = Some(0);

    let cell_y = |y: f64| (metrics::BASELINE - y) as i16;
    let os2 = Os2 {
        x_avg_char_width: ADVANCE as i16,
        y_subscript_x_size: 360,
        y_subscript_y_size: 720,
        y_subscript_y_offset: 150,
        y_superscript_x_size: 360,
        y_superscript_y_size: 720,
        y_superscript_y_offset: 500,
        y_strikeout_size: STROKE_WEIGHT as i16,
        y_strikeout_position: cell_y(metrics::AXIS),
        // Latin text, sans, book weight, monospaced: the proportion digit (9)
        // is what Windows and fontconfig read as "fixed pitch".
        panose_10: [2, 11, 5, 9, 0, 0, 0, 0, 0, 0],
        // Basic Latin, Latin-1, General Punctuation; Box Drawing, Geometric
        // Shapes, and non-BMP (Shavian has no bit of its own).
        ul_unicode_range_1: 1 << 0 | 1 << 1 | 1 << 31,
        ul_unicode_range_2: 1 << (43 - 32) | 1 << (45 - 32) | 1 << (57 - 32),
        ach_vend_id: Tag::new(b"NONE"),
        fs_selection: SelectionFlags::REGULAR | SelectionFlags::USE_TYPO_METRICS,
        us_first_char_index: ' ' as u16,
        us_last_char_index: 0xFFFF,
        s_typo_ascender: ASCENT,
        s_typo_descender: -DESCENT,
        s_typo_line_gap: 0,
        us_win_ascent: ASCENT as u16,
        us_win_descent: DESCENT as u16,
        ul_code_page_range_1: Some(1),
        ul_code_page_range_2: Some(0),
        sx_height: Some(cell_y(metrics::X_HEIGHT)),
        s_cap_height: Some(cell_y(metrics::ASCENDER)),
        us_default_char: Some(0),
        us_break_char: Some(' ' as u16),
        us_max_context: Some(0),
        ..Default::default()
    };

    let mut post = Post::new_v2(names.iter().map(String::as_str));
    post.underline_position = FWord::new(-cell_y(metrics::DESCENDER) / 2);
    post.underline_thickness = FWord::new(STROKE_WEIGHT as i16);
    post.is_fixed_pitch = 1;

    let version = format!("Version {}", env!("CARGO_PKG_VERSION"));
    let name = Name::new(
        [
            (NameId::FAMILY_NAME, FAMILY.to_string()),
            (NameId::SUBFAMILY_NAME, "Regular".into()),
            (NameId::UNIQUE_ID, format!("{FAMILY} Regular {version}")),
            (NameId::FULL_NAME, FAMILY.into()),
            (NameId::VERSION_STRING, version.clone()),
            (NameId::POSTSCRIPT_NAME, "ShawlineMono-Regular".into()),
        ]
        .into_iter()
        // Windows, Unicode BMP, US English: the one platform every OS reads.
        .map(|(id, s)| NameRecord::new(3, 1, 0x409, id, s.into()))
        .collect(),
    );

    let cmap = Cmap::from_mappings(mappings).expect("one glyph per character");

    let mut font = FontBuilder::new();
    font.add_table(&head)
        .and_then(|f| f.add_table(&hhea))
        .and_then(|f| f.add_table(&maxp))
        .and_then(|f| f.add_table(&os2))
        .and_then(|f| f.add_table(&hmtx))
        .and_then(|f| f.add_table(&cmap))
        .and_then(|f| f.add_table(&loca))
        .and_then(|f| f.add_table(&glyf))
        .and_then(|f| f.add_table(&name))
        .and_then(|f| f.add_table(&post))
        .expect("tables validate");
    font.build()
}

/// Skeleton y-down cell space to font y-up, origin on the baseline.
const TO_FONT: Affine = Affine::new([1.0, 0.0, 0.0, -1.0, 0.0, metrics::BASELINE]);

/// The outline of one skeleton, as a glyph.
fn glyph(sk: &Skeleton) -> Glyph {
    let mut ink = BezPath::new();
    for s in &sk.strokes {
        ink.extend(clockwise(kurbo::stroke(s, &pen_for(s), &StrokeOpts::default(), STROKE_TOLERANCE)));
    }
    for f in &sk.fills {
        // Filling the path and stroking its edge with a round pen is the
        // path's outward offset: the outer contour of the stroke. Fills are
        // convex (dots, arrowheads), so that contour holds all the ink and
        // the inner one, which would only cover the fill again, is dropped.
        let edge = kurbo::stroke(f, &pen(), &StrokeOpts::default(), STROKE_TOLERANCE);
        if let Some(outer) = subpaths(&edge).max_by(|a, b| a.area().abs().total_cmp(&b.area().abs())) {
            ink.extend(clockwise(outer));
        }
    }
    if ink.is_empty() {
        return Glyph::Empty;
    }
    simple(&ink)
}

/// `.notdef`: an empty box, the conventional "missing glyph" shape.
fn notdef() -> Glyph {
    let half = STROKE_WEIGHT / 2.0;
    let centre = Rect::new(100.0, metrics::ASCENDER, CELL_WIDTH - 100.0, metrics::DESCENDER);
    let mut ink = clockwise(centre.inflate(half, half).to_path(0.1));
    ink.extend(clockwise(centre.inflate(-half, -half).to_path(0.1)).reverse_subpaths());
    simple(&ink)
}

/// `ink` (cell space, closed contours) mapped to font space as a TrueType
/// glyph flagged as overlapping.
fn simple(ink: &BezPath) -> Glyph {
    let quads = to_quadratic(&(TO_FONT * ink.clone()));
    let mut g = SimpleGlyph::from_bezpath(&quads).expect("closed quadratic contours");
    g.overlaps = true;
    Glyph::Simple(g)
}

/// The subpaths of `path`, each starting at its move.
fn subpaths(path: &BezPath) -> impl Iterator<Item = BezPath> + '_ {
    let mut els = path.elements().iter().peekable();
    std::iter::from_fn(move || {
        let mut sub = vec![*els.next()?];
        while let Some(el) = els.next_if(|e| !matches!(e, PathEl::MoveTo(_))) {
            sub.push(*el);
        }
        Some(BezPath::from_vec(sub))
    })
}

/// `path` wound clockwise once flipped into font space, the TrueType outer
/// direction (negative shoelace area with y up). The flip negates kurbo's
/// signed area, so in cell space the area must be positive.
///
/// A stroke is reoriented as a whole, never subpath by subpath: a closed
/// centreline expands to an outer and an inner contour, and the inner one
/// must keep its opposite winding to stay a hole.
fn clockwise(path: BezPath) -> BezPath {
    if path.area() < 0.0 { path.reverse_subpaths() } else { path }
}

/// `path` with every cubic replaced by quadratics, each subpath explicitly
/// closed, as the `glyf` table needs.
fn to_quadratic(path: &BezPath) -> BezPath {
    let mut out = BezPath::new();
    let mut last = Point::ORIGIN;
    let mut open = false;
    for el in path.elements() {
        match *el {
            PathEl::MoveTo(p) => {
                if open {
                    out.close_path();
                }
                out.move_to(p);
                open = true;
            }
            PathEl::LineTo(p) => out.line_to(p),
            PathEl::QuadTo(c, p) => out.quad_to(c, p),
            PathEl::CurveTo(c1, c2, p) => {
                let cubic = kurbo::CubicBez::new(last, c1, c2, p);
                match cubic.approx_spline(QUAD_ACCURACY) {
                    Some(spline) => spline.to_quads().for_each(|q| out.quad_to(q.p1, q.p2)),
                    None => cubic.to_quads(QUAD_ACCURACY).for_each(|(_, _, q)| out.quad_to(q.p1, q.p2)),
                }
            }
            PathEl::ClosePath => {
                out.close_path();
                open = false;
            }
        }
        last = match *el {
            PathEl::MoveTo(p) | PathEl::LineTo(p) | PathEl::QuadTo(_, p) | PathEl::CurveTo(_, _, p) => p,
            PathEl::ClosePath => last,
        };
    }
    if open {
        out.close_path();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Shoelace area of each contour, in font space (y-up).
    fn contour_areas(g: &Glyph) -> Vec<f64> {
        let Glyph::Simple(g) = g else { panic!("empty glyph") };
        g.contours
            .iter()
            .map(|c| {
                let pts: Vec<(f64, f64)> = c.iter().map(|p| (f64::from(p.x), f64::from(p.y))).collect();
                let next = pts.iter().cycle().skip(1);
                pts.iter().zip(next).map(|(a, b)| a.0 * b.1 - b.0 * a.1).sum::<f64>() / 2.0
            })
            .collect()
    }

    #[test]
    fn contours_wind_clockwise_in_font_space() {
        // TrueType's outer direction is clockwise: negative shoelace area
        // with y up. Every open stroke and every fill is one outer contour.
        for ch in ['│', '┼', '▶', '·', '※', '\u{10450}'] {
            let areas = contour_areas(&glyph(&skeleton(ch).unwrap()));
            assert!(areas.iter().all(|&a| a < 0.0), "{ch:?}: {areas:?}");
        }
    }

    #[test]
    fn a_vertical_line_spans_the_cell_exactly() {
        let bbox = glyph(&skeleton('│').unwrap()).bbox().unwrap();
        let half = (STROKE_WEIGHT / 2.0) as i16;
        assert_eq!((bbox.y_min, bbox.y_max), (-DESCENT, ASCENT));
        assert_eq!((bbox.x_min, bbox.x_max), (300 - half, 300 + half));
    }

    #[test]
    fn notdef_is_a_box_with_a_hole() {
        let mut areas = contour_areas(&notdef());
        areas.sort_by(f64::total_cmp);
        assert!(areas.len() == 2 && areas[0] < 0.0 && areas[1] > 0.0, "{areas:?}");
    }
}
