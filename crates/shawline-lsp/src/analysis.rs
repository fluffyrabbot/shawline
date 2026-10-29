//! What the server reads out of a checked sheet: semantic tokens and hovers.
//! Both work in grid cells; the caller converts to LSP positions. Hover text
//! is `shawline_edit::describe`, shared with Studio.

use lsp_types::{SemanticTokenModifier, SemanticTokenType};
use shawline_doc::Doc;
use shawline_grid::Pos;
use shawline_syntax::ast::{Graph, Literal, Name, NodeKind};
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
        graph_tokens(&def.body, &width_at, &mut out);
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
    graph: &Graph,
    width_at: &impl Fn(Pos) -> usize,
    out: &mut Vec<Token>,
) {
    for node in &graph.nodes {
        match &node.kind {
            NodeKind::Call(name) | NodeKind::Inline(name) => {
                let kind = if is_primitive(name) { Kind::CallPrimitive } else { Kind::CallDefinition };
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
                    graph_tokens(&arm.body, width_at, out);
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

/// Whether a call names a primitive: one letter of SPEC §9's table.
fn is_primitive(name: &Name) -> bool {
    let mut chars = name.text.chars();
    matches!((chars.next(), chars.next()), (Some(c), None) if Prim::from_letter(c).is_some())
}

/// Hover text for the word covering `at`, with the hovered cells' first cell
/// and width ([`shawline_edit::describe`]).
pub fn hover(doc: &Doc, at: Pos, gloss: bool) -> Option<(Pos, usize, String)> {
    shawline_edit::describe(doc, at, gloss).map(|d| (d.at, d.width, d.text))
}
