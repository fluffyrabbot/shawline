//! Where cells sit in a drawn sheet, for hit-testing and overlays (PLAN P10,
//! lead-owned). The page asks this rather than keeping its own cell width
//! and row pitch, so zoom and the gloss layer change one place.

use shawline_glyph::{CELL_HEIGHT, CELL_WIDTH};
use shawline_grid::{Grid, Pos, Rect};

use crate::{MARGIN, row_pitch};

/// A drawing's geometry at one zoom. Points are in the units the drawing is
/// shown in (the page's CSS pixels), measured from the drawing's top-left
/// corner, margin included: where a pointer event lands on the `<svg>`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Geometry {
    /// Shown units per glyph unit: the zoom times the base scale.
    scale: f64,
    pitch: f64,
    rows: usize,
    cols: usize,
}

/// An axis-aligned area in shown units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Area {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Geometry {
    /// The geometry of `grid` drawn with or without the gloss layer, at
    /// `scale` shown units per glyph unit. [`crate::SCALE`] is the drawing's
    /// natural size; the page multiplies its zoom in.
    pub fn new(grid: &Grid, gloss: bool, scale: f64) -> Self {
        Self { scale, pitch: row_pitch(gloss), rows: grid.height(), cols: grid.width() }
    }

    /// The whole drawing's width and height, margin included.
    pub fn size(&self) -> (f64, f64) {
        let width = self.cols as f64 * CELL_WIDTH + 2.0 * MARGIN;
        let height = self.rows as f64 * self.pitch + 2.0 * MARGIN;
        (width * self.scale, height * self.scale)
    }

    /// The cell under a point, or `None` left of or above the grid. A point
    /// in a gloss strip is in the row above it, whose words the strip
    /// glosses. Points right of or below the grid name cells beyond it, as
    /// blank cells past a line's end are.
    pub fn cell_at(&self, x: f64, y: f64) -> Option<Pos> {
        let (x, y) = (x / self.scale - MARGIN, y / self.scale - MARGIN);
        (x >= 0.0 && y >= 0.0).then(|| Pos::new((y / self.pitch) as usize, (x / CELL_WIDTH) as usize))
    }

    /// The area of an inclusive cell rectangle, from its top row's top to
    /// its bottom row's cell. `under` takes it on through the strip below,
    /// and `over` back up through the strip above, so a highlight of cells
    /// that join vertically doesn't break at the strip between them.
    pub fn cell_rect(&self, rect: Rect, under: bool, over: bool) -> Area {
        let strip = self.pitch - CELL_HEIGHT;
        let top = rect.top as f64 * self.pitch - if over { strip } else { 0.0 };
        let end = rect.bottom as f64 * self.pitch + CELL_HEIGHT + if under { strip } else { 0.0 };
        Area {
            x: (rect.left as f64 * CELL_WIDTH + MARGIN) * self.scale,
            y: (top + MARGIN) * self.scale,
            width: (rect.right - rect.left + 1) as f64 * CELL_WIDTH * self.scale,
            height: (end - top) * self.scale,
        }
    }

    /// A cell's centre.
    pub fn cell_centre(&self, at: Pos) -> (f64, f64) {
        let x = at.col as f64 * CELL_WIDTH + CELL_WIDTH / 2.0 + MARGIN;
        let y = at.row as f64 * self.pitch + CELL_HEIGHT / 2.0 + MARGIN;
        (x * self.scale, y * self.scale)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_point_hits_the_cell_it_is_drawn_in_at_any_zoom_and_pitch() {
        let grid = Grid::parse("𐑐 ──▶ 𐑚\n│\n└──").unwrap();
        for gloss in [false, true] {
            for zoom in [0.5, 1.0, 4.0] {
                let g = Geometry::new(&grid, gloss, crate::SCALE * zoom);
                for row in 0..3 {
                    for col in 0..7 {
                        let at = Pos::new(row, col);
                        let (x, y) = g.cell_centre(at);
                        assert_eq!(g.cell_at(x, y), Some(at));
                        let area = g.cell_rect(Rect { top: row, left: col, bottom: row, right: col }, false, false);
                        assert_eq!(g.cell_at(area.x + 0.01, area.y + 0.01), Some(at));
                    }
                }
                // A strip belongs to the row above it.
                let below = g.cell_rect(Rect { top: 0, left: 0, bottom: 0, right: 0 }, true, false);
                assert_eq!(g.cell_at(below.x + 0.01, below.y + below.height - 0.01), Some(Pos::new(0, 0)));
                assert_eq!(g.cell_at(0.0, 0.0), None);
            }
        }
    }
}
