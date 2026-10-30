// The drawing as an editor, without the page (PLAN P10): what is selected,
// where the cell cursor is, the keyboard model, and how one gesture becomes
// one checked edit and one undo step. Nothing here touches the DOM; the
// page (app.js) gives it a host and draws what it says, and the Node checks
// give it a fake one.
//
// The text is the only state (Studio principle 1). An edit is sent to wasm
// as one `Edit` object and applied to the text through the host, which
// keeps the editor's undo history. A held gesture (a nudge while its key is
// down, a scrub while the pointer is) edits a preview revision instead,
// and writes the text once, on release.

/** What `]` and `[` step through at one cell, in this order. */
const ORDER = ["box", "arm", "text", "source", "sink", "branch", "wire"];

/** The largest Int (SPEC §9: `i64`). */
const LARGEST = 2n ** 63n - 1n;

/** Pixels of sideways drag per scrub step. */
export const SCRUB_PX = 8;

/** The entry point's name (SPEC §5): a new definition takes it while the sheet has none. */
export const MAIN = "𐑥𐑱𐑯";

/**
 * A new definition's name once the sheet has a main: 𐑯𐑿 ("new"), then
 * 𐑯𐑿𐑨, 𐑯𐑿𐑚, … A single letter would be reserved (`Y013`).
 */
const NEW = "𐑯𐑿";

/** The empty list, a constant written as a word. */
const NIL = "𐑯𐑦𐑤";

/** The first arm's guard on a switch the Box tool draws, so `+ arm` can add the default arm after it. */
export const FIRST_GUARD = "0";

const BOM = "﻿";

export const keyOf = ({ row, col }) => `${row}:${col}`;
export const same = (a, b) => Boolean(a && b) && a.row === b.row && a.col === b.col;
const before = (a, b) => a.row - b.row || a.col - b.col;
const within = (at, r) => at.row >= r.top && at.row <= r.bottom && at.col >= r.left && at.col <= r.right;
const cellOf = ({ row, col }) => ({ row, col });

/** The inclusive rectangle with corners at two cells. */
export const rectOf = (a, b) => ({
  top: Math.min(a.row, b.row), left: Math.min(a.col, b.col), bottom: Math.max(a.row, b.row), right: Math.max(a.col, b.col),
});

/**
 * What a revision offers the pointer and the keyboard, read once per text:
 * its boxes, sources, sinks, wires and texts, its wires by cell, and its
 * ports by cell.
 */
export function readView(doc) {
  const ends = doc.endpoints();
  const wires = doc.wires();
  const byCell = new Map();
  /** Cells that join a wire southward, through the strip under them. */
  const downs = new Set();
  for (const w of wires) {
    for (const c of w.cells) {
      const k = keyOf(c);
      byCell.set(k, [...(byCell.get(k) ?? []), w]);
      if (c.down) downs.add(k);
    }
  }
  const boxes = doc.boxes();
  /** Each port by its cell, with the box whose border holds it. */
  const ports = new Map();
  for (const box of boxes) for (const port of box.ports) ports.set(keyOf(port), { box, port });
  const src = doc.text;
  const lines = (src.startsWith(BOM) ? src.slice(1) : src).split("\n");
  return { src, boxes, texts: doc.texts(), sources: ends.sources, sinks: ends.sinks, wires, byCell, downs, ports, lines };
}

/** The character a cell holds, a space past its line's end. */
export const charOf = (view, { row, col }) => [...(view.lines[row] ?? "")][col] ?? " ";

/** The port at a cell, as `{ box, port }`, or undefined. */
export const portAt = (view, at) => view.ports.get(keyOf(at));

/** The definition port whose name is the text at `text`'s cell. */
const portNamed = (view, text) =>
  [...view.ports.values()].find(({ port }) => port.name && same(port.name, text));

/** The first of `names` not in `taken`, then pairs of them. */
function firstFree(names, taken) {
  for (const n of names) if (!taken.has(n)) return n;
  for (const a of names) for (const b of names) if (!taken.has(a + b)) return a + b;
  return null;
}

/**
 * The name the Box tool gives a new definition: 𐑥𐑱𐑯 while the sheet has
 * none, then the first free one of 𐑯𐑿 and 𐑯𐑿 followed by each of `names`
 * (wasm's `port_names()`). Without `main` it skips 𐑥𐑱𐑯: Make definition's
 * name, since parts taken out of a definition usually have inputs, and
 * main may have none (`C006`).
 */
export function definitionName(view, names, { main = true } = {}) {
  const taken = new Set(
    view.boxes.filter((b) => b.kind === "definition" && b.title).map((b) => b.title.content.replace(/^·/, "")),
  );
  return firstFree([...(main ? [MAIN] : []), NEW, ...names.map((n) => NEW + n)], taken);
}

/** The name a definition's new port takes: the first of `names` (wasm's `port_names()`) none of its ports has. */
export function portName(box, names) {
  return firstFree(names, new Set(box.ports.map((p) => p.name?.content).filter(Boolean)));
}

/**
 * Whether a box is a switch, or the empty square box `place_switch` draws,
 * which recognition reads as a node with no word until it has a selector
 * or a divider.
 */
export const switchLike = (box) => box.kind === "switch" || (box.kind === "node" && !box.word);

/** A node box's port counts: inputs on its left border, outputs on its right. */
export const portCounts = (box) => ({
  inputs: box.ports.filter((p) => p.side === "west").length,
  outputs: box.ports.filter((p) => p.side === "east").length,
});

/**
 * A switch's arms, top to bottom: each `{ top, bottom, guard, divider }`,
 * its interior rows `top..bottom`, its guard's text (`null` for the
 * default arm) and the row of the divider above it (`null` for the first).
 */
export function armsOf(box) {
  const lines = [{ row: box.top, guard: box.title }, ...box.dividers.map((d) => ({ row: d.row, guard: d.guard }))];
  return lines.map((line, i) => ({
    top: line.row + 1,
    bottom: (lines[i + 1]?.row ?? box.bottom) - 1,
    guard: line.guard,
    divider: i ? line.row : null,
  }));
}

/** The smallest integer guard no arm has yet. */
function nextGuard(arms) {
  const used = new Set(arms.map((a) => a.guard?.content));
  let n = 0;
  while (used.has(String(n))) n++;
  return String(n);
}

/** The arm selection for a switch's divider. */
const armTarget = (box, divider) => ({ kind: "arm", box, divider: divider.row, guard: divider.guard });

export const sourceAt = (view, at) =>
  view.sources.find((s) => s.row === at.row && at.col >= s.col && at.col < s.col + s.span);
export const sinkAt = (view, at) => view.sinks.find((s) => same(s, at));
/** The editable wire through a cell, if any. */
export const wireAt = (view, at) => (view.byCell.get(keyOf(at)) ?? []).find((w) => w.source) ?? null;
export const textAt = (view, at) => view.texts.find((t) => t.row === at.row && at.col >= t.col && at.col < t.col + t.span);

/** The innermost box under a cell: a node before its definition. */
export function boxAt(view, at) {
  const area = (b) => (b.bottom - b.top + 1) * (b.right - b.left + 1);
  return view.boxes.filter((b) => within(at, b)).sort((a, b) => area(a) - area(b))[0];
}

/** The box whose border holds a cell, or null: the innermost box under it, when the cell is on its border. */
export function borderAt(view, at) {
  const box = boxAt(view, at);
  const edge = box && (at.row === box.top || at.row === box.bottom || at.col === box.left || at.col === box.right);
  return edge ? box : null;
}

/**
 * The box whose border can take a new port at a cell, or null: a plain
 * stretch of a top, left or right border, not a corner, a port, a title
 * gap or a divider's end. Which side takes which port is the edit's to
 * say; this only spares the pointer cells that never take one.
 */
export function portSpot(view, at) {
  const box = borderAt(view, at);
  if (!box || at.row === box.bottom) return null;
  const corner = at.row === box.top && (at.col === box.left || at.col === box.right);
  const gap = at.row === box.top && box.gap && at.col >= box.gap.col && at.col < box.gap.col + box.gap.span;
  if (corner || gap || portAt(view, at) || box.dividers.some((d) => d.row === at.row)) return null;
  return box;
}

/** The arm whose divider runs through a cell, or null. */
function armAt(view, at) {
  const box = boxAt(view, at);
  const divider = box?.dividers.find((d) => d.row === at.row);
  return divider ? armTarget(box, divider) : null;
}

/** The cell a selection is about: where the cursor goes when it is selected. */
export function anchorOf(target) {
  switch (target.kind) {
    case "box":
      return { row: target.box.top, col: target.box.left };
    case "arm":
      return { row: target.divider, col: target.box.left };
    case "branch":
    case "sink":
      return { row: target.sink.row, col: target.sink.col };
    case "parts":
      return anchorOf(target.parts[0]);
    default:
      return { row: target.at.row, col: target.at.col };
  }
}

/** Whether two selections name the same thing. */
export function sameTarget(a, b) {
  if (!a || !b || a.kind !== b.kind) return false;
  if (a.kind === "wire") return a.wire.wire === b.wire.wire;
  if (a.kind === "parts") return a.parts.length === b.parts.length && a.parts.every((p, i) => sameTarget(p, b.parts[i]));
  return same(anchorOf(a), anchorOf(b));
}

/** Whether a selection covers a cell: a box's area, a text's cells, a wire's or branch's cells. */
function covers(target, at) {
  switch (target.kind) {
    case "box":
      return within(at, target.box);
    case "arm":
      return at.row === target.divider && at.col >= target.box.left && at.col <= target.box.right;
    case "parts":
      return target.parts.some((p) => covers(p, at));
    case "wire":
      return target.wire.cells.some((c) => same(c, at));
    case "branch":
      return same(target.sink, at) || target.cells.some((c) => same(c, at));
    case "sink":
      return same(target.sink, at);
    default:
      return at.row === target.at.row && at.col >= target.at.col && at.col < target.at.col + (target.at.span ?? 1);
  }
}

/** The source or text selection for a text: a source when a wire may start there. */
function textTarget(view, text) {
  const source = sourceAt(view, text);
  const at = { row: text.row, col: text.col, span: text.span };
  return source ? { kind: "source", at, source, text } : { kind: "text", at, text };
}

// Parts (PLAN P10, Gesture details: Extracting a definition): what Make
// definition takes out, selected one at a time or several together.

/**
 * Whether a selection is a part: a node box or a switch, picked whole, or
 * a constant or an inline node's word. An input's name is a source too,
 * but no part.
 */
function isPart(view, target) {
  if (target?.kind === "box") return target.box.kind !== "definition";
  return target?.kind === "source" && Boolean(target.text) && !portNamed(view, target.text);
}

/** The cell `extract` names a part by: a box's corner, or a text's first cell. */
export const partCell = (target) =>
  target.kind === "box" ? { row: target.box.top, col: target.box.left } : { row: target.text.row, col: target.text.col };

/** The part a Shift-click on a cell takes: a constant or an inline node's word there, or else the node or switch around it. */
export function partAt(view, at) {
  const text = textAt(view, at);
  const target = text && text.kind !== "comment" ? textTarget(view, text) : null;
  if (isPart(view, target)) return target;
  const box = boxAt(view, at);
  return box && box.kind !== "definition" ? { kind: "box", box } : null;
}

/**
 * The parts lying wholly inside a rectangle, for a marquee. Only the
 * outermost count: a switch is picked whole, with what it holds.
 */
export function partsIn(view, rect) {
  const inside = (r) => r.top >= rect.top && r.bottom <= rect.bottom && r.left >= rect.left && r.right <= rect.right;
  const found = view.boxes.filter((b) => b.kind !== "definition" && inside(b)).map((box) => ({ kind: "box", box }));
  for (const text of view.texts) {
    if (text.kind === "comment" || !inside({ top: text.row, bottom: text.row, left: text.col, right: text.col + text.span - 1 })) continue;
    const target = textTarget(view, text);
    if (isPart(view, target)) found.push(target);
  }
  const boxes = found.filter((p) => p.kind === "box").map((p) => p.box);
  return found.filter((p) => !boxes.some((b) => b !== p.box && within(partCell(p), b)));
}

/** Parts as one selection: none is null, one is itself, and more are `{ kind: "parts", parts }` in row-major order. */
function groupOf(parts) {
  if (parts.length < 2) return parts[0] ?? null;
  return { kind: "parts", parts: [...parts].sort((a, b) => before(partCell(a), partCell(b))) };
}

/** What a part is, as a description counts it. */
function partNoun(part) {
  if (part.kind === "box") return switchLike(part.box) ? "switch" : "node";
  return part.text.kind === "word" && part.text.content !== NIL ? "inline node" : "constant";
}

/** Parts counted in words: "2 nodes and a constant". */
function countParts(parts) {
  const counts = new Map();
  for (const p of parts) counts.set(partNoun(p), (counts.get(partNoun(p)) ?? 0) + 1);
  const said = [...counts].map(([noun, n]) => (n === 1 ? `${/^[aeiou]/.test(noun) ? "an" : "a"} ${noun}` : `${n} ${noun === "switch" ? "switches" : `${noun}s`}`));
  return said.length > 1 ? `${said.slice(0, -1).join(", ")} and ${said.at(-1)}` : said[0];
}

/**
 * Everything `]` and `[` select, in row-major order of the cell each is
 * about. Boxes, texts (a constant or an input's name is a source), the
 * tees that are sources, arrowheads (a wired one selects its branch) and
 * wires that are editable.
 */
export function objects(view) {
  const list = [];
  for (const box of view.boxes) {
    list.push({ kind: "box", box });
    for (const divider of box.dividers) list.push(armTarget(box, divider));
  }
  const texts = new Set();
  for (const text of view.texts) {
    if (text.kind === "comment") continue;
    const target = textTarget(view, text);
    if (target.source) texts.add(keyOf(target.source));
    list.push(target);
  }
  for (const s of view.sources) {
    if (!texts.has(keyOf(s))) list.push({ kind: "source", at: { row: s.row, col: s.col, span: s.span }, source: s });
  }
  for (const s of view.sinks) list.push(s.wired ? { kind: "branch", sink: s } : { kind: "sink", sink: s });
  for (const wire of view.wires) if (wire.source) list.push({ kind: "wire", wire, at: wire.cells[0] });
  return list.sort((a, b) => before(anchorOf(a), anchorOf(b)) || ORDER.indexOf(a.kind) - ORDER.indexOf(b.kind));
}

/** A number constant's value, or null for any other selection. */
function numberOf(target) {
  if (target?.kind !== "source" || target.text?.kind !== "number") return null;
  return BigInt(target.text.content);
}

const HINTS = {
  definition: "Arrow keys nudge it; Enter renames it; Delete removes it with everything inside.",
  switch: "A adds an arm; arrow keys nudge it; Delete removes it with everything inside.",
  node: "Arrow keys nudge it; Enter renames it; I and O add an input or output, with Shift take one away; Delete removes it.",
  empty: "P on its top border adds a selector and A an arm, which make it a switch; Delete removes it.",
  arm: "Delete removes it with everything in it.",
  wire: "Delete removes the whole wire, every branch.",
  branch: "Delete disconnects just this branch; W rewires it to another source.",
  source: "W wires it to an arrowhead; Enter edits its text; Delete removes it.",
  sink: "W wires it from a source, or with none in reach feeds it a new one; Delete removes its port.",
  text: "Enter edits it.",
  unnamed: "Enter names it.",
  parts: "M makes a definition of them; Shift-click adds or takes one away.",
};

const NOUNS = { definition: "Definition", switch: "Switch", node: "Node", empty: "Empty box" };

/** A box's kind as the hints and nouns name it: an empty square box is its own. */
const boxNoun = (box) => (switchLike(box) && box.kind === "node" ? "empty" : box.kind);

/** Markdown as one line of plain words, for a live region. */
const plain = (markdown) => markdown.replace(/\*\*|`/g, "").replace(/\s*\n+\s*/g, ". ");

/** The definition port at a cell when it has no name (`Y009`), or null. */
function unnamedPort(view, at) {
  const held = portAt(view, at);
  return held?.box.kind === "definition" && !held.port.name ? held.port : null;
}

/**
 * The drawing's editing state. `host` gives it the page:
 *
 * - `apply(doc, edit)`: the wasm export;
 * - `text()`: the editor's text; `write(text)`: replaces it as one undo step;
 * - `doc(text)`, `view(text)`: the revision of a text and its `readView`;
 * - `seed(doc)`: keeps an edit's revision for the reads that follow, and
 *   returns its text;
 * - `refused(refusal, edit, then)`, `status(text, kind)`: say what
 *   happened; a refusal carries the edit and its follow-up, for Make room;
 * - `changed()`: the selection, cursor or held revision changed;
 * - `palette(at, mode, to)` (`"place"`, `"inline"` on a wire, or
 *   `"source"` to feed the unwired arrowhead `to`),
 *   `editText(target)`, `name(target)` (the inspector's field, focused),
 *   `extracting()` (Make definition's name field, focused), `help()`,
 *   `zoom(how)`: open things;
 * - `gloss()`: whether descriptions include readings;
 * - `portNames`: wasm's `port_names()`, the names new ports take.
 */
export class Studio {
  constructor(host) {
    this.host = host;
    this.names = host.portNames;
    this.selection = null;
    this.cursor = null;
    /** Make definition's name while its field is open: `{ name }`, the default. */
    this.extraction = null;
    /** A preview revision a held gesture is editing: `{ base, text, map, tidied }`. */
    this.held = null;
    /** `W` mode: the end being wired, and the glowing ends it may reach. */
    this.wiring = null;
    /** The arrow key holding a nudge. */
    this.nudging = null;
    this.scrub = null;
    /** `B`'s box being drawn from the keyboard: `{ from }`, its first corner, to the cursor. */
    this.sketch = null;
  }

  /** The text shown and edited: a held preview, or the editor's. */
  get text() {
    return this.held?.text ?? this.host.text();
  }

  view() {
    return this.host.view(this.text);
  }

  doc() {
    return this.host.doc(this.text);
  }

  /** The cells of the branch into a wired arrowhead, as the drawing outlines them. */
  branchCells(sink, view = this.view()) {
    return this.host.doc(view.src).branch(sink.row, sink.col).map((c) => ({ ...c, down: view.downs.has(keyOf(c)) }));
  }

  /** Completes a target for selecting: a branch gets its cells. */
  resolve(target, view = this.view()) {
    if (target?.kind !== "branch" || target.cells) return target;
    const cells = this.branchCells(target.sink, view);
    return cells.length ? { kind: "branch", sink: target.sink, cells } : null;
  }

  /** What a click on a cell selects, or null. */
  targetAt(at, view = this.view()) {
    const sink = sinkAt(view, at);
    if (sink) return this.resolve(sink.wired ? { kind: "branch", sink } : { kind: "sink", sink }, view);
    const text = textAt(view, at);
    if (text && text.kind !== "comment") return textTarget(view, text);
    const source = sourceAt(view, at);
    if (source) return { kind: "source", at: { row: source.row, col: source.col, span: source.span }, source };
    const wire = wireAt(view, at);
    if (wire) return { kind: "wire", wire, at };
    const arm = armAt(view, at);
    if (arm) return arm;
    const box = boxAt(view, at);
    return box ? { kind: "box", box } : null;
  }

  select(target, { quiet = false } = {}) {
    this.selection = target ? this.resolve(target) : null;
    if (this.selection) this.cursor = anchorOf(this.selection);
    this.wiring = null;
    this.sketch = null;
    this.extraction = null;
    if (!quiet && this.selection) this.host.status(this.describe(), "hint");
    this.host.changed();
  }

  /** Selects the next object after the selection or cursor, or with `back` the one before. */
  step(back = false) {
    const view = this.view();
    const list = objects(view);
    if (!list.length) return;
    const here = this.selection ? list.findIndex((o) => sameTarget(o, this.selection)) : -1;
    let i;
    if (here >= 0) i = (here + (back ? -1 : 1) + list.length) % list.length;
    else if (this.cursor) {
      const after = list.findIndex((o) => before(anchorOf(o), this.cursor) > 0);
      const at = list.findIndex((o) => before(anchorOf(o), this.cursor) >= 0);
      i = back ? (at <= 0 ? list.length - 1 : at - 1) : after < 0 ? 0 : after;
    } else i = back ? list.length - 1 : 0;
    // A branch disconnect would refuse can't be selected; step past it.
    for (let tries = 0; tries < list.length; tries++, i = (i + (back ? -1 : 1) + list.length) % list.length) {
      const target = this.resolve(list[i], view);
      if (target) return void this.select(target);
    }
  }

  /** What the selection is, in words: the live region and the inspector say it. */
  describe(target = this.selection) {
    if (!target) return this.cursor ? `Cell ${this.cursor.row + 1}:${this.cursor.col + 1}` : "";
    const doc = this.doc();
    const view = this.view();
    if (target.kind === "parts") {
      const at = anchorOf(target);
      return `${target.parts.length} parts selected from ${at.row + 1}:${at.col + 1}: ${countParts(target.parts)}. ${HINTS.parts}`;
    }
    const said = (at) => {
      const d = at && doc.describe(at.row, at.col, this.host.gloss?.() ?? false);
      return d ? plain(d.text) : "";
    };
    let what;
    let about = "";
    let hint = HINTS[target.kind];
    switch (target.kind) {
      case "box": {
        const noun = boxNoun(target.box);
        what = NOUNS[noun] ?? "Box";
        hint = HINTS[noun];
        const word = target.box.word ?? target.box.title ?? view.texts.find((t) => within(t, target.box) && t.kind === "word");
        about = said(word);
        break;
      }
      case "arm":
        what = target.guard ? `Arm guarded ${target.guard.content}` : "Default arm";
        break;
      case "wire":
        what = "Wire";
        break;
      case "branch":
        what = "Branch into the arrowhead";
        break;
      case "sink":
        what = "Unwired arrowhead";
        break;
      case "source":
        what = target.text?.kind === "number" ? `Number ${target.text.content}` : target.text ? `Source ${target.text.content}` : "Source";
        about = said(target.at);
        break;
      default:
        what = `Text ${target.text.content}`;
        about = said(target.at);
    }
    if (["sink", "branch", "source"].includes(target.kind) && !target.text && unnamedPort(view, anchorOf(target))) {
      what = `${what} of a port with no name`;
      hint = `${HINTS.unnamed} ${hint}`;
    }
    return [`${what} at ${anchorOf(target).row + 1}:${anchorOf(target).col + 1}.`, about && `${about}.`, hint]
      .filter(Boolean)
      .join(" ");
  }

  /**
   * The text Enter edits and the inspector's field holds for a selection:
   * a text itself, a node's word, a definition's name, or an arm's guard.
   * A definition's port with no name (`Y009`) has an empty one at its
   * arrowhead, where `set_text` writes the name.
   */
  editable(target = this.selection, view = this.view()) {
    if (!target) return null;
    if ((target.kind === "source" || target.kind === "text") && target.text) return target.text.kind === "comment" ? null : target;
    if (["sink", "branch", "source"].includes(target.kind) && unnamedPort(view, anchorOf(target))) {
      const { row, col } = anchorOf(target);
      return { kind: "text", at: { row, col, span: 1 }, text: { row, col, span: 1, kind: "word", content: "" } };
    }
    const text =
      target.kind === "box" ? (target.box.kind === "node" ? target.box.word : target.box.kind === "definition" ? target.box.title : null)
      : target.kind === "arm" ? target.guard
      : null;
    const found = text && textAt(view, text);
    return found ? textTarget(view, found) : null;
  }

  // Edits.

  /**
   * One gesture's edit, applied to the shown revision and written as one
   * undo step (two when the sheet had to be formatted first). False when
   * it was refused, which the host has been told. `then(map)` runs once
   * the edit is written and the selection has followed it, to select what
   * the edit drew; a refusal hands it on, so Make room can run it too.
   */
  edit(edit, then = null) {
    const result = this.host.apply(this.doc(), edit);
    if (!result.ok) {
      this.host.refused(result, edit, then);
      return false;
    }
    this.commit(this.settle(result), undefined, then);
    return true;
  }

  /** An edit's result as the studio keeps it: its revision handed to the host. */
  settle(result) {
    return { text: this.host.seed(result.doc), map: result.map, tidied: result.tidied };
  }

  /**
   * Writes a settled result: the formatted text first, as its own undo
   * step, when the edit was made on it, then the edit. The selection and
   * cursor follow the edit's map, and then `then(map)` runs.
   */
  commit({ text, map, tidied }, base = this.host.text(), then = null) {
    // Writing refreshes the page, which may drop a selection it can't find
    // on the new text; the map knows where it went.
    const [selection, cursor] = [this.selection, this.cursor];
    if (tidied) {
      const formatted = this.host.doc(base).formatted();
      if (formatted !== undefined && formatted !== base) this.host.write(formatted);
    }
    this.host.write(text);
    if (map) this.host.mapped?.(map);
    this.follow(map, selection, cursor);
    then?.(map);
    map?.free();
    if (tidied) this.host.status("Formatted the sheet to edit it", "hint");
  }

  /** Carries the selection and cursor through an edit's map, into the shown text. */
  follow(map, from = this.selection, cursor = this.cursor) {
    const view = this.view();
    this.cursor = cursor && map ? { ...map.map_cell(cursor.row, cursor.col) } : cursor;
    this.selection = from && map ? this.carry(from, map, view) : null;
    if (this.selection) this.cursor = anchorOf(this.selection);
    this.host.changed();
  }

  carry(target, map, view) {
    switch (target.kind) {
      case "parts":
        return groupOf(target.parts.map((p) => this.carry(p, map, view)).filter(Boolean));
      case "box": {
        const r = map.map_box(target.box.top, target.box.left);
        const box = r && view.boxes.find((b) => b.top === r.top && b.left === r.left);
        return box ? { kind: "box", box } : null;
      }
      case "arm": {
        const r = map.map_box(target.box.top, target.box.left);
        const box = r && view.boxes.find((b) => b.top === r.top && b.left === r.left);
        const { row } = map.map_cell(target.divider, target.box.left);
        const divider = box?.dividers.find((d) => d.row === row);
        return divider ? armTarget(box, divider) : null;
      }
      case "wire": {
        const index = map.map_wire(target.wire.wire);
        const wire = index === undefined ? wireAt(view, map.map_cell(target.at.row, target.at.col)) : view.wires[index];
        return wire?.source ? { kind: "wire", wire, at: wire.cells[0] } : null;
      }
      case "source":
      case "text": {
        const t = map.map_text(target.at.row, target.at.col);
        const text = t && view.texts.find((x) => x.row === t.row && x.col === t.col);
        if (text) return textTarget(view, text);
        const source = !target.text && sourceAt(view, map.map_cell(target.at.row, target.at.col));
        return source ? { kind: "source", at: { row: source.row, col: source.col, span: source.span }, source } : null;
      }
      default: {
        const at = map.map_cell(target.sink.row, target.sink.col);
        const sink = sinkAt(view, at);
        if (!sink) return null;
        return this.resolve(sink.wired ? { kind: "branch", sink } : { kind: "sink", sink }, view);
      }
    }
  }

  // Held gestures: a preview revision edited step by step from `base`, and
  // written once.

  /** Starts holding the editor's text, keeping the selection to carry. */
  hold() {
    this.held ??= { base: this.host.text(), text: this.host.text(), map: null, tidied: false, selection: this.selection, cursor: this.cursor };
    return this.held;
  }

  /**
   * Tries one step of a held gesture: `edit` applied to the held base.
   * Done, it becomes the preview; refused, the preview stays at the last
   * step that fit. Returns the refusal, or null.
   */
  try(edit) {
    const held = this.hold();
    const result = this.host.apply(this.host.doc(held.base), edit);
    if (!result.ok) return result;
    const settled = this.settle(result);
    held.map?.free();
    Object.assign(held, settled);
    this.follow(settled.map, held.selection, held.cursor);
    this.host.previewed?.(held.text);
    return null;
  }

  /** Ends a held gesture: its last preview is written as one undo step. */
  release() {
    const held = this.held;
    if (!held) return;
    this.held = null;
    if (held.text === held.base) return void this.host.changed();
    this.selection = held.selection;
    this.cursor = held.cursor;
    this.commit(held, held.base);
  }

  /** Drops a held gesture without writing anything. */
  drop() {
    if (!this.held) return;
    this.held.map?.free();
    this.selection = this.held.selection;
    this.cursor = this.held.cursor;
    this.held = null;
    this.scrub = null;
    this.nudging = null;
    this.host.changed();
  }

  /** A nudge of the selected box by one cell, held until its key comes up. */
  nudge(key, dr, dc) {
    const box = (this.held?.selection ?? this.selection)?.box;
    if (!box) return;
    if (this.nudging && this.nudging.key !== key) this.release();
    this.hold();
    this.nudging ??= { key, dr: 0, dc: 0 };
    const next = { dr: this.nudging.dr + dr, dc: this.nudging.dc + dc };
    const edit = { kind: "move", corner: { row: box.top, col: box.left }, ...next };
    const refusal = this.try(edit);
    // Kept until the key comes up, when Make room can be offered from
    // where the box stopped.
    this.nudging.refused = refusal ? { refusal, dr, dc } : null;
    if (refusal) return void this.host.refused(refusal, edit);
    Object.assign(this.nudging, next);
  }

  // Scrubbing (PLAN P10, Decisions: Scrubbing): a selected number constant
  // dragged sideways, one step per SCRUB_PX, ten with Shift.

  /** Whether the selection can be scrubbed. */
  get scrubbable() {
    return numberOf(this.selection) !== null;
  }

  scrubStart() {
    const value = numberOf(this.selection);
    if (value === null) return false;
    this.hold();
    this.scrub = { at: { row: this.selection.at.row, col: this.selection.at.col }, from: value, value };
    return true;
  }

  /** The pointer is `dx` pixels right of where the scrub started. */
  scrubMove(dx, shift = false) {
    const s = this.scrub;
    if (!s) return;
    const steps = BigInt(Math.trunc(dx / SCRUB_PX)) * (shift ? 10n : 1n);
    let value = s.from + steps;
    value = value < 0n ? 0n : value > LARGEST ? LARGEST : value;
    if (value === s.value) return;
    // The text grows at its left end, so its first cell moves with it; the
    // held base still has it where the scrub began.
    const refusal = this.try({ kind: "set_text", at: s.at, content: String(value) });
    // Out of room for another digit: the value stops at the last that fit.
    if (refusal) return void this.host.status(refusal.reason, "refused");
    s.value = value;
  }

  scrubEnd() {
    if (!this.scrub) return;
    this.scrub = null;
    this.release();
  }

  // The keyboard model (PLAN P10, Gesture details: Keyboard).

  /**
   * A key went down on the drawing. True when the drawing took it, and the
   * page should prevent its default; Tab is never taken, so focus leaves.
   */
  keydown(e) {
    const mod = e.metaKey || e.ctrlKey || e.altKey;
    if (e.key === "Tab" || mod) return false;
    if (this.wiring) return this.wiringKey(e);
    const arrows = { ArrowUp: [-1, 0], ArrowDown: [1, 0], ArrowLeft: [0, -1], ArrowRight: [0, 1] };
    if (arrows[e.key]) {
      const [dr, dc] = arrows[e.key];
      if (!this.sketch && (this.held?.selection ?? this.selection)?.kind === "box") this.nudge(e.key, dr, dc);
      else this.moveCursor(dr, dc);
      return true;
    }
    if (e.repeat && this.held) return true;
    if (this.held) this.release();
    if (this.sketch && (e.key === "Enter" || e.key === "Escape")) {
      const { from, mode } = this.sketch;
      this.sketch = null;
      const rect = rectOf(from, this.cursor);
      if (e.key === "Escape") this.host.status("");
      else if (mode === "select") this.selectParts(partsIn(this.view(), rect), { add: e.shiftKey });
      else this.placeBox(rect);
      this.host.changed();
      return true;
    }
    switch (e.key) {
      case "]":
      case "[":
        if (e.shiftKey) this.extend(e.key === "[");
        else this.step(e.key === "[");
        return true;
      // Shift+] and Shift+[ on most layouts.
      case "}":
      case "{":
        this.extend(e.key === "{");
        return true;
      case "s":
      case "S":
        this.startSketch("select");
        return true;
      case "m":
      case "M":
        this.startExtract();
        return true;
      case "f":
      case "F":
        this.repairHere();
        return true;
      case "Enter": {
        const target = this.editable();
        if (target) this.host.editText(target);
        else this.host.status("Enter edits a selected text or renames a selected node: select one with ] or [.", "hint");
        return true;
      }
      case "w":
      case "W":
        this.startWiring();
        return true;
      case "/": {
        const at = this.cursor ?? { row: 0, col: 0 };
        this.host.palette(at, this.inlineAt(at) ? "inline" : "place");
        return true;
      }
      case "b":
      case "B":
        this.startSketch();
        return true;
      case "p":
      case "P":
        if (this.cursor) this.addPort(this.cursor);
        else this.host.status("P adds a port at the cursor: move it onto a box's border.", "hint");
        return true;
      case "a":
      case "A":
        this.addArm();
        return true;
      case "i":
      case "I":
      case "o":
      case "O":
        this.stepPorts(e.key.toLowerCase() === "i" ? "inputs" : "outputs", e.key === e.key.toUpperCase() ? -1 : 1);
        return true;
      case "Escape":
        if (this.selection) this.select(null);
        else if (this.cursor) (this.cursor = null), this.host.changed();
        this.host.status("");
        return true;
      case "Delete":
      case "Backspace":
        this.remove();
        return true;
      case "?":
        this.host.help();
        return true;
      case "r":
      case "R":
      case "c":
      case "C":
        this.room(e.key.toLowerCase() === "r" ? "row" : "column", e.key === e.key.toUpperCase() ? "delete" : "insert");
        return true;
      case "+":
      case "=":
        this.host.zoom("in");
        return true;
      case "-":
        this.host.zoom("out");
        return true;
      case "0":
        this.host.zoom("fit");
        return true;
      default:
        return false;
    }
  }

  /** A key came up: the end of a held nudge. */
  keyup(e) {
    if (this.nudging && e.key === this.nudging.key) {
      const blocked = this.nudging.refused;
      this.nudging = null;
      this.release();
      // The last step was refused: say so again from where the box is now,
      // with the one step that didn't fit, so Make room can be offered.
      const box = this.selection?.box;
      if (blocked && box) {
        this.host.refused(blocked.refusal, { kind: "move", corner: { row: box.top, col: box.left }, dr: blocked.dr, dc: blocked.dc });
      }
      return true;
    }
    return false;
  }

  moveCursor(dr, dc) {
    const c = this.cursor ?? { row: 0, col: 0 };
    this.cursor = { row: Math.max(0, c.row + dr), col: Math.max(0, c.col + dc) };
    this.selection = null;
    if (this.sketch) {
      const r = rectOf(this.sketch.from, this.cursor);
      const size = `${r.bottom - r.top + 1} rows by ${r.right - r.left + 1} columns`;
      if (this.sketch.mode === "select") {
        const n = partsIn(this.view(), r).length;
        this.host.status(`A rectangle ${size}, round ${n} part${n === 1 ? "" : "s"}. Enter selects them.`, "hint");
      } else this.host.status(`A box ${size}. Enter draws it.`, "hint");
    } else this.host.status(this.describe(), "hint");
    this.host.changed();
  }

  /** What Delete sends for a selection, or null when it removes nothing. */
  removal(target, view = this.view()) {
    const at = anchorOf(target);
    switch (target.kind) {
      case "box":
        // A node goes as `remove_node`, so its wires go as Phase 1 took them.
        return target.box.kind === "node" && target.box.word ? { kind: "remove_node", at } : { kind: "remove_box", at };
      case "arm":
        return { kind: "remove_arm", at };
      case "branch":
        return { kind: "disconnect", at };
      case "wire":
        return { kind: "remove_wire", at: cellOf(target.at) };
      case "sink":
        return portAt(view, target.sink) ? { kind: "remove_port", at } : null;
      case "source": {
        if (target.text && target.text.kind !== "word") return { kind: "remove_text", at };
        if (portAt(view, target.source)) return { kind: "remove_port", at: cellOf(target.source) };
        // A word that is a source and no port's name is an inline node's.
        return target.text ? { kind: "remove_node", at: cellOf(target.source) } : null;
      }
      default: {
        // A definition output's name, outside its box: the port goes with it.
        const named = target.text && portNamed(view, target.text);
        return named ? { kind: "remove_port", at: cellOf(named.port) } : null;
      }
    }
  }

  remove() {
    const target = this.selection;
    if (!target) return;
    const edit = this.removal(target);
    if (!edit) return void this.host.status("Delete removes a selected box, arm, port, wire, branch or constant.", "hint");
    this.selection = null;
    this.edit(edit);
  }

  // Drawing boxes, ports and arms (PLAN P10, Gesture details: Drawing
  // boxes), each gesture one edit.

  /** Connects a source to an arrowhead: a click on one and then the other. */
  connect(from, to) {
    return this.edit({ kind: "connect", from: cellOf(from), to: cellOf(to) });
  }

  /**
   * The edit the Box tool makes for a rectangle: a definition where its
   * top-left corner is at top level, named `definitionName`, and otherwise
   * a switch, in the definition or arm around it.
   */
  boxEdit(rect, view = this.view()) {
    const r = { top: rect.top, left: rect.left, bottom: rect.bottom, right: rect.right };
    if (!boxAt(view, { row: r.top, col: r.left })) return { kind: "place_definition", rect: r, name: definitionName(view, this.names) };
    return { kind: "place_switch", rect: r, guard: FIRST_GUARD };
  }

  /**
   * Draws a box at a rectangle. A new definition's name is selected and
   * its field opened, so typing renames it; a new switch is selected, so
   * `+ arm` is at hand.
   */
  placeBox(rect) {
    const edit = this.boxEdit(rect);
    return this.edit(edit, (map) => {
      const view = this.view();
      if (edit.kind === "place_definition") {
        const made = view.boxes.find((b) => b.kind === "definition" && b.title?.content === `·${edit.name}`);
        if (made) this.name(made.title, view);
        return;
      }
      const corner = map ? map.map_cell(rect.top, rect.left) : { row: rect.top, col: rect.left };
      const made = view.boxes.find((b) => b.top === corner.row && b.left === corner.col);
      if (made) this.select({ kind: "box", box: made });
    });
  }

  /**
   * Adds a port at a border cell. A definition's port takes `portName`,
   * and its name is selected with its field open, so typing renames it.
   */
  addPort(at) {
    const box = borderAt(this.view(), at);
    if (!box) return void this.host.status("A port goes on a box's border: put the cursor on one.", "hint");
    const cell = cellOf(at);
    if (box.kind !== "definition") return this.edit({ kind: "add_port", at: cell, name: null });
    return this.edit({ kind: "add_port", at: cell, name: portName(box, this.names) }, (map) => {
      const view = this.view();
      const port = portAt(view, map ? map.map_cell(at.row, at.col) : cell)?.port;
      if (port?.name) this.name(port.name, view);
    });
  }

  /** Selects a text an edit just drew and opens its field, for naming it. */
  name(text, view = this.view()) {
    const found = textAt(view, text);
    if (!found) return;
    this.select(textTarget(view, found), { quiet: true });
    this.host.name?.(this.selection);
  }

  /** The selected switch or empty box, or else the one holding the cursor. */
  switchAt() {
    if (this.selection?.kind === "box" || this.selection?.kind === "arm") {
      return switchLike(this.selection.box) ? this.selection.box : null;
    }
    const box = this.cursor && boxAt(this.view(), this.cursor);
    return box && switchLike(box) ? box : null;
  }

  /**
   * Where a new divider goes in an arm of `box`: the first row under the
   * last one holding anything inside the box, with no through-port at
   * either end, so each arm keeps a row. With no such row it is the arm's
   * last, which the edit refuses with what to try.
   */
  dividerRow(box, arm, view = this.view()) {
    const inside = (row) => [...(view.lines[row] ?? "")].slice(box.left + 1, box.right).some((c) => c !== " ");
    const ported = (row) => box.ports.some((p) => p.row === row);
    let last = arm.top - 1;
    for (let row = arm.top; row <= arm.bottom; row++) if (inside(row)) last = row;
    for (let row = Math.max(last + 1, arm.top + 1); row < arm.bottom; row++) if (!ported(row)) return row;
    return arm.bottom;
  }

  /**
   * `+ arm`: a divider for a new arm. A switch without a default arm gets
   * one at the bottom; with one, the new arm goes just above it, guarded
   * by the smallest integer no arm has, so the default stays last.
   */
  addArm(box = this.switchAt()) {
    if (!box) return void this.host.status("A adds an arm to a selected switch: select one with ] or [.", "hint");
    const arms = armsOf(box);
    const open = arms.every((a) => a.guard);
    const arm = open ? arms.at(-1) : (arms.at(-2) ?? arms.at(-1));
    const guard = open ? null : nextGuard(arms);
    return this.edit({ kind: "add_arm", at: { row: this.dividerRow(box, arm), col: box.left }, guard });
  }

  /**
   * The revision an edit would make of the shown one, for a ghost, or null
   * when it would be refused. Nothing is written; the caller frees it.
   */
  trial(edit) {
    const result = this.host.apply(this.doc(), edit);
    if (!result.ok) return null;
    result.map?.free();
    return result.doc;
  }

  /** The refusal an edit would meet on the shown revision, or null: a trial, for a live verdict. Nothing is written. */
  verdict(edit) {
    const trial = this.host.apply(this.doc(), edit);
    if (!trial.ok) return trial;
    trial.map?.free();
    trial.doc.free();
    return null;
  }

  /** Gives a node box `inputs` inputs and `outputs` outputs: the inspector's steppers. */
  setPorts(box, inputs, outputs) {
    return this.edit({ kind: "set_ports", corner: { row: box.top, col: box.left }, inputs, outputs });
  }

  /** I and O: one more port (`by` 1) or one fewer (-1) on the selected node's `side`, `"inputs"` or `"outputs"`. */
  stepPorts(side, by) {
    const box = this.selection?.kind === "box" ? this.selection.box : null;
    if (!box || box.kind !== "node" || !box.word) {
      return void this.host.status("I and O change a selected node's ports: select one with ] or [.", "hint");
    }
    const counts = portCounts(box);
    if (counts[side] + by < 0) return void this.host.status(`It has no ${side} to take away.`, "hint");
    counts[side] += by;
    return this.setPorts(box, counts.inputs, counts.outputs);
  }

  /**
   * Whether a node can be spliced into a wire at a cell, so a double-click
   * or `/` there opens the palette of 1→1 callees: the cell is a straight
   * horizontal stretch of a wire. The edit checks the stretch's length.
   */
  inlineAt(at, view = this.view()) {
    return (view.byCell.get(keyOf(at))?.length ?? 0) > 0 && ["─", "-"].includes(charOf(view, at));
  }

  /** A palette pick's edit. A node spliced into a wire is selected, its word at the cell picked. */
  pick(edit) {
    if (edit.kind !== "place_inline") return this.edit(edit);
    return this.edit(edit, (map) => {
      const view = this.view();
      const to = map ? map.map_cell(edit.at.row, edit.at.col) : edit.at;
      const text = textAt(view, to);
      if (text) this.select(textTarget(view, text), { quiet: true });
    });
  }

  /**
   * One `set_text` for a text's new content, from the inspector or in
   * place: a number, a name, a node's word. A definition's name keeps its
   * namer dot, typed or not.
   */
  retext(target, content) {
    const text = target.text;
    if (!text || content === text.content) return false;
    if (text.content.startsWith("·") && !content.startsWith("·")) content = `·${content}`;
    return this.edit({ kind: "set_text", at: cellOf(target.at), content });
  }

  /**
   * B: a box from the cursor, stretched by the arrow keys and drawn by
   * Enter. S (`mode` `"select"`): a rectangle the same way, and Enter
   * selects the parts inside it, as a marquee does.
   */
  startSketch(mode = "box") {
    this.cursor ??= { row: 0, col: 0 };
    this.selection = null;
    this.sketch = { from: { ...this.cursor }, mode };
    const said = mode === "select"
      ? "Selecting: arrow keys stretch the rectangle, Enter selects the nodes, switches and constants inside, Escape stops."
      : "Drawing a box: arrow keys stretch it, Enter draws it, Escape stops.";
    this.host.status(said, "hint");
    this.host.changed();
  }

  /** The rectangle `B` or `S` is drawing, or null. */
  get sketched() {
    return this.sketch && this.cursor ? rectOf(this.sketch.from, this.cursor) : null;
  }

  // Multiple selection and Make definition (PLAN P10, Gesture details:
  // Extracting a definition).

  /** The parts the selection names: its parts, the selection itself when it is one, or none. */
  get parts() {
    const s = this.selection;
    if (s?.kind === "parts") return s.parts;
    return s && isPart(this.view(), s) ? [s] : [];
  }

  /** Selects `found`, or with `add` the parts already selected and `found`. */
  selectParts(found, { add = false } = {}) {
    const list = add ? [...this.parts] : [];
    for (const p of found) if (!list.some((q) => sameTarget(q, p))) list.push(p);
    this.select(groupOf(list));
  }

  /** Shift-click: adds a part to the selection, or takes it away. */
  toggle(part) {
    if (!isPart(this.view(), part)) {
      return void this.host.status("Shift-click adds a node, a switch or a constant to the selection.", "hint");
    }
    const list = this.parts.filter((p) => !sameTarget(p, part));
    if (list.length === this.parts.length) list.push(part);
    this.select(groupOf(list));
  }

  /** Shift+] and Shift+[: adds the next or previous part after the cursor to the selection. */
  extend(back = false) {
    const view = this.view();
    const chosen = this.parts;
    const list = objects(view).filter((o) => isPart(view, o) && !chosen.some((p) => sameTarget(p, o)));
    if (!list.length) return void this.host.status("Every node, switch and constant is selected.", "hint");
    const here = this.cursor ?? { row: 0, col: 0 };
    const after = list.findIndex((o) => before(anchorOf(o), here) > 0);
    const at = list.findIndex((o) => before(anchorOf(o), here) >= 0);
    const next = back ? list[at <= 0 ? list.length - 1 : at - 1] : list[after < 0 ? 0 : after];
    this.selectParts([next], { add: true });
    // The next step goes on from the part just added.
    this.cursor = anchorOf(next);
    this.host.changed();
  }

  /** M, or Make definition: opens the name field for a definition of the selected parts, with a free name in it. */
  startExtract() {
    if (!this.parts.length) {
      const why = "M makes a definition of the selected nodes, switches and constants: Shift-click them, or drag round them on blank sheet.";
      return void this.host.status(why, "hint");
    }
    this.extraction = { name: definitionName(this.view(), this.names, { main: false }) };
    this.host.changed();
    this.host.extracting?.();
  }

  /**
   * Takes the selected parts out into a new definition titled `·name`,
   * placed below the sheet and called where they were, as one edit. The
   * call is selected. Refused, the name field stays open to correct it.
   */
  extract(name = this.extraction?.name) {
    const parts = this.parts;
    if (!parts.length || !name) return false;
    const word = name.replace(/^·/, "");
    return this.edit({ kind: "extract", picks: parts.map(partCell), name: word }, () => {
      this.extraction = null;
      const view = this.view();
      const call = view.boxes.find((b) => b.kind === "node" && b.word?.content === word);
      if (call) this.select({ kind: "box", box: call }, { quiet: true });
      this.host.status(`Made ·${word} below the sheet, and called it here.`, "hint");
    });
  }

  // Repairs (PLAN P10, Decisions: Repairs), as `Doc.repairs` offers them.

  /**
   * Applies a repair: its edit as one undo step, then its follow-up. A
   * repair that is all follow-up edits nothing. False when its edit was
   * refused, which the host has been told, with the follow-up for Make
   * room to run.
   */
  repair(r) {
    if (!r.edit) {
      this.followUp(r.then);
      return true;
    }
    return this.edit(r.edit, () => this.followUp(r.then));
  }

  /**
   * A repair's follow-up, at a cell of the sheet its edit left: the name
   * field opens on what is there, or wiring starts from it, as `W` does.
   */
  followUp(then) {
    if (then?.kind === "name_field") this.nameAt(then.at);
    else if (then?.kind === "draw_wire") this.wireFrom(then.at);
  }

  /** Selects what is at a cell and opens the inspector's name field on it. */
  nameAt(at) {
    const view = this.view();
    const target = this.targetAt(at, view);
    if (!this.editable(target, view)) return void this.host.status(`Nothing at ${at.row + 1}:${at.col + 1} takes a name.`, "hint");
    this.select(target, { quiet: true });
    this.host.name?.(this.selection);
  }

  /** Selects the source or arrowhead at a cell and starts wiring it, as `W` does. */
  wireFrom(at) {
    this.select(this.targetAt(at), { quiet: true });
    this.startWiring();
  }

  /** F: the repair for the diagnostic at the cursor, or else for one the selection covers. */
  repairHere() {
    const list = this.doc().repairs();
    const found = list.find((r) => same(r.at, this.cursor)) ?? list.find((r) => this.selection && covers(this.selection, r.at));
    if (!found) return void this.host.status("F fixes a diagnostic at the cursor that has a known fix, and there is none here.", "hint");
    return this.repair(found);
  }

  /** Inserts or deletes a row or column at the cursor. */
  room(axis, how) {
    if (!this.cursor) return void this.host.status("Put the cursor in a box first: ] selects, arrow keys move it.", "hint");
    this.edit({ kind: how, axis, at: { ...this.cursor }, count: 1 });
  }

  // `W`: wiring from the keyboard.

  /** The ends the selection may be wired to, row-major. */
  endsFor(target, view = this.view()) {
    if (target?.kind === "source") {
      const scope = target.source.scope;
      return view.sinks.filter((s) => !s.wired && s.scope === scope);
    }
    if (target?.kind === "sink" || target?.kind === "branch") {
      return view.sources.filter((s) => s.scope === target.sink.scope);
    }
    return [];
  }

  /**
   * W: wiring from the selected source or arrowhead. An unwired arrowhead
   * with no source in reach, as in a new arm, opens the palette of sources
   * instead, to feed it one (Decisions: Constants by mouse).
   */
  startWiring() {
    const target = this.selection;
    const ends = this.endsFor(target);
    if (!ends.length && target?.kind === "sink") {
      this.host.status("No source is in reach, so pick a new one to feed the arrowhead.", "hint");
      return void this.host.palette(cellOf(target.sink), "source", cellOf(target.sink));
    }
    if (!ends.length) {
      const why = target ? "Nothing in reach to wire it to." : "W wires a selected source or arrowhead: select one with ] or [.";
      return void this.host.status(why, "hint");
    }
    this.wiring = { from: target, ends, index: 0 };
    this.host.status("Wiring: ] and [ pick an end, Enter connects, Escape stops.", "hint");
    this.host.changed();
  }

  /** The glowing end `W` would wire to. */
  get end() {
    return this.wiring ? this.wiring.ends[this.wiring.index] : null;
  }

  wiringKey(e) {
    const w = this.wiring;
    if (e.key === "]" || e.key === "[" || e.key === "ArrowDown" || e.key === "ArrowUp") {
      const by = e.key === "[" || e.key === "ArrowUp" ? -1 : 1;
      w.index = (w.index + by + w.ends.length) % w.ends.length;
      this.host.status(`Wire to ${this.end.row + 1}:${this.end.col + 1}? Enter connects.`, "hint");
      this.host.changed();
      return true;
    }
    if (e.key === "Escape") {
      this.wiring = null;
      this.host.status("");
      this.host.changed();
      return true;
    }
    if (e.key !== "Enter") return true;
    const end = this.end;
    const from = w.from;
    this.wiring = null;
    const cell = (c) => ({ row: c.row, col: c.col });
    if (from.kind === "source") this.edit({ kind: "connect", from: cell(from.source), to: cell(end) });
    else if (from.kind === "sink") this.edit({ kind: "connect", from: cell(end), to: cell(from.sink) });
    else this.edit({ kind: "rewire", from: cell(end), to: cell(from.sink) });
    this.host.changed();
    return true;
  }
}

/**
 * The Box tool's pointer gestures, without the page (PLAN P10, Gesture
 * details: Drawing boxes). While it is on, a drag that starts on blank
 * sheet draws a box, and a press let go on the plain border it started on
 * adds a port there. The page hands it its pointer events; `locate(e)`
 * gives the cell under one, or null off the sheet.
 */
export class BoxTool {
  constructor(studio, locate) {
    this.studio = studio;
    this.locate = locate;
    this.on = false;
    /** The press under way: `{ kind, start, at, pointerId, verdict }`, `kind` `"rect"` or `"port"`. */
    this.drag = null;
  }

  /** A press: true when the tool takes it, so nothing else does. */
  down(e) {
    if (!this.on || this.drag || e.button !== 0) return false;
    const at = this.locate(e);
    if (!at) return false;
    const view = this.studio.view();
    const kind = portSpot(view, at) ? "port" : charOf(view, at) === " " ? "rect" : null;
    if (!kind) return false;
    this.drag = { kind, start: at, at, pointerId: e.pointerId, verdict: null };
    return true;
  }

  /**
   * The pointer moved. A drawn box gets a live verdict, as a moved box
   * does: the refusal its edit would meet, or null. True when the box to
   * show changed.
   */
  move(e) {
    const d = this.drag;
    if (!d || e.pointerId !== d.pointerId) return false;
    const at = this.locate(e);
    if (!at || same(at, d.at)) return false;
    d.at = at;
    if (d.kind === "rect") d.verdict = this.studio.verdict(this.studio.boxEdit(rectOf(d.start, at)));
    return d.kind === "rect";
  }

  /**
   * The press ends: a box is drawn where a drag ended, or a port added
   * where a press began and ended. Returns `"edit"` when it sent one,
   * `"click"` for a press on blank sheet let go where it began (a click,
   * for the page to handle), and null otherwise.
   */
  up(e) {
    const d = this.drag;
    if (!d || e.pointerId !== d.pointerId) return null;
    this.drag = null;
    const at = this.locate(e);
    if (!at) return null;
    if (d.kind === "port") return same(at, d.start) ? (this.studio.addPort(at), "edit") : null;
    if (same(at, d.start)) return "click";
    this.studio.placeBox(rectOf(d.start, at));
    return "edit";
  }

  /** Drops the press under way without an edit. */
  cancel() {
    this.drag = null;
  }

  /** The box a drag would draw, or null. */
  get rect() {
    return this.drag?.kind === "rect" ? rectOf(this.drag.start, this.drag.at) : null;
  }
}

/**
 * The marquee, without the page: with the Box tool off, a drag that
 * starts on blank sheet outside any node box selects the nodes, switches
 * and constants wholly inside the rectangle it sweeps, and with Shift adds
 * them to the selection. So definitions and switches move by their
 * borders and titles, and node boxes by any of their cells. `locate(e)`
 * gives the cell under a pointer event, or null off the sheet; `tool` is
 * the Box tool, which takes blank sheet while it is on.
 */
export class Marquee {
  constructor(studio, locate, tool) {
    Object.assign(this, { studio, locate, tool });
    /** The drag under way: `{ start, at, pointerId, add, parts }`, `parts` those it would select. */
    this.drag = null;
  }

  /** Whether a press at a cell would start a marquee: blank sheet outside any node box, with the Box tool off. */
  takes(at) {
    if (this.tool?.on) return false;
    const view = this.studio.view();
    const box = boxAt(view, at);
    return charOf(view, at) === " " && !(box?.kind === "node" && !switchLike(box));
  }

  /** A press: true when the marquee takes it, so nothing else does. */
  down(e) {
    if (this.drag || e.button !== 0) return false;
    const at = this.locate(e);
    if (!at || !this.takes(at)) return false;
    this.drag = { start: at, at, pointerId: e.pointerId, add: e.shiftKey, parts: [] };
    return true;
  }

  /** The pointer moved: true when the rectangle, and the parts it would select, changed. */
  move(e) {
    const d = this.drag;
    if (!d || e.pointerId !== d.pointerId) return false;
    const at = this.locate(e);
    if (!at || same(at, d.at)) return false;
    d.at = at;
    d.parts = partsIn(this.studio.view(), this.rect);
    return true;
  }

  /**
   * The press ends. Returns `"select"` when the drag selected what it
   * swept, `"click"` for a press let go where it began (for the page to
   * handle as a click), and null otherwise.
   */
  up(e) {
    const d = this.drag;
    if (!d || e.pointerId !== d.pointerId) return null;
    const at = this.locate(e) ?? d.at;
    if (same(at, d.start)) {
      this.drag = null;
      return "click";
    }
    d.at = at;
    const parts = partsIn(this.studio.view(), this.rect);
    this.drag = null;
    this.studio.selectParts(parts, { add: d.add });
    return "select";
  }

  /** Drops the drag under way without selecting. */
  cancel() {
    this.drag = null;
  }

  /** The rectangle swept, or null. */
  get rect() {
    return this.drag ? rectOf(this.drag.start, this.drag.at) : null;
  }
}

/** How long a Fix button is hovered or focused before its repair is drawn as a ghost, in milliseconds. */
export const GHOST_MS = 150;

/**
 * A repair's ghost (PLAN P10, Decisions: Repairs), without the page: once
 * its Fix button has been hovered or focused for `GHOST_MS`, the sheet its
 * edit would leave is drawn over the drawing until the pointer or the focus
 * leaves. `trial(edit)` gives that revision, or null when the edit would
 * be refused, which the press says; `show(doc)` and `hide()` draw and
 * remove it; `timers` stands in for the page's in tests. A repair that is
 * all follow-up has no ghost.
 */
export class RepairGhost {
  constructor({ trial, show, hide, timers = globalThis }) {
    Object.assign(this, { trial, show, hide, timers });
    this.timer = null;
    /** The revision drawn as the ghost, freed when it goes. */
    this.shown = null;
  }

  /** A Fix button was hovered or focused. */
  enter(repair) {
    this.leave();
    if (!repair.edit) return;
    this.timer = this.timers.setTimeout(() => {
      this.timer = null;
      this.shown = this.trial(repair.edit);
      if (this.shown) this.show(this.shown);
    }, GHOST_MS);
  }

  /** The pointer or the focus left it, or it was pressed. */
  leave() {
    if (this.timer !== null) this.timers.clearTimeout(this.timer);
    this.timer = null;
    if (!this.shown) return;
    this.hide();
    this.shown.free();
    this.shown = null;
  }
}

/** Every key the drawing takes, for the `?` overlay. */
export const KEYS = [
  ["] [", "Select the next or previous object"],
  ["Shift+] Shift+[", "Add the next or previous node, switch or constant to the selection"],
  ["Shift + click", "Add a node, switch or constant to the selection, or take it away"],
  ["Drag on blank sheet", "Select the nodes, switches and constants inside; with Shift, add them"],
  ["S", "Select from the cursor: arrow keys stretch a rectangle, Enter selects the nodes, switches and constants inside, Shift+Enter adds them"],
  ["M", "Make a definition of the selection: type its name, Enter makes it below the sheet and calls it here"],
  ["F", "Fix the diagnostic at the cursor, as its Fix button in Diagnostics does"],
  ["Arrow keys", "Nudge a selected box, or move the cursor"],
  ["Enter", "Edit a selected text, or rename a selected node or definition"],
  ["W", "Wire a selected source or arrowhead: ] and [ pick an end, Enter connects. An arrowhead with no source in reach gets the palette of sources"],
  ["Delete", "Remove the selected box, arm, port, wire, branch or constant"],
  ["/", "Open the palette at the cursor; on a wire, the 1→1 callees to splice into it"],
  ["B", "Draw a box from the cursor: arrow keys stretch it, Enter draws it, a definition at top level or a switch inside one"],
  ["P", "Add a port at the cursor, on a box's border"],
  ["A", "Add an arm to the selected switch"],
  ["I, Shift+I", "One more or one fewer input on the selected node"],
  ["O, Shift+O", "One more or one fewer output on the selected node"],
  ["R, Shift+R", "Insert or delete a row at the cursor"],
  ["C, Shift+C", "Insert or delete a column at the cursor"],
  ["+ −", "Zoom in or out"],
  ["0", "Fit the sheet in the pane"],
  ["Ctrl or ⌘ + wheel", "Zoom about the pointer; a pinch does too"],
  ["Space + drag", "Pan"],
  ["Ctrl or ⌘ + Z", "Undo; with Shift, redo"],
  ["Escape", "Stop drawing a box, selecting or wiring, leave the Box tool, clear the selection, then the cursor"],
  ["Tab", "Leave the drawing"],
  ["?", "Show or hide this list"],
];
