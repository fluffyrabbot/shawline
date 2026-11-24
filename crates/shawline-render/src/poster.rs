//! Posters' contract (PLAN P12): a sheet drawn on a paper page, titled with
//! its definitions' names. Lead-owned signatures; agent R implements them.
//!
//! The page is an SVG whose `width` and `height` are in millimetres, so it
//! prints at its size from any browser, and a PDF is the browser's print
//! dialog's to make. Everything on it, title included, is drawn with the
//! glyph pen, so a poster needs no fonts and is in the same hand as
//! [`render_with`](crate::render_with).

use shawline_grid::Grid;

use crate::Theme;

/// Space between the paper's edge and anything drawn, on every side.
pub const MARGIN_MM: f64 = 15.0;

/// The widest a cell is drawn, so a small sheet isn't blown up to fill the
/// page: `01-hello` would otherwise have cells 10 mm wide on A4.
pub const MAX_CELL_MM: f64 = 8.0;

/// Below this cell width the letters are hard to read in print. Studio's
/// Poster dialog warns and suggests a larger paper; the poster is still made.
pub const LEGIBLE_CELL_MM: f64 = 2.0;

/// A paper size.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Paper {
    #[default]
    A4,
    A3,
    /// US Letter.
    Letter,
}

impl Paper {
    /// Width and height in millimetres, portrait.
    pub const fn size_mm(self) -> (f64, f64) {
        match self {
            Paper::A4 => (210.0, 297.0),
            Paper::A3 => (297.0, 420.0),
            Paper::Letter => (215.9, 279.4),
        }
    }
}

/// How to draw a poster.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Poster {
    pub theme: Theme,
    /// Draw the drawing with its gloss strips, as [`Options::gloss`]
    /// does. The title shows readings either way.
    ///
    /// [`Options::gloss`]: crate::Options::gloss
    pub gloss: bool,
    pub paper: Paper,
    /// The definitions' names in sheet order, without their namer dots, as
    /// `shawline_doc::Doc::definition_names` gives them. The title shows
    /// each with its namer dot and its reading
    /// ([`shawline_word::reading`]) beneath, leaving out `𐑥𐑱𐑯` unless it is
    /// the only one. With no names, the page has no title band.
    pub names: Vec<String>,
}

/// The poster: a page of `poster.paper` in whichever orientation gives the
/// larger cell, [`MARGIN_MM`] clear on every side. Top to bottom: the title
/// band, the drawing scaled to fit what is left at no more than
/// [`MAX_CELL_MM`] per cell width and centred in it, and a one-line colophon
/// at the foot reading "drawn in Shawline" in the comment ink. The
/// background fills the whole page. The source text rides along in
/// `<desc>`, as [`render`](crate::render) puts it.
pub fn poster(grid: &Grid, poster: &Poster) -> String {
    let _ = (grid, poster);
    todo!("P12 wave 1, agent R")
}

/// The width in millimetres that [`poster`] draws one cell at, for Studio's
/// warning below [`LEGIBLE_CELL_MM`]. It is the cell width of the same
/// layout, orientation included.
pub fn cell_mm(grid: &Grid, poster: &Poster) -> f64 {
    let _ = (grid, poster);
    todo!("P12 wave 1, agent R")
}
