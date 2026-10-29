// The Shawline playground: an editor, a live drawing, diagnostics, and a
// Run button. All language work happens in the wasm package; this file only
// wires it to the page.
import init, {
  Doc, table, apply as applyEdit, cell_of_offset, offset_of_cell,
  Transliterator, annotate, reading, keys as keysOf, panic_message,
} from "./pkg/shawline_wasm.js";
import { Tour, TOUR_EXAMPLE } from "./tour.js";
import { Store, TOUR_SLOT } from "./drafts.js";
import { Docs } from "./docs.js";
import { Flight } from "./flight.js";
import { Layer } from "./layer.js";
import {
  Studio, KEYS, SCRUB_PX, readView, anchorOf, sameTarget, keyOf, same, sourceAt, sinkAt, wireAt, boxAt,
} from "./studio.js";
import { Choices, offer, placing } from "./palette.js";
import { ZOOM_STEP, clampZoom, defaultZoom, fitZoom, zoomAbout, cellUnder } from "./view.js";

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

const store = new Store((() => {
  // Reading `localStorage` itself throws where storage is disabled.
  try {
    return localStorage;
  } catch {
    return null;
  }
})());
const reducedMotion = matchMedia("(prefers-reduced-motion: reduce)");

// Positions are cells, as in every Shawline dump, but the textarea counts
// UTF-16 units: wasm converts between them (`cell_of_offset`,
// `offset_of_cell`), byte-order mark and surrogate pairs included.

const isHigh = (unit) => unit >= 0xd800 && unit <= 0xdbff;
const isLow = (unit) => unit >= 0xdc00 && unit <= 0xdfff;

// A leading byte-order mark isn't a cell (shawline-grid skips it).
const BOM = "﻿";

/** The text's rows, without a leading byte-order mark. */
const rowsOf = (text) => (text.startsWith(BOM) ? text.slice(1) : text).split("\n");

/** Puts the caret at a cell, or selects the cell itself. */
function jumpTo({ row, col }, select = true) {
  const text = editor.value;
  const at = offset_of_cell(text, row, col);
  const end = select && at < text.length && text[at] !== "\n" ? offset_of_cell(text, row, col + 1) : at;
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
    if (drag.kind === "wire" || drag.kind === "sink" || (drag.kind === "branch" && drag.view)) glow();
    if (drag.at && drag.kind === "box") showGhost();
    else if (drag.at && drag.kind !== "scrub" && drag.kind !== "pan") showPath();
  }
  if (menu) {
    menu.show();
    placeMenu();
  }
  showInspector();
  // A peek or a turn of the switch is what the tour's first step asks for;
  // a peek counts once it is over.
  if (layer.peeking) tour.update(current());
  else tour.looked(current());
}

// Live checking and drawing, a beat after the last keystroke.

let refreshTimer;
let shown = [];
/** Whether an undo happened since the last refresh, for the tour. */
let undoPending = false;
let storageWarned = false;
/**
 * The editor's revision, counted up on every change to its text. Run's
 * output keeps the one it ran.
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
  withdrawRoom();
  showOutputAge();
  clearTimeout(refreshTimer);
  refreshTimer = setTimeout(refresh, 120);
}

function refresh() {
  // Reading and drawing run here, unbounded: a sheet that freezes the page
  // leaves the mark, and the next load opens the original instead.
  const marked = Boolean(slot) && store.markDrawing(slot);
  shown = doc().diagnostics();
  if (undoPending) tour.undone(current());
  else tour.update(current());
  undoPending = false;
  // A selection made on other text (an undo, typing) no longer names anything.
  if (studio && studio.selection && !studio.held && !selectionFits()) studio.select(null, { quiet: true });
  showProblems();
  showDrawing();
  scheduleTrace();
  if (marked) store.unmarkDrawing();
}

/** Whether the selection still names something on the shown sheet. */
function selectionFits() {
  const target = studio.selection;
  const view = current();
  if (target.kind === "box") {
    const { top, left, bottom, right } = target.box;
    return view.boxes.some((b) => b.top === top && b.left === left && b.bottom === bottom && b.right === right);
  }
  if (target.kind === "wire") {
    const wire = view.wires[target.wire.wire];
    return Boolean(wire) && same(wire.cells[0], target.wire.cells[0]) && wire.cells.length === target.wire.cells.length;
  }
  return sameTarget(studio.targetAt(anchorOf(target), view), target);
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
  statusLine.textContent = `Something went wrong inside Studio (${message}). Your text is saved; reload the page to continue.`;
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

// Where cells sit (PLAN P10, Geometry and positions). wasm's `Geometry`
// owns cell sizes and the row pitch: `geo` at the drawing's zoom hit-tests
// pointer events, and `unit`, at zoom 1, places overlays. Overlays are drawn
// inside the rendered <svg>, in its own units, so `frame` carries `unit`'s
// pixels into them, read off the <svg>'s viewBox; a zoom then only resizes
// the <svg>.

let zoom = null;
let geo = null;
let unit = null;
let frame = { x: 0, y: 0, k: 1 };
/** The text the drawing shows. */
let drawn = null;

/**
 * Draws `src` at the drawing's zoom, glossed while the layer shows; false
 * when the text can't be read, and then the drawing and its geometry stay
 * as they were.
 */
function drawSheet(src) {
  const sheet = docs.get(src);
  const svg = sheet.render(document.documentElement.dataset.theme === "dark", layer.shown);
  if (svg === undefined) return false;
  const kept = [...drawing.querySelectorAll(":scope > .keep")];
  drawing.innerHTML = svg;
  drawing.append(...kept);
  unit?.free();
  unit = sheet.geometry(layer.shown, 1);
  const view = drawing.querySelector("svg").viewBox.baseVal;
  const size = unit.size();
  frame = { x: view.x, y: view.y, k: view.width / size.width };
  zoom ??= defaultZoom(unit.cell_rect(0, 0, 0, 0, false, false).width);
  drawn = src;
  sizeDrawing();
  return true;
}

/** Sizes the drawing for the zoom, and remakes the hit-testing geometry. */
function sizeDrawing() {
  const svg = drawing.querySelector("svg");
  if (!svg || drawn === null) return;
  geo?.free();
  geo = docs.get(drawn).geometry(layer.shown, zoom);
  const { width, height } = geo.size();
  Object.assign(svg.style, { width: `${width}px`, height: `${height}px` });
  $("zoom-level").textContent = `${Math.round(zoom * 100)}%`;
  if (menu) placeMenu();
  if (inline) placeInline();
}

/** Zooms to `next`, keeping the drawing's point under `pointer` (client pixels) still. */
function zoomTo(next, pointer) {
  next = clampZoom(next);
  if (!zoom || next === zoom) return;
  const host = drawing.getBoundingClientRect();
  const at = pointer
    ? { x: pointer.x - host.left, y: pointer.y - host.top }
    : { x: drawing.clientWidth / 2, y: drawing.clientHeight / 2 };
  const scroll = zoomAbout(zoom, next, { left: drawing.scrollLeft, top: drawing.scrollTop }, at);
  zoom = next;
  sizeDrawing();
  drawing.scrollLeft = scroll.left;
  drawing.scrollTop = scroll.top;
}

function zoomBy(how) {
  if (how === "fit") {
    if (!unit) return;
    return void zoomTo(fitZoom(unit.size(), { width: drawing.clientWidth, height: drawing.clientHeight }));
  }
  zoomTo(how === "in" ? zoom * ZOOM_STEP : zoom / ZOOM_STEP);
}

$("zoom-in").addEventListener("click", () => zoomBy("in"));
$("zoom-out").addEventListener("click", () => zoomBy("out"));
$("zoom-fit").addEventListener("click", () => zoomBy("fit"));

// Ctrl or Cmd with the wheel zooms about the pointer. A trackpad pinch
// arrives the same way, in small steps, and Safari's as gesture events; a
// mouse notch is capped so it zooms by under two-thirds.
drawing.addEventListener("wheel", (e) => {
  if (!(e.ctrlKey || e.metaKey)) return;
  e.preventDefault();
  zoomTo(zoom * Math.exp(-Math.max(-50, Math.min(50, e.deltaY)) * 0.01), { x: e.clientX, y: e.clientY });
}, { passive: false });
let pinched = null;
drawing.addEventListener("gesturestart", (e) => {
  e.preventDefault();
  pinched = zoom;
});
drawing.addEventListener("gesturechange", (e) => {
  e.preventDefault();
  if (pinched) zoomTo(pinched * e.scale, { x: e.clientX, y: e.clientY });
});
drawing.addEventListener("gestureend", () => (pinched = null));

/** Space held: a drag pans instead of editing. */
let spaceHeld = false;
drawing.addEventListener("keyup", (e) => {
  if (e.key === " ") spaceHeld = false;
  if (studio.keyup(e)) e.preventDefault();
});
drawing.addEventListener("blur", () => {
  spaceHeld = false;
  // A nudge whose key came up elsewhere still ends.
  if (studio.nudging) {
    studio.nudging = null;
    studio.release();
  }
});

function showDrawing() {
  if (!drawSheet(studio?.text ?? editor.value)) {
    // Unreadable text (a tab, a stray character): keep the last drawing, faded.
    drawing.querySelector("svg")?.classList.add("stale");
    return;
  }
  const background = drawing.querySelector("svg > rect");
  if (!studio?.held) {
    background?.after(
      ...shown.map((d, i) => {
        const cell = cellRect(`cell ${d.severity}`, d);
        cell.dataset.i = i;
        return cell;
      }),
    );
  }
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
  view.words ??= docs.get(view.src).glosses();
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
  if (!svg || !geo) return null;
  return cellUnder(geo, e.clientX, e.clientY, svg.getBoundingClientRect());
}

// The drawing as an editor (Studio Phases 1 and 2). Every gesture is one
// checked `Edit`, sent to wasm's `apply` through the Studio (studio.js) and
// applied through the textarea so undo works as it does for typing; a
// refused edit changes nothing and says why beside the cell it is about,
// and in the status line under the drawing.
//
// - Drag from a source (a node's ├, an input, a constant, an arm's input)
//   or from a wire: release on a free arrowhead to connect, or on blank
//   sheet to pick a node from the palette, placed there and wired in.
// - Drag from a free arrowhead to a source or a wire to connect, or onto
//   blank sheet to pick a source for it: a number, a text or a callee.
// - Drag a wired arrowhead away: its branch comes off. Release on a source
//   or a wire to rewire it there, anywhere else to leave it off.
// - Drag anything else on a box to move the box; the ghost says at once
//   whether the move would be refused.
// - Click to select; click a source and then an arrowhead to connect them.
//   A selected number constant scrubs: drag its digits sideways.
// - The keyboard model is the Studio's: `?` lists every key.

const SVG = "http://www.w3.org/2000/svg";
const LOCKED = "This wire can't be edited by mouse yet; edit the text.";

let dragged = false;
let pressed = null;
let drag = null;
let menu = null;
/** The Studio (studio.js), made once the wasm package has loaded. */
let studio = null;

const viewOf = (src) => docs.view(src, readView);
const current = () => viewOf(studio?.text ?? editor.value);

const sourceOf = (at, view = current()) => sourceAt(view, at);
const sinkOf = (at, view = current()) => sinkAt(view, at);
const wireOf = (at, view = current()) => wireAt(view, at);
/** A wire runs through the cell, but none the mouse can edit. */
const lockedAt = (at) => (current().byCell.get(keyOf(at))?.length ?? 0) > 0 && !wireOf(at);
const charAt = ({ row, col }) => [...(rowsOf(studio?.text ?? editor.value)[row] ?? "")][col] ?? " ";

drawing.addEventListener("click", (e) => {
  // The click that ends a drag isn't a request for the caret.
  if (dragged) return void (dragged = false);
  const at = cellAt(e);
  // Pressed on one cell and let go on another isn't a click on either.
  if (!at || !same(at, pressed)) return;
  const target = studio.targetAt(at);
  const chosen = studio.selection;
  // A source and then an arrowhead, or the other way round, connect.
  const sink = target && (target.kind === "sink" ? target.sink : null);
  if (chosen?.kind === "source" && sink && sink.scope === chosen.source.scope) {
    return void studio.edit({ kind: "connect", from: cellOf(chosen.source), to: cellOf(sink) });
  }
  if (chosen?.kind === "sink" && target?.kind === "source" && target.source.scope === chosen.sink.scope) {
    return void studio.edit({ kind: "connect", from: cellOf(target.source), to: cellOf(chosen.sink) });
  }
  if (target && !(target.kind === "box" && lockedAt(at))) {
    studio.select(target);
    focusDrawing(false);
    return;
  }
  studio.select(null);
  studio.cursor = at;
  showOverlays();
  // On a character (a locked wire, a border) the caret goes to it, since
  // the text is where it can be edited. A click on blank sheet only
  // deselects, so focus stays here and Delete can't reach the text.
  if (charAt(at) !== " ") jumpTo(at, false);
  else focusDrawing(false);
});

const cellOf = ({ row, col }) => ({ row, col });

drawing.addEventListener("dblclick", (e) => {
  const at = cellAt(e);
  const box = at && boxAt(current(), at);
  if (at && charAt(at) === " " && box && box.kind !== "node") openMenu(at, { mode: "place" });
});

function svgEl(tag, className, attrs = {}) {
  const node = document.createElementNS(SVG, tag);
  Object.entries(attrs).forEach(([k, v]) => node.setAttribute(k, v));
  if (className) node.setAttribute("class", className);
  return node;
}

/** A cell rectangle's area in the <svg>'s units; `under` and `over` carry it through the strips beside it. */
function areaOf({ top, left, bottom, right, under = false, over = false }) {
  const a = unit.cell_rect(top, left, bottom, right, Boolean(under), Boolean(over));
  return { x: frame.x + a.x * frame.k, y: frame.y + a.y * frame.k, width: a.width * frame.k, height: a.height * frame.k };
}

function svgRect(className, r) {
  return svgEl("rect", className, { ...areaOf(r), rx: 90 });
}

const cellRect = (className, { row, col, down }, span = 1) =>
  svgRect(className, { top: row, left: col, bottom: row, right: col + span - 1, under: down });
/** A cell's centre in the <svg>'s units. */
const pointOf = ({ row, col }) => {
  const { x, y } = unit.cell_centre(row, col);
  return { x: frame.x + x * frame.k, y: frame.y + y * frame.k };
};
const centre = (at) => {
  const { x, y } = pointOf(at);
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

/** The tour's rings, the cursor, the selection, `W`'s glowing ends and the
 * value badges, drawn again over every fresh drawing. */
function showOverlays() {
  const svg = drawing.querySelector("svg");
  // A detached branch is being dragged over a sheet that isn't the text yet.
  if (!svg || drag?.view || !unit) return;
  svg.querySelectorAll(".overlay").forEach((n) => n.remove());
  if (!studio?.held) svg.append(...tour.rings().map((r) => svgRect("overlay beacon", r)));
  const selected = studio?.selection;
  if (studio?.cursor && drawing.matches(":focus") && !selected) svg.append(cellRect("overlay cursor", studio.cursor));
  if (selected?.kind === "box") svg.append(svgRect("overlay selected", selected.box));
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
  if (selected?.kind === "source" || selected?.kind === "text") {
    const scrub = studio.scrubbable ? " scrub" : "";
    svg.append(cellRect(`overlay selected-text${scrub}`, selected.at, selected.at.span));
  }
  if (selected?.kind === "sink") svg.append(cellRect("overlay selected-text", selected.sink));
  if (studio?.wiring) {
    const { ends } = studio.wiring;
    const end = studio.end;
    svg.append(...ends.map((s) => cellRect(same(s, end) ? "overlay glow chosen" : "overlay glow", s, s.span ?? 1)));
    const from = studio.wiring.from;
    const anchor = from.kind === "source" ? from.source : from.sink;
    const found = docs.get(current().src).preview(from.kind !== "source", anchor.row, anchor.col, end.row, end.col);
    svg.append(svgEl("polyline", found ? "overlay wire-preview" : "overlay wire-preview loose", {
      points: (found ? found.path : [anchor, end]).map(centre).join(" "),
    }));
  }
  showBadges(svg);
}

/** Selection changed in the Studio: draw it, say it, and show it in the inspector. */
function selectionChanged() {
  if (drawn !== (studio.text)) showDrawing();
  else showOverlays();
  live.textContent = studio.describe();
  showInspector();
  const svg = drawing.querySelector("svg");
  svg?.classList.toggle("can-scrub", studio.scrubbable);
}

/**
 * Focuses the drawing with its ring only when `ring` says so. Focus moved
 * by script would otherwise inherit the editor's ring, and a mouse user
 * would see one after every click.
 */
function focusDrawing(ring) {
  drawing.focus({ preventScroll: true, focusVisible: ring });
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
  const blocked = drag.verdict ? " refused" : "";
  drag.ghost = (dr || dc) && at.top >= 0 && at.left >= 0 ? svgRect(`ghost${blocked}`, at) : null;
  if (drag.ghost) drawing.querySelector("svg")?.append(drag.ghost);
}

/**
 * A live verdict on a box drag: a trial move for each cell the pointer
 * reaches. The ghost is tinted, and the status says why, while the move
 * would be refused.
 */
function judgeMove() {
  const { box, dr, dc } = drag;
  drag.verdict = null;
  if (dr || dc) {
    const trial = applyEdit(doc(), { kind: "move", corner: { row: box.top, col: box.left }, dr, dc });
    if (trial.ok) {
      trial.map?.free();
      trial.doc.free();
    } else drag.verdict = trial;
  }
  if (drag.verdict) showStatus(drag.verdict.reason, "refused");
  else showStatus();
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
  const { view, pointerId, kind } = drag;
  hidePreview();
  drag.marks?.forEach((n) => n.remove());
  drag = null;
  drawing.classList.remove("dragging", "wiring", "panning");
  if (drawing.hasPointerCapture(pointerId)) drawing.releasePointerCapture(pointerId);
  if (kind === "scrub") studio.drop();
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

// Make room (PLAN P10, Decisions: No room). A refusal whose remedy is
// `make-room` is retried at once as `make_room`; only if that succeeds is
// Make room offered, by the refused cell, with the roomier sheet drawn as
// a ghost. One click writes it as one undo step.

let room = null;

function refused(refusal, edit) {
  refuse(refusal);
  if (refusal.remedy !== "make-room" || !edit || studio.held || drag) return;
  withdrawRoom();
  const base = studio.text;
  const roomed = applyEdit(docs.get(base), { kind: "make_room", edit });
  if (!roomed.ok) return;
  const ghost = el("div", "room-ghost keep");
  ghost.innerHTML = roomed.doc.render(document.documentElement.dataset.theme === "dark", layer.shown) ?? "";
  const g = roomed.doc.geometry(layer.shown, zoom);
  const svg = ghost.querySelector("svg");
  const size = g?.size();
  g?.free();
  if (svg && size) Object.assign(svg.style, { width: `${size.width}px`, height: `${size.height}px` });
  const own = drawing.querySelector("svg");
  ghost.style.left = `${own?.offsetLeft ?? 0}px`;
  ghost.style.top = `${own?.offsetTop ?? 0}px`;
  const button = el("button", "make-room keep", "Make room");
  button.type = "button";
  const added = roomed.inserted.map((i) => `${i.count} ${i.axis}${i.count > 1 ? "s" : ""}`).join(" and ");
  button.title = added ? `Insert ${added} so this fits` : "Insert room so this fits";
  button.addEventListener("pointerdown", (e) => e.stopPropagation());
  button.addEventListener("pointerenter", () => ghost.classList.add("shown"));
  button.addEventListener("pointerleave", () => ghost.classList.remove("shown"));
  button.addEventListener("focus", () => ghost.classList.add("shown"));
  button.addEventListener("blur", () => ghost.classList.remove("shown"));
  button.addEventListener("click", (e) => {
    e.stopPropagation();
    const offer = room;
    room = null;
    ghost.remove();
    button.remove();
    if (offer.base !== studio.text) return void offer.result.map?.free();
    studio.commit(studio.settle(offer.result), offer.base);
    focusDrawing(false);
  });
  place(button, { row: refusal.row + 1, col: refusal.col });
  drawing.append(ghost, button);
  room = { base, result: roomed, ghost, button };
}

/** Takes back an offer of room: the next action makes it stale. */
function withdrawRoom() {
  if (!room) return;
  room.ghost.remove();
  room.button.remove();
  room.result.map?.free();
  room.result.doc.free();
  room = null;
}

/** A cell's top-left corner in viewport pixels. */
function clientOf({ row, col }) {
  const svg = drawing.querySelector("svg");
  const box = svg.getBoundingClientRect();
  const { x, y } = geo.cell_rect(row, col, row, col, false, false);
  return { x: x + box.left, y: y + box.top };
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
  const at = offset_of_cell(editor.value, caret.row, caret.col);
  editor.setSelectionRange(at, at);
  scrollTo(scroll.x, scroll.y);
  clearTimeout(refreshTimer);
  refresh();
}

/** Writes a drawing edit's text as one undo step, keeping the caret's cell. */
function writeText(text) {
  const ring = drawing.matches(":focus-visible");
  replaceText(text, cell_of_offset(editor.value, editor.selectionStart));
  focusDrawing(ring);
}

// Dragging.

drawing.addEventListener("pointerdown", (e) => {
  // One drag at a time: a second finger is ignored.
  if (drag) return;
  const panning = e.button === 1 || (e.button === 0 && spaceHeld);
  if (e.button !== 0 && !panning) return;
  // A drag isn't always followed by a click, so its flag mustn't outlive
  // it and swallow the next real click.
  dragged = false;
  closeMenu();
  closeInline();
  withdrawRoom();
  showStatus();
  if (panning) {
    e.preventDefault();
    drawing.setPointerCapture(e.pointerId);
    drag = { kind: "pan", pointerId: e.pointerId, x: e.clientX, y: e.clientY, left: drawing.scrollLeft, top: drawing.scrollTop };
    drawing.classList.add("panning");
    return;
  }
  const at = cellAt(e);
  pressed = at;
  if (!at) return;
  const view = current();
  const chosen = studio.selection;
  let start = null;
  // A selected number constant scrubs; an unselected one starts a wire.
  if (studio.scrubbable && chosen.at.row === at.row && at.col >= chosen.at.col && at.col < chosen.at.col + chosen.at.span) {
    if (studio.scrubStart()) start = { kind: "scrub", x: e.clientX };
  }
  const source = !start && sourceOf(at, view);
  const sink = !start && !source && sinkOf(at, view);
  const wire = !start && !source && !sink && wireOf(at, view);
  if (start) {
    // Chosen above.
  } else if (source) start = { kind: "wire", from: cellOf(source), scope: source.scope };
  else if (sink && !sink.wired) start = { kind: "sink", sink, scope: sink.scope };
  else if (sink) {
    // The branch comes off once the pointer leaves the arrowhead; a refusal
    // (its wire isn't editable) is better said on the press.
    const cut = applyEdit(doc(), { kind: "disconnect", at: cellOf(sink) });
    if (!cut.ok) return void refuse(cut);
    cut.map?.free();
    // Kept as the second revision: the drawing shows it once the branch is off.
    start = { kind: "branch", sink, scope: sink.scope, cut: docs.seed(cut.doc) };
  } else if (wire) {
    const from = sourceOf(wire.source, view) ?? wire.source;
    start = { kind: "wire", from: cellOf(from), scope: from.scope };
  } else if (lockedAt(at)) {
    return void refuse({ ...at, reason: LOCKED });
  } else {
    const box = boxAt(view, at);
    if (box) start = { kind: "box", box, dr: 0, dc: 0, ghost: null, verdict: null };
  }
  if (!start) return;
  e.preventDefault();
  // preventDefault keeps focus where it was; Escape must reach the drawing.
  focusDrawing(false);
  drawing.setPointerCapture(e.pointerId);
  drag = { ...start, start: at, at, pointerId: e.pointerId, moved: false };
  drawing.classList.add(drag.kind === "box" || drag.kind === "scrub" ? "dragging" : "wiring");
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

/** The source a drag from an arrowhead would connect to, under the pointer. */
function sourceUnder(at, view = current()) {
  const source = sourceOf(at, view);
  if (source) return source;
  const wire = wireOf(at, view);
  return wire && (sourceOf(wire.source, view) ?? wire.source);
}

/** The path `connect` would draw for the drag (wasm's `Preview`), or a loose line. */
function showPath() {
  const svg = drawing.querySelector("svg");
  if (!svg) return;
  const view = drag.view ?? current();
  const { at } = drag;
  const sheet = docs.get(view.src);
  let anchor;
  let found;
  if (drag.kind === "wire") {
    anchor = drag.from;
    const sink = sinkOf(at, view);
    const to = sink && !sink.wired && sink.scope === drag.scope ? sink : at;
    found = !same(to, anchor) && sheet.preview(false, anchor.row, anchor.col, to.row, to.col);
  } else {
    anchor = drag.sink;
    const source = sourceUnder(at, view);
    const to = source && source.scope === drag.scope ? source : at;
    found = sheet.preview(true, anchor.row, anchor.col, to.row, to.col);
  }
  drag.path?.remove();
  drag.path = svgEl("polyline", found ? "wire-preview" : "wire-preview loose", {
    points: (found ? found.path : [anchor, at]).map(centre).join(" "),
  });
  svg.append(drag.path);
}

drawing.addEventListener("pointermove", (e) => {
  if (drag?.kind === "pan") {
    drawing.scrollLeft = drag.left - (e.clientX - drag.x);
    drawing.scrollTop = drag.top - (e.clientY - drag.y);
    return;
  }
  if (drag?.kind === "scrub") {
    if (e.pointerId === drag.pointerId) {
      drag.moved ||= Math.abs(e.clientX - drag.x) >= SCRUB_PX;
      studio.scrubMove(e.clientX - drag.x, e.shiftKey);
    }
    return;
  }
  const at = cellAt(e);
  if (!drag) {
    const svg = drawing.querySelector("svg");
    const chosen = studio.selection;
    const scrub = at && studio.scrubbable && chosen.at.row === at.row && at.col >= chosen.at.col && at.col < chosen.at.col + chosen.at.span;
    const sink = at && sinkOf(at);
    const over = at && !scrub && (sourceOf(at) || sink?.wired === false || wireOf(at));
    const locked = at && !over && !sink && lockedAt(at);
    svg?.classList.toggle("over-scrub", Boolean(scrub));
    svg?.classList.toggle("over-source", Boolean(over));
    svg?.classList.toggle("over-sink", Boolean(sink?.wired));
    svg?.classList.toggle("over-locked", Boolean(locked));
    svg?.classList.toggle("over-box", Boolean(at && !scrub && !over && !sink && !locked && boxAt(current(), at)));
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
    judgeMove();
    showGhost();
  } else {
    showPath();
  }
});

drawing.addEventListener("pointerup", (e) => {
  if (!drag || e.pointerId !== drag.pointerId) return;
  const finished = drag;
  if (finished.kind === "pan") return void endDrag();
  if (finished.kind === "scrub") {
    drag = null;
    drawing.classList.remove("dragging");
    if (drawing.hasPointerCapture(finished.pointerId)) drawing.releasePointerCapture(finished.pointerId);
    dragged = finished.moved;
    return void studio.scrubEnd();
  }
  const at = cellAt(e);
  endDrag();
  // Off the sheet, or back where it started: nothing. A press that never
  // moved is a click, which selects.
  if (!at || same(at, finished.start)) return void (dragged ||= finished.moved || !at);
  dragged = true;
  if (finished.kind === "box") {
    studio.edit({ kind: "move", corner: { row: finished.box.top, col: finished.box.left }, dr: finished.dr, dc: finished.dc });
  } else if (finished.kind === "branch") {
    // One edit either way: off, or off and onto the new source.
    const { cut, sink } = finished;
    const source = sourceUnder(at, viewOf(cut));
    if (!source) {
      if (studio.edit({ kind: "disconnect", at: cellOf(sink) })) showStatus("Branch removed. Undo puts it back.");
      return;
    }
    studio.edit({ kind: "rewire", from: cellOf(source), to: cellOf(sink) });
  } else if (finished.kind === "sink") {
    const source = sourceUnder(at);
    if (source) return void studio.edit({ kind: "connect", from: cellOf(source), to: cellOf(finished.sink) });
    // On blank sheet: a source to feed it (Decisions: Constants by mouse).
    if (charAt(at) === " " && boxAt(current(), at)) return void openMenu(at, { mode: "source", to: finished.sink });
    refuse({ ...at, reason: "Drop on a source or a wire to connect this arrowhead, or on blank sheet for a new source." });
  } else {
    const { from } = finished;
    const sink = sinkOf(at);
    const wire = wireOf(at);
    if (sink) studio.edit({ kind: "connect", from, to: cellOf(sink) });
    else if (wire) {
      const reason = same(wire.source, from) || same(sourceOf(wire.source), sourceOf(from))
        ? "This wire already carries that value. Drop on an arrowhead to add a branch."
        : "That wire has a source of its own. Drop on a free arrowhead.";
      refuse({ ...at, reason });
    } else if (charAt(at) === " ") openMenu(at, { mode: "feed", from });
    else refuse({ ...at, reason: "Drop on a free arrowhead to connect, or on blank sheet for a new node." });
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
    studio.drop();
    const ring = drawing.matches(":focus-visible");
    editor.focus({ preventScroll: true });
    document.execCommand(e.shiftKey ? "redo" : "undo");
    focusDrawing(ring);
    return;
  }
  if (e.key === " ") {
    spaceHeld = true;
    e.preventDefault();
    return;
  }
  if (e.key === "Escape") {
    // The release that follows a cancelled drag isn't a click.
    if (drag) dragged = true;
    endDrag();
    if (helpShown()) return void (e.preventDefault(), toggleHelp(false));
    if (menu) return void (e.preventDefault(), closeMenu());
  }
  if (drag) return;
  if (studio.keydown(e)) e.preventDefault();
});

drawing.addEventListener("focus", () => {
  // The cursor shows while the drawing has focus.
  studio.cursor ??= { row: 0, col: 0 };
  showOverlays();
  live.textContent = studio.describe();
});

// The `?` overlay: every key the drawing takes.

function helpShown() {
  return !$("keys-help").hidden;
}

function toggleHelp(on = !helpShown()) {
  const help = $("keys-help");
  if (on && !help.childElementCount) {
    const list = el("dl");
    for (const [k, what] of KEYS) list.append(el("dt", "", k), el("dd", "", what));
    const close = el("button", "quiet", "Close");
    close.type = "button";
    close.addEventListener("click", () => toggleHelp(false));
    help.append(el("h3", "", "Keys in the drawing"), list, close);
  }
  help.hidden = !on;
  if (!on) focusDrawing(true);
}

$("help").addEventListener("click", () => toggleHelp());

// The inspector (PLAN P10, Decisions: Inspector): the selection described
// as editor hover describes it, with a field for its text that commits one
// `set_text`. Enter on a selected text opens the same field in place.

const inspector = $("inspector");
const live = $("drawing-live");

/** A description's Markdown as HTML: paragraphs, bold and code spans only. */
function markdown(text) {
  const out = [];
  for (const para of text.split(/\n\n+/)) {
    const p = el("p");
    for (const [i, piece] of para.split(/(\*\*[^*]+\*\*|`[^`]+`)/).entries()) {
      if (i % 2 === 0) p.append(piece);
      else if (piece.startsWith("**")) p.append(el("strong", "", piece.slice(2, -2)));
      else p.append(el("code", "", piece.slice(1, -1)));
    }
    out.push(p);
  }
  return out;
}

function showInspector() {
  if (!studio) return;
  const target = studio.selection;
  if (!target) {
    inspector.replaceChildren(el("p", "hint", "Select something with a click, or with ] and [, to see what it is."));
    return;
  }
  const parts = [layer.say(el("p", "what"), studio.describe(target))];
  const word = target.kind === "box" ? current().texts.find((t) => t.kind === "word" && t.row >= target.box.top && t.row <= target.box.bottom && t.col >= target.box.left && t.col <= target.box.right) : target.at;
  const said = word && docs.get(current().src).describe(word.row, word.col, layer.shown);
  if (said) parts.push(...markdown(said.text));
  const text = (target.kind === "source" || target.kind === "text") && target.text;
  if (text && text.kind !== "comment") {
    const label = el("label", "field");
    const input = el("input");
    input.value = text.content;
    input.setAttribute("aria-label", "Text");
    const name = el("span", text.kind === "number" ? "name scrub" : "name", text.kind === "number" ? "Number" : "Text");
    if (text.kind === "number") {
      name.title = "Drag sideways to change it; Shift for steps of ten";
      scrubbing(name);
    }
    input.addEventListener("keydown", (e) => {
      e.stopPropagation();
      if (e.key === "Enter") commitText(target, input.value);
      if (e.key === "Escape") input.value = text.content;
    });
    input.addEventListener("change", () => commitText(target, input.value));
    label.append(name, input);
    parts.push(label);
  }
  inspector.replaceChildren(...parts);
}

/** One `set_text` for a text's new content, from the inspector or in place. */
function commitText(target, content) {
  closeInline();
  if (content === target.text.content) return;
  studio.edit({ kind: "set_text", at: cellOf(target.at), content });
}

/** The inspector's number scrubs as the constant does: drag it sideways. */
function scrubbing(handle) {
  handle.addEventListener("pointerdown", (e) => {
    if (e.button !== 0 || !studio.scrubStart()) return;
    e.preventDefault();
    handle.setPointerCapture(e.pointerId);
    const x = e.clientX;
    const move = (m) => studio.scrubMove(m.clientX - x, m.shiftKey);
    const up = () => {
      handle.removeEventListener("pointermove", move);
      handle.removeEventListener("pointerup", up);
      handle.removeEventListener("pointercancel", up);
      studio.scrubEnd();
    };
    handle.addEventListener("pointermove", move);
    handle.addEventListener("pointerup", up);
    handle.addEventListener("pointercancel", up);
  });
}

let inline = null;

/** Enter on a selected text: a field over its cells, committing one `set_text`. */
function editInline(target) {
  closeInline();
  const input = el("input", "inline-text keep");
  input.value = target.text.content;
  input.setAttribute("aria-label", "Edit the text");
  input.addEventListener("keydown", (e) => {
    e.stopPropagation();
    if (e.key === "Enter") commitText(target, input.value);
    if (e.key === "Escape") closeInline();
  });
  input.addEventListener("blur", () => setTimeout(closeInline));
  inline = { input, target };
  drawing.append(input);
  placeInline();
  input.focus({ preventScroll: true });
  input.select();
}

function placeInline() {
  const { input, target } = inline;
  place(input, target.at);
  const r = geo.cell_rect(target.at.row, target.at.col, target.at.row, target.at.col + Math.max(target.at.span, 4) - 1, false, false);
  Object.assign(input.style, { width: `${r.width}px`, height: `${r.height}px`, fontSize: `${r.height * 0.6}px` });
}

function closeInline() {
  if (!inline) return;
  const { input } = inline;
  inline = null;
  const had = input === document.activeElement;
  input.remove();
  if (had) focusDrawing(true);
}

// The context palette (palette.js): a combobox of what to place where a
// drag ended, or at the cursor. Arrow keys move through it, Enter picks,
// and a query matches meaning, letters, reading or keys.

function closeMenu() {
  // Focus in a removed palette would fall to the page, out of Escape's reach.
  if (menu?.contains(document.activeElement)) focusDrawing(false);
  menu?.remove();
  menu = null;
}

/**
 * Opens the palette at `at`. `mode` is `"feed"` with the dragged source
 * `from`, `"source"` with the unwired arrowhead `to` to feed, or
 * `"place"` with neither.
 */
function openMenu(at, { mode, from = null, to = null }) {
  closeMenu();
  const callees = docs.get(current().src).palette(from?.row, from?.col);
  // Every word can be found by how it sounds or how it is typed, shown or not.
  callees.forEach((c) => ((c.reading = reading(c.word)), (c.keys = keysOf(c.word))));
  const choices = new Choices(offer(callees, mode));
  menu = el("div", "palette");
  const filter = el("input");
  filter.placeholder = mode === "feed" ? "Feed it into…" : mode === "source" ? "Feed the arrowhead from…" : "Place…";
  filter.setAttribute("role", "combobox");
  filter.setAttribute("aria-expanded", "true");
  filter.setAttribute("aria-autocomplete", "list");
  filter.setAttribute("aria-controls", "palette-list");
  filter.setAttribute("aria-label", "Filter by meaning, letters, reading or keys");
  const list = el("ul");
  list.id = "palette-list";
  list.setAttribute("role", "listbox");
  menu.append(filter, list);

  const pick = (c) => {
    closeMenu();
    focusDrawing(false);
    placeChoice(c, at, { mode, from, to });
  };

  const show = () => {
    list.replaceChildren(
      ...choices.items.map((c, i) => {
        const item = el("li");
        item.id = `palette-${i}`;
        item.setAttribute("role", "option");
        item.setAttribute("aria-selected", String(i === choices.index));
        if (i === choices.index) item.classList.add("chosen");
        const arity = c.kind === "node" ? `${c.inputs}→${c.outputs}` : "";
        const word = el("span", "word", c.word);
        if (layer.shown && c.reading) word.append(el("span", "reading", c.reading));
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
    if (choices.chosen) {
      filter.setAttribute("aria-activedescendant", `palette-${choices.index}`);
      list.children[choices.index]?.scrollIntoView({ block: "nearest" });
    } else filter.removeAttribute("aria-activedescendant");
  };
  filter.addEventListener("input", () => {
    choices.filter(filter.value);
    show();
  });
  filter.addEventListener("keydown", (e) => {
    e.stopPropagation();
    if (e.key === "Tab") return closeMenu();
    const answer = choices.key(e.key);
    if (answer) e.preventDefault();
    if (answer === "close") closeMenu();
    else if (answer === "pick") pick(choices.chosen);
    else if (answer === "move") show();
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

/** Places a palette pick as one edit. */
function placeChoice(c, at, how) {
  studio.edit(placing(c, at, how));
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
// call of its definition. A held preview (a scrub, a nudge) is traced at
// once. Until a text's values arrive, the last ones stay, dimmed, on the
// wires the edit kept. Tracing is optional: if it fails or isn't available,
// the drawing has no badges, and the status beside the Values switch says
// why.

const TRACE_LIMIT_MS = 4000;
const FIRST_CALL = "Each wire shows the value it carried during the first call of its definition.";

let traceTimer;
/** The last trace: the text it traced, and its values. */
let traced = { src: null, values: [] };
/** Counts trace requests; a result for any but the latest is dropped. */
let traceAsked = 0;

// One trace at a time. A result for a text since edited is dropped: the
// newer text's trace is waiting, or is about to be asked for.
const tracer = new Flight({
  spawn: () => new Worker("worker.js", { type: "module" }),
  limit: TRACE_LIMIT_MS,
  current: () => traceAsked,
  settle: (job, outcome) => {
    if (outcome.broken !== undefined) {
      traced = { src: null, values: [] };
      showTrace("unavailable", `The values couldn’t be worked out${outcome.broken ? `: ${outcome.broken}` : ""}.`);
    } else if (outcome.timedOut) {
      traced = { src: job.src, values: [] };
      showTrace("unavailable", `Working out the values took longer than ${TRACE_LIMIT_MS / 1000} s, so it was stopped.`);
    } else {
      traced = { src: job.src, values: outcome.failed ? [] : outcome.wires };
      showTraced(outcome);
    }
    showOverlays();
  },
});

/** Says how the last trace went, with a jump to a runtime error. */
function showTraced({ failed, error, stopped }) {
  if (failed) return showTrace("unavailable", `The values couldn’t be worked out: ${failed}`);
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

function requestTrace(src = studio?.text ?? editor.value) {
  clearTimeout(traceTimer);
  if (untraceable()) {
    traced = { src, values: [] };
    // Nothing asked for before is wanted now, not even the trace in flight.
    tracer.cancel();
    if (values.checked) showTrace("unavailable", "The sheet has errors, so it can’t run.");
    else showTrace("");
    return showOverlays();
  }
  showTrace("computing…");
  tracer.request({ kind: "trace", src, rev: ++traceAsked });
}

/** Carries the last values through an edit's map, onto the wires it kept. */
function carryValues(map) {
  traced = {
    src: traced.src,
    values: traced.values.flatMap(({ wire, value }) => {
      const to = map.map_wire(wire);
      return to === undefined ? [] : [{ wire: to, value }];
    }),
  };
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
  const { wires: list, src } = current();
  if (!values.checked || !traced.values.length) return;
  // Until this text's values arrive, the last ones show dimmed.
  const group = svgEl("g", traced.src === src ? "overlay badges" : "overlay badges stale");
  svg.append(group);
  for (const { wire, value } of traced.values) {
    const w = list[wire];
    if (!w) continue;
    const badge = svgEl("g", "badge");
    const label = svgEl("text", "", {
      ...pointOf(anchor(w.cells)),
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
  const { row, col } = cell_of_offset(editor.value, editor.selectionStart);
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
  const caret = cell_of_offset(editor.value, editor.selectionStart);
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
  if (e.inputType === "historyUndo") undoPending = true;
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
    drawing.replaceChildren(el("p", "hint", "Studio needs WebAssembly, which didn’t load."));
    throw e;
  }
  readings.checked = store.get("readings") !== "false";
  layer = new Layer({ on: readings.checked, annotate, changed: relayer });
  studio = new Studio({
    apply: applyEdit,
    text: () => editor.value,
    write: writeText,
    doc: (text) => docs.get(text),
    view: viewOf,
    seed: (d) => docs.seed(d),
    refused,
    status: showStatus,
    changed: selectionChanged,
    mapped: carryValues,
    previewed: (text) => requestTrace(text),
    palette: (at) => {
      const box = boxAt(current(), at);
      if (!box || box.kind === "node") return void showStatus("/ opens the palette on blank sheet inside a definition: move the cursor there.", "hint");
      openMenu(at, { mode: "place" });
    },
    editText: editInline,
    help: () => toggleHelp(),
    zoom: zoomBy,
    gloss: () => layer.shown,
  });
  showInspector();
  // The page's own Shavian, marked in index.html, shows its readings too.
  layer.resay(document);
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
