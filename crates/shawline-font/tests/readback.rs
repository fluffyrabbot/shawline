//! Reads the built font back with skrifa and checks what editors rely on:
//! full coverage, one advance, and vertical metrics that span the cell.

use shawline_font::{ADVANCE, FAMILY, UNITS_PER_EM, build, characters};
use shawline_glyph::{CELL_HEIGHT, CELL_WIDTH, metrics};
use skrifa::instance::{LocationRef, Size};
use skrifa::raw::TableProvider;
use skrifa::raw::tables::os2::SelectionFlags;
use skrifa::string::StringId;
use skrifa::{FontRef, GlyphId, MetadataProvider};

fn with_font(f: impl FnOnce(&FontRef)) {
    let bytes = build();
    f(&FontRef::new(&bytes).expect("font parses"));
}

#[test]
fn every_repertoire_character_maps_to_a_600_unit_glyph() {
    with_font(|font| {
        let charmap = font.charmap();
        let advances = font.glyph_metrics(Size::unscaled(), LocationRef::default());
        let mut seen = std::collections::HashSet::new();
        for ch in characters() {
            let gid = charmap.map(ch).unwrap_or_else(|| panic!("{ch:?} is not mapped"));
            assert_ne!(gid, GlyphId::NOTDEF, "{ch:?} maps to .notdef");
            assert!(seen.insert(gid), "{ch:?} shares a glyph");
            assert_eq!(advances.advance_width(gid), Some(f32::from(ADVANCE)), "{ch:?} advance");
        }
        assert_eq!(charmap.map('a'), None, "plain English is outside the repertoire");
        let count = font.maxp().unwrap().num_glyphs();
        assert_eq!(usize::from(count), seen.len() + 1, "only .notdef is unmapped");
        assert_eq!(advances.advance_width(GlyphId::NOTDEF), Some(f32::from(ADVANCE)));
    });
}

#[test]
fn shavian_is_mapped_outside_the_bmp() {
    with_font(|font| {
        let charmap = font.charmap();
        for ch in '\u{10450}'..='\u{1047F}' {
            assert!(charmap.map(ch).is_some(), "{ch:?}");
        }
    });
}

#[test]
fn vertical_metrics_span_exactly_one_cell() {
    with_font(|font| {
        let cell = CELL_HEIGHT as i32;
        let head = font.head().unwrap();
        assert_eq!(head.units_per_em(), UNITS_PER_EM);
        assert_eq!(i32::from(UNITS_PER_EM), cell, "line-height 1.0 is one cell");

        let hhea = font.hhea().unwrap();
        let (asc, desc) = (i32::from(hhea.ascender().to_i16()), i32::from(hhea.descender().to_i16()));
        assert_eq!(hhea.line_gap().to_i16(), 0);
        assert_eq!(asc - desc, cell);
        assert_eq!(asc, metrics::BASELINE as i32, "the baseline sits where the skeletons put it");

        let os2 = font.os2().unwrap();
        assert!(os2.fs_selection().contains(SelectionFlags::USE_TYPO_METRICS));
        assert_eq!(os2.s_typo_line_gap(), 0);
        assert_eq!(i32::from(os2.s_typo_ascender()), asc);
        assert_eq!(i32::from(os2.s_typo_descender()), desc);
        assert_eq!(i32::from(os2.us_win_ascent()) + i32::from(os2.us_win_descent()), cell);
        assert_eq!(i32::from(os2.us_win_ascent()), asc);
    });
}

#[test]
fn ink_stays_inside_the_cell() {
    // Box drawing reaches the cell edges and stops there, so nothing is
    // clipped by the win metrics and nothing spills into a neighbour.
    with_font(|font| {
        let head = font.head().unwrap();
        let asc = metrics::BASELINE as i16;
        assert!(head.x_min() >= 0 && head.x_max() <= CELL_WIDTH as i16, "x {}..{}", head.x_min(), head.x_max());
        assert!(head.y_min() >= asc - CELL_HEIGHT as i16 && head.y_max() <= asc, "y {}..{}", head.y_min(), head.y_max());
        assert_eq!((head.x_min(), head.x_max()), (0, CELL_WIDTH as i16), "box lines touch both side edges");
        assert_eq!((head.y_min(), head.y_max()), (asc - CELL_HEIGHT as i16, asc), "and both row edges");
    });
}

#[test]
fn the_font_is_marked_monospaced_and_named() {
    with_font(|font| {
        assert_ne!(font.post().unwrap().is_fixed_pitch(), 0);
        assert_eq!(font.os2().unwrap().panose_10()[3], 9, "panose proportion: monospaced");
        assert_eq!(font.hhea().unwrap().number_of_h_metrics(), 1);
        let family = font.localized_strings(StringId::FAMILY_NAME).english_or_first().unwrap();
        assert_eq!(family.to_string(), FAMILY);
    });
}

#[test]
fn every_inked_glyph_is_flagged_as_overlapping() {
    with_font(|font| {
        let glyf = font.glyf().unwrap();
        let loca = font.loca(None).unwrap();
        let charmap = font.charmap();
        for ch in characters().filter(|&c| c != ' ') {
            let gid = charmap.map(ch).unwrap();
            let Some(skrifa::raw::tables::glyf::Glyph::Simple(g)) = loca.get_glyf(gid, &glyf).unwrap() else {
                panic!("{ch:?} has no simple outline");
            };
            assert!(g.has_overlapping_contours(), "{ch:?}");
        }
    });
}

#[test]
fn the_build_is_reproducible() {
    assert_eq!(build(), build());
}
