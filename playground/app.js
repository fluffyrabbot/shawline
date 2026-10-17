// The Shawline playground: an editor, a live drawing, diagnostics, and a
// Run button. All language work happens in the wasm package; this file only
// wires it to the page.
import init, { check, render, format, table, boxes, move_box, Transliterator } from "./pkg/shawline_wasm.js";

const $ = (id) => document.getElementById(id);
const editor = $("editor");
const marks = $("marks");
const drawing = $("drawing");
const problems = $("problems");
const output = $("output");
const runButton = $("run");
const translit = $("translit");
const examples = $("examples");

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
  showProblems();
  showDrawing();
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

let dragged = false;

drawing.addEventListener("click", (e) => {
  // The click that ends a drag isn't a request for the caret.
  if (dragged) return void (dragged = false);
  const at = cellAt(e);
  if (at) jumpTo(at, false);
});

// Moving boxes. Press on a node or a definition and drag: the box snaps by
// whole cells and a ghost shows where it would land. On release the move is
// one checked edit to the text (shawline-edit), undoable like typing; a
// refused move leaves everything as it was and says why.

let drag = null;
let found = { src: null, list: [] };

/** The innermost box under a cell: a node before its definition. */
function boxAt(at) {
  if (found.src !== editor.value) found = { src: editor.value, list: boxes(editor.value) };
  const area = (b) => (b.bottom - b.top + 1) * (b.right - b.left + 1);
  return found.list
    .filter((b) => at.row >= b.top && at.row <= b.bottom && at.col >= b.left && at.col <= b.right)
    .sort((a, b) => area(a) - area(b))[0];
}

function svgRect(className, { top, left, bottom, right }) {
  const rect = document.createElementNS("http://www.w3.org/2000/svg", "rect");
  const attrs = {
    x: left * CELL_W, y: top * CELL_H,
    width: (right - left + 1) * CELL_W, height: (bottom - top + 1) * CELL_H, rx: 90,
  };
  Object.entries(attrs).forEach(([k, v]) => rect.setAttribute(k, v));
  rect.setAttribute("class", className);
  return rect;
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
  drag = null;
  drawing.classList.remove("dragging");
}

/** Shows a refusal's reason by the cell it is about, for a moment. */
function refuse({ row, col, reason }) {
  const svg = drawing.querySelector("svg");
  if (!svg) return;
  const cell = svgRect("refused", { top: row, left: col, bottom: row, right: col });
  svg.append(cell);
  const view = svg.viewBox.baseVal;
  const box = svg.getBoundingClientRect();
  const host = drawing.getBoundingClientRect();
  const tip = el("div", "refusal", reason);
  tip.setAttribute("role", "status");
  tip.style.left = `${((col * CELL_W - view.x) * box.width) / view.width + box.left - host.left + drawing.scrollLeft}px`;
  tip.style.top = `${(((row + 1) * CELL_H - view.y) * box.height) / view.height + box.top - host.top + drawing.scrollTop}px`;
  drawing.append(tip);
  setTimeout(() => (tip.remove(), cell.remove()), 2800);
}

drawing.addEventListener("pointerdown", (e) => {
  if (e.button !== 0) return;
  const at = cellAt(e);
  const box = at && boxAt(at);
  if (!box) return;
  e.preventDefault();
  drawing.setPointerCapture(e.pointerId);
  drawing.classList.add("dragging");
  drag = { box, from: at, dr: 0, dc: 0, ghost: null };
});

drawing.addEventListener("pointermove", (e) => {
  const at = cellAt(e);
  if (!drag) {
    drawing.querySelector("svg")?.classList.toggle("over-box", Boolean(at && boxAt(at)));
    return;
  }
  if (!at) return;
  const [dr, dc] = [at.row - drag.from.row, at.col - drag.from.col];
  if (dr === drag.dr && dc === drag.dc) return;
  Object.assign(drag, { dr, dc });
  showGhost();
});

drawing.addEventListener("pointerup", () => {
  if (!drag) return;
  const { box, dr, dc } = drag;
  endDrag();
  if (!dr && !dc) return;
  dragged = true;
  const result = move_box(editor.value, box.top, box.left, dr, dc);
  if (!result.ok) return refuse(result);
  // Replacing the whole text through the editor keeps it on the undo stack.
  const caret = posOf(editor.value, editor.selectionStart);
  editor.focus({ preventScroll: true });
  editor.select();
  insertText(result.text);
  settle();
  const at = offsetOf(editor.value, caret.row, caret.col);
  editor.setSelectionRange(at, at);
  clearTimeout(refreshTimer);
  refresh();
});

drawing.addEventListener("pointercancel", endDrag);

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

async function loadExample(name) {
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
  startWorker();

  const names = await fetch("examples/index.json").then((r) => r.json());
  examples.replaceChildren(...names.map((name) => new Option(label(name), name)));
  addEventListener("hashchange", () => {
    const name = location.hash.slice(1);
    if (names.includes(name) && name !== examples.value) loadExample(name);
  });

  // A saved sheet wins unless the link names a different example.
  const linked = names.find((n) => `#${n}` === location.hash);
  const saved = store.get("source");
  if (saved !== null && (!linked || linked === store.get("example"))) {
    editor.value = saved;
    examples.value = store.get("example") ?? "";
    changed();
  } else {
    await loadExample(linked ?? "08-factorial");
  }
}

main();
