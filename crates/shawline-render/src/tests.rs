use resvg::{tiny_skia, usvg};

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
