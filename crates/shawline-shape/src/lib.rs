//! Stroke recognition: turns a [`Grid`] into a [`Diagram`] of boxes and text.
//!
//! Recognition is structural. A box is any closed rectangle of strokes, whatever
//! characters draw it, so a rough ASCII sketch and its canonical box-drawing
//! form produce the same diagram apart from [`BoxStyle`]. Wire tracing lands in
//! Wave A; see `docs/PLAN.md`.

mod boxes;
pub mod stroke;
mod text;

use std::fmt;

use shawline_grid::{Diagnostic, Grid, Pos};

pub use boxes::{BoxShape, BoxStyle, Port, PortKind};
pub use stroke::StrokeMap;
pub use text::{Text, TextKind};

#[derive(Clone, Debug)]
pub struct Diagram {
    /// Boxes in row-major order of their top-left corners.
    pub boxes: Vec<BoxShape>,
    /// Text runs in row-major order.
    pub texts: Vec<Text>,
    pub diagnostics: Vec<Diagnostic>,
    pub strokes: StrokeMap,
}

pub fn recognize(grid: &Grid) -> Diagram {
    let strokes = StrokeMap::new(grid);
    let boxes = boxes::find(grid, &strokes);
    let mut diagnostics = boxes::overlaps(&boxes);
    let mut texts = text::find(grid, &mut diagnostics);

    for t in &mut texts {
        let last = Pos::new(t.pos.row, t.pos.col + t.width - 1);
        t.title_of = boxes.iter().position(|b| {
            b.rect.top == t.pos.row
                && b.title_gap.is_some_and(|(start, end)| (start..end).contains(&t.pos.col))
        });
        t.owner = (0..boxes.len())
            .filter(|&i| {
                let r = boxes[i].rect;
                r.interior_contains(t.pos) && r.interior_contains(last)
            })
            .min_by_key(|&i| boxes[i].rect.area());
    }
    diagnostics.sort_by_key(|d| d.at);
    Diagram { boxes, texts, diagnostics, strokes }
}

/// The dump format checked by the corpus. One fact per line, stable order.
impl fmt::Display for Diagram {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, b) in self.boxes.iter().enumerate() {
            let style = match b.style {
                BoxStyle::Square => "square",
                BoxStyle::Rounded => "rounded",
                BoxStyle::Mixed => "mixed",
            };
            write!(f, "box {i} {style} {}", b.rect)?;
            write_place(f, " in", b.parent)?;
            writeln!(f)?;
            for p in &b.ports {
                let kind = match p.kind {
                    PortKind::Arrow => "arrow",
                    PortKind::Tee => "tee",
                };
                writeln!(f, "  port {} side {} {kind} {}", p.pos, p.side, p.dir)?;
            }
            for row in &b.dividers {
                writeln!(f, "  divider {row}")?;
            }
        }
        for t in &self.texts {
            let kind = match t.kind {
                TextKind::Word => "word",
                TextKind::Number => "number",
                TextKind::Literal => "literal",
                TextKind::Comment => "comment",
            };
            write!(f, "text {kind} {} {:?}", t.pos, t.content)?;
            write_place(f, " in", t.owner)?;
            write_place(f, " title", t.title_of)?;
            writeln!(f)?;
        }
        for d in &self.diagnostics {
            writeln!(f, "diagnostic {d}")?;
        }
        Ok(())
    }
}

fn write_place(f: &mut fmt::Formatter<'_>, label: &str, index: Option<usize>) -> fmt::Result {
    match index {
        Some(i) => write!(f, "{label} {i}"),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn diagram(src: &str) -> Diagram {
        recognize(&Grid::parse(src).unwrap())
    }

    #[test]
    fn titled_box_with_ports() {
        let d = diagram("╭─ ·𐑨𐑛 ─╮\n▶ 𐑩     ▶ 𐑚\n╰───────╯\n");
        let b = &d.boxes[0];
        assert_eq!(b.style, BoxStyle::Rounded);
        assert_eq!(b.title_gap, Some((2, 7)));
        assert_eq!(b.ports.len(), 2);
        assert!(b.ports[0].is_inward() && !b.ports[1].is_inward());
        let title = d.texts.iter().find(|t| t.title_of == Some(0)).unwrap();
        assert_eq!(title.content, "·𐑨𐑛");
        let names: Vec<_> = d.texts.iter().map(|t| (t.content.as_str(), t.owner)).collect();
        assert!(names.contains(&("𐑩", Some(0))) && names.contains(&("𐑚", None)));
    }

    #[test]
    fn ascii_sketch_matches_canonical_structure() {
        let canonical = diagram("┌───┐\n▶ 𐑐 ├─\n└───┘\n");
        let ascii = diagram("+---+\n> 𐑐 +-\n+---+\n");
        assert_eq!(canonical.boxes, ascii.boxes);
    }

    #[test]
    fn dividers_and_nesting() {
        let d = diagram(
            "╭──────────╮\n│ ┌──▼──┐  │\n│ │ 0   │  │\n│ ├┄┄┄┄┄┤  │\n│ │     │  │\n│ └─────┘  │\n╰──────────╯\n",
        );
        assert_eq!(d.boxes.len(), 2);
        assert_eq!(d.boxes[1].parent, Some(0));
        assert_eq!(d.boxes[1].dividers, vec![3]);
        assert_eq!(d.boxes[1].ports.len(), 1);
    }

    #[test]
    fn overlapping_boxes_are_diagnosed() {
        let d = diagram("┌──┐\n│ ┌┼─┐\n└─┼┘ │\n  └──┘\n");
        assert_eq!(d.diagnostics.len(), 1, "{:?}", d.diagnostics);
    }
}
