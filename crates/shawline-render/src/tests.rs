use resvg::{tiny_skia, usvg};
use shawline_grid::Pos;

use super::*;

fn grid(src: &str) -> Grid {
    Grid::parse(src).expect("test source is in the repertoire")
}

/// Rasterizes `svg` at `zoom` times its nominal size.
fn raster(svg: &str, zoom: f32) -> tiny_skia::Pixmap {
    let tree = usvg::Tree::from_str(svg, &usvg::Options::default()).expect("valid SVG");
    let size = tree.size().to_int_size().scale_by(zoom).expect("non-empty");
    let mut pixmap = tiny_skia::Pixmap::new(size.width(), size.height()).expect("non-empty");
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(zoom, zoom),
        &mut pixmap.as_mut(),
    );
    pixmap
}

/// The pixel at glyph-unit point (`x`, `y`) of the grid, at `zoom`.
fn pixel(p: &tiny_skia::Pixmap, zoom: f32, x: f64, y: f64) -> Rgb {
    let px = |v: f64| (((v + MARGIN) * SCALE) as f32 * zoom) as u32;
    let c = p.pixel(px(x), px(y)).expect("inside the image");
    Rgb(c.red(), c.green(), c.blue())
}

#[test]
fn output_is_valid_svg_of_the_expected_size() {
    let src = "╭─ ·𐑛𐑳𐑚 ─╮\n▶ 𐑯 ──┬──▶ 𐑬\n│     ╰─╮│\n╰───────▼╯\n";
    let svg = render(&grid(src), Theme::LIGHT);
    let tree = usvg::Tree::from_str(&svg, &usvg::Options::default()).expect("valid SVG");
    // 12 columns and 4 rows of 15 × 30 px cells, plus the margin.
    assert_eq!(
        (tree.size().width(), tree.size().height()),
        (12.0 * 15.0 + 30.0, 4.0 * 30.0 + 30.0)
    );
    assert!(!svg.contains("<text"), "no font dependence");
}

#[test]
fn a_long_line_is_one_seamless_path() {
    let svg = render(&grid(&"─".repeat(40)), Theme::LIGHT);
    assert!(svg.contains(r#"d="M0,600L24000,600""#), "{svg}");

    // Cell edges land on fractional pixels at this zoom. Any seam shows as
    // a column that holds less ink than its neighbours.
    let zoom = 1.37;
    let p = raster(&svg, zoom);
    let ink = |x: u32| {
        (0..p.height())
            .map(|y| 255 - p.pixel(x, y).unwrap().red() as u32)
            .sum::<u32>()
    };
    let from = ((MARGIN + 600.0) * SCALE) as f32 * zoom;
    let to = ((MARGIN + 39.0 * 600.0) * SCALE) as f32 * zoom;
    let columns: Vec<u32> = (from as u32..to as u32).map(ink).collect();
    let (lo, hi) = (columns.iter().min().unwrap(), columns.iter().max().unwrap());
    assert!(hi - lo <= 2, "ink per column varies from {lo} to {hi}");
}

#[test]
fn a_plain_box_border_is_one_closed_path() {
    let svg = render(&grid("╭──╮\n│  │\n╰──╯\n"), Theme::LIGHT);
    let border = svg
        .lines()
        .find(|l| l.contains(&format!(r#"stroke="{}""#, Theme::LIGHT.border)))
        .unwrap();
    assert_eq!(
        (border.matches('M').count(), border.matches('Z').count()),
        (1, 1),
        "{border}"
    );
}

#[test]
fn the_border_runs_through_a_port_arrowhead() {
    let src = "┌───┐\n│   │\n▶ 𐑐 │\n│   │\n└───┘\n";
    let theme = Theme::LIGHT;
    let p = raster(&render(&grid(src), theme), 4.0);
    // Row 2 holds the port; sample the border line just above and below its
    // head, inside the port's own cell.
    assert_eq!(pixel(&p, 4.0, 300.0, 2.0 * 1200.0 + 150.0), theme.border);
    assert_eq!(pixel(&p, 4.0, 300.0, 2.0 * 1200.0 + 1050.0), theme.border);
    assert_eq!(
        pixel(&p, 4.0, 300.0, 2.0 * 1200.0 + 600.0),
        theme.wire,
        "the head is wire ink"
    );
}

#[test]
fn themes_switch_every_ink() {
    let g = grid("※ 𐑐\n┌───┐\n▶ 𐑑 ├─\n└───┘\n");
    let (light, dark) = (render(&g, Theme::LIGHT), render(&g, Theme::DARK));
    for (svg, t) in [(&light, Theme::LIGHT), (&dark, Theme::DARK)] {
        for c in [t.background, t.text, t.border, t.wire, t.comment] {
            assert!(svg.contains(&c.to_string()), "{c} missing");
        }
    }
    assert!(!dark.contains(&Theme::LIGHT.background.to_string()));
}

#[test]
fn source_rides_along_in_desc() {
    let svg = render(&grid("«𐑐» ─▶ 𐑑\n"), Theme::DARK);
    assert!(svg.contains("<desc>«𐑐» ─▶ 𐑑</desc>"));
}

fn glossed(g: &Grid) -> String {
    render_with(g, Options { theme: Theme::LIGHT, gloss: true })
}

/// How much darker than the background each pixel row of the band `x`
/// (glyph units) is, from `y.0` to `y.1`, at `zoom`.
fn ink_rows(p: &tiny_skia::Pixmap, zoom: f32, x: (f64, f64), y: (f64, f64)) -> Vec<u32> {
    let px = |v: f64| (((v + MARGIN) * SCALE) as f32 * zoom) as u32;
    let paper = Theme::LIGHT.background.0;
    (px(y.0)..px(y.1))
        .map(|row| {
            (px(x.0)..px(x.1))
                .map(|col| paper.saturating_sub(p.pixel(col, row).unwrap().red()) as u32)
                .sum()
        })
        .collect()
}

#[test]
fn the_layer_off_is_the_plain_drawing() {
    let g = grid("╭─ ·𐑛𐑳𐑚 ─╮\n▶ 𐑯 ──┬──▶ 𐑬\n│     ╰─╮│\n╰───────▼╯\n");
    for theme in [Theme::LIGHT, Theme::DARK] {
        assert_eq!(render_with(&g, Options { theme, gloss: false }), render(&g, theme));
    }
    // The sheet has no comments, so only glosses use the comment ink.
    let comment = Theme::LIGHT.comment.to_string();
    assert!(!render(&g, Theme::LIGHT).contains(&comment));
    assert!(glossed(&g).contains(&comment));
}

#[test]
fn a_glossed_drawing_is_rows_times_the_pitch_tall() {
    let g = grid("╭─ ·𐑛𐑳𐑚 ─╮\n▶ 𐑯 ──┬──▶ 𐑬\n│     ╰─╮│\n╰───────▼╯\n");
    let tree = usvg::Tree::from_str(&glossed(&g), &usvg::Options::default()).expect("valid SVG");
    // Rows are 30 px cells plus a 15 px strip, the last row's included.
    assert_eq!(row_pitch(true) * SCALE, 45.0);
    assert_eq!(
        (tree.size().width(), tree.size().height()),
        (12.0 * 15.0 + 30.0, 4.0 * 45.0 + 30.0)
    );
}

#[test]
fn box_sides_and_wires_run_unbroken_through_the_strips() {
    // The left side runs through two stacked port arrowheads, which connect
    // only westward in the stroke table.
    let svg = glossed(&grid("┌───┐ │\n▶   │ │\n▶   │ │\n└───┘ │\n"));
    let pitch = row_pitch(true);
    // As in the plain drawing, the border breaks only at each head's centre,
    // where the head hides it: once around, and once between the heads.
    let border = format!(
        r#"d="M300,2400L300,600L2700,600L2700,6000L300,6000L300,4200M300,2400L300,4200" stroke="{}""#,
        Theme::LIGHT.border
    );
    assert!(svg.contains(&border), "{svg}");
    let wire = format!(r#"d="M3900,0L3900,{}""#, num(3.0 * pitch + CELL_HEIGHT));
    assert!(svg.contains(&wire), "{svg}");

    // Strip edges land on fractional pixels at this zoom. A gap shows as a
    // pixel row with no ink, a seam as one with more.
    let zoom = 1.37;
    let p = raster(&svg, zoom);
    let steady = |x: f64, y: (f64, f64)| {
        let ink = ink_rows(&p, zoom, (x - 150.0, x + 150.0), y);
        let (lo, hi) = (ink.iter().min().unwrap(), ink.iter().max().unwrap());
        assert!(*lo >= 100 && hi - lo <= 2, "ink per row at x {x} varies from {lo} to {hi}");
    };
    steady(3900.0, (300.0, 3.0 * pitch + 900.0));
    steady(2700.0, (900.0, 3.0 * pitch + 300.0));
    // Between the arrowheads' heads.
    for row in 0..3 {
        let strip = row as f64 * pitch + CELL_HEIGHT;
        steady(300.0, (strip - 300.0, strip + STRIP + 300.0));
    }
}

#[test]
fn a_glossed_box_border_is_still_one_closed_path() {
    let svg = glossed(&grid("╭──╮\n│  │\n│  │\n╰──╯\n"));
    let border = svg
        .lines()
        .find(|l| l.contains(&format!(r#"stroke="{}""#, Theme::LIGHT.border)))
        .unwrap();
    assert_eq!(
        (border.matches('M').count(), border.matches('Z').count()),
        (1, 1),
        "{border}"
    );
}

#[test]
fn a_stroke_into_an_open_strip_ends_as_in_the_plain_drawing() {
    // Nothing below the wire reaches up to meet it.
    let g = grid("│\n─\n");
    let svg = glossed(&g);
    assert!(svg.contains(r#"d="M300,0L300,1200M"#), "{svg}");
    let p = raster(&svg, 4.0);
    assert_eq!(pixel(&p, 4.0, 300.0, 1220.0), Theme::LIGHT.wire, "a round cap");
    assert_eq!(pixel(&p, 4.0, 300.0, 1500.0), Theme::LIGHT.background);
}

/// Where the strip shows each word's gloss.
fn spans(src: &str) -> Vec<Option<(f64, f64)>> {
    glosses(&grid(src)).iter().map(|g| g.span).collect()
}

#[test]
fn a_gloss_is_centred_under_its_word() {
    let g = grid(include_str!("../../../corpus/03-double.shaw"));
    let found = glosses(&g);
    let add = found.iter().find(|g| g.at == Pos::new(5, 12)).unwrap();
    assert_eq!((add.text(), add.span), ("add", Some((11.75, 13.25))));
    // The title ·𐑛𐑳𐑚𐑩𐑤 fills columns 3 to 8, and its reading is as long.
    let title = found.iter().find(|g| g.at == Pos::new(2, 3)).unwrap();
    assert_eq!((title.text(), title.span), ("=dub@l", Some((4.5, 7.5))));
}

#[test]
fn a_gloss_needs_a_glyph_for_every_character() {
    // A word that mixes in a digit is an error, but it is still drawn, and
    // its reading keeps the digit, which has no gloss glyph.
    let found = glosses(&grid("𐑯2\n𐑯\n"));
    assert_eq!(found[0].text(), "n2");
    assert_eq!(found.iter().map(|g| g.span).collect::<Vec<_>>(), [None, Some((0.25, 0.75))]);
}

#[test]
fn a_gloss_stays_within_the_drawing() {
    // 𐑺 reads `air`: a column and a half under a one-column word.
    assert_eq!(spans("𐑺  \n 𐑺 \n  𐑺\n"), [None, Some((0.75, 2.25)), None]);
}

#[test]
fn a_gloss_never_covers_a_vertical_stroke() {
    // The cell above connects south.
    assert_eq!(spans("┐𐑺  \n┐ 𐑺 \n"), [None, Some((1.75, 3.25))]);
    // The cell below connects north.
    assert_eq!(spans(" 𐑺  \n└───\n  𐑺 \n└───\n"), [None, Some((1.75, 3.25))]);
    // A box's side runs through, between ports that connect only westward.
    let src = "┌───┐\n▶𐑺  │\n▶ 𐑺 │\n└───┘\n";
    assert_eq!(spans(src), [None, Some((1.75, 3.25))]);
    let g = grid(src);
    assert!(strip::blocked(&g, &shawline_shape::recognize(&g))[1][0]);
}

#[test]
fn glosses_keep_one_advance_apart() {
    // Exactly one advance apart is enough.
    assert_eq!(spans(" 𐑺 𐑺 \n"), [Some((0.75, 2.25)), Some((2.75, 4.25))]);
    // `airair` would come within a quarter column.
    assert_eq!(spans(" 𐑺 𐑺𐑺 \n"), [Some((0.75, 2.25)), None]);
    // A dropped gloss holds no room.
    assert_eq!(spans(" 𐑺2 𐑺𐑺𐑺 \n"), [None, Some((3.25, 7.75))]);
}

#[test]
fn shown_glosses_are_drawn_in_their_strip_and_dropped_ones_nowhere() {
    let g = grid(" 𐑺 𐑺𐑺 \n▶ 𐑯 ──\n");
    let pitch = row_pitch(true);
    let found = glosses(&g);
    assert!(found.iter().any(Gloss::shown) && !found.iter().all(Gloss::shown));
    for gloss in &found {
        let (strokes, fills) = strip::pieces(std::slice::from_ref(gloss), pitch);
        let Some((left, right)) = gloss.span else {
            assert!(strokes.is_empty() && fills.is_empty());
            continue;
        };
        assert!(!strokes.is_empty());
        let top = gloss.at.row as f64 * pitch + CELL_HEIGHT;
        let strip = kurbo::Rect::new(left * CELL_WIDTH, top, right * CELL_WIDTH, top + STRIP);
        for s in &strokes {
            let bounds = kurbo::Shape::bounding_box(&s.path);
            assert_eq!(s.ink, Ink::Comment);
            assert_eq!(strip.union(bounds), strip, "{} at {bounds:?}", gloss.at);
        }
    }
}
