//! Rasterizes a real program through the built font (skrifa outlines filled
//! by tiny-skia, one glyph at a time as a text renderer composites them) and
//! checks that box lines join across cell edges at line-height 1.0.
//!
//! The renders are also written to the temp directory for inspection:
//! `shawline-font-04-abs.png` and `shawline-font-closeup.png`.

use shawline_font::{UNITS_PER_EM, build};
use shawline_glyph::{CELL_HEIGHT, CELL_WIDTH, metrics};
use skrifa::instance::{LocationRef, Size};
use skrifa::outline::{DrawSettings, OutlinePen};
use skrifa::{FontRef, MetadataProvider};
use tiny_skia::{FillRule, Paint, PathBuilder, Pixmap, Transform};

const PROGRAM: &str = include_str!("../../../corpus/04-abs.shaw");

struct Pen(PathBuilder);

impl OutlinePen for Pen {
    fn move_to(&mut self, x: f32, y: f32) {
        self.0.move_to(x, y);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.0.line_to(x, y);
    }
    fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) {
        self.0.quad_to(cx, cy, x, y);
    }
    fn curve_to(&mut self, a: f32, b: f32, c: f32, d: f32, x: f32, y: f32) {
        self.0.cubic_to(a, b, c, d, x, y);
    }
    fn close(&mut self) {
        self.0.close();
    }
}

/// Renders `text` at `px` pixels per em with rows exactly one em apart
/// (line-height 1.0). Returns the image and the cell size in pixels.
fn render(font: &FontRef, text: &str, px: f32) -> (Pixmap, f32, f32) {
    let scale = px / f32::from(UNITS_PER_EM);
    let (cw, ch) = (CELL_WIDTH as f32 * scale, CELL_HEIGHT as f32 * scale);
    let ascent = metrics::BASELINE as f32 * scale;
    let lines: Vec<Vec<char>> = text.lines().map(|l| l.chars().collect()).collect();
    let cols = lines.iter().map(Vec::len).max().unwrap_or(0);
    let mut img = Pixmap::new((cols as f32 * cw).ceil() as u32, (lines.len() as f32 * ch).ceil() as u32).unwrap();
    img.fill(tiny_skia::Color::WHITE);

    let charmap = font.charmap();
    let outlines = font.outline_glyphs();
    let mut paint = Paint::default();
    paint.set_color_rgba8(0, 0, 0, 255);
    for (row, line) in lines.iter().enumerate() {
        for (col, &c) in line.iter().enumerate() {
            let gid = charmap.map(c).unwrap_or_else(|| panic!("{c:?} unmapped"));
            let mut pen = Pen(PathBuilder::new());
            let settings = DrawSettings::unhinted(Size::new(px), LocationRef::default());
            outlines.get(gid).unwrap().draw(settings, &mut pen).unwrap();
            let Some(path) = pen.0.finish() else { continue };
            // Font space is y-up from the baseline; the image is y-down.
            let at = Transform::from_row(1.0, 0.0, 0.0, -1.0, col as f32 * cw, row as f32 * ch + ascent);
            img.fill_path(&path, &paint, FillRule::Winding, at, None);
        }
    }
    (img, cw, ch)
}

/// Darkness of pixel `(x, y)`, 0 (white) to 255 (black).
fn ink(img: &Pixmap, x: u32, y: u32) -> u8 {
    255 - img.pixel(x, y).unwrap().red()
}

#[test]
fn box_lines_join_across_cell_edges() {
    let bytes = build();
    let font = FontRef::new(&bytes).unwrap();
    // 48 px per em puts every cell edge on a pixel edge (24 × 48 px cells).
    let (img, cw, ch) = render(&font, PROGRAM, 48.0);
    img.save_png(std::env::temp_dir().join("shawline-font-04-abs.png")).unwrap();

    let grid: Vec<Vec<char>> = PROGRAM.lines().map(|l| l.chars().collect()).collect();
    let at = |r: usize, c: usize| grid.get(r).and_then(|l| l.get(c)).copied().unwrap_or(' ');
    let (cw, ch) = (cw as u32, ch as u32);
    // The centreline pixel columns/rows a 72-unit stroke covers completely.
    let (mid_x, mid_y) = (cw / 2 - 1..=cw / 2, ch / 2 - 1..=ch / 2);
    let (mut vertical, mut horizontal) = (0, 0);
    for (r, line) in grid.iter().enumerate() {
        for c in 0..line.len() {
            if at(r, c) == '│' && at(r + 1, c) == '│' {
                vertical += 1;
                let edge = (r as u32 + 1) * ch;
                for y in edge - 6..edge + 6 {
                    for x in mid_x.clone() {
                        let x = c as u32 * cw + x;
                        assert_eq!(ink(&img, x, y), 255, "gap below │ at row {r} col {c} (pixel {x},{y})");
                    }
                }
            }
            if at(r, c) == '─' && at(r, c + 1) == '─' {
                horizontal += 1;
                let edge = (c as u32 + 1) * cw;
                for x in edge - 4..edge + 4 {
                    for y in mid_y.clone() {
                        let y = r as u32 * ch + y;
                        assert_eq!(ink(&img, x, y), 255, "gap after ─ at row {r} col {c} (pixel {x},{y})");
                    }
                }
            }
        }
    }
    assert!(vertical > 20 && horizontal > 50, "the program exercises both joins");
}

#[test]
fn closeup_for_inspection() {
    let bytes = build();
    let font = FontRef::new(&bytes).unwrap();
    let text: String = PROGRAM.lines().skip(2).take(6).map(|l| format!("{l}\n")).collect();
    let sample = format!("{text}0123456789 · « » ※ ▶◀▲▼ ┄┆ ╭╮╰╯\n-|+.'~:><^v\n𐑐𐑑𐑒𐑓𐑔𐑕𐑖𐑗𐑘𐑙𐑚𐑛𐑜𐑝𐑞𐑟𐑠𐑡𐑢𐑣𐑤𐑥𐑦𐑧\n𐑨𐑩𐑪𐑫𐑬𐑭𐑮𐑯𐑰𐑱𐑲𐑳𐑴𐑵𐑶𐑷𐑸𐑹𐑺𐑻𐑼𐑽𐑾𐑿\n");
    let (img, _, _) = render(&font, &sample, 64.0);
    img.save_png(std::env::temp_dir().join("shawline-font-closeup.png")).unwrap();
    // A glyph with ink must darken some pixels: guards against an all-white
    // render from, say, contours winding against each other.
    let dark = img.pixels().iter().filter(|p| p.red() < 128).count();
    assert!(dark > 10_000, "only {dark} dark pixels");

    let letters: Vec<char> = ('\u{10450}'..='\u{1047F}').collect();
    let rows: String = letters.chunks(12).map(|r| format!("{}\n", r.iter().collect::<String>())).collect();
    let (img, _, _) = render(&font, &rows, 128.0);
    img.save_png(std::env::temp_dir().join("shawline-font-letters.png")).unwrap();
}
