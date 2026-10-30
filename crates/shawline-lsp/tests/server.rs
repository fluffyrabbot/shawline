//! Drives the server in-process over an in-memory connection, the way an
//! editor would.

use std::path::{Path, PathBuf};
use std::thread::JoinHandle;

use lsp_server::{Connection, Message, Notification, Request, RequestId};
use lsp_types::{Position, PositionEncodingKind, Range, TextEdit};
use serde_json::{Value, json};
use shawline_doc::Doc;
use shawline_edit::{apply, repairs};
use shawline_grid::Pos;
use shawline_lsp::position::{self, Encoding};

const URI: &str = "file:///sheet.shaw";

fn corpus(name: &str) -> String {
    let path = format!("{}/../../corpus/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

/// Every corpus program, as a name for [`corpus`], leaving out the
/// formatter's and fixer's expected outputs.
fn programs() -> Vec<String> {
    let root = PathBuf::from(format!("{}/../../corpus", env!("CARGO_MANIFEST_DIR")));
    let mut found = Vec::new();
    let mut pending = vec![root.clone()];
    while let Some(dir) = pending.pop() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            let name = path.to_string_lossy().into_owned();
            if path.is_dir() {
                pending.push(path);
            } else if name.ends_with(".shaw")
                && !name.ends_with(".fmt.shaw")
                && !name.ends_with(".fix.shaw")
            {
                let relative = path.strip_prefix(&root).map(Path::to_owned).unwrap_or(path);
                found.push(relative.to_string_lossy().into_owned());
            }
        }
    }
    found.sort();
    found
}

struct Client {
    conn: Connection,
    server: Option<JoinHandle<()>>,
    next_id: i32,
    enc: Encoding,
    /// The negotiated encoding, as the server announced it.
    announced: Value,
    /// Everything the server announced it can do.
    capabilities: Value,
    version: i32,
    text: String,
}

impl Client {
    fn start(offer: &[PositionEncodingKind]) -> Client {
        Client::start_with(offer, Value::Null)
    }

    /// Starts a server, passing `options` as `initializationOptions`.
    fn start_with(offer: &[PositionEncodingKind], options: Value) -> Client {
        let (server_side, conn) = Connection::memory();
        let server = std::thread::spawn(move || shawline_lsp::run(&server_side).unwrap());
        let mut client = Client {
            conn,
            server: Some(server),
            next_id: 0,
            enc: Encoding::Utf16,
            announced: Value::Null,
            capabilities: Value::Null,
            version: 0,
            text: String::new(),
        };
        let caps = json!({ "general": { "positionEncodings": offer } });
        let init = client.request(
            "initialize",
            json!({ "capabilities": caps, "initializationOptions": options }),
        );
        client.capabilities = init["capabilities"].clone();
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

    /// Sends new settings and waits for the diagnostics they republish.
    fn configure(&self, settings: Value) -> Vec<Value> {
        self.notify(
            "workspace/didChangeConfiguration",
            json!({ "settings": settings }),
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

    /// The hover's markdown at `pos`, if there is a hover.
    fn hover_text(&mut self, pos: Position) -> Option<String> {
        let hover = self.hover(pos);
        hover["contents"]["value"].as_str().map(str::to_owned)
    }

    /// The code actions for `range`, of the kinds in `only` (every kind
    /// when it is empty), with `diagnostics` as the client's context.
    fn code_actions(&mut self, range: Range, only: &[&str], diagnostics: &[Value]) -> Vec<Value> {
        let mut context = json!({ "diagnostics": diagnostics });
        if !only.is_empty() {
            context["only"] = json!(only);
        }
        let params = json!({ "textDocument": { "uri": URI }, "range": range, "context": context });
        let result = self.request("textDocument/codeAction", params);
        result.as_array().cloned().unwrap_or_default()
    }

    /// A code action's edits to the open document.
    fn action_edits(action: &Value) -> Vec<TextEdit> {
        serde_json::from_value(action["edit"]["changes"][URI].clone()).unwrap()
    }

    /// Applies a code action's edit, sends the text, and returns the
    /// diagnostics it gets.
    fn apply_action(&mut self, action: &Value) -> Vec<Value> {
        self.apply(&Client::action_edits(action));
        self.change(self.text.clone())
    }

    /// The whole of the local text, as a range.
    fn whole(&self) -> Range {
        Range::new(Position::new(0, 0), position::end_of(&self.text, self.enc))
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

#[test]
fn hover_reads_every_word() {
    let mut client = Client::start(&[PositionEncodingKind::UTF32]);
    client.open(&corpus("03-double.shaw"));
    // A comment word and both ports have only their reading.
    for (line, col, start, end, want) in [
        (0, 4, 2, 7, "Reading: `dubəl` · Type: `dub@l`"),
        (5, 2, 2, 3, "Reading: `n` · Type: `n`"),
        (5, 24, 24, 25, "Reading: `ow` · Type: `ow`"),
    ] {
        let hover = client.hover(Position::new(line, col));
        assert_eq!(hover["contents"]["value"], want, "{line}:{col}");
        assert_eq!(hover["range"], range(line, start, end), "{line}:{col}");
    }
    // A primitive keeps its description and adds its key and short name.
    assert_eq!(
        client.hover_text(Position::new(5, 12)).unwrap(),
        "**𐑐** add · 2→1\n\nMirror: **𐑚** subtract · 2→1\n\nReading: `p` · Type: `p` · add"
    );
    // A title reads with its namer dot, a call of it without.
    let title = client.hover_text(Position::new(2, 5)).unwrap();
    assert!(title.starts_with("**·𐑛𐑳𐑚𐑩𐑤** · 1→1\n\n"), "{title}");
    assert!(
        title.ends_with("\n\nReading: `dubəl` · Type: `=dub@l`"),
        "{title}"
    );
    let call = client.hover_text(Position::new(14, 12)).unwrap();
    assert!(call.starts_with("**·𐑛𐑳𐑚𐑩𐑤** · 1→1\n\n"), "{call}");
    assert!(
        call.ends_with("\n\nReading: `dubəl` · Type: `dub@l`"),
        "{call}"
    );
    // Numbers have nothing to read.
    assert_eq!(client.hover(Position::new(14, 2)), Value::Null);

    // Words inside `«»` text, and an unknown name, read too.
    client.open(&corpus("12-guards.shaw"));
    let hover = client.hover(Position::new(7, 6));
    assert_eq!(hover["contents"]["value"], "Reading: `wun` · Type: `wun`");
    assert_eq!(hover["range"], range(7, 5, 8));
    client.open(&corpus("95-unknown.shaw"));
    assert_eq!(
        client.hover_text(Position::new(5, 8)).as_deref(),
        Some("Reading: `foo` · Type: `foo`")
    );
}

#[test]
fn gloss_off_from_the_start() {
    let mut client = Client::start_with(&[PositionEncodingKind::UTF32], json!({ "gloss": false }));
    client.open(&corpus("03-double.shaw"));
    assert_eq!(client.hover(Position::new(0, 4)), Value::Null);
    assert_eq!(client.hover(Position::new(5, 2)), Value::Null);
    assert_eq!(
        client.hover_text(Position::new(5, 12)).unwrap(),
        "**𐑐** add · 2→1\n\nMirror: **𐑚** subtract · 2→1"
    );
    let title = client.hover_text(Position::new(2, 5)).unwrap();
    assert_eq!(title, "**·𐑛𐑳𐑚𐑩𐑤** · 1→1\n\nInputs: 𐑯\n\nOutputs: 𐑬");

    let diags = client.open(&corpus("95-unknown.shaw"));
    assert_eq!(diags[0]["message"], "unknown name 𐑓𐑵");
    assert_eq!(client.hover(Position::new(5, 8)), Value::Null);
}

#[test]
fn changing_the_setting_rewords_diagnostics() {
    let mut client = Client::start(&[PositionEncodingKind::UTF32]);
    let diags = client.open(&corpus("95-unknown.shaw"));
    assert_eq!(diags[0]["message"], "unknown name 𐑓𐑵 (foo)");

    let diags = client.configure(json!({ "shawline": { "gloss": false } }));
    assert_eq!(diags[0]["message"], "unknown name 𐑓𐑵");
    assert_eq!(client.hover(Position::new(5, 8)), Value::Null);

    // Settings that don't mention the gloss leave it as it was.
    let diags = client.configure(json!({ "shawline": { "serverPath": "shawline-lsp" } }));
    assert_eq!(diags[0]["message"], "unknown name 𐑓𐑵");
    let diags = client.configure(Value::Null);
    assert_eq!(diags[0]["message"], "unknown name 𐑓𐑵");

    let diags = client.configure(json!({ "shawline": { "gloss": true } }));
    assert_eq!(diags[0]["message"], "unknown name 𐑓𐑵 (foo)");
    assert_eq!(
        client.hover_text(Position::new(5, 8)).as_deref(),
        Some("Reading: `foo` · Type: `foo`")
    );
}

#[test]
fn capitals_type_their_lowercase_keys() {
    for offer in [vec![], vec![PositionEncodingKind::UTF32]] {
        let mut client = Client::start(&offer);
        let triggers =
            &client.capabilities["documentOnTypeFormattingProvider"]["moreTriggerCharacter"];
        for c in ["T", "H", "V"] {
            assert!(
                triggers.as_array().unwrap().contains(&json!(c)),
                "{c} in {triggers}"
            );
        }

        client.open("※ 𐑐 \n");
        client.type_keys(0, "Th");
        assert_eq!(client.text, "※ 𐑐 𐑔\n");

        client.open("※ 𐑐 \n");
        client.type_keys(0, "=DUB@L");
        assert_eq!(client.text, "※ 𐑐 ·𐑛𐑳𐑚𐑩𐑤\n");

        // A capital `V` keeps the `v` rule: a letter only inside a word run.
        client.open("※ 𐑐\n──\n");
        client.type_keys(1, "V");
        assert_eq!(client.text, "※ 𐑐\n──V\n");
        client.open("※ 𐑐\n");
        client.type_keys(0, "LV");
        assert_eq!(client.text, "※ 𐑐𐑤𐑝\n");
    }
}

#[test]
fn code_actions_are_announced() {
    let client = Client::start(&[]);
    assert_eq!(
        client.capabilities["codeActionProvider"]["codeActionKinds"],
        json!(["quickfix", "refactor.rewrite"])
    );
}

/// The one action titled "Convert keys to Shavian" among `actions`, checked
/// for its kind and that it claims no diagnostic.
fn conversion(actions: &[Value]) -> &Value {
    let [action] = actions else {
        panic!("{actions:?}")
    };
    assert_eq!(action["title"], "Convert keys to Shavian");
    assert_eq!(action["kind"], "refactor.rewrite");
    assert_eq!(action["isPreferred"], Value::Null);
    assert_eq!(action["diagnostics"], Value::Null);
    action
}

#[test]
fn convert_keys_in_a_selection() {
    for offer in [vec![], vec![PositionEncodingKind::UTF32]] {
        let mut client = Client::start(&offer);
        let enc = client.enc;
        let at = |text: &str, row: usize, col: usize| position::to_lsp(text, enc, row, col);

        client.open("※ dub@l 𐑐\n");
        let selection = Range::new(Position::new(0, 2), Position::new(0, 7));
        let actions = client.code_actions(selection, &["refactor"], &[]);
        let edits = Client::action_edits(conversion(&actions));
        assert_eq!(edits, [TextEdit::new(selection, "𐑛𐑳𐑚𐑩𐑤".into())]);
        client.apply_action(conversion(&actions));
        assert_eq!(client.text, "※ 𐑛𐑳𐑚𐑩𐑤 𐑐\n");

        // Nothing left to convert, no selection, or a kind that isn't asked for.
        let whole = client.whole();
        assert_eq!(
            client.code_actions(whole, &["refactor"], &[]),
            Vec::<Value>::new()
        );
        client.open("※ dub@l\n");
        let cursor = Range::new(Position::new(0, 4), Position::new(0, 4));
        assert_eq!(
            client.code_actions(cursor, &["refactor"], &[]),
            Vec::<Value>::new()
        );
        let selection = Range::new(Position::new(0, 2), Position::new(0, 7));
        assert_eq!(
            client.code_actions(selection, &["quickfix"], &[]),
            Vec::<Value>::new()
        );
        assert_eq!(
            client.code_actions(selection, &["refactor.extract"], &[]),
            Vec::<Value>::new()
        );
        conversion(&client.code_actions(selection, &["refactor.rewrite"], &[]));

        // A pasted block: every line, `v` by the word-run rule, capitals as keys.
        client.open("=dub@l\nlvl vow /vow\nDubul 2 ──v\n");
        let actions = client.code_actions(client.whole(), &["refactor"], &[]);
        client.apply_action(conversion(&actions));
        assert_eq!(client.text, "·𐑛𐑳𐑚𐑩𐑤\n𐑤𐑝𐑤 v𐑬 𐑝𐑬\n𐑛𐑳𐑚𐑳𐑤 2 ──v\n");

        // Keys right after a letter continue its word, so `v` is 𐑝 there.
        let text = "※ 𐑐vow\n";
        client.open(text);
        let selection = Range::new(at(text, 0, 3), at(text, 0, 6));
        let actions = client.code_actions(selection, &["refactor"], &[]);
        client.apply_action(conversion(&actions));
        assert_eq!(client.text, "※ 𐑐𐑝𐑬\n");
    }
}

/// The quick fixes a corpus case's `.repair` dump says it offers, in order:
/// the cell and code of each repair whose edit applies (`… ok DIGEST`).
/// `None` when the case keeps no dump, as `.repair` is selective.
fn dumped_fixes(name: &str) -> Option<Vec<(Pos, String)>> {
    let dump = name.replace(".shaw", ".repair");
    let path = format!("{}/../../corpus/{dump}", env!("CARGO_MANIFEST_DIR"));
    let dump = std::fs::read_to_string(path).ok()?;
    let fixes = dump
        .lines()
        .filter(|line| line.split(' ').any(|word| word == "ok"))
        .map(|line| {
            let mut words = line.split(' ');
            let (row, col) = words.next().unwrap().split_once(':').unwrap();
            let at = Pos::new(row.parse().unwrap(), col.parse().unwrap());
            (at, words.next().unwrap().to_owned())
        })
        .collect();
    Some(fixes)
}

#[test]
fn repairs_become_quick_fixes() {
    let (mut dumps, mut fixes) = (0, 0);
    for offer in [vec![], vec![PositionEncodingKind::UTF32]] {
        let mut client = Client::start(&offer);
        for name in programs() {
            let text = corpus(&name);
            let published = client.open(&text);
            // The sheet's quick fixes, in the order offered, are the dump's,
            // each on a diagnostic as it was published.
            let actions = client.code_actions(client.whole(), &["quickfix"], &[]);
            let offered: Vec<(Pos, String)> = actions
                .iter()
                .map(|action| {
                    let d = &action["diagnostics"][0];
                    assert!(published.contains(d), "{name}: {d}");
                    let range: Range = serde_json::from_value(d["range"].clone()).unwrap();
                    let at = position::lsp_to_cell(&text, client.enc, range.start);
                    (at, d["code"].as_str().unwrap().to_owned())
                })
                .collect();
            if let Some(dumped) = dumped_fixes(&name) {
                assert_eq!(offered, dumped, "{name}");
                dumps += 1;
            }

            // Each is its repair: labelled as it is, the only fix for its
            // diagnostic, and giving the text its edit gives. The cursor on
            // its diagnostic finds it too.
            let doc = Doc::new(text.as_str());
            let expected: Vec<(String, String)> = repairs(&doc)
                .into_iter()
                .filter_map(|r| {
                    let edited = apply(&doc, r.edit.as_ref()?).ok()?;
                    Some((r.label, edited.doc.text().to_owned()))
                })
                .collect();
            assert_eq!(actions.len(), expected.len(), "{name}");
            for (action, (label, edited)) in actions.iter().zip(&expected) {
                assert_eq!(action["kind"], "quickfix", "{name}");
                assert_eq!(action["title"], label.as_str(), "{name}");
                assert_eq!(action["isPreferred"], true, "{name}: {label}");
                client.apply(&Client::action_edits(action));
                assert_eq!(&client.text, edited, "{name}: {label}");
                client.text.clone_from(&text);
                let d = &action["diagnostics"][0];
                let range: Range = serde_json::from_value(d["range"].clone()).unwrap();
                let here = client.code_actions(range, &["quickfix"], std::slice::from_ref(d));
                assert!(here.contains(action), "{name}: {label} not at {d}");
            }
            fixes += actions.len();
        }
    }
    // Both encodings saw the same cases, so nothing here is vacuous.
    assert!(dumps >= 2 * 26 && fixes > 0, "{dumps} dumps, {fixes} fixes");
}

#[test]
fn quick_fix_adds_the_namer_dot() {
    let mut client = Client::start(&[]);
    let published = client.open(&corpus("static/03-no-namer-dot.shaw"));
    let [diagnostic] = published.as_slice() else {
        panic!("{published:?}")
    };
    assert_eq!(diagnostic["code"], "Y001");
    let range: Range = serde_json::from_value(diagnostic["range"].clone()).unwrap();
    let actions = client.code_actions(range, &["quickfix"], std::slice::from_ref(diagnostic));
    let [action] = actions.as_slice() else {
        panic!("{actions:?}")
    };
    assert_eq!(action["isPreferred"], true);
    assert_eq!(action["diagnostics"], json!([diagnostic]));
    let after = client.apply_action(action);
    assert!(
        after.iter().all(|d| d["severity"] != 1),
        "{after:?}\n{}",
        client.text
    );
    assert!(client.text.contains("╭─ ·𐑥𐑱𐑯 "), "{}", client.text);
}

#[test]
fn every_repair_with_an_edit_is_a_quick_fix() {
    // PLAN P10's repairs table: the codes whose repair has an edit, and those
    // whose repair is all follow-up, which a language server can't offer.
    let with_edit = [
        "S012", "Y001", "Y003", "Y004", "Y014", "Y015", "Y018", "C001", "C002", "C005", "C006",
        "W001", "W002",
    ];
    let follow_up_only = ["Y009", "Y013", "C004", "Y019"];
    let mut offered = std::collections::BTreeSet::new();
    let mut client = Client::start(&[]);
    for name in programs() {
        let text = corpus(&name);
        let before = client.open(&text);
        for action in client.code_actions(client.whole(), &["quickfix"], &[]) {
            let code = action["diagnostics"][0]["code"]
                .as_str()
                .unwrap()
                .to_owned();
            let count = |diagnostics: &[Value]| {
                diagnostics
                    .iter()
                    .filter(|d| d["code"] == code.as_str())
                    .count()
            };
            let after = client.apply_action(&action);
            assert!(
                count(&after) < count(&before),
                "{name}: {} leaves {code}\n{}",
                action["title"],
                client.text
            );
            offered.insert(code);
            client.text.clone_from(&text);
            client.change(text.clone());
        }
    }
    for code in with_edit {
        assert!(offered.contains(code), "no quick fix for {code}");
    }
    for code in follow_up_only {
        assert!(!offered.contains(code), "a quick fix for {code}");
    }
}
