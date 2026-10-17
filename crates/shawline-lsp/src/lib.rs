//! The Shawline language server: diagnostics, formatting, semantic tokens,
//! hovers, and transliteration as you type.
//!
//! The server is synchronous. Messages are handled strictly in arrival order,
//! which transliteration relies on: the `didChange` carrying a typed key must
//! land before the `onTypeFormatting` request that rewrites it.

pub mod analysis;
pub mod position;
pub mod typing;

use std::collections::HashMap;
use std::error::Error;

use lsp_server::{Connection, ErrorCode, Message, Notification, Request, Response};
use lsp_types::notification::{
    DidChangeTextDocument, DidCloseTextDocument, DidOpenTextDocument, Notification as _,
    PublishDiagnostics,
};
use lsp_types::request::{
    Formatting, HoverRequest, OnTypeFormatting, Request as _, SemanticTokensFullRequest,
};
use lsp_types::{
    DiagnosticSeverity, DocumentFormattingParams, DocumentOnTypeFormattingOptions,
    DocumentOnTypeFormattingParams, Hover, HoverContents, HoverParams, HoverProviderCapability,
    InitializeParams, MarkupContent, MarkupKind, NumberOrString, OneOf, PublishDiagnosticsParams,
    Range, SemanticToken, SemanticTokens, SemanticTokensFullOptions, SemanticTokensLegend,
    SemanticTokensOptions, SemanticTokensParams, SemanticTokensResult,
    SemanticTokensServerCapabilities, ServerCapabilities, TextDocumentSyncCapability,
    TextDocumentSyncKind, TextEdit, Uri,
};
use shawline_cli::Checked;
use shawline_grid::Diagnostic;

use position::Encoding;
use typing::Typing;

type BoxError = Box<dyn Error + Send + Sync>;

/// Runs the server on `connection` until the client shuts it down.
pub fn run(connection: &Connection) -> Result<(), BoxError> {
    let (id, params) = connection.initialize_start()?;
    let params: InitializeParams = serde_json::from_value(params)?;
    let offered = params
        .capabilities
        .general
        .as_ref()
        .and_then(|g| g.position_encodings.as_deref());
    let mut server = Server {
        enc: Encoding::negotiate(offered),
        docs: HashMap::new(),
    };
    connection.initialize_finish(
        id,
        serde_json::json!({
            "capabilities": capabilities(server.enc),
            "serverInfo": { "name": "shawline-lsp", "version": env!("CARGO_PKG_VERSION") },
        }),
    )?;
    for msg in &connection.receiver {
        match msg {
            Message::Request(req) => {
                if connection.handle_shutdown(&req)? {
                    return Ok(());
                }
                connection.sender.send(server.request(req).into())?;
            }
            Message::Notification(note) => {
                for out in server.notify(note) {
                    connection.sender.send(out.into())?;
                }
            }
            Message::Response(_) => {}
        }
    }
    Ok(())
}

/// What the server offers, given the negotiated position encoding.
pub fn capabilities(enc: Encoding) -> ServerCapabilities {
    let mut triggers = typing::trigger_characters().into_iter();
    let first = triggers.next().expect("the key table is not empty");
    ServerCapabilities {
        position_encoding: Some(enc.kind()),
        text_document_sync: Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL)),
        hover_provider: Some(HoverProviderCapability::Simple(true)),
        document_formatting_provider: Some(OneOf::Left(true)),
        document_on_type_formatting_provider: Some(DocumentOnTypeFormattingOptions {
            first_trigger_character: first,
            more_trigger_character: Some(triggers.collect()),
        }),
        semantic_tokens_provider: Some(SemanticTokensServerCapabilities::SemanticTokensOptions(
            SemanticTokensOptions {
                legend: SemanticTokensLegend {
                    token_types: analysis::TOKEN_TYPES.to_vec(),
                    token_modifiers: analysis::TOKEN_MODIFIERS.to_vec(),
                },
                full: Some(SemanticTokensFullOptions::Bool(true)),
                ..Default::default()
            },
        )),
        ..Default::default()
    }
}

struct Doc {
    text: String,
    version: i32,
    checked: Result<Checked, Vec<Diagnostic>>,
    typing: Typing,
}

impl Doc {
    fn set(&mut self, text: String, version: i32) {
        self.checked = shawline_cli::check(&text);
        self.text = text;
        self.version = version;
    }
}

struct Server {
    enc: Encoding,
    docs: HashMap<Uri, Doc>,
}

impl Server {
    fn request(&mut self, req: Request) -> Response {
        match req.method.as_str() {
            Formatting::METHOD => respond::<Formatting>(req, |p| self.formatting(p)),
            SemanticTokensFullRequest::METHOD => {
                respond::<SemanticTokensFullRequest>(req, |p| self.semantic_tokens(p))
            }
            HoverRequest::METHOD => respond::<HoverRequest>(req, |p| self.hover(p)),
            OnTypeFormatting::METHOD => respond::<OnTypeFormatting>(req, |p| self.on_type(p)),
            _ => Response::new_err(
                req.id,
                ErrorCode::MethodNotFound as i32,
                format!("unhandled method {}", req.method),
            ),
        }
    }

    fn notify(&mut self, note: Notification) -> Vec<Notification> {
        let uri = match note.method.as_str() {
            DidOpenTextDocument::METHOD => {
                let Ok(p) = note
                    .extract::<lsp_types::DidOpenTextDocumentParams>(DidOpenTextDocument::METHOD)
                else {
                    return Vec::new();
                };
                let doc = p.text_document;
                let checked = shawline_cli::check(&doc.text);
                let entry = Doc {
                    text: doc.text,
                    version: doc.version,
                    checked,
                    typing: Typing::default(),
                };
                self.docs.insert(doc.uri.clone(), entry);
                doc.uri
            }
            DidChangeTextDocument::METHOD => {
                let Ok(p) = note.extract::<lsp_types::DidChangeTextDocumentParams>(
                    DidChangeTextDocument::METHOD,
                ) else {
                    return Vec::new();
                };
                let uri = p.text_document.uri;
                // Sync is FULL, so the last change holds the whole text.
                let (Some(doc), Some(change)) = (
                    self.docs.get_mut(&uri),
                    p.content_changes.into_iter().last(),
                ) else {
                    return Vec::new();
                };
                doc.set(change.text, p.text_document.version);
                uri
            }
            DidCloseTextDocument::METHOD => {
                let Ok(p) = note
                    .extract::<lsp_types::DidCloseTextDocumentParams>(DidCloseTextDocument::METHOD)
                else {
                    return Vec::new();
                };
                self.docs.remove(&p.text_document.uri);
                let params = PublishDiagnosticsParams::new(p.text_document.uri, Vec::new(), None);
                return vec![Notification::new(PublishDiagnostics::METHOD.into(), params)];
            }
            _ => return Vec::new(),
        };
        let doc = &self.docs[&uri];
        let params =
            PublishDiagnosticsParams::new(uri.clone(), self.diagnostics(doc), Some(doc.version));
        vec![Notification::new(PublishDiagnostics::METHOD.into(), params)]
    }

    fn diagnostics(&self, doc: &Doc) -> Vec<lsp_types::Diagnostic> {
        let found = match &doc.checked {
            Ok(checked) => &checked.diagnostics,
            Err(grid) => grid,
        };
        found
            .iter()
            .map(|d| lsp_types::Diagnostic {
                range: position::cells_range(&doc.text, self.enc, d.at, 1),
                severity: Some(if d.is_warning() {
                    DiagnosticSeverity::WARNING
                } else {
                    DiagnosticSeverity::ERROR
                }),
                code: Some(NumberOrString::String(d.code.to_string())),
                source: Some("shawline".into()),
                message: d.message.clone(),
                ..Default::default()
            })
            .collect()
    }

    fn formatting(&self, p: DocumentFormattingParams) -> Option<Vec<TextEdit>> {
        let doc = self.docs.get(&p.text_document.uri)?;
        let formatted = shawline_cli::format(&doc.text).ok()?;
        if formatted == doc.text {
            return Some(Vec::new());
        }
        let whole = Range::new(Default::default(), position::end_of(&doc.text, self.enc));
        Some(vec![TextEdit::new(whole, formatted)])
    }

    fn semantic_tokens(&self, p: SemanticTokensParams) -> Option<SemanticTokensResult> {
        let doc = self.docs.get(&p.text_document.uri)?;
        let Ok(checked) = &doc.checked else {
            return Some(SemanticTokensResult::Tokens(SemanticTokens::default()));
        };
        let mut data = Vec::new();
        let (mut line, mut start) = (0, 0);
        for token in analysis::tokens(checked) {
            let range = position::cells_range(&doc.text, self.enc, token.at, token.width);
            let (token_type, token_modifiers_bitset) = token.kind.encode();
            let delta_line = range.start.line - line;
            let delta_start = if delta_line == 0 {
                range.start.character - start
            } else {
                range.start.character
            };
            data.push(SemanticToken {
                delta_line,
                delta_start,
                length: range.end.character - range.start.character,
                token_type,
                token_modifiers_bitset,
            });
            (line, start) = (range.start.line, range.start.character);
        }
        Some(SemanticTokensResult::Tokens(SemanticTokens {
            result_id: None,
            data,
        }))
    }

    fn hover(&self, p: HoverParams) -> Option<Hover> {
        let at = p.text_document_position_params;
        let doc = self.docs.get(&at.text_document.uri)?;
        let checked = doc.checked.as_ref().ok()?;
        let cell = position::lsp_to_cell(&doc.text, self.enc, at.position);
        let (start, width, text) = analysis::hover(checked, cell)?;
        Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: text,
            }),
            range: Some(position::cells_range(&doc.text, self.enc, start, width)),
        })
    }

    fn on_type(&mut self, p: DocumentOnTypeFormattingParams) -> Option<Vec<TextEdit>> {
        let at = p.text_document_position;
        let enc = self.enc;
        let doc = self.docs.get_mut(&at.text_document.uri)?;
        let mut key = p.ch.chars();
        let (Some(key), None) = (key.next(), key.next()) else {
            doc.typing.reset();
            return None;
        };
        let row = at.position.line as usize;
        let line = position::line(&doc.text, row);
        let cursor = position::from_lsp(&doc.text, enc, at.position);
        let r = doc.typing.on_type(line, row, cursor, key)?;
        let range = Range::new(
            position::to_lsp(&doc.text, enc, r.row, r.start),
            position::to_lsp(&doc.text, enc, r.row, r.end),
        );
        Some(vec![TextEdit::new(range, r.text)])
    }
}

/// Decodes a request's params, runs `f`, and encodes its result.
fn respond<R: lsp_types::request::Request>(
    req: Request,
    f: impl FnOnce(R::Params) -> R::Result,
) -> Response {
    match serde_json::from_value::<R::Params>(req.params) {
        Ok(params) => Response::new_ok(req.id, f(params)),
        Err(e) => Response::new_err(req.id, ErrorCode::InvalidParams as i32, e.to_string()),
    }
}
