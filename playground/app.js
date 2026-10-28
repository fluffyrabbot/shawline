// The Shawline playground: an editor, a live drawing, diagnostics, and a
// Run button. All language work happens in the wasm package; this file only
// wires it to the page.
import init, {
  Doc, table, move_box, connect, remove_wire, disconnect, remove_node, place_node, place_and_connect,
  Transliterator, row_pitch, annotate, reading, keys as keysOf, panic_message,
} from "./pkg/shawline_wasm.js";
import { Tour, TOUR_EXAMPLE } from "./tour.js";
import { Store, TOUR_SLOT } from "./drafts.js";
import { Docs } from "./docs.js";
import { Flight } from "./flight.js";
import { Layer, CELL_W } from "./layer.js";

const $ = (id) => document.getElementById(id);
const editor = $("editor");
const marks = $("marks");
const drawing = $("drawing");
const problems = $("problems");
const output = $("output");
const runButton = $("run");
const translit = $("translit");
const readings = $("readings");
const examples = $("examples");
const values = $("values");
const traceState = $("trace-status");
const statusLine = $("drawing-status");
const resetButton = $("reset");
const tour = new Tour($("tour"), () => showOverlays(), (text) => layer.message(text));
const RUN_LIMIT_MS = 10_000;

/** The gloss layer (layer.js), made once the wasm package has loaded. */
let layer;
/**
 * Where cells sit in the drawing on the page, as it was drawn: every
 * overlay and hit test goes through it, so they follow the row pitch.
 */
let geo;

const store = new Store((() => {
  // Reading `localStorage` itself throws where storage is disabled.
  try {
    return localStorage;
  } catch {
    return null;
  }
})());
const reducedMotion = matchMedia("(prefers-reduced-motion: reduce)");

// Positions are code points, as in every Shawline dump, but the textarea
// counts UTF-16 units and Shavian letters take two.

const isHigh = (unit) => unit >= 0xd800 && unit <= 0xdbff;
const isLow = (unit) => unit >= 0xdc00 && unit <= 0xdfff;
const width = (text, i) => (text.codePointAt(i) > 0xffff ? 2 : 1);

// A leading byte-order mark isn't a cell (shawline-grid skips it), so row 0's
// columns start after it.
const BOM = "\ufeff";

/** The text's rows, without a leading byte-order mark. */
const rowsOf = (text) => (text.startsWith(BOM) ? text.slice(1) : text).split("\n");

function offsetOf(text, row, col) {
  let i = row === 0 && text.startsWith(BOM) ? 1 : 0;
  for (let r = 0; r < row; r++) {
    const next = text.indexOf("\n", i);
    if (next < 0) return text.length;
    i = next + 1;
  }
  for (let c = 0; c < col && i < text.length && text[i] !== "\n"; c++) i += width(text, i);
  return i;
}

function posOf(text, offset) {
  const before = text.slice(0, offset);
  let lineStart = before.lastIndexOf("\n") + 1;
  if (lineStart === 0 && before.startsWith(BOM)) lineStart = 1;
  return { row: before.split("\n").length - 1, col: [...before.slice(lineStart)].length };
}

/** Puts the caret at a cell, or selects the cell itself. */
function jumpTo({ row, col }, select = true) {
  const text = editor.value;
  const at = offsetOf(text, row, col);
  const end = select && at < text.length && text[at] !== "\n" ? at + width(text, at) : at;
  editor.focus({ preventScroll: true });
  editor.setSelectionRange(at, end);
  const cell = parseFloat(getComputedStyle(editor).fontSize);
  const box = editor.parentElement;
  box.scrollLeft = Math.max(0, (col * cell) / 2 - box.clientWidth / 2);
  const top = editor.getBoundingClientRect().top + row * cell;
  if (top < 0 || top > innerHeight - cell) {
    scrollBy({ top: top - innerHeight / 3, behavior: reducedMotion.matches ? "auto" : "smooth" });
  }
}

/** Replaces the selection, keeping the browser's undo history where it can. */
function insertText(text) {
  const done = text
    ? document.execCommand("insertText", false, text)
    : document.execCommand("delete");
  if (!done) {
    editor.setRangeText(text, editor.selectionStart, editor.selectionEnd, "end");
    changed();
  }
}

/**
 * Replaces the whole text as edits the browser can undo, touching only the
 * span that differs. Chrome's redo of an insert over a span ending at the
 * text's final newline drops the first inserted line, so such a span is
 * replaced short of that newline, which goes in a second step.
 */
function replaceAll(text) {
  const old = editor.value;
  const n = Math.min(old.length, text.length);
  let from = 0;
  while (from < n && old[from] === text[from]) from++;
  // Never split a Shavian letter's surrogate pair.
  if (from > 0 && isHigh(old.charCodeAt(from - 1))) from--;
  let kept = 0;
  while (kept < n - from && old[old.length - 1 - kept] === text[text.length - 1 - kept]) kept++;
  if (kept > 0 && isLow(old.charCodeAt(old.length - kept))) kept--;
  const to = old.length - kept;
  const insert = text.slice(from, text.length - kept);
  const trailing = insert && to > from && to === old.length && old.endsWith("\n");
  editor.focus({ preventScroll: true });
  editor.setSelectionRange(from, trailing ? to - 1 : to);
  if (insert || from !== to) insertText(insert);
  if (trailing) {
    editor.setSelectionRange(editor.value.length - 1, editor.value.length);
    insertText("");
  }
}

// Transliteration (SPEC §13). Keys go through the wasm Transliterator, which
// says how many letters to take back and what to write instead: `t` shows
// 𐑑, then `h` turns it into 𐑔. Only word runs are transliterated, which
// matters for `v` alone: inside a word it is 𐑝, elsewhere the ▼ fallback.

let trans;
let keys;
let expectedCaret = -1;
let afterSeparator = false;

function settle() {
  trans?.reset();
  afterSeparator = false;
}

function inWord(at) {
  if (afterSeparator) return true;
  const text = editor.value;
  const cp = at >= 2 && isLow(text.charCodeAt(at - 1)) ? text.codePointAt(at - 2) : text.charCodeAt(at - 1);
  return (cp >= 0x10450 && cp <= 0x1047f) || cp === 0xb7;
}

editor.addEventListener("beforeinput", (e) => {
  const key = /^[A-Z]$/.test(e.data) ? e.data.toLowerCase() : e.data;
  const { selectionStart: start, selectionEnd: end } = editor;
  if (start !== expectedCaret || start !== end) settle();
  if (!translit.checked || e.inputType !== "insertText" || e.isComposing || !keys?.has(key) ||
      (key === "v" && !inWord(start))) {
    settle();
    return;
  }
  e.preventDefault();
  const edit = trans.push(key);
  afterSeparator = key === "/";
  let from = start;
  for (let i = 0; i < edit.delete; i++) from -= isLow(editor.value.charCodeAt(from - 1)) ? 2 : 1;
  editor.setSelectionRange(from, end);
  if (edit.insert || from !== end) insertText(edit.insert);
  expectedCaret = editor.selectionStart;
});

translit.addEventListener("change", () => {
  store.set("translit", translit.checked);
  settle();
});

// The gloss layer (PLAN P9): readings under every word and in messages, on
// by default. Holding Alt by itself flips it until Alt comes up; any other
// key ends that, so Option-typing never flashes it. Window listeners in the
// capture phase see every key, whatever has focus and whatever stops it.

readings.addEventListener("change", () => {
  store.set("readings", readings.checked);
  layer.set(readings.checked);
});
addEventListener("keydown", (e) => layer?.keydown(e), { capture: true });
addEventListener("keyup", (e) => layer?.keyup(e), { capture: true });
for (const type of ["pointerdown", "pointermove"]) addEventListener(type, (e) => layer?.pointer(e), { capture: true });
addEventListener("blur", () => layer?.cancel());
document.addEventListener("visibilitychange", () => layer?.cancel());

/**
 * Shows everything again as the layer now shows it: the drawing at its new
 * pitch, with whatever drag is under way, and every message.
 */
function relayer() {
  layer.resay(document);
  drawing.querySelectorAll(".refusal").forEach((n) => n.remove());
  if (drag?.view) drawSheet(drag.cut);
  else showDrawing();
  if (drag) {
    if (drag.kind !== "box" && (drag.kind !== "branch" || drag.view)) glow();
    if (drag.at && drag.kind === "box") showGhost();
    else if (drag.at && drag.path) showPath();
  }
  if (menu) {
    menu.show();
    placeMenu();
  }
  // A peek or a turn of the switch is what the tour's first step asks for;
  // a peek counts once it is over.
  if (layer.peeking) tour.update(current());
  else tour.looked(current());
}

// Live checking and drawing, a beat after the last keystroke.

let refreshTimer;
let shown = [];
let storageWarned = false;
/**
 * The editor's revision, counted up on every change to its text. Worker
 * jobs and their results carry it, and Run's output keeps the one it ran.
 */
let rev = 0;

const docs = new Docs((text) => new Doc(text));
/** The editor's revision, read once and shared by everything that asks. */
const doc = () => docs.get(editor.value);

function changed() {
  rev++;
  const lines = editor.value.split("\n");
  // Not Math.max(...): a paste of some 100,000 lines overflows its arguments.
  const cols = lines.reduce((widest, l) => Math.max(widest, [...l].length), 0);
  editor.style.width = `${cols + 2}ch`;
  editor.style.height = `calc(${lines.length + 1}em + var(--pad))`;
  saveDraft();
  // An edit is the next action, so a refusal shown before it is stale.
  showStatus();
  showOutputAge();
  clearTimeout(refreshTimer);
  refreshTimer = setTimeout(refresh, 120);
}

function refresh() {
  // Reading and drawing run here, unbounded: a sheet that freezes the page
  // leaves the mark, and the next load opens the original instead.
  const marked = Boolean(slot) && store.markDrawing(slot);
  shown = doc().diagnostics();
  tour.update(current());
  showProblems();
  showDrawing();
  scheduleTrace();
  if (marked) store.unmarkDrawing();
}

// Anything thrown and not caught (a Rust panic arrives as a bare
// RuntimeError) would otherwise leave a stale drawing and say nothing.
function showFailure(error) {
  let panic;
  try {
    panic = panic_message();
  } catch {
    // The package never loaded, so there was no panic to ask about.
  }
  const message = panic ?? String(error?.message ?? error);
  statusLine.textContent = `Something went wrong inside the playground (${message}). Your text is saved; reload the page to continue.`;
  statusLine.className = "drawing-status refused";
}
addEventListener("error", (e) => showFailure(e.error ?? e.message));
addEventListener("unhandledrejection", (e) => showFailure(e.reason));

function el(tag, className, text) {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text !== undefined) node.textContent = text;
  return node;
}

function showProblems() {
  const counts = {};
  shown.forEach((d) => (counts[d.severity] = (counts[d.severity] ?? 0) + 1));
  $("problem-count").textContent = Object.entries(counts)
    .map(([kind, n]) => `${n} ${kind}${n > 1 ? "s" : ""}`)
    .join(", ");

  problems.replaceChildren(
    ...(shown.length ? shown.map(problem) : [el("li", "clear", "Nothing to report.")]),
  );
  marks.replaceChildren(
    ...shown.map((d, i) => {
      const mark = el("span", `mark ${d.severity}`);
      mark.dataset.i = i;
      mark.style.left = `${d.col}ch`;
      mark.style.top = `${d.row}em`;
      return mark;
    }),
  );
}

function problem(d, i) {
  const item = el("li", d.severity);
  item.dataset.i = i;
  // One-based, like the CLI's diagnostic report.
  item.append(el("span", "pos", `${d.row + 1}:${d.col + 1}`), el("span", "code", d.code), layer.say(el("span", "message"), d.message));
  item.title = d.severity;
  item.addEventListener("click", () => jumpTo(d));
  item.addEventListener("mouseenter", () => highlight(i, true));
  item.addEventListener("mouseleave", () => highlight(i, false));
  return item;
}

function highlight(i, on) {
  document.querySelectorAll(`[data-i="${i}"]`).forEach((node) => node.classList.toggle("hot", on));
}

/**
 * Draws `src` at the editor's scale, glossed while the layer shows; false
 * when the text can't be read, and then the drawing and `geo` stay as they
 * were.
 */
function drawSheet(src) {
  const svg = docs.get(src).render(document.documentElement.dataset.theme === "dark", layer.shown);
  if (svg === undefined) return false;
  drawing.innerHTML = svg;
  geo = layer.geometry;
  const view = drawing.querySelector("svg").viewBox.baseVal;
  // A cell is half an em wide, as in the editor, whatever the row pitch.
  const em = parseFloat(getComputedStyle(editor).fontSize);
  drawing.firstElementChild.style.width = `${(view.width / CELL_W) * (em / 2)}px`;
  return true;
}

function showDrawing() {
  if (!drawSheet(editor.value)) {
    // Unreadable text (a tab, a stray character): keep the last drawing, faded.
    drawing.querySelector("svg")?.classList.add("stale");
    return;
  }
  const background = drawing.querySelector("svg > rect");
  background?.after(
    ...shown.map((d, i) => {
      const cell = cellRect(`cell ${d.severity}`, d);
      cell.dataset.i = i;
      return cell;
    }),
  );
  showWords();
  showOverlays();
}

/**
 * While the layer shows, every word says how it is read when hovered,
 * over the word and the strip under it: a gloss the strip had no room for
 * is still there.
 */
function showWords() {
  const svg = drawing.querySelector("svg");
  if (!layer.shown || !svg) return;
  const view = current();
  view.words ??= doc().glosses();
  const group = svgEl("g", "words");
  for (const { row, col, span, word, reading: sounds, keys: typed, gloss } of view.words) {
    const target = svgRect("", { top: row, left: col, bottom: row, right: col + span - 1, under: true });
    const title = svgEl("title");
    // The title is where a word is looked at closely, so it also teaches the
    // keys; the strip shows only how it sounds.
    const said = `${word} reads ${sounds}, typed ${typed}`;
    title.textContent = gloss ? `${said}: ${gloss}` : said;
    target.append(title);
    group.append(target);
  }
  svg.append(group);
}

/** The drawing's cell under a pointer event, or null off the sheet. */
function cellAt(e) {
  const svg = drawing.querySelector("svg");
  if (!svg) return null;
  const box = svg.getBoundingClientRect();
  if (e.clientX < box.left || e.clientX >= box.right || e.clientY < box.top || e.clientY >= box.bottom) return null;
  const view = svg.viewBox.baseVal;
  const x = view.x + ((e.clientX - box.left) * view.width) / box.width;
  const y = view.y + ((e.clientY - box.top) * view.height) / box.height;
  return geo.cellAt(x, y);
}

// The drawing as an editor (Studio Phase 1). Every gesture is one checked
// edit to the text (shawline-edit), applied through the textarea so undo
// works as it does for typing; a refused edit changes nothing and says why
// beside the cell it is about, and in the status line under the drawing.
//
// - Drag from a source (an output tee, an input, a constant, an arm's
//   input) or from a wire: release on a free arrowhead to connect, or on
//   empty space to pick a node from the palette, placed there and wired in.
// - Drag from a free arrowhead back to a source or a wire: connect, joining
//   that wire as a fan-out.
// - Drag a wired arrowhead away: its branch comes off. Release on a source
//   or a wire to reconnect it there, anywhere else to leave it off.
// - Drag anything else on a box to move the box.
// - Click a node, a wire or a wired arrowhead to select it; Delete removes
//   the node, the whole wire, or just that arrowhead's branch.
// - Escape, or letting go off the sheet, cancels a drag.

const SVG = "http://www.w3.org/2000/svg";
const keyOf = ({ row, col }) => `${row}:${col}`;
const same = (a, b) => a && b && a.row === b.row && a.col === b.col;
const LOCKED = "This wire can't be edited by mouse yet; edit the text.";

let dragged = false;
let pressed = null;
let drag = null;
let selected = null;
let menu = null;

// What a sheet offers to the pointer, read once per text and kept with its
// Doc: the editor's, and the sheet under a branch being dragged, with that
// branch taken off.
function viewOf(src) {
  return docs.view(src, (sheet) => {
    const ends = sheet.endpoints();
    const list = sheet.wires();
    const byCell = new Map();
    /** Cells that join a wire southward, through the strip under them. */
    const downs = new Set();
    for (const w of list) {
      for (const c of w.cells) {
        const k = keyOf(c);
        byCell.set(k, [...(byCell.get(k) ?? []), w]);
        if (c.down) downs.add(k);
      }
    }
    return { src, boxes: sheet.boxes(), sources: ends.sources, sinks: ends.sinks, wires: list, byCell, downs };
  });
}

const current = () => viewOf(editor.value);

const sourceAt = (at, view = current()) =>
  view.sources.find((s) => s.row === at.row && at.col >= s.col && at.col < s.col + s.span);
const sinkAt = (at, view = current()) => view.sinks.find((s) => same(s, at));
/** The editable wire through a cell, if any. */
const wireAt = (at, view = current()) => (view.byCell.get(keyOf(at)) ?? []).find((w) => w.source) ?? null;
/** A wire runs through the cell, but none the mouse can edit. */
const lockedAt = (at) => (current().byCell.get(keyOf(at))?.length ?? 0) > 0 && !wireAt(at);
const charAt = ({ row, col }) => [...(rowsOf(editor.value)[row] ?? "")][col] ?? " ";

drawing.addEventListener("click", (e) => {
  // The click that ends a drag isn't a request for the caret.
  if (dragged) return void (dragged = false);
  const at = cellAt(e);
  // Pressed on one cell and let go on another isn't a click on either.
  if (!at || !same(at, pressed)) return;
  const sink = sinkAt(at);
  const wire = wireAt(at);
  const box = boxAt(at);
  if (sink?.wired) selectBranch(sink);
  else if (wire) select({ kind: "wire", at, wire });
  else if (box?.kind === "node" && !lockedAt(at)) select({ kind: "node", box });
  else {
    select(null);
    // On a character (a locked wire, a name, a border) the caret goes to
    // it, since the text is where it can be edited. A click on blank sheet
    // only deselects, so focus stays here and Delete can't reach the text.
    if (charAt(at) !== " ") jumpTo(at, false);
  }
});

drawing.addEventListener("dblclick", (e) => {
  const at = cellAt(e);
  if (at && charAt(at) === " " && boxAt(at) && boxAt(at).kind !== "node") openMenu(at, null);
});

/** The innermost box under a cell: a node before its definition. */
function boxAt(at) {
  const area = (b) => (b.bottom - b.top + 1) * (b.right - b.left + 1);
  return current()
    .boxes.filter((b) => at.row >= b.top && at.row <= b.bottom && at.col >= b.left && at.col <= b.right)
    .sort((a, b) => area(a) - area(b))[0];
}

function svgEl(tag, className, attrs = {}) {
  const node = document.createElementNS(SVG, tag);
  Object.entries(attrs).forEach(([k, v]) => node.setAttribute(k, v));
  if (className) node.setAttribute("class", className);
  return node;
}

/** A cell rectangle; `under` and `over` carry it through the strips beside it. */
function svgRect(className, { top, left, bottom, right, under, over }) {
  return svgEl("rect", className, { ...geo.rect({ top, left, bottom, right }, { under, over }), rx: 90 });
}

const cellRect = (className, { row, col, down }, span = 1) =>
  svgRect(className, { top: row, left: col, bottom: row, right: col + span - 1, under: down });
const centre = (at) => {
  const { x, y } = geo.centre(at);
  return `${x},${y}`;
};

/**
 * Cells as rectangles, one per straight run, so an outline reads as one.
 * A one-column run reaches through the gloss strip to the cells it joins
 * above and below (`down` on a cell says it joins the cell under it).
 */
function runs(cells) {
  const across = [];
  for (const { row, col } of cells) {
    const last = across[across.length - 1];
    if (last && last.top === row && last.right + 1 === col) last.right = col;
    else across.push({ top: row, left: col, bottom: row, right: col });
  }
  const out = [];
  for (const r of across) {
    const above = r.left === r.right && out.find((o) => o.left === r.left && o.right === r.left && o.bottom + 1 === r.top);
    if (above) above.bottom = r.top;
    else out.push(r);
  }
  const down = new Set(cells.filter((c) => c.down).map(keyOf));
  for (const r of out) {
    if (r.left !== r.right) continue;
    r.under = down.has(keyOf({ row: r.bottom, col: r.left }));
    r.over = down.has(keyOf({ row: r.top - 1, col: r.left }));
  }
  return out;
}

/** The tour's rings, selection and value badges, drawn again over every
 * fresh drawing. */
function showOverlays() {
  const svg = drawing.querySelector("svg");
  // A detached branch is being dragged over a sheet that isn't the text yet.
  if (!svg || drag?.view) return;
  svg.querySelectorAll(".overlay").forEach((n) => n.remove());
  svg.append(...tour.rings().map((r) => svgRect("overlay beacon", r)));
  if (selected && selected.src !== editor.value) selected = null;
  if (selected?.kind === "node") svg.append(svgRect("overlay selected", selected.box));
  if (selected?.kind === "wire") {
    const group = svgEl("g", "overlay selected-wire");
    // Each cell reaches through the strip to the cell it joins below.
    selected.wire.cells.forEach((c) => group.append(cellRect("", c)));
    svg.append(group);
  }
  if (selected?.kind === "branch") {
    const group = svgEl("g", "overlay selected-branch");
    group.append(...runs(selected.cells).map((r) => svgRect("", r)), cellRect("head", selected.sink));
    svg.append(group);
  }
  showBadges(svg);
}

const HINTS = {
  node: "Node selected. Delete removes it and its wires; drag it to move it.",
  wire: "Wire selected. Delete removes the whole wire, every branch.",
  branch: "Arrowhead selected. Delete disconnects just this branch; drag it onto a source to rewire it.",
};

function select(target) {
  selected = target && { ...target, src: editor.value };
  // A refusal outlives a deselection; only the next action clears it.
  if (target) showStatus(HINTS[target.kind], "hint");
  else if (statusLine.classList.contains("hint")) showStatus();
  showOverlays();
  if (target) focusDrawing(false);
}

/**
 * Focuses the drawing with its ring only when `ring` says so. Focus moved
 * by script would otherwise inherit the editor's ring, and a mouse user
 * would see one after every click.
 */
function focusDrawing(ring) {
  drawing.focus({ preventScroll: true, focusVisible: ring });
}

/**
 * Selects the branch into a wired arrowhead: the cells `disconnect` would
 * clear, which are its stroke back to the tee.
 */
function selectBranch(sink) {
  const src = editor.value;
  const cut = disconnect(doc(), sink.row, sink.col);
  if (!cut.ok) return select(null);
  const text = cut.doc.text;
  // Only its text is wanted; the drag that may follow cuts it again.
  cut.doc.free();
  const before = rowsOf(src).map((l) => [...l]);
  const after = rowsOf(text).map((l) => [...l]);
  const cells = [];
  before.forEach((line, row) =>
    line.forEach((ch, col) => {
      const at = { row, col };
      const k = keyOf(at);
      if (ch !== (after[row]?.[col] ?? " ") && current().byCell.has(k)) cells.push({ ...at, down: current().downs.has(k) });
    }),
  );
  select({ kind: "branch", sink, cells });
}

/** Says something about the drawing until the next action: a refusal, or
 * what the selection is. Empty clears it. */
function showStatus(text = "", kind = "") {
  layer.say(statusLine, text);
  statusLine.className = kind ? `drawing-status ${kind}` : "drawing-status";
}

function showGhost() {
  const { box, dr, dc } = drag;
  drag.ghost?.remove();
  const at = { top: box.top + dr, left: box.left + dc, bottom: box.bottom + dr, right: box.right + dc };
  drag.ghost = (dr || dc) && at.top >= 0 && at.left >= 0 ? svgRect("ghost", at) : null;
  if (drag.ghost) drawing.querySelector("svg")?.append(drag.ghost);
}

/** Takes the drag's preview away, as when the pointer leaves the sheet. */
function hidePreview() {
  drag.ghost?.remove();
  drag.path?.remove();
  drag.ghost = drag.path = null;
}

/** Ends the drag without committing it, and puts back a detached branch. */
function endDrag() {
  if (!drag) return;
  const { view, pointerId } = drag;
  hidePreview();
  drag.marks?.forEach((n) => n.remove());
  drag = null;
  drawing.classList.remove("dragging", "wiring");
  if (drawing.hasPointerCapture(pointerId)) drawing.releasePointerCapture(pointerId);
  if (view) showDrawing();
}

/** Shows a refusal's reason by the cell it is about, for a moment, and in
 * the status line until the next action. */
function refuse({ row, col, reason }) {
  showStatus(reason, "refused");
  const svg = drawing.querySelector("svg");
  if (!svg) return;
  const cell = cellRect("refused", { row, col });
  svg.append(cell);
  const tip = layer.say(el("div", "refusal"), reason);
  place(tip, { row: row + 1, col });
  drawing.append(tip);
  setTimeout(() => (tip.remove(), cell.remove()), 2800);
}

/** A cell's top-left corner in viewport pixels. */
function clientOf({ row, col }) {
  const svg = drawing.querySelector("svg");
  const view = svg.viewBox.baseVal;
  const box = svg.getBoundingClientRect();
  const { x, y } = geo.rect({ top: row, left: col, bottom: row, right: col });
  return {
    x: ((x - view.x) * box.width) / view.width + box.left,
    y: ((y - view.y) * box.height) / view.height + box.top,
  };
}

/** Positions an HTML element inside #drawing at a cell's top-left corner. */
function place(node, at) {
  const { x, y } = clientOf(at);
  const host = drawing.getBoundingClientRect();
  node.style.left = `${x - host.left + drawing.scrollLeft}px`;
  node.style.top = `${y - host.top + drawing.scrollTop}px`;
}

/**
 * Replaces the whole text through the editor, so it can be undone like
 * typing, and puts the caret at `caret`.
 */
function replaceText(text, caret) {
  const scroll = { x: scrollX, y: scrollY };
  replaceAll(text);
  settle();
  const at = offsetOf(editor.value, caret.row, caret.col);
  editor.setSelectionRange(at, at);
  scrollTo(scroll.x, scroll.y);
  clearTimeout(refreshTimer);
  refresh();
}

/** Applies a drawing edit's text, keeping the caret's cell. */
function applyText(text) {
  const ring = drawing.matches(":focus-visible");
  replaceText(text, posOf(editor.value, editor.selectionStart));
  focusDrawing(ring);
}

/** Applies an edit's result: its revision becomes the editor's, already read. */
function apply(result) {
  if (!result.ok) return void refuse(result);
  applyText(docs.seed(result.doc));
}

// Dragging.

drawing.addEventListener("pointerdown", (e) => {
  // One drag at a time: a second finger is ignored.
  if (e.button !== 0 || drag) return;
  // A drag isn't always followed by a click, so its flag mustn't outlive
  // it and swallow the next real click.
  dragged = false;
  closeMenu();
  showStatus();
  const at = cellAt(e);
  pressed = at;
  if (!at) return;
  const source = sourceAt(at);
  const sink = sinkAt(at);
  const wire = wireAt(at);
  let start = null;
  if (source) start = { kind: "wire", from: { row: source.row, col: source.col }, scope: source.scope };
  else if (sink && !sink.wired) start = { kind: "sink", sink, scope: sink.scope };
  else if (sink) {
    // The branch comes off once the pointer leaves the arrowhead; a refusal
    // (its wire isn't editable) is better said on the press.
    const cut = disconnect(doc(), sink.row, sink.col);
    if (!cut.ok) return void refuse(cut);
    // Kept as the second revision: the drawing shows it once the branch is off.
    start = { kind: "branch", sink, scope: sink.scope, cut: docs.seed(cut.doc) };
  } else if (wire) {
    const from = sourceAt(wire.source) ?? wire.source;
    start = { kind: "wire", from: { row: from.row, col: from.col }, scope: from.scope };
  } else if (lockedAt(at)) {
    return void refuse({ ...at, reason: LOCKED });
  } else {
    const box = boxAt(at);
    if (box) start = { kind: "box", box, dr: 0, dc: 0, ghost: null };
  }
  if (!start) return;
  e.preventDefault();
  // preventDefault keeps focus where it was; Escape must reach the drawing.
  focusDrawing(false);
  drawing.setPointerCapture(e.pointerId);
  drag = { ...start, start: at, at, pointerId: e.pointerId, moved: false };
  drawing.classList.add(drag.kind === "box" ? "dragging" : "wiring");
  if (drag.kind === "wire" || drag.kind === "sink") glow();
});

/** Lights up where the drag can end: free sinks for a source, sources for a sink. */
function glow() {
  const svg = drawing.querySelector("svg");
  const { sources, sinks } = drag.view ?? current();
  const marks =
    drag.kind === "wire"
      ? sinks.filter((s) => !s.wired && s.scope === drag.scope).map((s) => cellRect("glow", s))
      : sources.filter((s) => s.scope === drag.scope).map((s) => cellRect("glow", s, s.span));
  svg?.append(...marks);
  drag.marks = marks;
}

/** Takes the dragged branch off: the drawing shows the sheet without it
 * until the drag ends. */
function detach() {
  drag.marks?.forEach((n) => n.remove());
  drag.view = viewOf(drag.cut);
  drawSheet(drag.cut);
  glow();
}

/** The source a sink-first drag would connect to, under the pointer. */
function sourceUnder(at, view = current()) {
  const source = sourceAt(at, view);
  if (source) return source;
  const wire = wireAt(at, view);
  return wire && (sourceAt(wire.source, view) ?? wire.source);
}

function showPath() {
  const svg = drawing.querySelector("svg");
  if (!svg) return;
  const view = drag.view ?? current();
  const { at } = drag;
  // `preview` runs from the sink end back to the source, or, when the
  // source already has a wire, back to the tee joining it: then the path
  // ends on that wire and mustn't be drawn on to the source.
  const tail = (source) => (source?.wired ? [] : [source]);
  let cells;
  let found;
  if (drag.kind === "wire") {
    const sink = sinkAt(at, view);
    const free = sink && !sink.wired && sink.scope === drag.scope;
    const to = free ? sink : at;
    found = !same(to, drag.from) && docs.get(view.src).preview(drag.from.row, drag.from.col, to.row, to.col);
    const from = tail(sourceAt(drag.from, view) ?? { ...drag.from, wired: false });
    if (found) cells = [...(free ? [sink] : []), ...found, ...from];
    else cells = [at, drag.from];
  } else {
    const source = sourceUnder(at, view);
    found = source && source.scope === drag.scope &&
      docs.get(view.src).preview(source.row, source.col, drag.sink.row, drag.sink.col);
    cells = found ? [drag.sink, ...found, ...tail(source)] : [drag.sink, at];
  }
  drag.path?.remove();
  drag.path = svgEl("polyline", found ? "wire-preview" : "wire-preview loose", {
    points: cells.map(centre).join(" "),
  });
  svg.append(drag.path);
}

drawing.addEventListener("pointermove", (e) => {
  const at = cellAt(e);
  if (!drag) {
    const svg = drawing.querySelector("svg");
    const sink = at && sinkAt(at);
    const over = at && (sourceAt(at) || sink?.wired === false || wireAt(at));
    const locked = at && !over && !sink && lockedAt(at);
    svg?.classList.toggle("over-source", Boolean(over));
    svg?.classList.toggle("over-sink", Boolean(sink?.wired));
    svg?.classList.toggle("over-locked", Boolean(locked));
    svg?.classList.toggle("over-box", Boolean(at && !over && !sink && !locked && boxAt(at)));
    return;
  }
  if (e.pointerId !== drag.pointerId) return;
  if (!at) {
    // Letting go off the sheet cancels, so the preview goes.
    if (drag.at) hidePreview();
    drag.at = null;
    return;
  }
  if (same(at, drag.at)) return;
  drag.at = at;
  drag.moved ||= !same(at, drag.start);
  if (drag.kind === "branch" && drag.moved && !drag.view) detach();
  if (drag.kind === "box") {
    Object.assign(drag, { dr: at.row - drag.start.row, dc: at.col - drag.start.col });
    showGhost();
  } else {
    showPath();
  }
});

drawing.addEventListener("pointerup", (e) => {
  if (!drag || e.pointerId !== drag.pointerId) return;
  const finished = drag;
  const at = cellAt(e);
  endDrag();
  // Off the sheet, or back where it started: nothing. A press that never
  // moved is a click, which selects.
  if (!at || same(at, finished.start)) return void (dragged ||= finished.moved || !at);
  dragged = true;
  if (finished.kind === "box") {
    apply(move_box(doc(), finished.box.top, finished.box.left, finished.dr, finished.dc));
  } else if (finished.kind === "branch") {
    // One edit either way: off, or off and on again at the new source.
    const { cut, sink } = finished;
    const source = sourceUnder(at, viewOf(cut));
    if (!source) {
      applyText(cut);
      return void showStatus("Branch removed. Undo puts it back.");
    }
    apply(connect(docs.get(cut), source.row, source.col, sink.row, sink.col));
  } else if (finished.kind === "sink") {
    const source = sourceUnder(at);
    if (!source) return refuse({ ...at, reason: "Drop on a source or a wire to connect this arrowhead." });
    apply(connect(doc(), source.row, source.col, finished.sink.row, finished.sink.col));
  } else {
    const { from } = finished;
    const sink = sinkAt(at);
    const wire = wireAt(at);
    if (sink) apply(connect(doc(), from.row, from.col, sink.row, sink.col));
    else if (wire) {
      const reason = same(wire.source, from) || same(sourceAt(wire.source), sourceAt(from))
        ? "This wire already carries that value. Drop on an arrowhead to add a branch."
        : "That wire has a source of its own. Drop on a free arrowhead.";
      refuse({ ...at, reason });
    } else if (charAt(at) === " ") openMenu(at, from);
    else refuse({ ...at, reason: "Drop on a free arrowhead to connect, or on empty space for a new node." });
  }
});

drawing.addEventListener("pointercancel", endDrag);
drawing.addEventListener("lostpointercapture", (e) => drag?.pointerId === e.pointerId && endDrag());

drawing.addEventListener("keydown", (e) => {
  const mod = e.metaKey || e.ctrlKey;
  if (mod && e.key.toLowerCase() === "z") {
    // The drawing has no history of its own: undo is the editor's.
    e.preventDefault();
    endDrag();
    const ring = drawing.matches(":focus-visible");
    editor.focus({ preventScroll: true });
    document.execCommand(e.shiftKey ? "redo" : "undo");
    focusDrawing(ring);
  } else if ((e.key === "Delete" || e.key === "Backspace") && selected && !drag) {
    e.preventDefault();
    const target = selected;
    select(null);
    if (target.kind === "node") apply(remove_node(doc(), target.box.top, target.box.left));
    else if (target.kind === "branch") apply(disconnect(doc(), target.sink.row, target.sink.col));
    else apply(remove_wire(doc(), target.at.row, target.at.col));
  } else if (e.key === "Escape") {
    // The release that follows a cancelled drag isn't a click.
    if (drag) dragged = true;
    endDrag();
    closeMenu();
    select(null);
    showStatus();
  }
});

// The context palette: callees to place where a drag ended, wired from the
// dragged source into the new node's first input.

function closeMenu() {
  // Focus in a removed palette would fall to the page, out of Escape's reach.
  if (menu?.contains(document.activeElement)) focusDrawing(false);
  menu?.remove();
  menu = null;
}

function openMenu(at, from) {
  closeMenu();
  const offered = doc().palette(from?.row, from?.col);
  // Every word can be found by how it sounds or how it is typed, shown or not.
  offered.forEach((c) => ((c.reading = reading(c.word)), (c.keys = keysOf(c.word))));
  menu = el("div", "palette");
  menu.setAttribute("role", "dialog");
  menu.setAttribute("aria-label", "Place a node");
  const filter = el("input");
  filter.placeholder = from ? "Feed it into…" : "Place a node…";
  filter.setAttribute("aria-label", "Filter by meaning or letter");
  const list = el("ul");
  list.setAttribute("role", "listbox");
  menu.append(filter, list);

  // The first input lands on the drop row, with the box's top border
  // above it. A drop a row or two off the source's row snaps level with
  // it first, so the new wire runs straight instead of jogging.
  const rows = from && from.row !== at.row && Math.abs(from.row - at.row) <= 2 ? [from.row, at.row] : [at.row];
  // Placing and wiring are one edit, composed in wasm: an unwired node
  // isn't what the drag asked for, so a refused connect places nothing.
  const pick = (c) => {
    closeMenu();
    const place = (row) => {
      const corner = { row: Math.max(0, row - 1), col: at.col };
      return from
        ? place_and_connect(doc(), corner.row, corner.col, c.word, c.inputs, c.outputs, from.row, from.col)
        : place_node(doc(), corner.row, corner.col, c.word, c.inputs, c.outputs);
    };
    let unwired = null;
    let unplaced = null;
    for (const row of rows) {
      const placed = place(row);
      if (placed.ok) return apply(placed);
      if (placed.step === "connect") unwired ??= placed;
      // The drop row is tried last; its refusal is the one to show.
      else unplaced = placed;
    }
    if (!unwired) return refuse(unplaced);
    refuse({ ...at, reason: `Nothing placed: the new node couldn't be wired in (${unwired.reason}).` });
  };

  const show = () => {
    const q = filter.value.trim().toLowerCase();
    const items = offered.filter(
      (c) =>
        !q || c.word.includes(q) || c.reading.includes(q) || c.keys.includes(q) || c.gloss.toLowerCase().includes(q),
    );
    list.replaceChildren(
      ...items.map((c, i) => {
        const item = el("li");
        item.setAttribute("role", "option");
        if (i === 0) item.classList.add("first");
        const arity = `${c.inputs}→${c.outputs}`;
        const word = el("span", "word", c.word);
        if (layer.shown) word.append(el("span", "reading", c.reading));
        const gloss = layer.message(c.gloss);
        item.append(word, el("span", "gloss", gloss), el("span", "arity", arity));
        if (c.mirror) item.append(el("span", "mirror", `⇄ ${c.mirror}`));
        item.title = c.mirror ? `${gloss}; its mirror ${layer.message(c.mirror)} is the dual` : gloss;
        item.addEventListener("pointerdown", (e) => e.stopPropagation());
        item.addEventListener("click", (e) => {
          e.stopPropagation();
          pick(c);
        });
        return item;
      }),
    );
    menu.items = items;
  };
  filter.addEventListener("input", show);
  filter.addEventListener("keydown", (e) => {
    e.stopPropagation();
    if (e.key === "Escape") closeMenu();
    if (e.key === "Enter" && menu.items[0]) pick(menu.items[0]);
  });
  menu.addEventListener("pointerdown", (e) => e.stopPropagation());
  menu.addEventListener("click", (e) => e.stopPropagation());
  show();
  // A flip of the layer shows the list again and moves it with its cell.
  Object.assign(menu, { at, show });
  document.body.append(menu);
  placeMenu();
  // After the click that ends the drag, which would take focus back.
  setTimeout(() => filter.focus({ preventScroll: true }));
}

/**
 * Fixed over the page rather than inside the drawing's scroll box, which
 * would clip it: below the drop point, or above it when there's no room.
 */
function placeMenu() {
  const { at } = menu;
  const below = clientOf({ row: at.row + 1, col: at.col });
  const above = clientOf(at).y - menu.offsetHeight - 8;
  const room = below.y + menu.offsetHeight + 8 <= innerHeight;
  menu.style.left = `${Math.max(8, Math.min(below.x, innerWidth - menu.offsetWidth - 8))}px`;
  menu.style.top = `${Math.max(8, room || above < 8 ? below.y : above)}px`;
}

document.addEventListener("pointerdown", (e) => {
  if (menu && !menu.contains(e.target)) closeMenu();
});
// A fixed palette would drift from its drop point as the page scrolls.
addEventListener("scroll", (e) => menu && !menu.contains(e.target) && closeMenu(), { capture: true, passive: true });

// Live value badges: after each edit, a beat later, the sheet is traced off
// the main thread and each wire shows the value it carried during the first
// call of its definition. Tracing is optional: if it fails or isn't
// available, the drawing has no badges, and the status beside the Values
// switch says why.

const TRACE_LIMIT_MS = 4000;
const FIRST_CALL = "Badges show the value each wire carried during the first call of its definition.";

let traceTimer;
/** The last trace of the editor's text: the revision it traced, and its values. */
let traced = { rev: -1, values: [] };

// One trace at a time. A result for a text since edited is dropped: the
// newer text's trace is waiting, or is about to be asked for.
const tracer = new Flight({
  spawn: () => new Worker("worker.js", { type: "module" }),
  limit: TRACE_LIMIT_MS,
  current: () => rev,
  settle: (job, outcome) => {
    if (outcome.broken !== undefined) {
      traced = { rev: -1, values: [] };
      showTrace("unavailable", `Tracing failed to start${outcome.broken ? `: ${outcome.broken}` : ""}.`);
    } else if (outcome.timedOut) {
      traced = { rev: job.rev, values: [] };
      showTrace("unavailable", `Tracing took longer than ${TRACE_LIMIT_MS / 1000} s, so it was stopped.`);
    } else {
      traced = { rev: job.rev, values: outcome.failed ? [] : outcome.wires };
      showTraced(outcome);
    }
    showOverlays();
  },
});

/** Says how the last trace went, with a jump to a runtime error. */
function showTraced({ failed, error, stopped }) {
  if (failed) return showTrace("unavailable", `Tracing failed: ${failed}`);
  // A static error means the sheet couldn't run at all; runtime codes are R.
  if (error && !error.code.startsWith("R")) return showTrace("unavailable", "The sheet has errors, so it can’t run.");
  if (error) return showTrace("runtime error", `${error.code}: ${error.message}. Wires after it have no value.`, error);
  if (stopped) return showTrace("stopped at step budget", "The run hit its step budget, so wires it hadn’t reached have no value.");
  showTrace("complete");
}

function showTrace(state, detail = "", at = null) {
  const title = [FIRST_CALL, detail].filter(Boolean).join(" ");
  layer.say(traceState, title, { title: true });
  if (!at) return void traceState.replaceChildren(state);
  // One-based, like the diagnostics.
  const jump = el("button", "link", `runtime error ${at.row + 1}:${at.col + 1}`);
  jump.type = "button";
  layer.say(jump, `${title} Click to show where it failed.`, { title: true });
  jump.addEventListener("click", () => jumpTo(at));
  traceState.replaceChildren(jump);
}

const untraceable = () => !values.checked || shown.some((d) => d.severity === "error");

function scheduleTrace() {
  clearTimeout(traceTimer);
  if (untraceable()) return requestTrace();
  showTrace("computing…");
  traceTimer = setTimeout(requestTrace, 250);
}

function requestTrace() {
  if (untraceable()) {
    traced = { rev, values: [] };
    // Nothing asked for before is wanted now, not even the trace in flight.
    tracer.cancel();
    if (values.checked) showTrace("unavailable", "The sheet has errors, so it can’t run.");
    else showTrace("");
    return showOverlays();
  }
  showTrace("computing…");
  tracer.request({ kind: "trace", src: editor.value, rev });
}

/** The cell a wire's badge sits on: the middle of its longest straight row. */
function anchor(cells) {
  let best = null;
  let run = [];
  const flush = () => {
    if (run.length && (!best || run.length > best.length)) best = run;
    run = [];
  };
  for (const c of cells) {
    const last = run[run.length - 1];
    if (last && (last.row !== c.row || last.col + 1 !== c.col)) flush();
    if (charAt(c) === "─") run.push(c);
    else flush();
  }
  flush();
  return best ? best[Math.floor((best.length - 1) / 2)] : cells[0];
}

function showBadges(svg) {
  if (!values.checked || traced.rev !== rev) return;
  const { wires: list } = current();
  const group = svgEl("g", "overlay badges");
  svg.append(group);
  for (const { wire, value } of traced.values) {
    const w = list[wire];
    if (!w) continue;
    const badge = svgEl("g", "badge");
    const label = svgEl("text", "", {
      ...geo.centre(anchor(w.cells)),
      "text-anchor": "middle",
      "dominant-baseline": "central",
    });
    label.textContent = value;
    const title = svgEl("title");
    title.textContent = value;
    badge.append(title, label);
    group.append(badge);
    const box = label.getBBox();
    const pad = { x: 140, y: 40 };
    badge.prepend(svgEl("rect", "", {
      x: box.x - pad.x, y: box.y - pad.y,
      width: box.width + 2 * pad.x, height: box.height + 2 * pad.y,
      rx: (box.height + 2 * pad.y) / 2,
    }));
  }
}

values.addEventListener("change", () => {
  store.set("values", values.checked);
  requestTrace();
});

document.addEventListener("selectionchange", () => {
  if (document.activeElement !== editor) return;
  const { row, col } = posOf(editor.value, editor.selectionStart);
  $("cursor").textContent = `${row + 1}:${col + 1}`;
});

// Running happens in a worker, so a program that never stops can be cut off.
// Its output keeps the revision it ran; once the text moves on, the output
// is dimmed and says so, and its error no longer links into the text.

const runner = new Flight({
  spawn: () => new Worker("worker.js", { type: "module" }),
  limit: RUN_LIMIT_MS,
  settle: finish,
});
/** The revision the output shows the run of, or null when it shows none. */
let outputRev = null;
let outputMs = "";

function setRunning(on) {
  runButton.textContent = on ? "Stop" : "Run";
  runButton.classList.toggle("running", on);
  showOutputAge();
}

/** Says, beside the output, whether it is running, took how long, or describes an earlier text. */
function showOutputAge() {
  const stale = outputRev !== null && outputRev !== rev;
  output.classList.toggle("stale", stale);
  const link = output.querySelector("button.failure");
  if (link) {
    link.disabled = stale;
    link.title = stale ? "From an earlier version of the text, so it no longer points into it" : "Show where it failed";
  }
  $("output-meta").textContent = runner.busy
    ? "running…"
    : [outputMs, stale && "from an earlier version"].filter(Boolean).join(" · ");
}

/** Shows a message in place of output. */
function say(text, className = "hint") {
  outputRev = null;
  outputMs = "";
  output.replaceChildren(layer.say(el("span", className), text));
  showOutputAge();
}

function runProgram() {
  if (runner.busy) return stop("Stopped.");
  runner.request({ kind: "run", src: editor.value, rev });
  setRunning(true);
}

function finish(job, result) {
  setRunning(false);
  if (result.timedOut) {
    runner.warm();
    return say(`Stopped after ${RUN_LIMIT_MS / 1000} seconds.`);
  }
  if (result.broken !== undefined) {
    // The next Run starts a fresh worker; starting one here could loop on
    // a module that never loads.
    return say(`The runner failed${result.broken ? ` (${result.broken})` : ""}. Run starts a fresh one.`, "failure");
  }
  if (result.failed) {
    runner.warm();
    return say(`The runner crashed (${result.failed}). It has been restarted.`, "failure");
  }
  if (result.error) {
    // The wasm text is zero-based, as in every dump; the page counts from
    // one, like the diagnostics and the CLI's report.
    const { at } = result;
    const text = at ? `error ${at.row + 1}:${at.col + 1} ${at.code}: ${at.message}` : result.error;
    if (at) {
      const failure = layer.say(el("button", "failure"), text);
      failure.type = "button";
      failure.addEventListener("click", () => jumpTo(at));
      output.replaceChildren(failure);
    } else {
      output.replaceChildren(layer.say(el("span", "failure"), text));
    }
  } else if (result.diagnostics.some((d) => d.severity === "error")) {
    say("The sheet has errors, so it can’t run yet. See Diagnostics.");
  } else if (!result.outputs.length) {
    say("·𐑥𐑱𐑯 has no outputs.");
  } else {
    output.textContent = result.outputs.join("\n");
    if (job.rev === rev) tour.ran(current());
  }
  outputRev = job.rev;
  outputMs = `${result.ms < 1 ? "under 1" : Math.round(result.ms)} ms`;
  showOutputAge();
}

function stop(message) {
  runner.stop();
  runner.warm();
  setRunning(false);
  say(message);
}

runButton.addEventListener("click", runProgram);
editor.addEventListener("keydown", (e) => {
  if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
    e.preventDefault();
    runProgram();
  }
});

// Format, theme, examples.

let noteTimer;
function note(text) {
  $("note").textContent = text;
  clearTimeout(noteTimer);
  noteTimer = setTimeout(() => ($("note").textContent = ""), 2500);
}

$("format").addEventListener("click", () => {
  const formatted = doc().formatted();
  if (formatted === undefined) return note("Fix the character errors first.");
  if (formatted === editor.value) return note("Already canonical.");
  const caret = posOf(editor.value, editor.selectionStart);
  replaceAll(formatted);
  settle();
  jumpTo(caret, false);
  note("Formatted.");
});

$("theme").addEventListener("click", () => {
  const theme = document.documentElement.dataset.theme === "dark" ? "light" : "dark";
  document.documentElement.dataset.theme = theme;
  store.set("theme", theme);
  showDrawing();
});

/** `03-double` as the picker shows it: `03 · double`. */
function label(name) {
  const [, number, rest] = name.match(/^(\d+)-(.*)$/) ?? [, "", name];
  return `${number ? `${number} · ` : ""}${rest.replace(/-ascii$/, " (ASCII)").replaceAll("-", " ")}`;
}

// The editor holds one slot at a time: an example's draft, or the tour's
// (drafts.js). Every keystroke saves into it, and opening a slot replaces
// the text as one undoable edit, so nothing typed is ever lost: it is in
// its slot's draft, and an undo brings it back.

let slot = null;
const originals = new Map();
/** Each slot opening, so an undo past one returns to the slot it left. */
const opened = [];
let opening = 0;

const baseOf = (name) => (name === TOUR_SLOT ? TOUR_EXAMPLE : name);
const optionLabel = (name, edited) =>
  `${name === TOUR_SLOT ? `Tour · ${label(TOUR_EXAMPLE)}` : label(name)}${edited ? " · edited" : ""}`;

async function fetchOk(url, as = "text") {
  const r = await fetch(url);
  if (!r.ok) throw new Error(`${url}: ${r.status} ${r.statusText}`);
  return r[as]();
}

async function originalOf(name) {
  if (!originals.has(name)) originals.set(name, await fetchOk(`examples/${name}.shaw`));
  return originals.get(name);
}

function saveDraft() {
  if (!slot) return;
  if (store.saveDraft(slot, editor.value, originals.get(baseOf(slot)))) {
    storageWarned = false;
  } else if (!storageWarned) {
    storageWarned = true;
    note("Couldn’t save a draft: browser storage is full or off. Edits last until you leave.");
  }
  showEdited();
}

/** Shows whether the slot differs from its original, by the picker. */
function showEdited() {
  const base = slot && baseOf(slot);
  const edited = Boolean(slot) &&
    (originals.has(base) ? editor.value !== originals.get(base) : store.draft(slot) !== null);
  resetButton.hidden = !edited;
  const option = [...examples.options].find((o) => o.value === slot);
  if (option) option.textContent = optionLabel(slot, edited);
}

/** Makes `next` the slot the editor saves into, without touching the text. */
function enter(next) {
  if (next !== TOUR_SLOT) tour.stop();
  slot = next;
  store.set("example", next);
  examples.value = next;
  // The tour has no link of its own: a reload returns to it by `example`.
  history.replaceState(null, "", next === TOUR_SLOT ? location.pathname + location.search : `#${next}`);
  showEdited();
}

/**
 * Opens a slot: its draft, or with `fresh` (or no draft) its example's
 * original. `initial` is the page's first text, which has nothing to undo
 * to. False when the text couldn't be loaded; then nothing changes.
 */
async function openSlot(next, { fresh = false, initial = false } = {}) {
  const gen = ++opening;
  const original = await originalOf(baseOf(next)).catch(() => undefined);
  if (gen !== opening) return false;
  const draft = fresh ? null : store.draft(next);
  const text = draft ?? original;
  if (text === undefined) {
    note(`Couldn’t load ${label(baseOf(next))}.`);
    examples.value = slot ?? "";
    return false;
  }
  const from = { slot, text: editor.value };
  enter(next);
  say("Run the program to see what ·𐑥𐑱𐑯 returns.");
  if (initial) {
    editor.value = text;
    settle();
    changed();
  } else {
    const back = document.activeElement;
    const ring = back?.matches(":focus-visible");
    replaceText(text, { row: 0, col: 0 });
    // Only a real edit leaves an undo step to follow back.
    if (from.text !== text) opened.push({ from, to: { slot: next, text } });
    if (opened.length > 50) opened.shift();
    // Arrowing through the picker must keep working.
    if (back && back !== document.body) back.focus({ preventScroll: true, focusVisible: ring });
  }
  if (draft !== null) note("Your draft is back. Reset loads the original.");
  return true;
}

/**
 * Follows an undo or redo across a slot opening: the text is then the
 * other slot's, so the editor must save into that slot again.
 */
function followHistory(inputType) {
  const undo = inputType === "historyUndo";
  if (!undo && inputType !== "historyRedo") return;
  for (let i = opened.length - 1; i >= 0; i--) {
    const [here, there] = undo ? [opened[i].to, opened[i].from] : [opened[i].from, opened[i].to];
    if (here.slot === slot && there.text === editor.value) {
      if (there.slot !== slot) enter(there.slot);
      return;
    }
  }
}

editor.addEventListener("input", (e) => {
  followHistory(e.inputType);
  changed();
});

examples.addEventListener("change", () => openSlot(examples.value));

resetButton.addEventListener("click", async () => {
  if (!(await openSlot(slot, { fresh: true }))) return;
  editor.focus({ preventScroll: true });
  note("Original loaded. Undo brings your edits back.");
});

async function startTour(initial = false) {
  if (await openSlot(TOUR_SLOT, { fresh: true, initial })) tour.start(current());
}

$("tour-start").addEventListener("click", () => startTour());

async function main() {
  try {
    await init();
  } catch (e) {
    drawing.replaceChildren(el("p", "hint", "The playground needs WebAssembly, which didn’t load."));
    throw e;
  }
  readings.checked = store.get("readings") !== "false";
  layer = new Layer({ on: readings.checked, rowPitch: row_pitch, annotate, changed: relayer });
  geo = layer.geometry;
  trans = new Transliterator();
  const entries = table();
  keys = new Set(["/", ...entries.flatMap(([k]) => [...k])]);
  $("keys").replaceChildren(
    ...entries.map(([k, letter, name]) => {
      const item = el("li");
      item.append(el("span", "letter", letter), el("span", "keys", k), el("span", "name", name));
      return item;
    }),
  );
  translit.checked = store.get("translit") !== "false";
  values.checked = store.get("values") !== "false";
  runner.warm();

  let names = [];
  let listed = true;
  try {
    names = await fetchOk("examples/index.json", "json");
  } catch {
    listed = false;
  }
  store.migrate(names);
  const stopped = store.setAsideStopped();
  const last = store.get("example");
  const options = names.map((name) => new Option(optionLabel(name, store.draft(name) !== null), name));
  if (!listed) {
    // The picker still names what is open, and says why it holds nothing else.
    const failed = new Option("Examples didn’t load", "");
    failed.disabled = true;
    options.push(failed);
    if (last && last !== TOUR_SLOT) options.push(new Option(optionLabel(last, store.draft(last) !== null), last));
  }
  // The tour's slot is in the picker only to name it while it's open.
  const tourOption = new Option(optionLabel(TOUR_SLOT, false), TOUR_SLOT);
  tourOption.hidden = true;
  examples.replaceChildren(...options, tourOption);
  addEventListener("hashchange", () => {
    const name = location.hash.slice(1);
    if (names.includes(name) && name !== slot) openSlot(name);
  });

  // A link opens its example, with the draft if there is one; otherwise
  // the slot open last time. A first visit starts the tour.
  const linked = names.find((n) => `#${n}` === location.hash);
  if (linked) await openSlot(linked, { initial: true });
  else if (last === TOUR_SLOT || names.includes(last) || (last && store.draft(last) !== null)) {
    await openSlot(last, { initial: true });
  } else {
    await startTour(true);
  }
  if (!listed) note("Couldn’t load the list of examples.");
  if (stopped !== null) offerStopped(stopped);
}

/**
 * Says that `stopped`'s draft stopped the last page while it was being
 * drawn, and offers it back: to open anyway, or to download and inspect.
 */
function offerStopped(stopped) {
  const text = store.stopped(stopped);
  if (text === null) return;
  const open = el("button", "", "Open the draft anyway");
  open.type = "button";
  open.addEventListener("click", async () => {
    if (store.restoreStopped(stopped)) await openSlot(stopped);
  });
  const download = el("a", "", "download it");
  download.href = URL.createObjectURL(new Blob([text], { type: "text/plain" }));
  download.download = `${baseOf(stopped)}.shaw`;
  say(`The page stopped while drawing your draft of ${label(baseOf(stopped))}, so the original is open. `);
  output.append(open, " or ", download, ".");
}

main();
