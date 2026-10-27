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
use shawline_edit::{Refusal, RefusalKind};
use shawline_grid::{Diagnostic, Dir, Pos};
use shawline_render::{Options, Theme};
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
    /// array of `{ row, col, span, word, reading, gloss, shown }`, where
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
                let kind = match b.parent {
                    None => "definition",
                    // A switch has dividers or a selector on its top edge.
                    Some(_) if !b.dividers.is_empty() || b.ports.iter().any(|p| p.side == Dir::N) => {
                        "switch"
                    }
                    Some(_) => "node",
                };
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

    /// The cells a wire dragged from the source at `from_row`, `from_col`
    /// would take if released at `to_row`, `to_col`, as an array of `{ row,
    /// col }`; `undefined` when nothing fits. Nothing is edited.
    pub fn preview(&self, from_row: u32, from_col: u32, to_row: u32, to_col: u32) -> Option<Array> {
        let path = shawline_edit::preview(&self.0, at(from_row, from_col), at(to_row, to_col))?;
        Some(path.into_iter().map(cell).collect())
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

// Checked edits (`shawline_edit`). Each takes the revision to edit and
// returns `{ ok: true, doc }` with the next revision, or `{ ok: false, row,
// col, reason, kind }` saying where and why it was refused, where `kind` is
// `RefusalKind::name()`; a refused edit makes no revision, and the one
// passed in is still the sheet.

/// Moves the box whose top-left corner is at `row`, `col` by `dr` rows and
/// `dc` columns (`shawline_edit::move_box`).
#[wasm_bindgen]
pub fn move_box(doc: &Doc, row: u32, col: u32, dr: i32, dc: i32) -> Object {
    edited(shawline_edit::move_box(&doc.0, at(row, col), dr as isize, dc as isize))
}

/// Connects the source at `from_row`, `from_col` to the sink arrowhead at
/// `to_row`, `to_col` (`shawline_edit::connect`): a new wire, or a branch
/// joining the source's wire with a tee.
#[wasm_bindgen]
pub fn connect(doc: &Doc, from_row: u32, from_col: u32, to_row: u32, to_col: u32) -> Object {
    edited(shawline_edit::connect(&doc.0, at(from_row, from_col), at(to_row, to_col)))
}

/// Removes the whole wire through the cell, with every branch.
#[wasm_bindgen]
pub fn remove_wire(doc: &Doc, row: u32, col: u32) -> Object {
    edited(shawline_edit::remove_wire(&doc.0, at(row, col)))
}

/// Removes only the branch reaching the sink arrowhead at the cell.
#[wasm_bindgen]
pub fn disconnect(doc: &Doc, row: u32, col: u32) -> Object {
    edited(shawline_edit::disconnect(&doc.0, at(row, col)))
}

/// Places a canonical node box for `word` with its top-left corner at the
/// cell, `inputs` arrowheads and `outputs` tees, and no wires. Done, it
/// also has `inputs` and `outputs`: the new node's port cells, top to
/// bottom, as arrays of `{ row, col }`.
#[wasm_bindgen]
pub fn place_node(doc: &Doc, row: u32, col: u32, word: &str, inputs: u16, outputs: u16) -> Object {
    match shawline_edit::place_node(&doc.0, at(row, col), word, inputs, outputs) {
        Ok(placed) => {
            let o = made(placed.doc);
            set(&o, "inputs", &placed.inputs.into_iter().map(cell).collect::<Array>());
            set(&o, "outputs", &placed.outputs.into_iter().map(cell).collect::<Array>());
            o
        }
        Err(refusal) => refused(refusal),
    }
}

/// Places a node as `place_node` does, then connects the source at
/// `from_row`, `from_col` to its first input: the palette's gesture after a
/// wire dropped on empty space, as one edit. An unwired node isn't what
/// the gesture asked for, so when either step is refused nothing is placed;
/// the refusal's `step` is `"place"` or `"connect"`, saying which.
#[wasm_bindgen]
#[allow(clippy::too_many_arguments)] // wasm-bindgen takes cells as plain numbers
pub fn place_and_connect(
    doc: &Doc,
    row: u32,
    col: u32,
    word: &str,
    inputs: u16,
    outputs: u16,
    from_row: u32,
    from_col: u32,
) -> Object {
    let corner = at(row, col);
    let placed = match shawline_edit::place_node(&doc.0, corner, word, inputs, outputs) {
        Ok(placed) => placed,
        Err(refusal) => return stepped(refusal, "place"),
    };
    let Some(&sink) = placed.inputs.first() else {
        let reason = format!("{word} has no input to wire the source into");
        return stepped(Refusal { at: corner, kind: RefusalKind::Target, reason }, "connect");
    };
    match shawline_edit::connect(&placed.doc, at(from_row, from_col), sink) {
        Ok(wired) => made(wired),
        Err(refusal) => stepped(refusal, "connect"),
    }
}

/// Removes the node box with its top-left corner at the cell, the wires it
/// feeds and the branches into its inputs.
#[wasm_bindgen]
pub fn remove_node(doc: &Doc, row: u32, col: u32) -> Object {
    edited(shawline_edit::remove_node(&doc.0, at(row, col)))
}

fn edited(result: Result<shawline_doc::Doc, Refusal>) -> Object {
    match result {
        Ok(doc) => made(doc),
        Err(refusal) => refused(refusal),
    }
}

/// `{ ok: true, doc }`, handing the revision to JS, which must free it.
fn made(doc: shawline_doc::Doc) -> Object {
    let o = Object::new();
    set(&o, "ok", &true.into());
    set(&o, "doc", &Doc(doc).into());
    o
}

fn refused(refusal: Refusal) -> Object {
    let o = Object::new();
    set(&o, "ok", &false.into());
    set(&o, "row", &(refusal.at.row as u32).into());
    set(&o, "col", &(refusal.at.col as u32).into());
    set(&o, "reason", &refusal.reason.into());
    set(&o, "kind", &refusal.kind.name().into());
    o
}

fn stepped(refusal: Refusal, step: &str) -> Object {
    let o = refused(refusal);
    set(&o, "step", &step.into());
    o
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

/// The distance from one row's top to the next in [`Doc::render`]'s SVG
/// units, with or without the gloss strip (`shawline_render::row_pitch`).
/// A row's cell sits at the top of its pitch, and the strip, if any, under
/// it; `row_pitch(false)` is the cell's own height.
#[wasm_bindgen]
pub fn row_pitch(gloss: bool) -> f64 {
    shawline_render::row_pitch(gloss)
}

/// `message` with each Shavian word's reading after it in parentheses, as
/// in `𐑥𐑱𐑯 (mayn)` (`shawline_word::annotate`): how the page shows a
/// diagnostic, a runtime error or a refusal while Readings is on.
#[wasm_bindgen]
pub fn annotate(message: &str) -> String {
    shawline_word::annotate(message)
}

/// The keys that type `word` (SPEC §13, Reading): `·𐑥𐑱𐑯` reads `=mayn`.
#[wasm_bindgen]
pub fn reading(word: &str) -> String {
    shawline_word::reading(word)
}

/// The transliteration table (SPEC §13) as an array of `[keys, letter]`.
#[wasm_bindgen]
pub fn table() -> Array {
    shawline_word::TABLE
        .iter()
        .map(|(keys, letter)| Array::of2(&(*keys).into(), &letter.to_string().into()))
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
    let severity = match d.code.as_bytes().first() {
        Some(b'W') => "warning",
        Some(b'L') => "lint",
        _ => "error",
    };
    let o = Object::new();
    set(&o, "row", &(d.at.row as u32).into());
    set(&o, "col", &(d.at.col as u32).into());
    set(&o, "code", &d.code.into());
    set(&o, "message", &d.message.as_str().into());
    set(&o, "severity", &severity.into());
    o.into()
}

fn set(target: &Object, key: &str, value: &JsValue) {
    // Defining a property on a fresh plain object can't fail.
    let _ = Reflect::set(target, &key.into(), value);
}
