//! Renders a row of glyphs large, over their guide lines, for close inspection.
//!
//! `cargo run -p shawline-glyph --example strip -- [--gloss] <chars> [png-path]`
//!
//! With `--gloss` the row is gloss glyphs, half a cell each, at the same scale.

use resvg::{tiny_skia, usvg};
use shawline_glyph::{CELL_HEIGHT, CELL_WIDTH, GLOSS_HEIGHT, GLOSS_WIDTH, gloss_skeleton, metrics, skeleton};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1).peekable();
    let gloss = args.next_if_eq("--gloss").is_some();
    let chars: Vec<char> = args.next().ok_or("usage: strip [--gloss] <chars> [png]")?.chars().collect();
    let png = args.next().unwrap_or_else(|| "/tmp/shawline-strip.png".into());

    let (cell_w, cell_h, k) = if gloss { (GLOSS_WIDTH, GLOSS_HEIGHT, 0.5) } else { (CELL_WIDTH, CELL_HEIGHT, 1.0) };
    let width = cell_w * chars.len() as f64;
    let mut body = String::new();
    for g in [metrics::ASCENDER, metrics::X_HEIGHT, metrics::BASELINE, metrics::DESCENDER] {
        let g = g * k;
        body += &format!(r##"<line x1="0" y1="{g}" x2="{width}" y2="{g}" stroke="#c8d0e0" stroke-width="4"/>"##);
    }
    for (i, &ch) in chars.iter().enumerate() {
        let x = i as f64 * cell_w;
        let s = if gloss { gloss_skeleton(ch) } else { skeleton(ch) };
        let s = s.ok_or_else(|| format!("{ch:?} has no skeleton"))?;
        body += &format!(
            r##"<rect x="{x}" y="0" width="{cell_w}" height="{cell_h}" fill="none" stroke="#eee" stroke-width="4"/><g transform="translate({x} 0)">{}</g>"##,
            s.to_svg()
        );
    }
    let svg = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {width} {cell_h}" color="#1d1d1f"><rect width="100%" height="100%" fill="#fff"/>{body}</svg>"##,
        w = width / 4.0,
        h = cell_h / 4.0,
    );
    let tree = usvg::Tree::from_str(&svg, &usvg::Options::default())?;
    let size = tree.size().to_int_size();
    let mut pixmap = tiny_skia::Pixmap::new(size.width(), size.height()).ok_or("empty")?;
    resvg::render(&tree, tiny_skia::Transform::default(), &mut pixmap.as_mut());
    pixmap.save_png(&png)?;
    println!("wrote {png}");
    Ok(())
}
