// The Shawline playground: an editor, a live drawing, diagnostics, and a
// Run button. All language work happens in the wasm package; this file only
// wires it to the page.
import init, {
  check, render, format, table, boxes, move_box, connect, remove_wire, remove_node, place_node,
  palette, endpoints, preview, wires, Transliterator,
} from "./pkg/shawline_wasm.js";
import { Tour, TOUR_EXAMPLE } from "./tour.js";

const $ = (id) => document.getElementById(id);
const editor = $("editor");
const marks = $("marks");
const drawing = $("drawing");
const problems = $("problems");
const output = $("output");
const runButton = $("run");
const translit = $("translit");
const examples = $("examples");
const values = $("values");
const tour = new Tour($("tour"), () => showOverlays());

// One cell in the renderer's SVG units (shawline-glyph CELL_WIDTH/HEIGHT).
const CELL_W = 600;
const CELL_H = 1200;
const RUN_LIMIT_MS = 10_000;

const store = {
  get: (key) => localStorage.getItem(`shawline.${key}`),
  set: (key, value) => localStorage.setItem(`shawline.${key}`, value),
};

// Positions are code points, as in every Shawline dump, but the textarea
// counts UTF-16 units and Shavian letters take two.

const isLow = (unit) => unit >= 0xdc00 && unit <= 0xdfff;
const width = (text, i) => (text.codePointAt(i) > 0xffff ? 2 : 1);

function offsetOf(text, row, col) {
  let i = 0;
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
  const lineStart = before.lastIndexOf("\n") + 1;
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
  if (top < 0 || top > innerHeight - cell) scrollBy({ top: top - innerHeight / 3, behavior: "smooth" });
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

// Live checking and drawing, a beat after the last keystroke.

let refreshTimer;
let shown = [];

function changed() {
  const lines = editor.value.split("\n");
  const cols = Math.max(0, ...lines.map((l) => [...l].length));
  editor.style.width = `${cols + 2}ch`;
  editor.style.height = `calc(${lines.length + 1}em + var(--pad))`;
  store.set("source", editor.value);
  clearTimeout(refreshTimer);
  refreshTimer = setTimeout(refresh, 120);
}

function refresh() {
  shown = check(editor.value);
  tour.update(current());
  showProblems();
  showDrawing();
  scheduleTrace();
}

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
  item.append(el("span", "pos", `${d.row + 1}:${d.col + 1}`), el("span", "code", d.code), el("span", "message", d.message));
  item.title = d.severity;
  item.addEventListener("click", () => jumpTo(d));
  item.addEventListener("mouseenter", () => highlight(i, true));
  item.addEventListener("mouseleave", () => highlight(i, false));
  return item;
}

function highlight(i, on) {
  document.querySelectorAll(`[data-i="${i}"]`).forEach((node) => node.classList.toggle("hot", on));
}

function showDrawing() {
  const svg = render(editor.value, document.documentElement.dataset.theme === "dark");
  if (svg === undefined) {
    // Unreadable text (a tab, a stray character): keep the last drawing, faded.
    drawing.querySelector("svg")?.classList.add("stale");
    return;
  }
  drawing.innerHTML = svg;
  const view = drawing.querySelector("svg").viewBox.baseVal;
  const cell = parseFloat(getComputedStyle(editor).fontSize);
  drawing.firstElementChild.style.width = `${(view.width / CELL_H) * cell}px`;
  const background = drawing.querySelector("svg > rect");
  const ns = "http://www.w3.org/2000/svg";
  background?.after(
    ...shown.map((d, i) => {
      const cell = document.createElementNS(ns, "rect");
      const attrs = { x: d.col * CELL_W, y: d.row * CELL_H, width: CELL_W, height: CELL_H, rx: 90 };
      Object.entries(attrs).forEach(([k, v]) => cell.setAttribute(k, v));
      cell.setAttribute("class", `cell ${d.severity}`);
      cell.dataset.i = i;
      return cell;
    }),
  );
  showOverlays();
}

/** The drawing's cell under a pointer event, or null off the sheet. */
function cellAt(e) {
  const svg = drawing.querySelector("svg");
  if (!svg) return null;
  const box = svg.getBoundingClientRect();
  const view = svg.viewBox.baseVal;
  const x = view.x + ((e.clientX - box.left) * view.width) / box.width;
  const y = view.y + ((e.clientY - box.top) * view.height) / box.height;
  return x >= 0 && y >= 0 ? { row: Math.floor(y / CELL_H), col: Math.floor(x / CELL_W) } : null;
}

// The drawing as an editor (Studio Phase 1). Every gesture is one checked
// edit to the text (shawline-edit), applied through the textarea so undo
// works as it does for typing; a refused edit changes nothing and says why
// beside the cell it is about.
//
// - Drag from a source (an output tee, an input, a constant, an arm's
//   input) or from a wire: release on a free arrowhead to connect, or on
//   empty space to pick a node from the palette, placed there and wired in.
// - Drag from a free arrowhead back to a source or a wire: connect, joining
//   that wire as a fan-out.
// - Drag anything else on a box to move the box.
// - Click a node or a wire to select it; Delete removes it.

const SVG = "http://www.w3.org/2000/svg";
const keyOf = ({ row, col }) => `${row}:${col}`;
const same = (a, b) => a && b && a.row === b.row && a.col === b.col;

let dragged = false;
let drag = null;
let selected = null;
let menu = null;

// What the sheet offers to the pointer, read once per text.
let sheet = { src: null };

function current() {
  const src = editor.value;
  if (sheet.src === src) return sheet;
  const ends = endpoints(src);
  const list = wires(src);
  const byCell = new Map();
  for (const w of list) {
    for (const c of w.cells) {
      const k = keyOf(c);
      byCell.set(k, [...(byCell.get(k) ?? []), w]);
    }
  }
  sheet = { src, boxes: boxes(src), sources: ends.sources, sinks: ends.sinks, wires: list, byCell };
  return sheet;
}

const sourceAt = (at) =>
  current().sources.find((s) => s.row === at.row && at.col >= s.col && at.col < s.col + s.span);
const sinkAt = (at) => current().sinks.find((s) => same(s, at));
/** The editable wire through a cell, if any. */
const wireAt = (at) => (current().byCell.get(keyOf(at)) ?? []).find((w) => w.source) ?? null;
const charAt = ({ row, col }) => [...(editor.value.split("\n")[row] ?? "")][col] ?? " ";

drawing.addEventListener("click", (e) => {
  // The click that ends a drag isn't a request for the caret.
  if (dragged) return void (dragged = false);
  const at = cellAt(e);
  if (!at) return;
  const wire = wireAt(at);
  const box = boxAt(at);
  if (wire) select({ kind: "wire", at, wire });
  else if (box?.kind === "node") select({ kind: "node", box });
  else {
    select(null);
    jumpTo(at, false);
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

function svgRect(className, { top, left, bottom, right }) {
  return svgEl("rect", className, {
    x: left * CELL_W, y: top * CELL_H,
    width: (right - left + 1) * CELL_W, height: (bottom - top + 1) * CELL_H, rx: 90,
  });
}

const cellRect = (className, { row, col }, span = 1) =>
  svgRect(className, { top: row, left: col, bottom: row, right: col + span - 1 });
const centre = ({ row, col }) => `${col * CELL_W + CELL_W / 2},${row * CELL_H + CELL_H / 2}`;

/** The tour's rings, selection and value badges, drawn again over every
 * fresh drawing. */
function showOverlays() {
  const svg = drawing.querySelector("svg");
  if (!svg) return;
  svg.querySelectorAll(".overlay").forEach((n) => n.remove());
  svg.append(...tour.rings().map((r) => svgRect("overlay beacon", r)));
  if (selected && selected.src !== editor.value) selected = null;
  if (selected?.kind === "node") svg.append(svgRect("overlay selected", selected.box));
  if (selected?.kind === "wire") {
    const group = svgEl("g", "overlay selected-wire");
    selected.wire.cells.forEach((c) => group.append(cellRect("", c)));
    svg.append(group);
  }
  showBadges(svg);
}

function select(target) {
  selected = target && { ...target, src: editor.value };
  showOverlays();
  if (target) drawing.focus({ preventScroll: true });
}

function showGhost() {
  const { box, dr, dc } = drag;
  drag.ghost?.remove();
  const at = { top: box.top + dr, left: box.left + dc, bottom: box.bottom + dr, right: box.right + dc };
  drag.ghost = (dr || dc) && at.top >= 0 && at.left >= 0 ? svgRect("ghost", at) : null;
  if (drag.ghost) drawing.querySelector("svg")?.append(drag.ghost);
}

function endDrag() {
  drag?.ghost?.remove();
  drag?.marks?.forEach((n) => n.remove());
  drag?.path?.remove();
  drag = null;
  drawing.classList.remove("dragging", "wiring");
}

/** Shows a refusal's reason by the cell it is about, for a moment. */
function refuse({ row, col, reason }) {
  const svg = drawing.querySelector("svg");
  if (!svg) return;
  const cell = cellRect("refused", { row, col });
  svg.append(cell);
  const tip = el("div", "refusal", reason);
  tip.setAttribute("role", "status");
  place(tip, { row: row + 1, col });
  drawing.append(tip);
  setTimeout(() => (tip.remove(), cell.remove()), 2800);
}

/** A cell's top-left corner in viewport pixels. */
function clientOf({ row, col }) {
  const svg = drawing.querySelector("svg");
  const view = svg.viewBox.baseVal;
  const box = svg.getBoundingClientRect();
  return {
    x: ((col * CELL_W - view.x) * box.width) / view.width + box.left,
    y: ((row * CELL_H - view.y) * box.height) / view.height + box.top,
  };
}

/** Positions an HTML element inside #drawing at a cell's top-left corner. */
function place(node, at) {
  const { x, y } = clientOf(at);
  const host = drawing.getBoundingClientRect();
  node.style.left = `${x - host.left + drawing.scrollLeft}px`;
  node.style.top = `${y - host.top + drawing.scrollTop}px`;
}

/** Replaces the whole text through the editor, so the edit can be undone. */
function applyText(text) {
  const caret = posOf(editor.value, editor.selectionStart);
  editor.focus({ preventScroll: true });
  editor.select();
  insertText(text);
  settle();
  const at = offsetOf(editor.value, caret.row, caret.col);
  editor.setSelectionRange(at, at);
  clearTimeout(refreshTimer);
  refresh();
  drawing.focus({ preventScroll: true });
}

function apply(result) {
  if (!result.ok) return void refuse(result);
  applyText(result.text);
}

// Dragging.

drawing.addEventListener("pointerdown", (e) => {
  if (e.button !== 0) return;
  // A drag isn't always followed by a click, so its flag mustn't outlive
  // it and swallow the next real click.
  dragged = false;
  closeMenu();
  const at = cellAt(e);
  if (!at) return;
  const source = sourceAt(at);
  const sink = sinkAt(at);
  const wire = wireAt(at);
  let start = null;
  if (source) start = { kind: "wire", from: { row: source.row, col: source.col }, scope: source.scope };
  else if (sink && !sink.wired) start = { kind: "sink", sink, scope: sink.scope };
  else if (wire) {
    const from = sourceAt(wire.source) ?? wire.source;
    start = { kind: "wire", from: { row: from.row, col: from.col }, scope: from.scope };
  } else {
    const box = boxAt(at);
    if (box) start = { kind: "box", box, dr: 0, dc: 0, ghost: null };
  }
  if (!start) return;
  e.preventDefault();
  drawing.setPointerCapture(e.pointerId);
  drag = { ...start, start: at, at };
  drawing.classList.add(drag.kind === "box" ? "dragging" : "wiring");
  if (drag.kind !== "box") glow();
});

/** Lights up where the drag can end: free sinks for a source, sources for a sink. */
function glow() {
  const svg = drawing.querySelector("svg");
  const { sources, sinks } = current();
  const marks =
    drag.kind === "wire"
      ? sinks.filter((s) => !s.wired && s.scope === drag.scope).map((s) => cellRect("glow", s))
      : sources.filter((s) => s.scope === drag.scope).map((s) => cellRect("glow", s, s.span));
  svg?.append(...marks);
  drag.marks = marks;
}

/** The source a sink-first drag would connect to, under the pointer. */
function sourceUnder(at) {
  const source = sourceAt(at);
  if (source) return source;
  const wire = wireAt(at);
  return wire && (sourceAt(wire.source) ?? wire.source);
}

function showPath() {
  const svg = drawing.querySelector("svg");
  if (!svg) return;
  const src = editor.value;
  const { at } = drag;
  let cells;
  if (drag.kind === "wire") {
    const sink = sinkAt(at);
    const free = sink && !sink.wired && sink.scope === drag.scope;
    const to = free ? sink : at;
    const found = !same(to, drag.from) && preview(src, drag.from.row, drag.from.col, to.row, to.col);
    if (found && free) cells = [sink, ...found, drag.from];
    else if (found) cells = [...found, drag.from];
    else cells = [at, drag.from];
  } else {
    const source = sourceUnder(at);
    const found = source && source.scope === drag.scope &&
      preview(src, source.row, source.col, drag.sink.row, drag.sink.col);
    cells = found ? [drag.sink, ...found, source] : [drag.sink, at];
  }
  drag.path?.remove();
  drag.path = svgEl("polyline", cells.length > 2 ? "wire-preview" : "wire-preview loose", {
    points: cells.map(centre).join(" "),
  });
  svg.append(drag.path);
}

drawing.addEventListener("pointermove", (e) => {
  const at = cellAt(e);
  if (!drag) {
    const over = at && (sourceAt(at) || sinkAt(at)?.wired === false || wireAt(at));
    const svg = drawing.querySelector("svg");
    svg?.classList.toggle("over-source", Boolean(over));
    svg?.classList.toggle("over-box", Boolean(!over && at && boxAt(at)));
    return;
  }
  if (!at || same(at, drag.at)) return;
  drag.at = at;
  if (drag.kind === "box") {
    Object.assign(drag, { dr: at.row - drag.start.row, dc: at.col - drag.start.col });
    showGhost();
  } else {
    showPath();
  }
});

drawing.addEventListener("pointerup", () => {
  if (!drag) return;
  const finished = drag;
  endDrag();
  if (same(finished.at, finished.start)) return;
  dragged = true;
  const src = editor.value;
  const { at } = finished;
  if (finished.kind === "box") {
    apply(move_box(src, finished.box.top, finished.box.left, finished.dr, finished.dc));
  } else if (finished.kind === "sink") {
    const source = sourceUnder(at);
    if (!source) return refuse({ ...at, reason: "Drop on a source or a wire to connect this arrowhead." });
    apply(connect(src, source.row, source.col, finished.sink.row, finished.sink.col));
  } else {
    const { from } = finished;
    const sink = sinkAt(at);
    const wire = wireAt(at);
    if (sink) apply(connect(src, from.row, from.col, sink.row, sink.col));
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

drawing.addEventListener("keydown", (e) => {
  const mod = e.metaKey || e.ctrlKey;
  if (mod && e.key.toLowerCase() === "z") {
    // The drawing has no history of its own: undo is the editor's.
    e.preventDefault();
    editor.focus({ preventScroll: true });
    document.execCommand(e.shiftKey ? "redo" : "undo");
    drawing.focus({ preventScroll: true });
  } else if ((e.key === "Delete" || e.key === "Backspace") && selected) {
    e.preventDefault();
    const src = editor.value;
    const target = selected;
    selected = null;
    if (target.kind === "node") apply(remove_node(src, target.box.top, target.box.left));
    else apply(remove_wire(src, target.at.row, target.at.col));
  } else if (e.key === "Escape") {
    endDrag();
    closeMenu();
    select(null);
  }
});

// The context palette: callees to place where a drag ended, wired from the
// dragged source into the new node's first input.

function closeMenu() {
  menu?.remove();
  menu = null;
}

function openMenu(at, from) {
  closeMenu();
  const offered = palette(editor.value, from?.row, from?.col);
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
  const pick = (c) => {
    closeMenu();
    let first = null;
    for (const row of rows) {
      const corner = { row: Math.max(0, row - 1), col: at.col };
      const placed = place_node(editor.value, corner.row, corner.col, c.word, c.inputs, c.outputs);
      if (!placed.ok) continue;
      if (!from || !c.inputs) return applyText(placed.text);
      const wired = connect(placed.text, from.row, from.col, corner.row + 1, corner.col);
      if (wired.ok) return applyText(wired.text);
      first ??= { placed, wired };
    }
    if (!first) return refuse(place_node(editor.value, Math.max(0, at.row - 1), at.col, c.word, c.inputs, c.outputs));
    applyText(first.placed.text);
    refuse(first.wired);
  };

  const show = () => {
    const q = filter.value.trim().toLowerCase();
    const items = offered.filter((c) => !q || c.word.includes(q) || c.gloss.toLowerCase().includes(q));
    list.replaceChildren(
      ...items.map((c, i) => {
        const item = el("li");
        item.setAttribute("role", "option");
        if (i === 0) item.classList.add("first");
        const arity = `${c.inputs}→${c.outputs}`;
        item.append(el("span", "word", c.word), el("span", "gloss", c.gloss), el("span", "arity", arity));
        if (c.mirror) item.append(el("span", "mirror", `⇄ ${c.mirror}`));
        item.title = c.mirror ? `${c.gloss}; its mirror ${c.mirror} is the dual` : c.gloss;
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
  // Fixed over the page rather than inside the drawing's scroll box, which
  // would clip it: below the drop point, or above it when there's no room.
  document.body.append(menu);
  const below = clientOf({ row: at.row + 1, col: at.col });
  const above = clientOf(at).y - menu.offsetHeight - 8;
  const room = below.y + menu.offsetHeight + 8 <= innerHeight;
  menu.style.left = `${Math.max(8, Math.min(below.x, innerWidth - menu.offsetWidth - 8))}px`;
  menu.style.top = `${Math.max(8, room || above < 8 ? below.y : above)}px`;
  // After the click that ends the drag, which would take focus back.
  setTimeout(() => filter.focus({ preventScroll: true }));
}

document.addEventListener("pointerdown", (e) => {
  if (menu && !menu.contains(e.target)) closeMenu();
});
// A fixed palette would drift from its drop point as the page scrolls.
addEventListener("scroll", (e) => menu && !menu.contains(e.target) && closeMenu(), { capture: true, passive: true });

// Live value badges: after each edit, a beat later, the sheet is traced off
// the main thread and each wire shows the value it carried during the first
// call of its definition. Tracing is optional: if it fails or isn't
// available, the drawing simply has no badges.

let tracer = null;
let traceGen = 0;
let traceTimer;
let traceKill;
let traced = { src: null, values: [] };

function startTracer() {
  tracer = new Worker("worker.js", { type: "module" });
  tracer.onmessage = ({ data }) => {
    if (data.gen !== traceGen) return;
    clearTimeout(traceKill);
    if (data.failed) stopTracer();
    traced = { src: data.src, values: data.failed ? [] : data.wires };
    showOverlays();
  };
  tracer.onerror = () => {
    stopTracer();
    traced = { src: null, values: [] };
    showOverlays();
  };
}

function stopTracer() {
  tracer?.terminate();
  tracer = null;
}

function scheduleTrace() {
  clearTimeout(traceTimer);
  traceTimer = setTimeout(requestTrace, 250);
}

function requestTrace() {
  const src = editor.value;
  if (!values.checked || shown.some((d) => d.severity === "error")) {
    traced = { src, values: [] };
    return showOverlays();
  }
  if (!tracer) startTracer();
  const gen = ++traceGen;
  tracer.postMessage({ kind: "trace", src, gen });
  clearTimeout(traceKill);
  traceKill = setTimeout(() => gen === traceGen && stopTracer(), 4000);
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
  if (!values.checked || traced.src !== editor.value) return;
  const { wires: list } = current();
  const group = svgEl("g", "overlay badges");
  svg.append(group);
  for (const { wire, value } of traced.values) {
    const w = list[wire];
    if (!w) continue;
    const at = anchor(w.cells);
    const badge = svgEl("g", "badge");
    const label = svgEl("text", "", {
      x: at.col * CELL_W + CELL_W / 2,
      y: at.row * CELL_H + CELL_H / 2,
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

editor.addEventListener("input", changed);

document.addEventListener("selectionchange", () => {
  if (document.activeElement !== editor) return;
  const { row, col } = posOf(editor.value, editor.selectionStart);
  $("cursor").textContent = `${row + 1}:${col + 1}`;
});

// Running happens in a worker, so a program that never stops can be cut off.

let worker;
let running = null;

function startWorker() {
  worker = new Worker("worker.js", { type: "module" });
  worker.onmessage = ({ data }) => finish(data);
}

function setRunning(on) {
  clearTimeout(running);
  running = on ? setTimeout(() => stop(`Stopped after ${RUN_LIMIT_MS / 1000} seconds.`), RUN_LIMIT_MS) : null;
  runButton.textContent = on ? "Stop" : "Run";
  runButton.classList.toggle("running", on);
  $("output-meta").textContent = on ? "running…" : "";
}

function say(text, className = "hint") {
  output.replaceChildren(el("span", className, text));
}

function runProgram() {
  if (running) return stop("Stopped.");
  setRunning(true);
  worker.postMessage(editor.value);
}

function finish(result) {
  setRunning(false);
  $("output-meta").textContent = `${result.ms < 1 ? "under 1" : Math.round(result.ms)} ms`;
  if (result.error) {
    const failure = el("span", "failure", result.error);
    failure.title = "Show where it failed";
    failure.addEventListener("click", () => jumpTo(result.at));
    output.replaceChildren(failure);
  } else if (result.diagnostics.some((d) => d.severity === "error")) {
    say("The sheet has errors, so it can’t run yet. See Diagnostics.");
  } else if (!result.outputs.length) {
    say("·𐑥𐑱𐑯 has no outputs.");
  } else {
    output.textContent = result.outputs.join("\n");
    tour.ran(current());
  }
}

function stop(message) {
  worker.terminate();
  startWorker();
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
  const formatted = format(editor.value);
  if (formatted === undefined) return note("Fix the character errors first.");
  if (formatted === editor.value) return note("Already canonical.");
  const caret = posOf(editor.value, editor.selectionStart);
  editor.focus();
  editor.select();
  insertText(formatted);
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

function label(name) {
  const [, number, rest] = name.match(/^(\d+)-(.*)$/);
  return `${number} · ${rest.replace(/-ascii$/, " (ASCII)").replaceAll("-", " ")}`;
}

/** Loads an example; any other than the tour's ends the tour. */
async function loadExample(name) {
  if (name !== TOUR_EXAMPLE) tour.stop();
  const text = await fetch(`examples/${name}.shaw`).then((r) => r.text());
  editor.value = text;
  examples.value = name;
  store.set("example", name);
  history.replaceState(null, "", `#${name}`);
  settle();
  say("Run the program to see what ·𐑥𐑱𐑯 returns.");
  $("output-meta").textContent = "";
  changed();
}

examples.addEventListener("change", () => loadExample(examples.value));

async function startTour() {
  await loadExample(TOUR_EXAMPLE);
  tour.start(current());
}

$("tour-start").addEventListener("click", startTour);

async function main() {
  try {
    await init();
  } catch (e) {
    drawing.replaceChildren(el("p", "hint", "The playground needs WebAssembly, which didn’t load."));
    throw e;
  }
  trans = new Transliterator();
  const entries = table();
  keys = new Set(["/", ...entries.flatMap(([k]) => [...k])]);
  $("keys").replaceChildren(
    ...entries.map(([k, letter]) => {
      const item = el("li");
      item.append(el("span", "letter", letter), el("span", "keys", k));
      return item;
    }),
  );
  translit.checked = store.get("translit") !== "false";
  values.checked = store.get("values") !== "false";
  startWorker();

  const names = await fetch("examples/index.json").then((r) => r.json());
  examples.replaceChildren(...names.map((name) => new Option(label(name), name)));
  addEventListener("hashchange", () => {
    const name = location.hash.slice(1);
    if (names.includes(name) && name !== examples.value) loadExample(name);
  });

  // A saved sheet wins unless the link names a different example. A first
  // visit with no link starts the tour.
  const linked = names.find((n) => `#${n}` === location.hash);
  const saved = store.get("source");
  if (saved !== null && (!linked || linked === store.get("example"))) {
    editor.value = saved;
    examples.value = store.get("example") ?? "";
    changed();
  } else if (linked) {
    await loadExample(linked);
  } else {
    await startTour();
  }
}

main();
