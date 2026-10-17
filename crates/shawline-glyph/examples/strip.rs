//! Renders a row of glyphs large, over their guide lines, for close inspection.
//!
//! `cargo run -p shawline-glyph --example strip -- <chars> [png-path]`

use resvg::{tiny_skia, usvg};
use shawline_glyph::{CELL_HEIGHT, CELL_WIDTH, metrics, skeleton};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let chars: Vec<char> = args.next().ok_or("usage: strip <chars> [png]")?.chars().collect();
    let png = args.next().unwrap_or_else(|| "/tmp/shawline-strip.png".into());

    let width = CELL_WIDTH * chars.len() as f64;
    let mut body = String::new();
    for g in [metrics::ASCENDER, metrics::X_HEIGHT, metrics::BASELINE, metrics::DESCENDER] {
        body += &format!(r##"<line x1="0" y1="{g}" x2="{width}" y2="{g}" stroke="#c8d0e0" stroke-width="4"/>"##);
    }
    for (i, &ch) in chars.iter().enumerate() {
        let x = i as f64 * CELL_WIDTH;
        let s = skeleton(ch).ok_or_else(|| format!("{ch:?} is not in the repertoire"))?;
        body += &format!(
            r##"<rect x="{x}" y="0" width="{CELL_WIDTH}" height="{CELL_HEIGHT}" fill="none" stroke="#eee" stroke-width="4"/><g transform="translate({x} 0)">{}</g>"##,
            s.to_svg()
        );
    }
    let svg = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {width} {CELL_HEIGHT}" color="#1d1d1f"><rect width="100%" height="100%" fill="#fff"/>{body}</svg>"##,
        w = width / 4.0,
        h = CELL_HEIGHT / 4.0,
    );
    let tree = usvg::Tree::from_str(&svg, &usvg::Options::default())?;
    let size = tree.size().to_int_size();
    let mut pixmap = tiny_skia::Pixmap::new(size.width(), size.height()).ok_or("empty")?;
    resvg::render(&tree, tiny_skia::Transform::default(), &mut pixmap.as_mut());
    pixmap.save_png(&png)?;
    println!("wrote {png}");
    Ok(())
}
