//! Stroke recognition: turns a [`Grid`] into a [`Diagram`] of boxes and text.
//!
//! Recognition is structural. A box is any closed rectangle of strokes, whatever
//! characters draw it, so a rough ASCII sketch and its canonical box-drawing
//! form produce the same diagram apart from [`BoxStyle`]. Wires are traced from
//! the strokes left over once boxes are found, against the contract in [`wire`].

mod boxes;
pub mod stroke;
mod text;
mod trace;
pub mod wire;

use std::collections::HashSet;
use std::fmt;

use shawline_grid::{Diagnostic, Grid, Pos};

pub use boxes::{BoxShape, BoxStyle, Divider, Port, PortKind, Slip};
pub use stroke::StrokeMap;
pub use text::{Text, TextKind};
pub use wire::{EndKind, Wire, WireEnd};

#[derive(Clone, Debug)]
pub struct Diagram {
    /// Boxes in row-major order of their top-left corners.
    pub boxes: Vec<BoxShape>,
    /// Wires in order of their first cell.
    pub wires: Vec<Wire>,
    /// Text runs in row-major order.
    pub texts: Vec<Text>,
    pub diagnostics: Vec<Diagnostic>,
    pub strokes: StrokeMap,
}

pub fn recognize(grid: &Grid) -> Diagram {
    // Text comes first so comments and literals can't draw geometry.
    let mut diagnostics = Vec::new();
    let mut texts = text::find(grid, &mut diagnostics);
    let masked: HashSet<Pos> =
        texts.iter().filter(|t| masks(grid, t)).flat_map(Text::cells).collect();
    let strokes = StrokeMap::masking(grid, |p| masked.contains(&p));
    let boxes = boxes::find(grid, &strokes);
    diagnostics.extend(boxes::overlaps(&boxes));
    diagnostics.extend(boxes::slips(&boxes));

    for t in &mut texts {
        let last = Pos::new(t.pos.row, t.pos.col + t.width - 1);
        t.title_of = boxes.iter().position(|b| {
            b.rect.top == t.pos.row
                && b.title_gap.is_some_and(|(start, end)| (start..end).contains(&t.pos.col))
        });
        t.divider_of = boxes.iter().position(|b| {
            b.dividers.iter().any(|d| {
                d.row == t.pos.row
                    && d.gap.is_some_and(|(start, end)| (start..end).contains(&t.pos.col))
            })
        });
        t.owner = (0..boxes.len())
            .filter(|&i| {
                let r = boxes[i].rect;
                r.interior_contains(t.pos) && r.interior_contains(last)
            })
            .min_by_key(|&i| boxes[i].rect.area());
    }
    let wires = trace::trace(grid, &strokes, &boxes, &mut diagnostics);
    diagnostics.sort_by_key(|d| d.at);
    Diagram { boxes, wires, texts, diagnostics, strokes }
}

/// Whether `t`'s cells are hidden from stroke recognition. An unclosed
/// literal is left alone: it is already `S002`, and masking the rest of its
/// row would also erase the border of the box it sits in.
fn masks(grid: &Grid, t: &Text) -> bool {
    match t.kind {
        TextKind::Comment => true,
        TextKind::Literal => grid.get(Pos::new(t.pos.row, t.pos.col + t.width - 1)) == '»',
        TextKind::Word | TextKind::Number => false,
    }
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
            for d in &b.dividers {
                write!(f, "  divider {}", d.row)?;
                if let Some((start, end)) = d.gap {
                    write!(f, " gap {start}-{end}")?;
                }
                writeln!(f)?;
            }
            for s in &b.slips {
                writeln!(f, "  slip {} home {}", s.at, s.home)?;
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
            write_place(f, " divider", t.divider_of)?;
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
        assert_eq!(d.boxes[1].dividers, vec![Divider { row: 3, gap: None }]);
        assert_eq!(d.boxes[1].ports.len(), 1);
    }

    #[test]
    fn divider_guard_gap() {
        let d = diagram("┌─ 0 ─▼──┐\n│        │\n├┄ 1 ┄┄┄┄┤\n│        │\n└────────┘\n");
        assert_eq!(d.boxes[0].dividers, vec![Divider { row: 2, gap: Some((2, 5)) }]);
        let guard = d.texts.iter().find(|t| t.content == "1").unwrap();
        assert_eq!((guard.divider_of, guard.owner), (Some(0), Some(0)));
    }

    fn codes(d: &Diagram) -> Vec<(Pos, &str)> {
        d.diagnostics.iter().map(|d| (d.at, d.code)).collect()
    }

    const NODE: &str = "┌───┐\n▶ 𐑐 ├─\n└───┘\n";

    #[test]
    fn slipped_edge_cell_is_border() {
        let src = NODE.replace("└───┘", "└─ ─┘").replace("┘\n", "┘\n  ─\n");
        let d = diagram(&src);
        assert_eq!(d.boxes.len(), 1, "{d}");
        let b = &d.boxes[0];
        assert_eq!(b.slips, [Slip { at: Pos::new(3, 2), home: Pos::new(2, 2) }]);
        assert_eq!(b.ports.len(), 2);
        assert_eq!(codes(&d), [(Pos::new(3, 2), "S012")]);
        assert_eq!(d.wires.len(), 1, "the slipped cell is not a wire");
        assert!(d.to_string().contains("  slip 3:2 home 2:2\n"));
    }

    #[test]
    fn slipped_vertical_and_inward() {
        let src = "┌───┐\n│ 𐑐 ├─\n│  │\n└───┘\n";
        let d = diagram(src);
        assert_eq!(d.boxes[0].slips, [Slip { at: Pos::new(2, 3), home: Pos::new(2, 4) }]);
    }

    #[test]
    fn slipped_corner_fixes_style() {
        let d = diagram("╭───╮\n│ 𐑐 │\n╰───\n    ╯\n");
        let b = &d.boxes[0];
        assert_eq!((b.rect, b.style), (shawline_grid::Rect { top: 0, left: 0, bottom: 2, right: 4 }, BoxStyle::Rounded));
        assert_eq!(b.slips, [Slip { at: Pos::new(3, 4), home: Pos::new(2, 4) }]);
        let mixed = diagram("╭───╮\n│ 𐑐 │\n╰───\n    ┘\n");
        assert_eq!(mixed.boxes[0].style, BoxStyle::Mixed);
    }

    #[test]
    fn not_slips() {
        let two_out = NODE.replace("└───┘", "└─ ─┘").replace("┘\n", "┘\n\n  ─\n");
        let joined = NODE.replace("└───┘", "└─ ─┘").replace("┘\n", "┘\n  ──\n");
        let two_corners = "┌───┐\n│ 𐑐 │\n└───\n\n    ┘\n";
        let missing_two = "┌───\n│ 𐑐 │\n└───\n    ┘\n";
        for src in [two_out.as_str(), joined.as_str(), two_corners, missing_two] {
            let d = diagram(src);
            assert!(d.boxes.is_empty(), "{src}\n{d}");
            assert!(!codes(&d).iter().any(|&(_, c)| c == "S012"));
        }
    }

    #[test]
    fn closed_box_has_no_slips() {
        let d = diagram(NODE);
        assert!(d.boxes[0].slips.is_empty());
        assert!(!d.to_string().contains("slip"));
    }

    #[test]
    fn comments_and_literals_draw_nothing() {
        for src in ["※ ─\n", "─ ※ ─\n", "«─» ─\n", "※ ┌┐\n※ └┘\n"] {
            let d = diagram(src);
            assert!(d.boxes.is_empty(), "{src}\n{d}");
            let in_text = |w: &Wire| w.cells.iter().any(|&p| d.strokes.is_masked(p));
            assert!(!d.wires.iter().any(in_text), "{src}\n{d}");
        }
        assert_eq!(diagram("※ ─\n").wires.len(), 0);
        let d = diagram("─ ※ ─\n");
        assert_eq!(d.wires.len(), 1);
        assert!(d.wires[0].ends.iter().all(|e| e.kind == EndKind::Open), "{d}");
        assert!(d.diagnostics.is_empty(), "{:?}", d.diagnostics);
    }

    #[test]
    fn ascii_joint_ignores_comment_neighbours() {
        let d = diagram("  +─\n※ │\n");
        assert_eq!(d.strokes.get(Pos::new(0, 2)).map(|s| s.conn), Some(stroke::E));
    }

    #[test]
    fn commented_stroke_is_not_a_slip() {
        let src = NODE.replace("└───┘", "└─ ─┘").replace("┘\n", "┘\n※ ─\n");
        let d = diagram(&src);
        assert!(d.boxes.is_empty(), "{d}");
        assert!(!codes(&d).iter().any(|&(_, c)| c == "S012"));
    }

    #[test]
    fn unclosed_literal_keeps_its_box() {
        let d = diagram("┌────┐\n│ «𐑐 │\n└────┘\n");
        assert_eq!(d.boxes.len(), 1, "{d}");
        assert!(codes(&d).contains(&(Pos::new(1, 2), "S002")));
    }

    #[test]
    fn overlapping_boxes_are_diagnosed() {
        let d = diagram("┌──┐\n│ ┌┼─┐\n└─┼┘ │\n  └──┘\n");
        assert_eq!(d.diagnostics.len(), 1, "{:?}", d.diagnostics);
    }
}
