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
    let result = Object::new();
    let corner = Pos::new(row as usize, col as usize);
    match shawline_edit::move_box(src, corner, dr as isize, dc as isize) {
        Ok(text) => {
            set(&result, "ok", &true.into());
            set(&result, "text", &text.into());
        }
        Err(refusal) => {
            set(&result, "ok", &false.into());
            set(&result, "row", &(refusal.at.row as u32).into());
            set(&result, "col", &(refusal.at.col as u32).into());
            set(&result, "reason", &refusal.reason.into());
        }
    }
    result
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
