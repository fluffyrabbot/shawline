//! What the server reads out of a checked sheet: semantic tokens and hovers.
//! Both work in grid cells; the caller converts to LSP positions.

use lsp_types::{SemanticTokenModifier, SemanticTokenType};
use shawline_doc::Doc;
use shawline_grid::repertoire::is_word_char;
use shawline_grid::{Grid, Pos};
use shawline_syntax::ast::{Definition, Graph, Literal, Name, NodeKind, Sheet};
use shawline_vm::bytecode::Prim;

/// Token types, in legend order.
pub const TOKEN_TYPES: [SemanticTokenType; 7] = [
    SemanticTokenType::FUNCTION,
    SemanticTokenType::PARAMETER,
    SemanticTokenType::NUMBER,
    SemanticTokenType::STRING,
    SemanticTokenType::KEYWORD,
    SemanticTokenType::ENUM_MEMBER,
    SemanticTokenType::COMMENT,
];

/// Token modifiers, in legend order (bit 0 first).
pub const TOKEN_MODIFIERS: [SemanticTokenModifier; 3] = [
    SemanticTokenModifier::DECLARATION,
    SemanticTokenModifier::DEFINITION,
    SemanticTokenModifier::DEFAULT_LIBRARY,
];

/// What a highlighted run of cells is, mapped onto standard token types.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    /// A definition's `·`-name in its title: `function` + declaration, definition.
    DefinitionName,
    /// A definition port's name: `parameter` + declaration.
    Port,
    /// A call of a user definition (or of an unknown name): `function`.
    CallDefinition,
    /// A call of a primitive: `function` + defaultLibrary.
    CallPrimitive,
    /// A number constant feeding a wire: `number`.
    Number,
    /// A text constant: `string`.
    Text,
    /// `𐑯𐑦𐑤`, the empty list: `keyword`.
    Nil,
    /// A switch arm's guard: `enumMember`, since it labels a case.
    Guard,
    /// `※ …`: `comment`.
    Comment,
}

impl Kind {
    /// The legend index of the token type and the modifier bitset.
    pub fn encode(self) -> (u32, u32) {
        const DECLARATION: u32 = 1;
        const DEFINITION: u32 = 2;
        const DEFAULT_LIBRARY: u32 = 4;
        match self {
            Kind::DefinitionName => (0, DECLARATION | DEFINITION),
            Kind::Port => (1, DECLARATION),
            Kind::CallDefinition => (0, 0),
            Kind::CallPrimitive => (0, DEFAULT_LIBRARY),
            Kind::Number => (2, 0),
            Kind::Text => (3, 0),
            Kind::Nil => (4, 0),
            Kind::Guard => (5, 0),
            Kind::Comment => (6, 0),
        }
    }
}

/// A highlighted run of `width` cells starting at `at`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Token {
    pub at: Pos,
    pub width: usize,
    pub kind: Kind,
}

/// Every token in the sheet, sorted by position. None when the text has
/// grid errors.
pub fn tokens(doc: &Doc) -> Vec<Token> {
    let (Some(diagram), Some(sheet)) = (doc.diagram(), doc.sheet()) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let width_at = |at: Pos| {
        diagram
            .texts
            .iter()
            .find(|t| t.pos == at)
            .map_or(1, |t| t.width)
    };
    for def in &sheet.definitions {
        out.push(Token {
            at: def.name.at,
            width: name_width(&def.name) + 1,
            kind: Kind::DefinitionName,
        });
        for port in def.inputs.iter().chain(&def.outputs) {
            out.push(Token {
                at: port.name.at,
                width: name_width(&port.name),
                kind: Kind::Port,
            });
        }
        graph_tokens(sheet, &def.body, &width_at, &mut out);
    }
    for t in &diagram.texts {
        if t.kind == shawline_shape::TextKind::Comment {
            out.push(Token {
                at: t.pos,
                width: t.width,
                kind: Kind::Comment,
            });
        }
    }
    out.sort();
    out.dedup_by_key(|t| t.at);
    out
}

fn graph_tokens(
    sheet: &Sheet,
    graph: &Graph,
    width_at: &impl Fn(Pos) -> usize,
    out: &mut Vec<Token>,
) {
    for node in &graph.nodes {
        match &node.kind {
            NodeKind::Call(name) | NodeKind::Inline(name) => {
                let kind = match callee(sheet, name) {
                    Callee::Primitive(_) => Kind::CallPrimitive,
                    Callee::Definition(_) | Callee::Unknown => Kind::CallDefinition,
                };
                out.push(Token {
                    at: name.at,
                    width: name_width(name),
                    kind,
                });
            }
            NodeKind::Const(lit) => {
                let at = Pos::new(node.rect.top, node.rect.left);
                let width = node.rect.right - node.rect.left + 1;
                out.push(Token {
                    at,
                    width,
                    kind: literal_kind(lit),
                });
            }
            NodeKind::Switch(switch) => {
                for arm in &switch.arms {
                    if let Some(guard) = &arm.guard {
                        out.push(Token {
                            at: guard.at,
                            width: width_at(guard.at),
                            kind: Kind::Guard,
                        });
                    }
                    graph_tokens(sheet, &arm.body, width_at, out);
                }
            }
        }
    }
}

fn literal_kind(lit: &Literal) -> Kind {
    match lit {
        Literal::Int(_) => Kind::Number,
        Literal::Text(_) => Kind::Text,
        Literal::Nil => Kind::Nil,
    }
}

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

/// Hover text for the word covering `at`, with the hovered cells' first cell
/// and width. Call words and definition titles are described. With `gloss`,
/// every Shavian word (ports, text and comments too) also gets a line with
/// its reading and the keys that type it, and a primitive its short name on
/// that line.
pub fn hover(doc: &Doc, at: Pos, gloss: bool) -> Option<(Pos, usize, String)> {
    let described = doc.sheet().and_then(|sheet| describe(sheet, at));
    let word = if gloss {
        doc.grid().ok().and_then(|grid| word_at(grid, at))
    } else {
        None
    };
    let Some((start, width, word)) = word else {
        return described.map(|d| (d.at, d.width, d.text));
    };
    let mut reading =
        format!("Reading: `{}` · Type: `{}`", shawline_word::reading(&word), shawline_word::keys(&word));
    if let Some(p) = described.as_ref().and_then(|d| d.prim) {
        reading.push_str(" · ");
        reading.push_str(p.gloss());
    }
    Some(match described {
        Some(d) => (d.at, d.width, format!("{}\n\n{reading}", d.text)),
        None => (start, width, reading),
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

fn describe(sheet: &Sheet, at: Pos) -> Option<Described> {
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
