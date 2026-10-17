//! Drives the server in-process over an in-memory connection, the way an
//! editor would.

use std::thread::JoinHandle;

use lsp_server::{Connection, Message, Notification, Request, RequestId};
use lsp_types::{Position, PositionEncodingKind, Range, TextEdit};
use serde_json::{Value, json};
use shawline_lsp::position::{self, Encoding};

const URI: &str = "file:///sheet.shaw";

fn corpus(name: &str) -> String {
    let path = format!("{}/../../corpus/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

struct Client {
    conn: Connection,
    server: Option<JoinHandle<()>>,
    next_id: i32,
    enc: Encoding,
    /// The negotiated encoding, as the server announced it.
    announced: Value,
    version: i32,
    text: String,
}

impl Client {
    fn start(offer: &[PositionEncodingKind]) -> Client {
        let (server_side, conn) = Connection::memory();
        let server = std::thread::spawn(move || shawline_lsp::run(&server_side).unwrap());
        let mut client = Client {
            conn,
            server: Some(server),
            next_id: 0,
            enc: Encoding::Utf16,
            announced: Value::Null,
            version: 0,
            text: String::new(),
        };
        let caps = json!({ "general": { "positionEncodings": offer } });
        let init = client.request("initialize", json!({ "capabilities": caps }));
        client.announced = init["capabilities"]["positionEncoding"].clone();
        client.enc = match client.announced.as_str() {
            Some("utf-32") => Encoding::Utf32,
            _ => Encoding::Utf16,
        };
        client.notify("initialized", json!({}));
        client
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        self.next_id += 1;
        let id = RequestId::from(self.next_id);
        self.conn
            .sender
            .send(Request::new(id.clone(), method.into(), params).into())
            .unwrap();
        loop {
            match self.conn.receiver.recv().unwrap() {
                Message::Response(r) if r.id == id => {
                    return r
                        .response_result
                        .unwrap_or_else(|e| panic!("{method}: {e:?}"));
                }
                _ => {}
            }
        }
    }

    fn notify(&self, method: &str, params: Value) {
        self.conn
            .sender
            .send(Notification::new(method.into(), params).into())
            .unwrap();
    }

    /// Waits for the next published diagnostics.
    fn diagnostics(&self) -> Vec<Value> {
        loop {
            if let Message::Notification(n) = self.conn.receiver.recv().unwrap()
                && n.method == "textDocument/publishDiagnostics"
            {
                return n.params["diagnostics"].as_array().unwrap().clone();
            }
        }
    }

    fn open(&mut self, text: &str) -> Vec<Value> {
        self.text = text.to_string();
        self.notify(
            "textDocument/didOpen",
            json!({ "textDocument": { "uri": URI, "languageId": "shawline", "version": 0, "text": text } }),
        );
        self.diagnostics()
    }

    fn change(&mut self, text: String) -> Vec<Value> {
        self.version += 1;
        self.text = text;
        self.notify(
            "textDocument/didChange",
            json!({
                "textDocument": { "uri": URI, "version": self.version },
                "contentChanges": [{ "text": self.text }],
            }),
        );
        self.diagnostics()
    }

    fn at(&self, pos: Position) -> Value {
        json!({ "textDocument": { "uri": URI }, "position": pos })
    }

    fn hover(&mut self, pos: Position) -> Value {
        let params = self.at(pos);
        self.request("textDocument/hover", params)
    }

    /// Applies edits (non-overlapping, as LSP requires) to the local text.
    fn apply(&mut self, edits: &[TextEdit]) {
        let mut edits = edits.to_vec();
        edits.sort_by_key(|e| std::cmp::Reverse((e.range.start.line, e.range.start.character)));
        for e in edits {
            let start = self.offset(e.range.start);
            let end = self.offset(e.range.end);
            self.text.replace_range(start..end, &e.new_text);
        }
    }

    fn offset(&self, pos: Position) -> usize {
        let line_start: usize = self
            .text
            .split('\n')
            .take(pos.line as usize)
            .map(|l| l.len() + 1)
            .sum();
        let col = position::from_lsp(&self.text, self.enc, pos);
        let line = position::line(&self.text, pos.line as usize);
        line_start + line.chars().take(col).map(char::len_utf8).sum::<usize>()
    }

    /// Types `keys` at the end of line `row` as an editor with formatOnType
    /// would: insert, sync, ask, apply.
    fn type_keys(&mut self, row: u32, keys: &str) {
        let mut cursor =
            position::end_of(position::line(&self.text, row as usize), self.enc).character;
        for key in keys.chars() {
            let pos = Position::new(row, cursor);
            let mut text = self.text.clone();
            let at = self.offset(pos);
            text.insert(at, key);
            self.change(text);
            cursor += if self.enc == Encoding::Utf16 {
                key.len_utf16() as u32
            } else {
                1
            };
            let mut params = self.at(Position::new(row, cursor));
            params["ch"] = json!(key.to_string());
            params["options"] = json!({ "tabSize": 4, "insertSpaces": true });
            let result = self.request("textDocument/onTypeFormatting", params);
            let edits: Vec<TextEdit> = serde_json::from_value(result).unwrap_or_default();
            if let [edit] = edits.as_slice() {
                let inserted: u32 = match self.enc {
                    Encoding::Utf16 => edit.new_text.encode_utf16().count() as u32,
                    Encoding::Utf32 => edit.new_text.chars().count() as u32,
                };
                cursor = edit.range.start.character + inserted;
            }
            if !edits.is_empty() {
                self.apply(&edits);
                self.change(self.text.clone());
            }
        }
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        self.request("shutdown", Value::Null);
        self.notify("exit", Value::Null);
        if let Some(server) = self.server.take() {
            server.join().unwrap();
        }
    }
}

fn range(line: u32, start: u32, end: u32) -> Value {
    json!(Range::new(
        Position::new(line, start),
        Position::new(line, end)
    ))
}

#[test]
fn diagnostics_for_valid_and_invalid_programs() {
    let mut client = Client::start(&[]);
    assert_eq!(client.open(&corpus("03-double.shaw")), Vec::<Value>::new());

    let diags = client.open(&corpus("95-unknown.shaw"));
    assert_eq!(diags.len(), 1, "{diags:?}");
    assert_eq!(diags[0]["code"], "C001");
    assert_eq!(diags[0]["severity"], 1);
    // `𐑓` at 5:8 spans two UTF-16 units.
    assert_eq!(diags[0]["range"], range(5, 8, 10));

    let diags = client.open(&corpus("82-no-arm.shaw"));
    assert_eq!(diags.len(), 1, "{diags:?}");
    assert_eq!(diags[0]["code"], "W001");
    assert_eq!(diags[0]["severity"], 2);

    // Grid errors come through too, and edits re-check.
    let diags = client.change("abc\n".into());
    let codes: Vec<&Value> = diags.iter().map(|d| &d["code"]).collect();
    assert_eq!(codes, ["G003", "G003", "G003"]);
}

#[test]
fn utf16_columns_on_a_shavian_line() {
    let mut client = Client::start(&[PositionEncodingKind::UTF8, PositionEncodingKind::UTF16]);
    assert_eq!(client.announced, "utf-16");
    client.open(&corpus("03-double.shaw"));
    // Line 5 is `▶ 𐑯 ──┬───▶ 𐑐 ├…`: the `𐑐` in cell 12 starts at unit 13.
    let hover = client.hover(Position::new(5, 13));
    assert_eq!(hover["range"], range(5, 13, 15));
    // Unit 14 is the second half of `𐑐`.
    assert_eq!(
        client.hover(Position::new(5, 14))["range"],
        range(5, 13, 15)
    );
    assert_eq!(client.hover(Position::new(5, 12)), Value::Null);
}

#[test]
fn utf32_columns_are_grid_columns() {
    let mut client = Client::start(&[PositionEncodingKind::UTF16, PositionEncodingKind::UTF32]);
    assert_eq!(client.announced, "utf-32");
    client.open(&corpus("03-double.shaw"));
    assert_eq!(
        client.hover(Position::new(5, 12))["range"],
        range(5, 12, 13)
    );
    let diags = client.open(&corpus("95-unknown.shaw"));
    assert_eq!(diags[0]["range"], range(5, 8, 9));
}

#[test]
fn formatting_produces_the_canonical_form() {
    let mut client = Client::start(&[]);
    client.open(&corpus("03-double-ascii.shaw"));
    let params = json!({
        "textDocument": { "uri": URI },
        "options": { "tabSize": 4, "insertSpaces": true },
    });
    let edits: Vec<TextEdit> =
        serde_json::from_value(client.request("textDocument/formatting", params.clone())).unwrap();
    assert_eq!(edits.len(), 1);
    client.apply(&edits);
    assert_eq!(client.text, corpus("03-double-ascii.fmt.shaw"));

    client.open("abc\n");
    assert_eq!(
        client.request("textDocument/formatting", params),
        Value::Null
    );
}

/// Absolute `(line, start, length, type, modifiers)` tokens.
fn semantic_tokens(client: &mut Client) -> Vec<[u64; 5]> {
    let result = client.request(
        "textDocument/semanticTokens/full",
        json!({ "textDocument": { "uri": URI } }),
    );
    let data: Vec<u64> = serde_json::from_value(result["data"].clone()).unwrap();
    let (mut line, mut start) = (0, 0);
    data.chunks(5)
        .map(|t| {
            if t[0] > 0 {
                start = 0;
            }
            line += t[0];
            start += t[1];
            [line, start, t[2], t[3], t[4]]
        })
        .collect()
}

// Legend indices (see `analysis::TOKEN_TYPES`, `TOKEN_MODIFIERS`).
const FUNCTION: u64 = 0;
const PARAMETER: u64 = 1;
const NUMBER: u64 = 2;
const STRING: u64 = 3;
const ENUM_MEMBER: u64 = 5;
const COMMENT: u64 = 6;
const DECLARATION: u64 = 1;
const DEFINITION: u64 = 2;
const DEFAULT_LIBRARY: u64 = 4;

#[test]
fn semantic_tokens_tell_names_apart() {
    let mut client = Client::start(&[PositionEncodingKind::UTF32]);
    client.open(&corpus("03-double.shaw"));
    let tokens = semantic_tokens(&mut client);
    for want in [
        [0, 0, 7, COMMENT, 0],
        [2, 3, 6, FUNCTION, DECLARATION | DEFINITION],
        [5, 2, 1, PARAMETER, DECLARATION],
        [5, 12, 1, FUNCTION, DEFAULT_LIBRARY],
        [5, 24, 1, PARAMETER, DECLARATION],
        [14, 2, 2, NUMBER, 0],
        [14, 10, 5, FUNCTION, 0],
    ] {
        assert!(tokens.contains(&want), "{want:?} not in {tokens:?}");
    }

    client.open(&corpus("12-guards.shaw"));
    let tokens = semantic_tokens(&mut client);
    for want in [
        [6, 5, 1, ENUM_MEMBER, 0],
        [8, 5, 1, ENUM_MEMBER, 0],
        [7, 4, 5, STRING, 0],
    ] {
        assert!(tokens.contains(&want), "{want:?} not in {tokens:?}");
    }
}

#[test]
fn semantic_tokens_measure_utf16() {
    let mut client = Client::start(&[]);
    client.open(&corpus("03-double.shaw"));
    let tokens = semantic_tokens(&mut client);
    // `·𐑛𐑳𐑚𐑩𐑤`: the dot is one unit, each letter two.
    assert!(
        tokens.contains(&[2, 3, 11, FUNCTION, DECLARATION | DEFINITION]),
        "{tokens:?}"
    );
    assert!(
        tokens.contains(&[5, 13, 2, FUNCTION, DEFAULT_LIBRARY]),
        "{tokens:?}"
    );
}

#[test]
fn hover_on_calls() {
    let mut client = Client::start(&[PositionEncodingKind::UTF32]);
    client.open(&corpus("03-double.shaw"));
    let prim = client.hover(Position::new(5, 12));
    let text = prim["contents"]["value"].as_str().unwrap();
    assert!(text.contains("**𐑐** add · 2→1"), "{text}");
    assert!(text.contains("Mirror: **𐑚** subtract · 2→1"), "{text}");

    let def = client.hover(Position::new(14, 12));
    let text = def["contents"]["value"].as_str().unwrap();
    assert!(text.contains("**·𐑛𐑳𐑚𐑩𐑤** · 1→1"), "{text}");
    assert!(text.contains("Inputs: 𐑯"), "{text}");
    assert!(text.contains("Outputs: 𐑬"), "{text}");

    client.open(&corpus("09-length.shaw"));
    let tokens = semantic_tokens(&mut client);
    let split = tokens
        .iter()
        .find(|t| t[3] == FUNCTION && t[4] == DEFAULT_LIBRARY)
        .unwrap();
    let text =
        client.hover(Position::new(split[0] as u32, split[1] as u32))["contents"]["value"].clone();
    assert!(text.as_str().unwrap().contains("Mirror:"), "{text}");
}

#[test]
fn transliteration_sequences() {
    for offer in [vec![], vec![PositionEncodingKind::UTF32]] {
        let mut client = Client::start(&offer);
        client.open("※ 𐑐 \n");
        client.type_keys(0, "th");
        assert_eq!(client.text, "※ 𐑐 𐑔\n");

        client.open("※ 𐑐 \n");
        client.type_keys(0, "t/h");
        assert_eq!(client.text, "※ 𐑐 𐑑𐑣\n");

        // Next to a wire, `v` is the arrowhead fallback.
        client.open("※ 𐑐\n──\n");
        client.type_keys(1, "v");
        assert_eq!(client.text, "※ 𐑐\n──v\n");

        // Inside a word run it is 𐑝.
        client.open("※ 𐑐\n");
        client.type_keys(0, "lv");
        assert_eq!(client.text, "※ 𐑐𐑤𐑝\n");
    }
}
