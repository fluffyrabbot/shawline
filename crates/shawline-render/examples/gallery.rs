//! Renders every corpus program to SVG, in both themes.
//!
//! `cargo run -p shawline-render --example gallery -- <out-dir> [--png [zoom]]`
//!
//! Writes `<name>.svg` (light) and `<name>.dark.svg` for each `corpus/*.shaw`,
//! skipping formatter expectations (`*.fmt.shaw`) and programs with grid
//! errors. `--png` also rasterizes each one for inspection.

use std::path::{Path, PathBuf};

use resvg::{tiny_skia, usvg};
use shawline_grid::Grid;
use shawline_render::{Theme, render};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let out = PathBuf::from(
        args.next()
            .ok_or("usage: gallery <out-dir> [--png [zoom]]")?,
    );
    let png = args.next().as_deref() == Some("--png");
    let zoom: f32 = args.next().map(|z| z.parse()).transpose()?.unwrap_or(2.0);
    std::fs::create_dir_all(&out)?;

    let corpus = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus");
    let mut programs: Vec<PathBuf> = std::fs::read_dir(&corpus)?
        .map(|e| e.map(|e| e.path()))
        .collect::<Result<_, _>>()?;
    programs.retain(|p| {
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
        name.ends_with(".shaw") && !name.ends_with(".fmt.shaw")
    });
    programs.sort();

    for path in programs {
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or("bad file name")?;
        let Ok(grid) = Grid::parse(&std::fs::read_to_string(&path)?) else {
            println!("skip {stem}: grid errors");
            continue;
        };
        for (theme, suffix) in [(Theme::LIGHT, ""), (Theme::DARK, ".dark")] {
            let svg = render(&grid, theme);
            let file = out.join(format!("{stem}{suffix}.svg"));
            std::fs::write(&file, &svg)?;
            if png {
                rasterize(&svg, zoom, &file.with_extension("png"))?;
            }
        }
        println!("wrote {stem}");
    }
    Ok(())
}

fn rasterize(svg: &str, zoom: f32, to: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let tree = usvg::Tree::from_str(svg, &usvg::Options::default())?;
    let size = tree.size().to_int_size().scale_by(zoom).ok_or("bad zoom")?;
    let mut pixmap = tiny_skia::Pixmap::new(size.width(), size.height()).ok_or("empty drawing")?;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(zoom, zoom),
        &mut pixmap.as_mut(),
    );
    pixmap.save_png(to)?;
    Ok(())
}
