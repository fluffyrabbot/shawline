//! WebAssembly bindings for the Shawline playground: a thin layer over
//! `shawline_cli` and `shawline_word` that turns results into plain JS
//! objects. No pipeline logic lives here.
//!
//! Positions are zero-based `row`/`col` in code points, as in every dump.

use js_sys::{Array, Object, Reflect};
use shawline_cli::Theme;
use shawline_grid::{Diagnostic, Dir, Pos};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = console)]
    fn error(message: &str);
}

/// Sends Rust panics to the console, so a pipeline bug shows its message
/// instead of a bare `unreachable` trap.
#[wasm_bindgen(start)]
pub fn start() {
    std::panic::set_hook(Box::new(|info| error(&info.to_string())));
}

/// Every static diagnostic for `src`, sorted by position: an array of
/// `{ row, col, code, message, severity }`, where `severity` is `"error"`,
/// `"warning"` or `"lint"`.
#[wasm_bindgen]
pub fn check(src: &str) -> Array {
    match shawline_cli::check(src) {
        Ok(checked) => diagnostics(&checked.diagnostics),
        Err(errors) => diagnostics(&errors),
    }
}

/// Checks and runs `src`. Returns `{ outputs, error, at, diagnostics }`:
/// `outputs` holds main's outputs as printed lines (SPEC §9), `error` is the
/// runtime error as `error ROW:COL CODE: message` (or `null`), `at` is that
/// error as a diagnostic object (absent without one), and `diagnostics` is
/// the same array `check` returns. A sheet with static errors doesn't run,
/// so it has no outputs and no runtime error.
#[wasm_bindgen]
pub fn run(src: &str) -> Object {
    let result = Object::new();
    let outputs = Array::new();
    let mut runtime = JsValue::NULL;
    let found = match shawline_cli::check(src) {
        Ok(checked) => {
            if let Some(program) = &checked.program {
                match shawline_cli::run(program) {
                    Ok(values) => values.iter().for_each(|v| {
                        outputs.push(&v.to_string().into());
                    }),
                    Err(e) => {
                        runtime = format!("error {} {}: {}", e.at, e.code, e.message).into();
                        set(&result, "at", &diagnostic(&e));
                    }
                }
            }
            checked.diagnostics
        }
        Err(errors) => errors,
    };
    set(&result, "outputs", &outputs);
    set(&result, "error", &runtime);
    set(&result, "diagnostics", &diagnostics(&found));
    result
}

/// The canonical form of `src` (SPEC §11), or `undefined` when the text has
/// grid errors and can't be read as a sheet.
#[wasm_bindgen]
pub fn format(src: &str) -> Option<String> {
    shawline_cli::format(src).ok()
}

/// `src` drawn as a standalone SVG in the light or dark theme, or
/// `undefined` when the text has grid errors.
#[wasm_bindgen]
pub fn render(src: &str, dark: bool) -> Option<String> {
    shawline_cli::render(src, if dark { Theme::DARK } else { Theme::LIGHT }).ok()
}

/// The node and definition boxes of `src`, for hit-testing: an array of
/// `{ top, left, bottom, right, kind }` (inclusive cell rectangles, `kind`
/// `"node"` or `"definition"`), empty when the text has grid errors.
#[wasm_bindgen]
pub fn boxes(src: &str) -> Array {
    let Ok(grid) = shawline_grid::Grid::parse(src) else { return Array::new() };
    let diagram = shawline_shape::recognize(&grid);
    diagram
        .boxes
        .iter()
        .filter_map(|b| {
            let kind = match b.parent {
                None => "definition",
                // A switch has dividers or a selector on its top edge.
                Some(_) if b.dividers.is_empty() && !b.ports.iter().any(|p| p.side == Dir::N) => {
                    "node"
                }
                Some(_) => return None,
            };
            let o = Object::new();
            set(&o, "top", &(b.rect.top as u32).into());
            set(&o, "left", &(b.rect.left as u32).into());
            set(&o, "bottom", &(b.rect.bottom as u32).into());
            set(&o, "right", &(b.rect.right as u32).into());
            set(&o, "kind", &kind.into());
            Some(JsValue::from(o))
        })
        .collect()
}

/// Moves the box whose top-left corner is at `row`, `col` by `dr` rows and
/// `dc` columns, as one checked edit (`shawline_edit::move_box`). Returns
/// `{ ok: true, text }` with the new canonical text, or `{ ok: false, row,
/// col, reason }` saying where and why the move was refused; then the sheet
/// is unchanged.
#[wasm_bindgen]
pub fn move_box(src: &str, row: u32, col: u32, dr: i32, dc: i32) -> Object {
    edited(shawline_edit::move_box(src, at(row, col), dr as isize, dc as isize))
}

/// The result of a checked edit, as `move_box` returns it.
fn edited(result: Result<String, shawline_edit::Refusal>) -> Object {
    let o = Object::new();
    match result {
        Ok(text) => {
            set(&o, "ok", &true.into());
            set(&o, "text", &text.into());
        }
        Err(refusal) => {
            set(&o, "ok", &false.into());
            set(&o, "row", &(refusal.at.row as u32).into());
            set(&o, "col", &(refusal.at.col as u32).into());
            set(&o, "reason", &refusal.reason.into());
        }
    }
    o
}

fn at(row: u32, col: u32) -> Pos {
    Pos::new(row as usize, col as usize)
}

fn cell(p: Pos) -> JsValue {
    let o = Object::new();
    set(&o, "row", &(p.row as u32).into());
    set(&o, "col", &(p.col as u32).into());
    o.into()
}

/// Connects the source at `from_row`, `from_col` to the sink arrowhead at
/// `to_row`, `to_col` (`shawline_edit::connect`): a new wire, or a branch
/// joining the source's wire with a tee. Returns what `move_box` returns.
#[wasm_bindgen]
pub fn connect(src: &str, from_row: u32, from_col: u32, to_row: u32, to_col: u32) -> Object {
    edited(shawline_edit::connect(src, at(from_row, from_col), at(to_row, to_col)))
}

/// Removes the whole wire through the cell, with every branch.
#[wasm_bindgen]
pub fn remove_wire(src: &str, row: u32, col: u32) -> Object {
    edited(shawline_edit::remove_wire(src, at(row, col)))
}

/// Removes only the branch reaching the sink arrowhead at the cell.
#[wasm_bindgen]
pub fn disconnect(src: &str, row: u32, col: u32) -> Object {
    edited(shawline_edit::disconnect(src, at(row, col)))
}

/// Places a canonical node box for `word` with its top-left corner at the
/// cell, `inputs` arrowheads and `outputs` tees, and no wires.
#[wasm_bindgen]
pub fn place_node(src: &str, row: u32, col: u32, word: &str, inputs: u16, outputs: u16) -> Object {
    edited(shawline_edit::place_node(src, at(row, col), word, inputs, outputs))
}

/// Removes the node box with its top-left corner at the cell, the wires it
/// feeds and the branches into its inputs.
#[wasm_bindgen]
pub fn remove_node(src: &str, row: u32, col: u32) -> Object {
    edited(shawline_edit::remove_node(src, at(row, col)))
}

/// What the context palette offers: an array of `{ word, inputs, outputs,
/// gloss, mirror }` (`mirror` is `null` but for primitives). Pass the
/// dragged source's cell to offer only callees with an input.
#[wasm_bindgen]
pub fn palette(src: &str, from_row: Option<u32>, from_col: Option<u32>) -> Array {
    let from = from_row.zip(from_col).map(|(r, c)| at(r, c));
    shawline_edit::palette(src, from)
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

/// Every source and sink, for dragging wires: `{ sources, sinks }`, each an
/// array of `{ row, col, span, scope, wired }`. `span` cells from `col`
/// eastward name the endpoint; a source reaches only sinks with the same
/// `scope` string, and a `wired` sink takes no second wire.
#[wasm_bindgen]
pub fn endpoints(src: &str) -> Object {
    let (sources, sinks) = shawline_edit::endpoints(src);
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

/// The cells a wire dragged from the source at `from_row`, `from_col` would
/// take if released at `to_row`, `to_col`, as an array of `{ row, col }`;
/// `undefined` when nothing fits. Nothing is edited.
#[wasm_bindgen]
pub fn preview(src: &str, from_row: u32, from_col: u32, to_row: u32, to_col: u32) -> Option<Array> {
    let path = shawline_edit::preview(src, at(from_row, from_col), at(to_row, to_col))?;
    Some(path.into_iter().map(cell).collect())
}

/// Every wire, for hit-testing and value badges: an array of `{ wire,
/// cells, source }`, where `wire` is the index `trace` reports, `cells` its
/// `{ row, col }` cells in row-major order, and `source` the cell naming its
/// source for `connect` (`null` when the wire isn't editable by mouse).
#[wasm_bindgen]
pub fn wires(src: &str) -> Array {
    let Ok(grid) = shawline_grid::Grid::parse(src) else { return Array::new() };
    let diagram = shawline_shape::recognize(&grid);
    let sources = shawline_edit::wire_sources(src);
    diagram
        .wires
        .iter()
        .enumerate()
        .map(|(i, w)| {
            let o = Object::new();
            set(&o, "wire", &(i as u32).into());
            set(&o, "cells", &w.cells.iter().copied().map(cell).collect::<Array>());
            let source = sources.get(i).copied().flatten();
            set(&o, "source", &source.map_or(JsValue::NULL, cell));
            JsValue::from(o)
        })
        .collect()
}

/// The value on every wire, for live badges (`shawline_cli::trace`):
/// `{ wires, error, stopped }`. `wires` holds `{ wire, value }` with each
/// value printed as SPEC §9 prints it, cut to 24 characters. `error` is the
/// runtime error that ended the run, or the first static error when the
/// sheet can't run (then `wires` is empty), as a diagnostic object; `null`
/// otherwise. `stopped` says the run hit its step budget.
#[wasm_bindgen]
pub fn trace(src: &str) -> Object {
    let o = Object::new();
    let values = Array::new();
    let (error, stopped) = match shawline_cli::trace(src) {
        Ok(found) => {
            for (wire, value) in &found.wires {
                let pair = Object::new();
                set(&pair, "wire", &(*wire as u32).into());
                set(&pair, "value", &shawline_vm::show(value, 24).into());
                values.push(&pair);
            }
            (found.error.as_ref().map_or(JsValue::NULL, diagnostic), found.stopped)
        }
        Err(errors) => (errors.first().map_or(JsValue::NULL, diagnostic), false),
    };
    set(&o, "wires", &values);
    set(&o, "error", &error);
    set(&o, "stopped", &stopped.into());
    o
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
