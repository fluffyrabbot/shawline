/**
 * One revision of a sheet (`shawline_doc::Doc`): its text, and each stage
 * read from it on first use and kept. It never changes; an edit returns a
 * new one. Call `free()` on every one you drop.
 */
export class Doc {
    static __wrap(ptr) {
        const obj = Object.create(Doc.prototype);
        obj.__wbg_ptr = ptr;
        DocFinalization.register(obj, obj.__wbg_ptr, obj);
        return obj;
    }
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        DocFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_doc_free(ptr, 0);
    }
    /**
     * Every box, in row-major order of its top-left corner, for
     * hit-testing and drawing boxes, ports and arms: an array of `{ top,
     * left, bottom, right, kind, parent, word, title, gap, ports,
     * dividers }`. Empty when the text has grid errors.
     *
     * - `top`…`right` is the inclusive cell rectangle, and `kind` is
     *   `"definition"`, `"switch"` or `"node"` (`shawline_edit::box_kind`).
     *   The square box `place_switch` draws is a `"node"` with no `word`
     *   until it has a selector or a divider.
     * - `parent` is the index in this array of the innermost box around
     *   it, or `null` at top level.
     * - `word` is a node's word and `title` the text in its top border's
     *   gap (a definition's `·`-name, a switch's first guard), each a text
     *   as [`Doc::texts`] gives it, or `null`. `gap` is that gap's cells,
     *   `{ col, span }`, or `null`.
     * - `ports` are its border's ports in row-major order, each `{ row,
     *   col, side, kind, name }`: `side` is `"north"`, `"east"`, `"south"`
     *   or `"west"`, `kind` is `"arrowhead"` or `"tee"`, and `name` is a
     *   definition port's name as a text, or `null`.
     * - `dividers` are a switch's dividers top to bottom, each `{ row,
     *   gap, guard }`, with `guard` the text in its gap, or `null` above a
     *   default arm.
     * @returns {Array<any>}
     */
    boxes() {
        const ret = wasm.doc_boxes(this.__wbg_ptr);
        return ret;
    }
    /**
     * The cells `disconnect` would clear for the arrowhead at the cell
     * (`shawline_edit::branch`), row-major, as an array of `{ row, col }`:
     * a branch to select. Empty when disconnecting it would be refused.
     * @param {number} row
     * @param {number} col
     * @returns {Array<any>}
     */
    branch(row, col) {
        const ret = wasm.doc_branch(this.__wbg_ptr, row, col);
        return ret;
    }
    /**
     * What the word covering the cell is, in the words editor hover uses
     * (`shawline_edit::describe`): `{ row, col, width, text }`, where
     * `text` is Markdown, or `undefined` when there is nothing to say.
     * With `gloss`, every Shavian word also gets its reading and keys.
     * @param {number} row
     * @param {number} col
     * @param {boolean} gloss
     * @returns {object | undefined}
     */
    describe(row, col, gloss) {
        const ret = wasm.doc_describe(this.__wbg_ptr, row, col, gloss);
        return ret;
    }
    /**
     * Every static diagnostic, sorted by position: an array of `{ row,
     * col, code, message, severity }`, where `severity` is `"error"`,
     * `"warning"` or `"lint"`.
     * @returns {Array<any>}
     */
    diagnostics() {
        const ret = wasm.doc_diagnostics(this.__wbg_ptr);
        return ret;
    }
    /**
     * Every source and sink, for dragging wires: `{ sources, sinks }`,
     * each an array of `{ row, col, span, scope, wired }`. `span` cells
     * from `col` eastward name the endpoint; a source reaches only sinks
     * with the same `scope` string, and a `wired` sink takes no second
     * wire.
     * @returns {object}
     */
    endpoints() {
        const ret = wasm.doc_endpoints(this.__wbg_ptr);
        return ret;
    }
    /**
     * The canonical form of the text (SPEC §11), or `undefined` when it
     * has grid errors and can't be read as a sheet.
     * @returns {string | undefined}
     */
    formatted() {
        const ret = wasm.doc_formatted(this.__wbg_ptr);
        let v1;
        if (ret[0] !== 0) {
            v1 = getStringFromWasm0(ret[0], ret[1]).slice();
            wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        }
        return v1;
    }
    /**
     * Where cells sit in this revision's drawing, glossed or plain, at
     * `zoom` times its natural size; `undefined` when the text has grid
     * errors.
     * @param {boolean} gloss
     * @param {number} zoom
     * @returns {Geometry | undefined}
     */
    geometry(gloss, zoom) {
        const ret = wasm.doc_geometry(this.__wbg_ptr, gloss, zoom);
        return ret === 0 ? undefined : Geometry.__wrap(ret);
    }
    /**
     * Every Shavian word on the sheet in row-major order, for hover: an
     * array of `{ row, col, span, word, reading, keys, gloss, shown }`, where
     * `span` is the word's length in cells, `gloss` a primitive's short
     * name (`null` for any other word), and `shown` whether the strip has
     * room to show it. Empty when the text has grid errors.
     * @returns {Array<any>}
     */
    glosses() {
        const ret = wasm.doc_glosses(this.__wbg_ptr);
        return ret;
    }
    /**
     * A revision holding `text`. Nothing is read until something asks.
     * @param {string} text
     */
    constructor(text) {
        const ptr0 = passStringToWasm0(text, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.doc_new(ptr0, len0);
        this.__wbg_ptr = ret;
        DocFinalization.register(this, this.__wbg_ptr, this);
        return this;
    }
    /**
     * What the context palette offers: an array of `{ word, inputs,
     * outputs, gloss, mirror }` (`mirror` is `null` but for primitives).
     * Pass the dragged source's cell to offer only callees with an input.
     * @param {number | null} [from_row]
     * @param {number | null} [from_col]
     * @returns {Array<any>}
     */
    palette(from_row, from_col) {
        const ret = wasm.doc_palette(this.__wbg_ptr, isLikeNone(from_row) ? Number.MAX_SAFE_INTEGER : (from_row) >>> 0, isLikeNone(from_col) ? Number.MAX_SAFE_INTEGER : (from_col) >>> 0);
        return ret;
    }
    /**
     * What `connect` would draw for a wire dragged from the cell at
     * `row`, `col` and released at `to_row`, `to_col`
     * (`shawline_edit::preview`): `{ path, joins_wire }`, where `path` is
     * the whole polyline as an array of `{ row, col }`, ends included, and
     * `joins_wire` says it starts on the source's wire at a new tee.
     * `from_sink` says the drag is anchored on an unwired arrowhead rather
     * than a source. `undefined` when nothing fits. Nothing is edited.
     * @param {boolean} from_sink
     * @param {number} row
     * @param {number} col
     * @param {number} to_row
     * @param {number} to_col
     * @returns {object | undefined}
     */
    preview(from_sink, row, col, to_row, to_col) {
        const ret = wasm.doc_preview(this.__wbg_ptr, from_sink, row, col, to_row, to_col);
        return ret;
    }
    /**
     * The sheet drawn as a standalone SVG in the light or dark theme, or
     * `undefined` when the text has grid errors. With `gloss`, a strip
     * under every row shows each word's reading or primitive name, and rows
     * sit [`row_pitch`]`(true)` apart; without it the drawing is plain.
     * @param {boolean} dark
     * @param {boolean} gloss
     * @returns {string | undefined}
     */
    render(dark, gloss) {
        const ret = wasm.doc_render(this.__wbg_ptr, dark, gloss);
        let v1;
        if (ret[0] !== 0) {
            v1 = getStringFromWasm0(ret[0], ret[1]).slice();
            wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        }
        return v1;
    }
    /**
     * The known fixes for this revision's diagnostics
     * (`shawline_edit::repairs`), in diagnostic order and at most one per
     * diagnostic: an array of `{ at, code, label, edit, then }`. `at` is
     * the diagnostic's cell as `{ row, col }`, and `label` what its button
     * says. `edit` is the edit as [`apply`] takes it, or `null` when the
     * follow-up is all of the fix. `then` is `{ kind, at }`, where `kind`
     * is `"none"`, `"name_field"` or `"draw_wire"` and `at` the cell the
     * follow-up acts on (`null` for `"none"`): a cell of the sheet `edit`
     * leaves, or of this one when `edit` is `null`. A repair is offered
     * only when its edit works. Nothing is applied.
     * @returns {Array<any>}
     */
    repairs() {
        const ret = wasm.doc_repairs(this.__wbg_ptr);
        return ret;
    }
    /**
     * The text, as the editor holds it.
     * @returns {string}
     */
    get text() {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.doc_text(this.__wbg_ptr);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * Every text run in row-major order, for selecting and editing: an
     * array of `{ row, col, span, kind, content }`, where `span` is its
     * width in cells, delimiters included, `kind` is `"word"`, `"number"`,
     * `"literal"` or `"comment"`, and `content` is the text without
     * delimiters. Empty when the text has grid errors.
     * @returns {Array<any>}
     */
    texts() {
        const ret = wasm.doc_texts(this.__wbg_ptr);
        return ret;
    }
    /**
     * Every wire, for hit-testing and value badges: an array of `{ wire,
     * cells, source }`, where `wire` is the index `trace` reports, `cells`
     * its `{ row, col, down }` cells in row-major order (`down` says the
     * cell joins the wire southward, so a highlight runs on through the
     * gloss strip below it), and `source` the cell naming its source for
     * `connect` (`null` when the wire isn't editable by mouse).
     * @returns {Array<any>}
     */
    wires() {
        const ret = wasm.doc_wires(this.__wbg_ptr);
        return ret;
    }
}
if (Symbol.dispose) Doc.prototype[Symbol.dispose] = Doc.prototype.free;

/**
 * Where cells sit in a drawing as [`Doc::render`] draws it, at one zoom
 * (`shawline_render::Geometry`). Points are in the page's CSS pixels,
 * measured from the drawn `<svg>`'s top-left corner, margin included.
 * Call `free()` on every one you drop.
 */
export class Geometry {
    static __wrap(ptr) {
        const obj = Object.create(Geometry.prototype);
        obj.__wbg_ptr = ptr;
        GeometryFinalization.register(obj, obj.__wbg_ptr, obj);
        return obj;
    }
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        GeometryFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_geometry_free(ptr, 0);
    }
    /**
     * The cell under a point, as `{ row, col }`; `undefined` left of or
     * above the grid. A point in a gloss strip is in the row above it.
     * @param {number} x
     * @param {number} y
     * @returns {object | undefined}
     */
    cell_at(x, y) {
        const ret = wasm.geometry_cell_at(this.__wbg_ptr, x, y);
        return ret;
    }
    /**
     * A cell's centre, `{ x, y }`.
     * @param {number} row
     * @param {number} col
     * @returns {object}
     */
    cell_centre(row, col) {
        const ret = wasm.geometry_cell_centre(this.__wbg_ptr, row, col);
        return ret;
    }
    /**
     * The area of an inclusive cell rectangle, `{ x, y, width, height }`.
     * `under` takes it on through the gloss strip below, and `over` back
     * up through the strip above.
     * @param {number} top
     * @param {number} left
     * @param {number} bottom
     * @param {number} right
     * @param {boolean} under
     * @param {boolean} over
     * @returns {object}
     */
    cell_rect(top, left, bottom, right, under, over) {
        const ret = wasm.geometry_cell_rect(this.__wbg_ptr, top, left, bottom, right, under, over);
        return ret;
    }
    /**
     * The whole drawing: `{ width, height }`.
     * @returns {object}
     */
    size() {
        const ret = wasm.geometry_size(this.__wbg_ptr);
        return ret;
    }
}
if (Symbol.dispose) Geometry.prototype[Symbol.dispose] = Geometry.prototype.free;

/**
 * Where an edit took the old sheet's parts (`Edited::map`), asked part by
 * part: the page carries its selection and cursor through it. Every query
 * names a part by its old cell and answers with the new one, or
 * `undefined` when the edit removed it. Call `free()` when done.
 */
export class Mapping {
    static __wrap(ptr) {
        const obj = Object.create(Mapping.prototype);
        obj.__wbg_ptr = ptr;
        MappingFinalization.register(obj, obj.__wbg_ptr, obj);
        return obj;
    }
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        MappingFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_mapping_free(ptr, 0);
    }
    /**
     * The box whose top-left corner was at the cell: its new `{ top,
     * left, bottom, right }`.
     * @param {number} row
     * @param {number} col
     * @returns {object | undefined}
     */
    map_box(row, col) {
        const ret = wasm.mapping_map_box(this.__wbg_ptr, row, col);
        return ret;
    }
    /**
     * Where a cell went, for the cursor: a cell of a text keeps its place
     * in that text; any other follows the nearest part of the box holding
     * it that the edit kept (its texts, the boxes inside it and its own
     * corners), so it moves with a moved box and past inserted rows. A
     * cell near nothing kept stays put. Always a cell.
     * @param {number} row
     * @param {number} col
     * @returns {object}
     */
    map_cell(row, col) {
        const ret = wasm.mapping_map_cell(this.__wbg_ptr, row, col);
        return ret;
    }
    /**
     * The text through the cell: its new `{ row, col, span }`, `span`
     * being its width in cells.
     * @param {number} row
     * @param {number} col
     * @returns {object | undefined}
     */
    map_text(row, col) {
        const ret = wasm.mapping_map_text(this.__wbg_ptr, row, col);
        return ret;
    }
    /**
     * The new index of wire `wire` (`Doc.wires`' `wire`).
     * @param {number} wire
     * @returns {number | undefined}
     */
    map_wire(wire) {
        const ret = wasm.mapping_map_wire(this.__wbg_ptr, wire);
        return ret === Number.MAX_SAFE_INTEGER ? undefined : ret;
    }
}
if (Symbol.dispose) Mapping.prototype[Symbol.dispose] = Mapping.prototype.free;

/**
 * An incremental transliterator for one editor (SPEC §13).
 */
export class Transliterator {
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        TransliteratorFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_transliterator_free(ptr, 0);
    }
    /**
     * A transliterator with nothing pending.
     */
    constructor() {
        const ret = wasm.transliterator_new();
        this.__wbg_ptr = ret;
        TransliteratorFinalization.register(this, this.__wbg_ptr, this);
        return this;
    }
    /**
     * Feeds one typed key. Returns `{ delete, insert }`: delete `delete`
     * code points before the cursor, then insert `insert`.
     * @param {string} key
     * @returns {object}
     */
    push(key) {
        const char0 = key.codePointAt(0);
        _assertChar(char0);
        const ret = wasm.transliterator_push(this.__wbg_ptr, char0);
        return ret;
    }
    /**
     * Settles whatever is pending; call it when the cursor moves away.
     */
    reset() {
        wasm.transliterator_reset(this.__wbg_ptr);
    }
}
if (Symbol.dispose) Transliterator.prototype[Symbol.dispose] = Transliterator.prototype.free;

/**
 * `message` with each Shavian word's reading after it in parentheses, as
 * in `𐑥𐑱𐑯 (mayn)` (`shawline_word::annotate`): how the page shows a
 * diagnostic, a runtime error or a refusal while Readings is on.
 * @param {string} message
 * @returns {string}
 */
export function annotate(message) {
    let deferred2_0;
    let deferred2_1;
    try {
        const ptr0 = passStringToWasm0(message, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.annotate(ptr0, len0);
        deferred2_0 = ret[0];
        deferred2_1 = ret[1];
        return getStringFromWasm0(ret[0], ret[1]);
    } finally {
        wasm.__wbindgen_free(deferred2_0, deferred2_1, 1);
    }
}

/**
 * Applies one edit to `doc` (`shawline_edit::apply`). Done, it returns `{
 * ok: true, doc, tidied, map }`: the next revision, whether the edit was
 * made on the formatted text (the page applies that text first, as its own
 * undo step), and a [`Mapping`] from the old sheet's parts to the new
 * one's (`null` when either side can't be read). A `make_room` edit also
 * has `inserted`, an array of `{ axis, row, col, count }`. Refused, it
 * returns `{ ok: false, row, col, reason, kind, remedy }`, where `kind` is
 * `RefusalKind::name()` and `remedy` is `"make-room"`, `"format"` or
 * `null`; the revision passed in is still the sheet. An edit object the
 * page wrote wrong is refused as `target`.
 * @param {Doc} doc
 * @param {any} edit
 * @returns {object}
 */
export function apply(doc, edit) {
    _assertClass(doc, Doc);
    const ret = wasm.apply(doc.__wbg_ptr, edit);
    return ret;
}

/**
 * The cell at `offset` UTF-16 units into `text`, as a textarea's
 * selection counts (`shawline_doc::position::cell_of_offset`): `{ row,
 * col }`. A leading byte-order mark is no cell, and an offset on a line
 * ending is just past that line's last cell.
 * @param {string} text
 * @param {number} offset
 * @returns {object}
 */
export function cell_of_offset(text, offset) {
    const ptr0 = passStringToWasm0(text, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
    const len0 = WASM_VECTOR_LEN;
    const ret = wasm.cell_of_offset(ptr0, len0, offset);
    return ret;
}

/**
 * The keys that type `word` (SPEC §13): `·𐑛𐑳𐑚𐑩𐑤` is typed `=dub@l`.
 * @param {string} word
 * @returns {string}
 */
export function keys(word) {
    let deferred2_0;
    let deferred2_1;
    try {
        const ptr0 = passStringToWasm0(word, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.keys(ptr0, len0);
        deferred2_0 = ret[0];
        deferred2_1 = ret[1];
        return getStringFromWasm0(ret[0], ret[1]);
    } finally {
        wasm.__wbindgen_free(deferred2_0, deferred2_1, 1);
    }
}

/**
 * The offset in UTF-16 units of the cell at `row`, `col` in `text`
 * (`shawline_doc::position::offset_of_cell`): where a textarea puts its
 * caret to stand on that cell.
 * @param {string} text
 * @param {number} row
 * @param {number} col
 * @returns {number}
 */
export function offset_of_cell(text, row, col) {
    const ptr0 = passStringToWasm0(text, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
    const len0 = WASM_VECTOR_LEN;
    const ret = wasm.offset_of_cell(ptr0, len0, row, col);
    return ret >>> 0;
}

/**
 * The message of the last panic, if one has happened. The trap that ends
 * a panic reaches JS as a bare `RuntimeError`, so the page asks here what
 * went wrong.
 * @returns {string | undefined}
 */
export function panic_message() {
    const ret = wasm.panic_message();
    let v1;
    if (ret[0] !== 0) {
        v1 = getStringFromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
    }
    return v1;
}

/**
 * The names a new port takes, in order (`shawline_edit::PORT_NAMES`: 𐑨,
 * 𐑚, 𐑒, …), as an array of one-letter strings. The Box tool names a
 * definition's new port with the first its ports don't have, as
 * `extract` names the ports it makes.
 * @returns {Array<any>}
 */
export function port_names() {
    const ret = wasm.port_names();
    return ret;
}

/**
 * How `word` sounds (SPEC §13, Reading): `·𐑛𐑳𐑚𐑩𐑤` reads `dubəl`.
 * @param {string} word
 * @returns {string}
 */
export function reading(word) {
    let deferred2_0;
    let deferred2_1;
    try {
        const ptr0 = passStringToWasm0(word, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.reading(ptr0, len0);
        deferred2_0 = ret[0];
        deferred2_1 = ret[1];
        return getStringFromWasm0(ret[0], ret[1]);
    } finally {
        wasm.__wbindgen_free(deferred2_0, deferred2_1, 1);
    }
}

/**
 * Checks and runs `src`, for the runner worker. Returns `{ outputs, error,
 * at, diagnostics }`: `outputs` holds main's outputs as printed lines (SPEC
 * §9), `error` is the runtime error as `error ROW:COL CODE: message` (or
 * `null`), `at` is that error as a diagnostic object (absent without one),
 * and `diagnostics` is the array `Doc.diagnostics` returns. A sheet with
 * static errors doesn't run, so it has no outputs and no runtime error.
 * @param {string} src
 * @returns {object}
 */
export function run(src) {
    const ptr0 = passStringToWasm0(src, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
    const len0 = WASM_VECTOR_LEN;
    const ret = wasm.run(ptr0, len0);
    return ret;
}

/**
 * Sends Rust panics to the console, so a pipeline bug shows its message
 * instead of a bare `unreachable` trap, and keeps it for [`panic_message`].
 */
export function start() {
    wasm.start();
}

/**
 * The transliteration table (SPEC §13) as an array of `[keys, letter,
 * name]`, where `name` is the letter's traditional name (`peep`).
 * @returns {Array<any>}
 */
export function table() {
    const ret = wasm.table();
    return ret;
}

/**
 * The value on every wire of `src`, for the tracer worker's live badges
 * (`shawline_doc::Doc::trace`): `{ wires, error, stopped }`. `wires` holds
 * `{ wire, value }` with each value printed as SPEC §9 prints it, cut to
 * 24 characters. `error` is the runtime error that ended the run, or the
 * first static error when the sheet can't run (then `wires` is empty), as
 * a diagnostic object; `null` otherwise. `stopped` says the run hit its
 * step budget.
 * @param {string} src
 * @returns {object}
 */
export function trace(src) {
    const ptr0 = passStringToWasm0(src, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
    const len0 = WASM_VECTOR_LEN;
    const ret = wasm.trace(ptr0, len0);
    return ret;
}
function __wbg_get_imports() {
    const import0 = {
        __proto__: null,
        __wbg___wbindgen_boolean_get_fa956cfa2d1bd751: function(arg0) {
            const v = arg0;
            const ret = typeof(v) === 'boolean' ? v : undefined;
            return isLikeNone(ret) ? 0xFFFFFF : ret ? 1 : 0;
        },
        __wbg___wbindgen_is_null_ea9085d691f535d3: function(arg0) {
            const ret = arg0 === null;
            return ret;
        },
        __wbg___wbindgen_is_undefined_c05833b95a3cf397: function(arg0) {
            const ret = arg0 === undefined;
            return ret;
        },
        __wbg___wbindgen_number_get_394265ed1e1b84ee: function(arg0, arg1) {
            const obj = arg1;
            const ret = typeof(obj) === 'number' ? obj : undefined;
            getDataViewMemory0().setFloat64(arg0 + 8 * 1, isLikeNone(ret) ? 0 : ret, true);
            getDataViewMemory0().setInt32(arg0 + 4 * 0, !isLikeNone(ret), true);
        },
        __wbg___wbindgen_string_get_b0ca35b86a603356: function(arg0, arg1) {
            const obj = arg1;
            const ret = typeof(obj) === 'string' ? obj : undefined;
            var ptr1 = isLikeNone(ret) ? 0 : passStringToWasm0(ret, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            var len1 = WASM_VECTOR_LEN;
            getDataViewMemory0().setInt32(arg0 + 4 * 1, len1, true);
            getDataViewMemory0().setInt32(arg0 + 4 * 0, ptr1, true);
        },
        __wbg___wbindgen_throw_344f42d3211c4765: function(arg0, arg1) {
            throw new Error(getStringFromWasm0(arg0, arg1));
        },
        __wbg_doc_new: function(arg0) {
            const ret = Doc.__wrap(arg0);
            return ret;
        },
        __wbg_error_d6ceccf651c6d6b6: function(arg0, arg1) {
            console.error(getStringFromWasm0(arg0, arg1));
        },
        __wbg_from_13e323c65fc8f464: function(arg0) {
            const ret = Array.from(arg0);
            return ret;
        },
        __wbg_get_78f252d074a84d0b: function() { return handleError(function (arg0, arg1) {
            const ret = Reflect.get(arg0, arg1);
            return ret;
        }, arguments); },
        __wbg_get_unchecked_6e0ad6d2a41b06f6: function(arg0, arg1) {
            const ret = arg0[arg1 >>> 0];
            return ret;
        },
        __wbg_isArray_0677c962b281d01a: function(arg0) {
            const ret = Array.isArray(arg0);
            return ret;
        },
        __wbg_length_370319915dc99107: function(arg0) {
            const ret = arg0.length;
            return ret;
        },
        __wbg_mapping_new: function(arg0) {
            const ret = Mapping.__wrap(arg0);
            return ret;
        },
        __wbg_new_32b398fb48b6d94a: function() {
            const ret = new Array();
            return ret;
        },
        __wbg_new_da52cf8fe3429cb2: function() {
            const ret = new Object();
            return ret;
        },
        __wbg_of_b0cd2e09b31a9684: function(arg0, arg1, arg2) {
            const ret = Array.of(arg0, arg1, arg2);
            return ret;
        },
        __wbg_push_d2ae3af0c1217ae6: function(arg0, arg1) {
            const ret = arg0.push(arg1);
            return ret;
        },
        __wbg_set_8535240470bf2500: function() { return handleError(function (arg0, arg1, arg2) {
            const ret = Reflect.set(arg0, arg1, arg2);
            return ret;
        }, arguments); },
        __wbindgen_cast_0000000000000001: function(arg0) {
            // Cast intrinsic for `F64 -> Externref`.
            const ret = arg0;
            return ret;
        },
        __wbindgen_cast_0000000000000002: function(arg0, arg1) {
            // Cast intrinsic for `Ref(String) -> Externref`.
            const ret = getStringFromWasm0(arg0, arg1);
            return ret;
        },
        __wbindgen_init_externref_table: function() {
            const table = wasm.__wbindgen_externrefs;
            const offset = table.grow(4);
            table.set(0, undefined);
            table.set(offset + 0, undefined);
            table.set(offset + 1, null);
            table.set(offset + 2, true);
            table.set(offset + 3, false);
        },
    };
    return {
        __proto__: null,
        "./shawline_wasm_bg.js": import0,
    };
}

const DocFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_doc_free(ptr, 1));
const GeometryFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_geometry_free(ptr, 1));
const MappingFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_mapping_free(ptr, 1));
const TransliteratorFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_transliterator_free(ptr, 1));

function addToExternrefTable0(obj) {
    const idx = wasm.__externref_table_alloc();
    wasm.__wbindgen_externrefs.set(idx, obj);
    return idx;
}

function _assertChar(c) {
    if (typeof(c) === 'number' && (c >= 0x110000 || (c >= 0xD800 && c < 0xE000))) throw new Error(`expected a valid Unicode scalar value, found ${c}`);
}

function _assertClass(instance, klass) {
    if (!(instance instanceof klass)) {
        throw new Error(`expected instance of ${klass.name}`);
    }
}

let cachedDataViewMemory0 = null;
function getDataViewMemory0() {
    if (cachedDataViewMemory0 === null || cachedDataViewMemory0.buffer.detached === true || (cachedDataViewMemory0.buffer.detached === undefined && cachedDataViewMemory0.buffer !== wasm.memory.buffer)) {
        cachedDataViewMemory0 = new DataView(wasm.memory.buffer);
    }
    return cachedDataViewMemory0;
}

function getStringFromWasm0(ptr, len) {
    return decodeText(ptr >>> 0, len);
}

let cachedUint8ArrayMemory0 = null;
function getUint8ArrayMemory0() {
    if (cachedUint8ArrayMemory0 === null || cachedUint8ArrayMemory0.byteLength === 0) {
        cachedUint8ArrayMemory0 = new Uint8Array(wasm.memory.buffer);
    }
    return cachedUint8ArrayMemory0;
}

function handleError(f, args) {
    try {
        return f.apply(this, args);
    } catch (e) {
        const idx = addToExternrefTable0(e);
        wasm.__wbindgen_exn_store(idx);
    }
}

function isLikeNone(x) {
    return x === undefined || x === null;
}

function passStringToWasm0(arg, malloc, realloc) {
    if (realloc === undefined) {
        const buf = cachedTextEncoder.encode(arg);
        const ptr = malloc(buf.length, 1) >>> 0;
        getUint8ArrayMemory0().subarray(ptr, ptr + buf.length).set(buf);
        WASM_VECTOR_LEN = buf.length;
        return ptr;
    }

    let len = arg.length;
    let ptr = malloc(len, 1) >>> 0;

    const mem = getUint8ArrayMemory0();

    let offset = 0;

    for (; offset < len; offset++) {
        const code = arg.charCodeAt(offset);
        if (code > 0x7F) break;
        mem[ptr + offset] = code;
    }
    if (offset !== len) {
        if (offset !== 0) {
            arg = arg.slice(offset);
        }
        ptr = realloc(ptr, len, len = offset + arg.length * 3, 1) >>> 0;
        const view = getUint8ArrayMemory0().subarray(ptr + offset, ptr + len);
        const ret = cachedTextEncoder.encodeInto(arg, view);

        offset += ret.written;
        ptr = realloc(ptr, len, offset, 1) >>> 0;
    }

    WASM_VECTOR_LEN = offset;
    return ptr;
}

let cachedTextDecoder = new TextDecoder('utf-8', { ignoreBOM: true, fatal: true });
cachedTextDecoder.decode();
const MAX_SAFARI_DECODE_BYTES = 2146435072;
let numBytesDecoded = 0;
function decodeText(ptr, len) {
    numBytesDecoded += len;
    if (numBytesDecoded >= MAX_SAFARI_DECODE_BYTES) {
        cachedTextDecoder = new TextDecoder('utf-8', { ignoreBOM: true, fatal: true });
        cachedTextDecoder.decode();
        numBytesDecoded = len;
    }
    return cachedTextDecoder.decode(getUint8ArrayMemory0().subarray(ptr, ptr + len));
}

const cachedTextEncoder = new TextEncoder();

if (!('encodeInto' in cachedTextEncoder)) {
    cachedTextEncoder.encodeInto = function (arg, view) {
        const buf = cachedTextEncoder.encode(arg);
        view.set(buf);
        return {
            read: arg.length,
            written: buf.length
        };
    };
}

let WASM_VECTOR_LEN = 0;

let wasmModule, wasmInstance, wasm;
function __wbg_finalize_init(instance, module) {
    wasmInstance = instance;
    wasm = instance.exports;
    wasmModule = module;
    cachedDataViewMemory0 = null;
    cachedUint8ArrayMemory0 = null;
    wasm.__wbindgen_start();
    return wasm;
}

async function __wbg_load(module, imports) {
    if (typeof Response === 'function' && module instanceof Response) {
        if (typeof WebAssembly.instantiateStreaming === 'function') {
            try {
                return await WebAssembly.instantiateStreaming(module, imports);
            } catch (e) {
                const validResponse = module.ok && expectedResponseType(module.type);

                if (validResponse && module.headers.get('Content-Type') !== 'application/wasm') {
                    console.warn("`WebAssembly.instantiateStreaming` failed because your server does not serve Wasm with `application/wasm` MIME type. Falling back to `WebAssembly.instantiate` which is slower. Original error:\n", e);

                } else { throw e; }
            }
        }

        const bytes = await module.arrayBuffer();
        return await WebAssembly.instantiate(bytes, imports);
    } else {
        const instance = await WebAssembly.instantiate(module, imports);

        if (instance instanceof WebAssembly.Instance) {
            return { instance, module };
        } else {
            return instance;
        }
    }

    function expectedResponseType(type) {
        switch (type) {
            case 'basic': case 'cors': case 'default': return true;
        }
        return false;
    }
}

function initSync(module) {
    if (wasm !== undefined) return wasm;


    if (module !== undefined) {
        if (Object.getPrototypeOf(module) === Object.prototype) {
            ({module} = module)
        } else {
            console.warn('using deprecated parameters for `initSync()`; pass a single object instead')
        }
    }

    const imports = __wbg_get_imports();
    if (!(module instanceof WebAssembly.Module)) {
        module = new WebAssembly.Module(module);
    }
    const instance = new WebAssembly.Instance(module, imports);
    return __wbg_finalize_init(instance, module);
}

async function __wbg_init(module_or_path) {
    if (wasm !== undefined) return wasm;


    if (module_or_path !== undefined) {
        if (Object.getPrototypeOf(module_or_path) === Object.prototype) {
            ({module_or_path} = module_or_path)
        } else {
            console.warn('using deprecated parameters for the initialization function; pass a single object instead')
        }
    }

    if (module_or_path === undefined) {
        module_or_path = new URL('shawline_wasm_bg.wasm', import.meta.url);
    }
    const imports = __wbg_get_imports();

    if (typeof module_or_path === 'string' || (typeof Request === 'function' && module_or_path instanceof Request) || (typeof URL === 'function' && module_or_path instanceof URL)) {
        module_or_path = fetch(module_or_path);
    }

    const { instance, module } = await __wbg_load(await module_or_path, imports);

    return __wbg_finalize_init(instance, module);
}

export { initSync, __wbg_init as default };
