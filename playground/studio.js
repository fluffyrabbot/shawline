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

/**
 * The names a definition's new port takes, the first not already one of
 * its ports' names: PLAN P10's naming for extracted ports, 𐑨, 𐑚, 𐑒, …,
 * the letters typed a, b, c, … in alphabet order.
 */
export const PORT_NAMES = [..."𐑨𐑚𐑒𐑛𐑧𐑓𐑜𐑣𐑦𐑡𐑤𐑥𐑯𐑪𐑐𐑮𐑕𐑑𐑳𐑝𐑢𐑘𐑟"];

/** The entry point's name (SPEC §5): a new definition takes it while the sheet has none. */
export const MAIN = "𐑥𐑱𐑯";

/**
 * A new definition's name once the sheet has a main: 𐑯𐑿 ("new"), then
 * 𐑯𐑿𐑨, 𐑯𐑿𐑚, … A single letter would be reserved (`Y013`).
 */
const NEW = "𐑯𐑿";

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

/** The name the Box tool gives a new definition: 𐑥𐑱𐑯 while the sheet has none. */
export function definitionName(view) {
  const taken = new Set(
    view.boxes.filter((b) => b.kind === "definition" && b.title).map((b) => b.title.content.replace(/^·/, "")),
  );
  return firstFree([MAIN, NEW, ...PORT_NAMES.map((n) => NEW + n)], taken);
}

/** The name a definition's new port takes: the first of `PORT_NAMES` none of its ports has. */
export function portName(box) {
  return firstFree(PORT_NAMES, new Set(box.ports.map((p) => p.name?.content).filter(Boolean)));
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
    case "wire":
      return { row: target.at.row, col: target.at.col };
    default:
      return { row: target.at.row, col: target.at.col };
  }
}

/** Whether two selections name the same thing. */
export function sameTarget(a, b) {
  if (!a || !b || a.kind !== b.kind) return false;
  if (a.kind === "wire") return a.wire.wire === b.wire.wire;
  return same(anchorOf(a), anchorOf(b));
}

/** The source or text selection for a text: a source when a wire may start there. */
function textTarget(view, text) {
  const source = sourceAt(view, text);
  const at = { row: text.row, col: text.col, span: text.span };
  return source ? { kind: "source", at, source, text } : { kind: "text", at, text };
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
  sink: "W wires it from a source; Delete removes its port.",
  text: "Enter edits it.",
};

const NOUNS = { definition: "Definition", switch: "Switch", node: "Node", empty: "Empty box" };

/** A box's kind as the hints and nouns name it: an empty square box is its own. */
const boxNoun = (box) => (switchLike(box) && box.kind === "node" ? "empty" : box.kind);

/** Markdown as one line of plain words, for a live region. */
const plain = (markdown) => markdown.replace(/\*\*|`/g, "").replace(/\s*\n+\s*/g, ". ");

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
 * - `palette(at, mode)` (`"place"`, or `"inline"` on a wire),
 *   `editText(target)`, `name(target)` (the inspector's field, focused),
 *   `help()`, `zoom(how)`: open things;
 * - `gloss()`: whether descriptions include readings.
 */
export class Studio {
  constructor(host) {
    this.host = host;
    this.selection = null;
    this.cursor = null;
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
    return [`${what} at ${anchorOf(target).row + 1}:${anchorOf(target).col + 1}.`, about && `${about}.`, hint]
      .filter(Boolean)
      .join(" ");
  }

  /**
   * The text Enter edits and the inspector's field holds for a selection:
   * a text itself, a node's word, a definition's name, or an arm's guard.
   */
  editable(target = this.selection, view = this.view()) {
    if (!target) return null;
    if ((target.kind === "source" || target.kind === "text") && target.text) return target.text.kind === "comment" ? null : target;
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
      const { from } = this.sketch;
      this.sketch = null;
      if (e.key === "Enter") this.placeBox(rectOf(from, this.cursor));
      else this.host.status("");
      this.host.changed();
      return true;
    }
    switch (e.key) {
      case "]":
      case "[":
        this.step(e.key === "[");
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
      this.host.status(`A box ${r.bottom - r.top + 1} rows by ${r.right - r.left + 1} columns. Enter draws it.`, "hint");
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
    if (!boxAt(view, { row: r.top, col: r.left })) return { kind: "place_definition", rect: r, name: definitionName(view) };
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
    return this.edit({ kind: "add_port", at: cell, name: portName(box) }, (map) => {
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

  /** B: a box from the cursor, stretched by the arrow keys and drawn by Enter. */
  startSketch() {
    this.cursor ??= { row: 0, col: 0 };
    this.selection = null;
    this.sketch = { from: { ...this.cursor } };
    this.host.status("Drawing a box: arrow keys stretch it, Enter draws it, Escape stops.", "hint");
    this.host.changed();
  }

  /** The rectangle `B` is drawing, or null. */
  get sketched() {
    return this.sketch && this.cursor ? rectOf(this.sketch.from, this.cursor) : null;
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

  startWiring() {
    const target = this.selection;
    const ends = this.endsFor(target);
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

/** Every key the drawing takes, for the `?` overlay. */
export const KEYS = [
  ["] [", "Select the next or previous object"],
  ["Arrow keys", "Nudge a selected box, or move the cursor"],
  ["Enter", "Edit a selected text, or rename a selected node or definition"],
  ["W", "Wire a selected source or arrowhead: ] and [ pick an end, Enter connects"],
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
  ["Escape", "Stop drawing a box or wiring, leave the Box tool, clear the selection, then the cursor"],
  ["Tab", "Leave the drawing"],
  ["?", "Show or hide this list"],
];
