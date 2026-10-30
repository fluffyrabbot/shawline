//! The Shawline language server: diagnostics, formatting, semantic tokens,
//! hovers, quick fixes, and transliteration as you type or of a selection.
//!
//! The gloss layer (PLAN P9) is on unless the client turns it off, with
//! `{"gloss": false}` in `initializationOptions` or `{"shawline": {"gloss":
//! false}}` in `workspace/didChangeConfiguration`. It adds each word's reading
//! to hovers and to diagnostic messages.
//!
//! The server is synchronous. Messages are handled strictly in arrival order,
//! which transliteration relies on: the `didChange` carrying a typed key must
//! land before the `onTypeFormatting` request that rewrites it.

pub mod actions;
pub mod analysis;
pub mod position;
pub mod typing;

use std::collections::HashMap;
use std::error::Error;

use lsp_server::{Connection, ErrorCode, Message, Notification, Request, Response};
use lsp_types::notification::{
    DidChangeConfiguration, DidChangeTextDocument, DidCloseTextDocument, DidOpenTextDocument,
    Notification as _, PublishDiagnostics,
};
use lsp_types::request::{
    CodeActionRequest, Formatting, HoverRequest, OnTypeFormatting, Request as _,
    SemanticTokensFullRequest,
};
use lsp_types::{
    CodeAction, CodeActionKind, CodeActionOptions, CodeActionOrCommand, CodeActionParams,
    CodeActionProviderCapability, CodeActionResponse, WorkspaceEdit,
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
use shawline_doc::Doc;
use shawline_edit::Repair;
use shawline_grid::Severity;

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
        enc: position::negotiate(offered),
        docs: HashMap::new(),
        gloss: params
            .initialization_options
            .as_ref()
            .and_then(|options| options["gloss"].as_bool())
            .unwrap_or(true),
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
        position_encoding: Some(position::kind(enc)),
        text_document_sync: Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL)),
        hover_provider: Some(HoverProviderCapability::Simple(true)),
        code_action_provider: Some(CodeActionProviderCapability::Options(CodeActionOptions {
            code_action_kinds: Some(vec![
                CodeActionKind::QUICKFIX,
                CodeActionKind::REFACTOR_REWRITE,
            ]),
            ..Default::default()
        })),
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

/// An open document: one revision per version the client sends.
struct Open {
    doc: Doc,
    version: i32,
    typing: Typing,
}

struct Server {
    enc: Encoding,
    docs: HashMap<Uri, Open>,
    /// Whether hovers and messages carry readings (`shawline.gloss`).
    gloss: bool,
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
            CodeActionRequest::METHOD => respond::<CodeActionRequest>(req, |p| self.code_action(p)),
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
                let entry = Open {
                    doc: Doc::new(doc.text),
                    version: doc.version,
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
                let (Some(open), Some(change)) = (
                    self.docs.get_mut(&uri),
                    p.content_changes.into_iter().last(),
                ) else {
                    return Vec::new();
                };
                open.doc = Doc::new(change.text);
                open.version = p.text_document.version;
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
            DidChangeConfiguration::METHOD => {
                let Ok(p) = note.extract::<lsp_types::DidChangeConfigurationParams>(
                    DidChangeConfiguration::METHOD,
                ) else {
                    return Vec::new();
                };
                if let Some(gloss) = p.settings["shawline"]["gloss"].as_bool() {
                    self.gloss = gloss;
                }
                // The setting rewords messages, so every open document's go out again.
                let mut uris: Vec<&Uri> = self.docs.keys().collect();
                uris.sort_by_key(|uri| uri.as_str());
                return uris.into_iter().map(|uri| self.publish(uri)).collect();
            }
            _ => return Vec::new(),
        };
        vec![self.publish(&uri)]
    }

    /// The diagnostics notification for the open document `uri`.
    fn publish(&self, uri: &Uri) -> Notification {
        let open = &self.docs[uri];
        let params = PublishDiagnosticsParams::new(
            uri.clone(),
            self.diagnostics(&open.doc),
            Some(open.version),
        );
        Notification::new(PublishDiagnostics::METHOD.into(), params)
    }

    fn diagnostics(&self, doc: &Doc) -> Vec<lsp_types::Diagnostic> {
        doc.diagnostics()
            .iter()
            .map(|d| self.diagnostic(doc, d))
            .collect()
    }

    /// `d` as published, so a code action can attach the very same one.
    fn diagnostic(&self, doc: &Doc, d: &shawline_grid::Diagnostic) -> lsp_types::Diagnostic {
        lsp_types::Diagnostic {
            range: position::cells_range(doc.text(), self.enc, d.at, 1),
            severity: Some(match d.severity() {
                Severity::Error => DiagnosticSeverity::ERROR,
                Severity::Warning => DiagnosticSeverity::WARNING,
                Severity::Lint => DiagnosticSeverity::HINT,
            }),
            code: Some(NumberOrString::String(d.code.to_string())),
            source: Some("shawline".into()),
            message: if self.gloss {
                shawline_word::annotate(&d.message)
            } else {
                d.message.clone()
            },
            ..Default::default()
        }
    }

    fn formatting(&self, p: DocumentFormattingParams) -> Option<Vec<TextEdit>> {
        let doc = &self.docs.get(&p.text_document.uri)?.doc;
        let formatted = doc.formatted()?;
        if formatted == doc.text() {
            return Some(Vec::new());
        }
        let whole = Range::new(Default::default(), position::end_of(doc.text(), self.enc));
        Some(vec![TextEdit::new(whole, formatted.to_owned())])
    }

    fn semantic_tokens(&self, p: SemanticTokensParams) -> Option<SemanticTokensResult> {
        let doc = &self.docs.get(&p.text_document.uri)?.doc;
        let mut data = Vec::new();
        let (mut line, mut start) = (0, 0);
        for token in analysis::tokens(doc) {
            let range = position::cells_range(doc.text(), self.enc, token.at, token.width);
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
        let doc = &self.docs.get(&at.text_document.uri)?.doc;
        let cell = position::lsp_to_cell(doc.text(), self.enc, at.position);
        let (start, width, text) = analysis::hover(doc, cell, self.gloss)?;
        Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: text,
            }),
            range: Some(position::cells_range(doc.text(), self.enc, start, width)),
        })
    }

    fn on_type(&mut self, p: DocumentOnTypeFormattingParams) -> Option<Vec<TextEdit>> {
        let at = p.text_document_position;
        let enc = self.enc;
        let open = self.docs.get_mut(&at.text_document.uri)?;
        let mut key = p.ch.chars();
        let (Some(key), None) = (key.next(), key.next()) else {
            open.typing.reset();
            return None;
        };
        let text = open.doc.text();
        let row = at.position.line as usize;
        let line = position::line(text, row);
        let cursor = position::from_lsp(text, enc, at.position);
        let r = open.typing.on_type(line, row, cursor, key)?;
        let range = Range::new(
            position::to_lsp(text, enc, r.row, r.start),
            position::to_lsp(text, enc, r.row, r.end),
        );
        Some(vec![TextEdit::new(range, r.text)])
    }

    /// The quick fixes for the diagnostics the range touches, then the
    /// conversion of the keys in it, each when the client's `only` asks for
    /// its kind.
    fn code_action(&self, p: CodeActionParams) -> Option<CodeActionResponse> {
        let uri = p.text_document.uri;
        let doc = &self.docs.get(&uri)?.doc;
        let wanted = |kind: &CodeActionKind| {
            p.context
                .only
                .as_ref()
                .is_none_or(|only| only.iter().any(|o| covers(o, kind)))
        };
        let mut offered: CodeActionResponse = Vec::new();
        if wanted(&CodeActionKind::QUICKFIX) {
            offered.extend(self.quick_fixes(&uri, doc, p.range, shawline_edit::repairs));
        }
        if wanted(&CodeActionKind::REFACTOR_REWRITE) {
            let selected = position::selected(doc.text(), self.enc, p.range);
            let converted = typing::convert(selected.before, selected.text);
            if converted != selected.text {
                offered.push(CodeActionOrCommand::CodeAction(CodeAction {
                    title: "Convert keys to Shavian".into(),
                    kind: Some(CodeActionKind::REFACTOR_REWRITE),
                    edit: Some(workspace_edit(
                        &uri,
                        TextEdit::new(selected.range, converted),
                    )),
                    ..Default::default()
                }));
            }
        }
        Some(offered)
    }

    /// The repairs `repairs` finds, made into quick fixes for the
    /// diagnostics of `doc` (open at `uri`) that `range` touches. Code
    /// actions are asked for at every move of the cursor, so `repairs` runs
    /// only when some diagnostic is touched.
    fn quick_fixes(
        &self,
        uri: &Uri,
        doc: &Doc,
        range: Range,
        repairs: impl FnOnce(&Doc) -> Vec<Repair>,
    ) -> Vec<CodeActionOrCommand> {
        let touched = |d: &shawline_grid::Diagnostic| {
            actions::touches(position::cells_range(doc.text(), self.enc, d.at, 1), range)
        };
        if !doc.diagnostics().iter().any(touched) {
            return Vec::new();
        }
        actions::fixes(doc, &repairs(doc), touched)
            .into_iter()
            .map(|fix| {
                let edit = actions::replacement(doc.text(), &fix.text, self.enc);
                CodeActionOrCommand::CodeAction(CodeAction {
                    title: fix.title,
                    kind: Some(CodeActionKind::QUICKFIX),
                    diagnostics: Some(vec![self.diagnostic(doc, fix.diagnostic)]),
                    edit: Some(workspace_edit(uri, edit)),
                    is_preferred: fix.preferred.then_some(true),
                    ..Default::default()
                })
            })
            .collect()
    }
}

/// True if `only`, a kind a client asks for, asks for `kind`. Kinds are
/// dotted paths, and asking for one asks for every kind beneath it.
fn covers(only: &CodeActionKind, kind: &CodeActionKind) -> bool {
    let (only, kind) = (only.as_str(), kind.as_str());
    only.is_empty()
        || kind
            .strip_prefix(only)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('.'))
}

/// One edit to the document at `uri`.
fn workspace_edit(uri: &Uri, edit: TextEdit) -> WorkspaceEdit {
    WorkspaceEdit {
        changes: Some(HashMap::from([(uri.clone(), vec![edit])])),
        ..Default::default()
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

#[cfg(test)]
mod tests {
    use super::*;
    use lsp_types::Position;
    use shawline_edit::{Edit, Followup, apply};
    use shawline_grid::Pos;

    // Until `shawline_edit::repairs` offers anything, these hand the server
    // repairs of their own.

    fn corpus(name: &str) -> String {
        let path = format!("{}/../../corpus/{name}", env!("CARGO_MANIFEST_DIR"));
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
    }

    /// `text` with one edit applied.
    fn applied(text: &str, enc: Encoding, edit: &TextEdit) -> String {
        let offset = |pos: Position| {
            position::selected(text, enc, Range::new(Position::new(0, 0), pos))
                .text
                .len()
        };
        let (start, end) = (offset(edit.range.start), offset(edit.range.end));
        format!("{}{}{}", &text[..start], edit.new_text, &text[end..])
    }

    #[test]
    fn a_repair_is_a_quick_fix_on_its_diagnostic() {
        let uri: Uri = "file:///sheet.shaw".parse().unwrap();
        let text = corpus("static/12-unused-input.shaw");
        let doc = Doc::new(text.as_str());
        let at = Pos::new(6, 0);
        let edit = Edit::RemovePort(at);
        let repairs = [Repair {
            at,
            code: "W002",
            label: "Remove the unused input".into(),
            edit: Some(edit.clone()),
            then: Followup::None,
        }];
        let fixed = apply(&doc, &edit).unwrap().doc.text().to_owned();
        for enc in [Encoding::Utf16, Encoding::Utf32] {
            let server = Server {
                enc,
                docs: HashMap::new(),
                gloss: true,
            };
            let published = server.diagnostics(&doc);
            let [diagnostic] = published.as_slice() else {
                panic!("{published:?}")
            };
            let r = diagnostic.range;
            // A selection over it or the cursor at either edge finds it.
            let whole = Range::new(Position::new(0, 0), position::end_of(&text, enc));
            for range in [
                r,
                Range::new(r.start, r.start),
                Range::new(r.end, r.end),
                whole,
            ] {
                let offered = server.quick_fixes(&uri, &doc, range, |_| repairs.to_vec());
                let [CodeActionOrCommand::CodeAction(action)] = offered.as_slice() else {
                    panic!("{offered:?}")
                };
                assert_eq!(action.title, "Remove the unused input");
                assert_eq!(action.kind, Some(CodeActionKind::QUICKFIX));
                assert_eq!(
                    action.diagnostics.as_deref(),
                    Some(std::slice::from_ref(diagnostic))
                );
                assert_eq!(action.is_preferred, Some(true));
                let edits = action
                    .edit
                    .as_ref()
                    .and_then(|e| e.changes.as_ref()?.get(&uri))
                    .unwrap();
                let [edit] = edits.as_slice() else {
                    panic!("{edits:?}")
                };
                assert_eq!(applied(&text, enc, edit), fixed);
            }
            let elsewhere = Range::new(Position::new(1, 0), Position::new(1, 0));
            assert!(
                server
                    .quick_fixes(&uri, &doc, elsewhere, |_| repairs.to_vec())
                    .is_empty()
            );
        }
    }

    #[test]
    fn only_asks_by_dotted_prefix() {
        let kind = |s: &'static str| CodeActionKind::new(s);
        assert!(covers(&kind("refactor"), &CodeActionKind::REFACTOR_REWRITE));
        assert!(covers(
            &kind("refactor.rewrite"),
            &CodeActionKind::REFACTOR_REWRITE
        ));
        assert!(covers(&kind(""), &CodeActionKind::QUICKFIX));
        assert!(!covers(
            &kind("refactor.re"),
            &CodeActionKind::REFACTOR_REWRITE
        ));
        assert!(!covers(
            &kind("refactor.rewrite.more"),
            &CodeActionKind::REFACTOR_REWRITE
        ));
        assert!(!covers(
            &kind("quickfix"),
            &CodeActionKind::REFACTOR_REWRITE
        ));
    }
}
