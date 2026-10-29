//! WebAssembly bindings for the Shawline playground: a thin layer over
//! `shawline_doc`, `shawline_edit` and `shawline_word` that turns results
//! into plain JS objects. No pipeline logic lives here.
//!
//! The page holds each revision of its text as a [`Doc`], so everything
//! one refresh reads (diagnostics, boxes, endpoints, wires, the drawing)
//! comes from a single reading, and every edit takes a `Doc` and returns
//! the next. A `Doc` lives in wasm memory until JS calls its `free()`.
//! The workers can't share that memory, so `run` and `trace` take text.
//!
//! Positions are zero-based `row`/`col` in code points, as in every dump.
//!
//! Messages cross into JS as written. The page shows them through
//! [`annotate`] while its Readings layer is on (PLAN P9), so turning the
//! layer off, or peeking, needs no second reading and no second run.

use js_sys::{Array, Object, Reflect};
use shawline_doc::TRACE_STEPS;
use shawline_doc::position::{self, Encoding};
use shawline_edit::{Anchor, Axis, Edit, Insertion, NewSource, Refusal, RefusalKind};
use shawline_grid::{Diagnostic, Dir, Pos, Rect};
use shawline_render::{Options, SCALE, Theme};
use shawline_shape::{Diagram, TextKind};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = console)]
    fn error(message: &str);
}

thread_local! {
    static PANIC: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

/// Sends Rust panics to the console, so a pipeline bug shows its message
/// instead of a bare `unreachable` trap, and keeps it for [`panic_message`].
#[wasm_bindgen(start)]
pub fn start() {
    std::panic::set_hook(Box::new(|info| {
        let message = info.to_string();
        error(&message);
        PANIC.with(|p| *p.borrow_mut() = Some(message));
    }));
}

/// The message of the last panic, if one has happened. The trap that ends
/// a panic reaches JS as a bare `RuntimeError`, so the page asks here what
/// went wrong.
#[wasm_bindgen]
pub fn panic_message() -> Option<String> {
    PANIC.with(|p| p.borrow().clone())
}

/// One revision of a sheet (`shawline_doc::Doc`): its text, and each stage
/// read from it on first use and kept. It never changes; an edit returns a
/// new one. Call `free()` on every one you drop.
#[wasm_bindgen]
pub struct Doc(shawline_doc::Doc);

#[wasm_bindgen]
impl Doc {
    /// A revision holding `text`. Nothing is read until something asks.
    #[wasm_bindgen(constructor)]
    pub fn new(text: &str) -> Doc {
        Doc(shawline_doc::Doc::new(text))
    }

    /// The text, as the editor holds it.
    #[wasm_bindgen(getter)]
    pub fn text(&self) -> String {
        self.0.text().to_owned()
    }

    /// Every static diagnostic, sorted by position: an array of `{ row,
    /// col, code, message, severity }`, where `severity` is `"error"`,
    /// `"warning"` or `"lint"`.
    pub fn diagnostics(&self) -> Array {
        diagnostics(self.0.diagnostics())
    }

    /// The canonical form of the text (SPEC §11), or `undefined` when it
    /// has grid errors and can't be read as a sheet.
    pub fn formatted(&self) -> Option<String> {
        self.0.formatted().map(str::to_owned)
    }

    /// The sheet drawn as a standalone SVG in the light or dark theme, or
    /// `undefined` when the text has grid errors. With `gloss`, a strip
    /// under every row shows each word's reading or primitive name, and rows
    /// sit [`row_pitch`]`(true)` apart; without it the drawing is plain.
    pub fn render(&self, dark: bool, gloss: bool) -> Option<String> {
        let theme = if dark { Theme::DARK } else { Theme::LIGHT };
        Some(shawline_render::render_with(self.0.grid().ok()?, Options { theme, gloss }))
    }

    /// Every Shavian word on the sheet in row-major order, for hover: an
    /// array of `{ row, col, span, word, reading, keys, gloss, shown }`, where
    /// `span` is the word's length in cells, `gloss` a primitive's short
    /// name (`null` for any other word), and `shown` whether the strip has
    /// room to show it. Empty when the text has grid errors.
    pub fn glosses(&self) -> Array {
        let Ok(grid) = self.0.grid() else { return Array::new() };
        shawline_render::glosses(grid)
            .into_iter()
            .map(|g| {
                let o = Object::new();
                set(&o, "row", &(g.at.row as u32).into());
                set(&o, "col", &(g.at.col as u32).into());
                set(&o, "span", &(g.word.chars().count() as u32).into());
                set(&o, "shown", &g.shown().into());
                set(&o, "gloss", &g.gloss.map_or(JsValue::NULL, JsValue::from));
                set(&o, "reading", &g.reading.into());
                set(&o, "keys", &g.keys.into());
                set(&o, "word", &g.word.into());
                JsValue::from(o)
            })
            .collect()
    }

    /// Every box, for hit-testing: an array of `{ top, left, bottom, right,
    /// kind }` (inclusive cell rectangles, `kind` `"definition"`,
    /// `"switch"` or `"node"`), empty when the text has grid errors.
    pub fn boxes(&self) -> Array {
        let Some(diagram) = self.0.diagram() else { return Array::new() };
        diagram
            .boxes
            .iter()
            .map(|b| {
                let corner = Pos::new(b.rect.top, b.rect.left);
                let kind = shawline_edit::box_kind(&self.0, corner).map_or("node", |k| k.name());
                let o = Object::new();
                set(&o, "top", &(b.rect.top as u32).into());
                set(&o, "left", &(b.rect.left as u32).into());
                set(&o, "bottom", &(b.rect.bottom as u32).into());
                set(&o, "right", &(b.rect.right as u32).into());
                set(&o, "kind", &kind.into());
                JsValue::from(o)
            })
            .collect()
    }

    /// Every text run in row-major order, for selecting and editing: an
    /// array of `{ row, col, span, kind, content }`, where `span` is its
    /// width in cells, delimiters included, `kind` is `"word"`, `"number"`,
    /// `"literal"` or `"comment"`, and `content` is the text without
    /// delimiters. Empty when the text has grid errors.
    pub fn texts(&self) -> Array {
        let Some(diagram) = self.0.diagram() else { return Array::new() };
        diagram
            .texts
            .iter()
            .map(|t| {
                let kind = match t.kind {
                    TextKind::Word => "word",
                    TextKind::Number => "number",
                    TextKind::Literal => "literal",
                    TextKind::Comment => "comment",
                };
                let o = cell_object(t.pos);
                set(&o, "span", &(t.width as u32).into());
                set(&o, "kind", &kind.into());
                set(&o, "content", &t.content.as_str().into());
                JsValue::from(o)
            })
            .collect()
    }

    /// Every source and sink, for dragging wires: `{ sources, sinks }`,
    /// each an array of `{ row, col, span, scope, wired }`. `span` cells
    /// from `col` eastward name the endpoint; a source reaches only sinks
    /// with the same `scope` string, and a `wired` sink takes no second
    /// wire.
    pub fn endpoints(&self) -> Object {
        let (sources, sinks) = shawline_edit::endpoints(&self.0);
        let list = |ends: Vec<shawline_edit::Endpoint>| -> Array {
            ends.into_iter()
                .map(|e| {
                    let o = Object::new();
                    set(&o, "row", &(e.at.row as u32).into());
                    set(&o, "col", &(e.at.col as u32).into());
                    set(&o, "span", &(e.span as u32).into());
                    set(&o, "scope", &format!("{}:{}", e.scope.0, e.scope.1).into());
                    set(&o, "wired", &e.wired.into());
                    JsValue::from(o)
                })
                .collect()
        };
        let o = Object::new();
        set(&o, "sources", &list(sources));
        set(&o, "sinks", &list(sinks));
        o
    }

    /// Every wire, for hit-testing and value badges: an array of `{ wire,
    /// cells, source }`, where `wire` is the index `trace` reports, `cells`
    /// its `{ row, col, down }` cells in row-major order (`down` says the
    /// cell joins the wire southward, so a highlight runs on through the
    /// gloss strip below it), and `source` the cell naming its source for
    /// `connect` (`null` when the wire isn't editable by mouse).
    pub fn wires(&self) -> Array {
        let Some(diagram) = self.0.diagram() else { return Array::new() };
        let sources = shawline_edit::wire_sources(&self.0);
        diagram
            .wires
            .iter()
            .enumerate()
            .map(|(i, w)| {
                let o = Object::new();
                set(&o, "wire", &(i as u32).into());
                let cells = w.cells.iter().zip(&w.links).map(|(&p, &links)| {
                    let o = cell_object(p);
                    set(&o, "down", &(links & Dir::S.bit() != 0).into());
                    JsValue::from(o)
                });
                set(&o, "cells", &cells.collect::<Array>());
                let source = sources.get(i).copied().flatten();
                set(&o, "source", &source.map_or(JsValue::NULL, cell));
                JsValue::from(o)
            })
            .collect()
    }

    /// What the context palette offers: an array of `{ word, inputs,
    /// outputs, gloss, mirror }` (`mirror` is `null` but for primitives).
    /// Pass the dragged source's cell to offer only callees with an input.
    pub fn palette(&self, from_row: Option<u32>, from_col: Option<u32>) -> Array {
        let from = from_row.zip(from_col).map(|(r, c)| at(r, c));
        shawline_edit::palette(&self.0, from)
            .into_iter()
            .map(|c| {
                let o = Object::new();
                set(&o, "word", &c.word.into());
                set(&o, "inputs", &c.inputs.into());
                set(&o, "outputs", &c.outputs.into());
                set(&o, "gloss", &c.gloss.into());
                set(&o, "mirror", &c.mirror.map_or(JsValue::NULL, JsValue::from));
                JsValue::from(o)
            })
            .collect()
    }

    /// What `connect` would draw for a wire dragged from the cell at
    /// `row`, `col` and released at `to_row`, `to_col`
    /// (`shawline_edit::preview`): `{ path, joins_wire }`, where `path` is
    /// the whole polyline as an array of `{ row, col }`, ends included, and
    /// `joins_wire` says it starts on the source's wire at a new tee.
    /// `from_sink` says the drag is anchored on an unwired arrowhead rather
    /// than a source. `undefined` when nothing fits. Nothing is edited.
    pub fn preview(&self, from_sink: bool, row: u32, col: u32, to_row: u32, to_col: u32) -> Option<Object> {
        let anchor = if from_sink { Anchor::Sink(at(row, col)) } else { Anchor::Source(at(row, col)) };
        let preview = shawline_edit::preview(&self.0, anchor, at(to_row, to_col))?;
        let o = Object::new();
        set(&o, "path", &preview.path.into_iter().map(cell).collect::<Array>());
        set(&o, "joins_wire", &preview.joins_wire.into());
        Some(o)
    }

    /// The cells `disconnect` would clear for the arrowhead at the cell
    /// (`shawline_edit::branch`), row-major, as an array of `{ row, col }`:
    /// a branch to select. Empty when disconnecting it would be refused.
    pub fn branch(&self, row: u32, col: u32) -> Array {
        shawline_edit::branch(&self.0, at(row, col)).into_iter().map(cell).collect()
    }

    /// What the word covering the cell is, in the words editor hover uses
    /// (`shawline_edit::describe`): `{ row, col, width, text }`, where
    /// `text` is Markdown, or `undefined` when there is nothing to say.
    /// With `gloss`, every Shavian word also gets its reading and keys.
    pub fn describe(&self, row: u32, col: u32, gloss: bool) -> Option<Object> {
        let d = shawline_edit::describe(&self.0, at(row, col), gloss)?;
        let o = cell_object(d.at);
        set(&o, "width", &(d.width as u32).into());
        set(&o, "text", &d.text.into());
        Some(o)
    }

    /// Where cells sit in this revision's drawing, glossed or plain, at
    /// `zoom` times its natural size; `undefined` when the text has grid
    /// errors.
    pub fn geometry(&self, gloss: bool, zoom: f64) -> Option<Geometry> {
        let grid = self.0.grid().ok()?;
        Some(Geometry(shawline_render::Geometry::new(grid, gloss, SCALE * zoom)))
    }
}

/// Checks and runs `src`, for the runner worker. Returns `{ outputs, error,
/// at, diagnostics }`: `outputs` holds main's outputs as printed lines (SPEC
/// §9), `error` is the runtime error as `error ROW:COL CODE: message` (or
/// `null`), `at` is that error as a diagnostic object (absent without one),
/// and `diagnostics` is the array `Doc.diagnostics` returns. A sheet with
/// static errors doesn't run, so it has no outputs and no runtime error.
#[wasm_bindgen]
pub fn run(src: &str) -> Object {
    let result = Object::new();
    let outputs = Array::new();
    let mut runtime = JsValue::NULL;
    let doc = shawline_doc::Doc::new(src);
    match doc.run() {
        Some(Ok(values)) => values.iter().for_each(|v| {
            outputs.push(&v.to_string().into());
        }),
        Some(Err(e)) => {
            runtime = format!("error {} {}: {}", e.at, e.code, e.message).into();
            set(&result, "at", &diagnostic(&e));
        }
        None => {}
    }
    set(&result, "outputs", &outputs);
    set(&result, "error", &runtime);
    set(&result, "diagnostics", &diagnostics(doc.diagnostics()));
    result
}

/// The value on every wire of `src`, for the tracer worker's live badges
/// (`shawline_doc::Doc::trace`): `{ wires, error, stopped }`. `wires` holds
/// `{ wire, value }` with each value printed as SPEC §9 prints it, cut to
/// 24 characters. `error` is the runtime error that ended the run, or the
/// first static error when the sheet can't run (then `wires` is empty), as
/// a diagnostic object; `null` otherwise. `stopped` says the run hit its
/// step budget.
#[wasm_bindgen]
pub fn trace(src: &str) -> Object {
    let o = Object::new();
    let values = Array::new();
    let doc = shawline_doc::Doc::new(src);
    let (error, stopped) = match doc.trace(TRACE_STEPS) {
        Some(found) => {
            for (wire, value) in &found.wires {
                let pair = Object::new();
                set(&pair, "wire", &(*wire as u32).into());
                set(&pair, "value", &shawline_vm::show(value, 24).into());
                values.push(&pair);
            }
            (found.error.as_ref().map_or(JsValue::NULL, diagnostic), found.stopped)
        }
        // The first static error, as the old glue reported it.
        None => {
            let first = doc.diagnostics().iter().find(|d| !d.is_warning());
            (first.map_or(JsValue::NULL, diagnostic), false)
        }
    };
    set(&o, "wires", &values);
    set(&o, "error", &error);
    set(&o, "stopped", &stopped.into());
    o
}

// Checked edits (`shawline_edit`). The page sends one `Edit` per gesture to
// `apply`, written as a plain object whose `kind` is the edit's `.studio`
// step name:
//
// - `{ kind: "move", corner, dr, dc }`
// - `{ kind: "connect", from, to }`, `{ kind: "rewire", from, to }`
// - `{ kind: "remove_wire", at }`, `{ kind: "disconnect", at }`,
//   `{ kind: "remove_node", at }`, `{ kind: "remove_text", at }`
// - `{ kind: "place_node", corner, word, inputs, outputs }`
// - `{ kind: "place_connected", at, word, inputs, outputs, from }`
// - `{ kind: "feed", to, source }`, where `source` is `{ kind: "constant",
//   content }` or `{ kind: "node", word, inputs, outputs }`
// - `{ kind: "insert" | "delete", axis: "row" | "column", at, count }`
// - `{ kind: "make_room", edit }`
// - `{ kind: "set_text" | "place_text", at, content }`
// - `{ kind: "place_definition", rect, name }`, `{ kind: "place_switch",
//   rect, guard }`, `{ kind: "add_port", at, name }`, `{ kind: "add_arm",
//   at, guard }`, `{ kind: "place_inline", at, word }`
//
// Cells are `{ row, col }` and rectangles `{ top, left, bottom, right }`.

/// Applies one edit to `doc` (`shawline_edit::apply`). Done, it returns `{
/// ok: true, doc, tidied, map }`: the next revision, whether the edit was
/// made on the formatted text (the page applies that text first, as its own
/// undo step), and a [`Mapping`] from the old sheet's parts to the new
/// one's (`null` when either side can't be read). A `make_room` edit also
/// has `inserted`, an array of `{ axis, row, col, count }`. Refused, it
/// returns `{ ok: false, row, col, reason, kind, remedy }`, where `kind` is
/// `RefusalKind::name()` and `remedy` is `"make-room"`, `"format"` or
/// `null`; the revision passed in is still the sheet. An edit object the
/// page wrote wrong is refused as `target`.
#[wasm_bindgen]
pub fn apply(doc: &Doc, edit: &JsValue) -> Object {
    let edit = match edit_of(edit) {
        Ok(edit) => edit,
        Err(why) => return refused(Refusal::new(Pos::new(0, 0), RefusalKind::Target, why)),
    };
    // The roomed variant also reports what it inserted.
    if let Edit::MakeRoom(inner) = &edit {
        return match shawline_edit::make_room(&doc.0, inner) {
            Ok(roomed) => {
                let o = made(&doc.0, roomed.edited);
                let inserted = roomed.inserted.iter().map(|i| {
                    let o = cell_object(i.at);
                    set(&o, "axis", &i.axis.name().into());
                    set(&o, "count", &(i.count as u32).into());
                    JsValue::from(o)
                });
                set(&o, "inserted", &inserted.collect::<Array>());
                o
            }
            Err(refusal) => refused(refusal),
        };
    }
    match shawline_edit::apply(&doc.0, &edit) {
        Ok(edited) => made(&doc.0, edited),
        Err(refusal) => refused(refusal),
    }
}

/// The edit a page object describes, or why it describes none.
fn edit_of(value: &JsValue) -> Result<Edit, String> {
    let kind = string(value, "kind")?;
    let edit = match kind.as_str() {
        "move" => Edit::Move {
            corner: pos(value, "corner")?,
            dr: number(value, "dr")? as isize,
            dc: number(value, "dc")? as isize,
        },
        "connect" => Edit::Connect { from: pos(value, "from")?, to: pos(value, "to")? },
        "rewire" => Edit::Rewire { from: pos(value, "from")?, to: pos(value, "to")? },
        "remove_wire" => Edit::RemoveWire(pos(value, "at")?),
        "disconnect" => Edit::Disconnect(pos(value, "at")?),
        "remove_node" => Edit::RemoveNode(pos(value, "at")?),
        "remove_text" => Edit::RemoveText(pos(value, "at")?),
        "place_node" => Edit::PlaceNode {
            corner: pos(value, "corner")?,
            word: string(value, "word")?,
            inputs: count(value, "inputs")?,
            outputs: count(value, "outputs")?,
        },
        "feed" => {
            let source = field(value, "source")?;
            let source = match string(&source, "kind")?.as_str() {
                "constant" => NewSource::Constant(string(&source, "content")?),
                "node" => NewSource::Node {
                    word: string(&source, "word")?,
                    inputs: count(&source, "inputs")?,
                    outputs: count(&source, "outputs")?,
                },
                other => return Err(format!("no source {other}")),
            };
            Edit::Feed { to: pos(value, "to")?, source }
        }
        "place_connected" => Edit::PlaceConnected {
            at: pos(value, "at")?,
            word: string(value, "word")?,
            inputs: count(value, "inputs")?,
            outputs: count(value, "outputs")?,
            from: pos(value, "from")?,
        },
        "insert" | "delete" => {
            let axis = match string(value, "axis")?.as_str() {
                "row" => Axis::Row,
                "column" => Axis::Column,
                other => return Err(format!("no axis {other}")),
            };
            let n = index(value, "count")?;
            if n == 0 {
                return Err("a count of at least one".into());
            }
            let insertion = Insertion { axis, at: pos(value, "at")?, count: n };
            if kind == "insert" { Edit::Insert(insertion) } else { Edit::Delete(insertion) }
        }
        "make_room" => Edit::MakeRoom(Box::new(edit_of(&field(value, "edit")?)?)),
        "set_text" => Edit::SetText { at: pos(value, "at")?, content: string(value, "content")? },
        "place_text" => Edit::PlaceText { at: pos(value, "at")?, content: string(value, "content")? },
        "place_definition" => Edit::PlaceDefinition { rect: rect(value, "rect")?, name: string(value, "name")? },
        "place_switch" => Edit::PlaceSwitch { rect: rect(value, "rect")?, guard: optional(value, "guard")? },
        "add_port" => Edit::AddPort { at: pos(value, "at")?, name: optional(value, "name")? },
        "add_arm" => Edit::AddArm { at: pos(value, "at")?, guard: optional(value, "guard")? },
        "place_inline" => Edit::PlaceInline { at: pos(value, "at")?, word: string(value, "word")? },
        other => return Err(format!("no edit {other}")),
    };
    Ok(edit)
}

fn field(value: &JsValue, key: &str) -> Result<JsValue, String> {
    Reflect::get(value, &key.into())
        .ok()
        .filter(|v| !v.is_undefined() && !v.is_null())
        .ok_or_else(|| format!("the edit has no {key}"))
}

fn number(value: &JsValue, key: &str) -> Result<f64, String> {
    field(value, key)?.as_f64().filter(|n| n.is_finite()).ok_or_else(|| format!("{key} isn't a number"))
}

fn count(value: &JsValue, key: &str) -> Result<u16, String> {
    let n = number(value, key)?;
    (0.0..=f64::from(u16::MAX)).contains(&n).then_some(n as u16).ok_or_else(|| format!("{key} is out of range"))
}

fn string(value: &JsValue, key: &str) -> Result<String, String> {
    field(value, key)?.as_string().ok_or_else(|| format!("{key} isn't text"))
}

fn optional(value: &JsValue, key: &str) -> Result<Option<String>, String> {
    match field(value, key) {
        Ok(v) => v.as_string().map(Some).ok_or_else(|| format!("{key} isn't text")),
        Err(_) => Ok(None),
    }
}

fn index(value: &JsValue, key: &str) -> Result<usize, String> {
    let n = number(value, key)?;
    (n >= 0.0).then_some(n as usize).ok_or_else(|| format!("{key} is negative"))
}

fn pos(value: &JsValue, key: &str) -> Result<Pos, String> {
    let cell = field(value, key)?;
    Ok(Pos::new(index(&cell, "row")?, index(&cell, "col")?))
}

fn rect(value: &JsValue, key: &str) -> Result<Rect, String> {
    let r = field(value, key)?;
    Ok(Rect { top: index(&r, "top")?, left: index(&r, "left")?, bottom: index(&r, "bottom")?, right: index(&r, "right")? })
}

/// `{ ok: true, doc, tidied, map }`, handing the revision and the mapping
/// to JS, which must free both.
fn made(input: &shawline_doc::Doc, edited: shawline_edit::Edited) -> Object {
    // The map starts from the input's canonical revision (`Edited::map`).
    let from = input.canonical().and_then(shawline_doc::Doc::diagram).cloned();
    let to = edited.doc.diagram().cloned();
    let o = Object::new();
    set(&o, "ok", &true.into());
    set(&o, "tidied", &edited.tidied.into());
    let map = match (from, to) {
        (Some(from), Some(to)) => Mapping { from, to, map: edited.map }.into(),
        _ => JsValue::NULL,
    };
    set(&o, "map", &map);
    set(&o, "doc", &Doc(edited.doc).into());
    o
}

fn refused(refusal: Refusal) -> Object {
    let o = Object::new();
    set(&o, "ok", &false.into());
    set(&o, "row", &(refusal.at.row as u32).into());
    set(&o, "col", &(refusal.at.col as u32).into());
    set(&o, "reason", &refusal.reason.into());
    set(&o, "kind", &refusal.kind.name().into());
    set(&o, "remedy", &refusal.remedy.map_or(JsValue::NULL, |r| r.name().into()));
    o
}

/// Where an edit took the old sheet's parts (`Edited::map`), asked part by
/// part: the page carries its selection and cursor through it. Every query
/// names a part by its old cell and answers with the new one, or
/// `undefined` when the edit removed it. Call `free()` when done.
#[wasm_bindgen]
pub struct Mapping {
    from: Diagram,
    to: Diagram,
    map: shawline_edit::Mapping,
}

#[wasm_bindgen]
impl Mapping {
    /// The box whose top-left corner was at the cell: its new `{ top,
    /// left, bottom, right }`.
    pub fn map_box(&self, row: u32, col: u32) -> Option<Object> {
        let corner = at(row, col);
        let i = self.from.boxes.iter().position(|b| (b.rect.top, b.rect.left) == (corner.row, corner.col))?;
        let r = self.to.boxes[(*self.map.boxes.get(i)?)?].rect;
        let o = Object::new();
        set(&o, "top", &(r.top as u32).into());
        set(&o, "left", &(r.left as u32).into());
        set(&o, "bottom", &(r.bottom as u32).into());
        set(&o, "right", &(r.right as u32).into());
        Some(o)
    }

    /// The text through the cell: its new `{ row, col, span }`, `span`
    /// being its width in cells.
    pub fn map_text(&self, row: u32, col: u32) -> Option<Object> {
        let t = &self.to.texts[self.text(at(row, col))?.1];
        let o = cell_object(t.pos);
        set(&o, "span", &(t.width as u32).into());
        Some(o)
    }

    /// The new index of wire `wire` (`Doc.wires`' `wire`).
    pub fn map_wire(&self, wire: u32) -> Option<u32> {
        self.map.wires.get(wire as usize).copied().flatten().map(|w| w as u32)
    }

    /// Where a cell went, for the cursor: a cell of a text keeps its place
    /// in that text; any other follows the nearest part of the box holding
    /// it that the edit kept (its texts, the boxes inside it and its own
    /// corners), so it moves with a moved box and past inserted rows. A
    /// cell near nothing kept stays put. Always a cell.
    pub fn map_cell(&self, row: u32, col: u32) -> Object {
        cell_object(self.cell(at(row, col)))
    }
}

impl Mapping {
    /// The old and new index of the text through `p`.
    fn text(&self, p: Pos) -> Option<(usize, usize)> {
        let i = self
            .from
            .texts
            .iter()
            .position(|t| t.pos.row == p.row && (t.pos.col..t.pos.col + t.width).contains(&p.col))?;
        Some((i, (*self.map.texts.get(i)?)?))
    }

    fn cell(&self, p: Pos) -> Pos {
        if let Some((i, j)) = self.text(p) {
            let (old, new) = (&self.from.texts[i], &self.to.texts[j]);
            let offset = (p.col - old.pos.col).min(new.width.saturating_sub(1));
            return Pos::new(new.pos.row, new.pos.col + offset);
        }
        let inside = |r: Rect| (r.top..=r.bottom).contains(&p.row) && (r.left..=r.right).contains(&p.col);
        let area = |r: Rect| (r.bottom - r.top + 1) * (r.right - r.left + 1);
        let holder = (0..self.from.boxes.len())
            .filter(|&b| inside(self.from.boxes[b].rect))
            .min_by_key(|&b| area(self.from.boxes[b].rect));
        let corners = |r: Rect| [Pos::new(r.top, r.left), Pos::new(r.bottom, r.right)];
        // Pairs of an old cell and the cell it went to.
        let mut anchors: Vec<(Pos, Pos)> = Vec::new();
        for (b, shape) in self.from.boxes.iter().enumerate() {
            let Some(Some(n)) = self.map.boxes.get(b) else { continue };
            if shape.parent == holder || Some(b) == holder {
                anchors.extend(corners(shape.rect).into_iter().zip(corners(self.to.boxes[*n].rect)));
            }
        }
        for (t, text) in self.from.texts.iter().enumerate() {
            let Some(Some(n)) = self.map.texts.get(t) else { continue };
            if text.owner == holder || (holder.is_some() && text.title_of == holder) {
                anchors.push((text.pos, self.to.texts[*n].pos));
            }
        }
        let distance = |q: Pos| p.row.abs_diff(q.row) + p.col.abs_diff(q.col);
        let Some((old, new)) = anchors.into_iter().min_by_key(|(old, _)| distance(*old)) else { return p };
        let row = (p.row + new.row).saturating_sub(old.row);
        let col = (p.col + new.col).saturating_sub(old.col);
        Pos::new(row, col)
    }
}
fn at(row: u32, col: u32) -> Pos {
    Pos::new(row as usize, col as usize)
}

fn cell(p: Pos) -> JsValue {
    cell_object(p).into()
}

fn cell_object(p: Pos) -> Object {
    let o = Object::new();
    set(&o, "row", &(p.row as u32).into());
    set(&o, "col", &(p.col as u32).into());
    o
}

/// The cell at `offset` UTF-16 units into `text`, as a textarea's
/// selection counts (`shawline_doc::position::cell_of_offset`): `{ row,
/// col }`. A leading byte-order mark is no cell, and an offset on a line
/// ending is just past that line's last cell.
#[wasm_bindgen]
pub fn cell_of_offset(text: &str, offset: u32) -> Object {
    cell_object(position::cell_of_offset(text, Encoding::Utf16, offset as usize))
}

/// The offset in UTF-16 units of the cell at `row`, `col` in `text`
/// (`shawline_doc::position::offset_of_cell`): where a textarea puts its
/// caret to stand on that cell.
#[wasm_bindgen]
pub fn offset_of_cell(text: &str, row: u32, col: u32) -> u32 {
    position::offset_of_cell(text, Encoding::Utf16, at(row, col)) as u32
}

/// Where cells sit in a drawing as [`Doc::render`] draws it, at one zoom
/// (`shawline_render::Geometry`). Points are in the page's CSS pixels,
/// measured from the drawn `<svg>`'s top-left corner, margin included.
/// Call `free()` on every one you drop.
#[wasm_bindgen]
pub struct Geometry(shawline_render::Geometry);

#[wasm_bindgen]
impl Geometry {
    /// The whole drawing: `{ width, height }`.
    pub fn size(&self) -> Object {
        let (width, height) = self.0.size();
        let o = Object::new();
        set(&o, "width", &width.into());
        set(&o, "height", &height.into());
        o
    }

    /// The cell under a point, as `{ row, col }`; `undefined` left of or
    /// above the grid. A point in a gloss strip is in the row above it.
    pub fn cell_at(&self, x: f64, y: f64) -> Option<Object> {
        self.0.cell_at(x, y).map(cell_object)
    }

    /// The area of an inclusive cell rectangle, `{ x, y, width, height }`.
    /// `under` takes it on through the gloss strip below, and `over` back
    /// up through the strip above.
    pub fn cell_rect(&self, top: u32, left: u32, bottom: u32, right: u32, under: bool, over: bool) -> Object {
        let r = Rect { top: top as usize, left: left as usize, bottom: bottom as usize, right: right as usize };
        let a = self.0.cell_rect(r, under, over);
        let o = Object::new();
        set(&o, "x", &a.x.into());
        set(&o, "y", &a.y.into());
        set(&o, "width", &a.width.into());
        set(&o, "height", &a.height.into());
        o
    }

    /// A cell's centre, `{ x, y }`.
    pub fn cell_centre(&self, row: u32, col: u32) -> Object {
        let (x, y) = self.0.cell_centre(at(row, col));
        let o = Object::new();
        set(&o, "x", &x.into());
        set(&o, "y", &y.into());
        o
    }
}

/// `message` with each Shavian word's reading after it in parentheses, as
/// in `𐑥𐑱𐑯 (mayn)` (`shawline_word::annotate`): how the page shows a
/// diagnostic, a runtime error or a refusal while Readings is on.
#[wasm_bindgen]
pub fn annotate(message: &str) -> String {
    shawline_word::annotate(message)
}

/// How `word` sounds (SPEC §13, Reading): `·𐑛𐑳𐑚𐑩𐑤` reads `dubəl`.
#[wasm_bindgen]
pub fn reading(word: &str) -> String {
    shawline_word::reading(word)
}

/// The keys that type `word` (SPEC §13): `·𐑛𐑳𐑚𐑩𐑤` is typed `=dub@l`.
#[wasm_bindgen]
pub fn keys(word: &str) -> String {
    shawline_word::keys(word)
}

/// The transliteration table (SPEC §13) as an array of `[keys, letter,
/// name]`, where `name` is the letter's traditional name (`peep`).
#[wasm_bindgen]
pub fn table() -> Array {
    shawline_word::TABLE
        .iter()
        .map(|&(keys, letter)| {
            let name = shawline_word::letter_name(letter).unwrap_or_default();
            Array::of3(&keys.into(), &letter.to_string().into(), &name.into())
        })
        .collect()
}

/// An incremental transliterator for one editor (SPEC §13).
#[wasm_bindgen]
#[derive(Default)]
pub struct Transliterator(shawline_word::Transliterator);

#[wasm_bindgen]
impl Transliterator {
    /// A transliterator with nothing pending.
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self::default()
    }

    /// Feeds one typed key. Returns `{ delete, insert }`: delete `delete`
    /// code points before the cursor, then insert `insert`.
    pub fn push(&mut self, key: char) -> Object {
        let edit = self.0.push(key);
        let result = Object::new();
        set(&result, "delete", &(edit.delete as u32).into());
        set(&result, "insert", &edit.insert.into());
        result
    }

    /// Settles whatever is pending; call it when the cursor moves away.
    pub fn reset(&mut self) {
        self.0.reset();
    }
}

fn diagnostics(list: &[Diagnostic]) -> Array {
    list.iter().map(diagnostic).collect()
}

fn diagnostic(d: &Diagnostic) -> JsValue {
    let o = Object::new();
    set(&o, "row", &(d.at.row as u32).into());
    set(&o, "col", &(d.at.col as u32).into());
    set(&o, "code", &d.code.into());
    set(&o, "message", &d.message.as_str().into());
    set(&o, "severity", &d.severity().name().into());
    o.into()
}

fn set(target: &Object, key: &str, value: &JsValue) {
    // Defining a property on a fresh plain object can't fail.
    let _ = Reflect::set(target, &key.into(), value);
}
