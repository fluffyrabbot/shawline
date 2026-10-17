//! Writes `specimen.svg` next to this crate's manifest and rasterizes it for
//! self-inspection.
//!
//! `cargo run -p shawline-glyph --example specimen -- [png-path] [zoom]`
//! (defaults: `/tmp/shawline-specimen.png`, zoom 1).

use resvg::{tiny_skia, usvg};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let png = args.next().unwrap_or_else(|| "/tmp/shawline-specimen.png".into());
    let zoom: f32 = args.next().map(|z| z.parse()).transpose()?.unwrap_or(1.0);

    let svg = shawline_glyph::specimen();
    let out = concat!(env!("CARGO_MANIFEST_DIR"), "/specimen.svg");
    std::fs::write(out, &svg)?;

    let mut opt = usvg::Options::default();
    opt.fontdb_mut().load_system_fonts();
    let tree = usvg::Tree::from_str(&svg, &opt)?;
    let size = tree.size().to_int_size().scale_by(zoom).ok_or("bad zoom")?;
    let mut pixmap = tiny_skia::Pixmap::new(size.width(), size.height()).ok_or("empty sheet")?;
    resvg::render(&tree, tiny_skia::Transform::from_scale(zoom, zoom), &mut pixmap.as_mut());
    pixmap.save_png(&png)?;
    println!("wrote {out} and {png}");
    Ok(())
}
