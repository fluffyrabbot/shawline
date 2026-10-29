//! What a word on the sheet is, in words: one description for editor hover,
//! Studio's inspector, the palette and the drawing's hover title (PLAN P10).
//! Moved verbatim from the language server's hover.

use shawline_doc::Doc;
use shawline_grid::repertoire::is_word_char;
use shawline_grid::{Grid, Pos};
use shawline_syntax::ast::{Definition, Graph, Name, NodeKind, Sheet};
use shawline_vm::bytecode::Prim;

use crate::Description;

fn name_width(name: &Name) -> usize {
    name.text.chars().count()
}

enum Callee<'a> {
    Primitive(Prim),
    Definition(&'a Definition),
    Unknown,
}

fn callee<'a>(sheet: &'a Sheet, name: &Name) -> Callee<'a> {
    let mut chars = name.text.chars();
    if let (Some(c), None) = (chars.next(), chars.next())
        && let Some(p) = Prim::from_letter(c)
    {
        return Callee::Primitive(p);
    }
    sheet
        .definitions
        .iter()
        .find(|d| d.name.text == name.text)
        .map_or(Callee::Unknown, Callee::Definition)
}

pub(crate) fn describe(doc: &Doc, at: Pos, gloss: bool) -> Option<Description> {
    let described = doc.sheet().and_then(|sheet| described(sheet, at));
    let word = if gloss {
        doc.grid().ok().and_then(|grid| word_at(grid, at))
    } else {
        None
    };
    let Some((start, width, word)) = word else {
        return described.map(|d| Description { at: d.at, width: d.width, text: d.text });
    };
    let mut reading =
        format!("Reading: `{}` · Type: `{}`", shawline_word::reading(&word), shawline_word::keys(&word));
    if let Some(p) = described.as_ref().and_then(|d| d.prim) {
        reading.push_str(" · ");
        reading.push_str(p.gloss());
    }
    Some(match described {
        Some(d) => Description { at: d.at, width: d.width, text: format!("{}\n\n{reading}", d.text) },
        None => Description { at: start, width, text: reading },
    })
}

/// What hover says about a call word or definition title.
struct Described {
    at: Pos,
    width: usize,
    text: String,
    /// The primitive a call names.
    prim: Option<Prim>,
}

fn described(sheet: &Sheet, at: Pos) -> Option<Described> {
    let covers = |start: Pos, width: usize| {
        start.row == at.row && (start.col..start.col + width).contains(&at.col)
    };
    for def in &sheet.definitions {
        if covers(def.name.at, name_width(&def.name) + 1) {
            return Some(Described {
                at: def.name.at,
                width: name_width(&def.name) + 1,
                text: definition_hover(def),
                prim: None,
            });
        }
        if let Some(found) = graph_hover(sheet, &def.body, &covers) {
            return Some(found);
        }
    }
    None
}

fn graph_hover(
    sheet: &Sheet,
    graph: &Graph,
    covers: &impl Fn(Pos, usize) -> bool,
) -> Option<Described> {
    for node in &graph.nodes {
        match &node.kind {
            NodeKind::Call(name) | NodeKind::Inline(name) if covers(name.at, name_width(name)) => {
                let (text, prim) = match callee(sheet, name) {
                    Callee::Primitive(p) => (primitive_hover(p), Some(p)),
                    Callee::Definition(def) => (definition_hover(def), None),
                    Callee::Unknown => return None,
                };
                return Some(Described {
                    at: name.at,
                    width: name_width(name),
                    text,
                    prim,
                });
            }
            NodeKind::Switch(switch) => {
                for arm in &switch.arms {
                    if let Some(found) = graph_hover(sheet, &arm.body, covers) {
                        return Some(found);
                    }
                }
            }
            _ => {}
        }
    }
    None
}

/// The word (SPEC §4) covering `at`, read straight off the grid so that
/// ports, `«»` text and comments count: its first cell, width and text.
/// Numbers are not words here, since they have nothing to read.
fn word_at(grid: &Grid, at: Pos) -> Option<(Pos, usize, String)> {
    let cells = grid.row(at.row);
    if !cells.get(at.col).is_some_and(|&c| is_word_char(c)) {
        return None;
    }
    let start = cells[..at.col]
        .iter()
        .rposition(|&c| !is_word_char(c))
        .map_or(0, |i| i + 1);
    let end = cells[at.col..]
        .iter()
        .position(|&c| !is_word_char(c))
        .map_or(cells.len(), |n| at.col + n);
    let word: String = cells[start..end].iter().collect();
    if word.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some((Pos::new(at.row, start), end - start, word))
}

fn primitive_line(p: Prim) -> String {
    let (ins, outs) = p.arity();
    format!("**{}** {} · {ins}→{outs}", p.letter(), p.meaning())
}

fn primitive_hover(p: Prim) -> String {
    format!(
        "{}\n\nMirror: {}",
        primitive_line(p),
        primitive_line(p.mirror())
    )
}

fn definition_hover(def: &Definition) -> String {
    let names = |ports: &[shawline_syntax::ast::PortDecl]| {
        if ports.is_empty() {
            "none".to_string()
        } else {
            ports
                .iter()
                .map(|p| p.name.text.as_str())
                .collect::<Vec<_>>()
                .join(" ")
        }
    };
    format!(
        "**·{}** · {}→{}\n\nInputs: {}\n\nOutputs: {}",
        def.name.text,
        def.inputs.len(),
        def.outputs.len(),
        names(&def.inputs),
        names(&def.outputs),
    )
}
