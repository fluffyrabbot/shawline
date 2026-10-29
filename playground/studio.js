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
const ORDER = ["box", "text", "source", "sink", "branch", "wire"];

/** The largest Int (SPEC §9: `i64`). */
const LARGEST = 2n ** 63n - 1n;

/** Pixels of sideways drag per scrub step. */
export const SCRUB_PX = 8;

export const keyOf = ({ row, col }) => `${row}:${col}`;
export const same = (a, b) => Boolean(a && b) && a.row === b.row && a.col === b.col;
const before = (a, b) => a.row - b.row || a.col - b.col;
const within = (at, r) => at.row >= r.top && at.row <= r.bottom && at.col >= r.left && at.col <= r.right;

/**
 * What a revision offers the pointer and the keyboard, read once per text:
 * its boxes, sources, sinks, wires and texts, and its wires by cell.
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
  return { src: doc.text, boxes: doc.boxes(), texts: doc.texts(), sources: ends.sources, sinks: ends.sinks, wires, byCell, downs };
}

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

/** The cell a selection is about: where the cursor goes when it is selected. */
export function anchorOf(target) {
  switch (target.kind) {
    case "box":
      return { row: target.box.top, col: target.box.left };
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
  for (const box of view.boxes) list.push({ kind: "box", box });
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
  box: "Arrow keys nudge it; Delete removes a node.",
  wire: "Delete removes the whole wire, every branch.",
  branch: "Delete disconnects just this branch; W rewires it to another source.",
  source: "W wires it to an arrowhead; Enter edits its text.",
  sink: "W wires it from a source.",
  text: "Enter edits it.",
};

const NOUNS = { definition: "Definition", switch: "Switch", node: "Node" };

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
 * - `refused(refusal, edit)`, `status(text, kind)`: say what happened;
 * - `changed()`: the selection, cursor or held revision changed;
 * - `palette(at)`, `editText(target)`, `help()`, `zoom(how)`: open things;
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
    const box = boxAt(view, at);
    return box ? { kind: "box", box } : null;
  }

  select(target, { quiet = false } = {}) {
    this.selection = target ? this.resolve(target) : null;
    if (this.selection) this.cursor = anchorOf(this.selection);
    this.wiring = null;
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
    switch (target.kind) {
      case "box": {
        what = NOUNS[target.box.kind] ?? "Box";
        const word = view.texts.find((t) => within(t, target.box) && t.kind === "word");
        about = said(word);
        break;
      }
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
    return [`${what} at ${anchorOf(target).row + 1}:${anchorOf(target).col + 1}.`, about && `${about}.`, HINTS[target.kind]]
      .filter(Boolean)
      .join(" ");
  }

  // Edits.

  /**
   * One gesture's edit, applied to the shown revision and written as one
   * undo step (two when the sheet had to be formatted first). False when
   * it was refused, which the host has been told.
   */
  edit(edit) {
    const result = this.host.apply(this.doc(), edit);
    if (!result.ok) {
      this.host.refused(result, edit);
      return false;
    }
    this.commit(this.settle(result));
    return true;
  }

  /** An edit's result as the studio keeps it: its revision handed to the host. */
  settle(result) {
    return { text: this.host.seed(result.doc), map: result.map, tidied: result.tidied };
  }

  /**
   * Writes a settled result: the formatted text first, as its own undo
   * step, when the edit was made on it, then the edit. The selection and
   * cursor follow the edit's map.
   */
  commit({ text, map, tidied }, base = this.host.text()) {
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
    map?.free();
    if (tidied) this.host.status("Formatted the sheet to edit it", "hint");
  }

  /**
   * Two edits written as one undo step: `first`, then the edit `next(view)`
   * names on its result (null when there is none). Refused whole when
   * either is. For gestures no single `Edit` covers yet.
   */
  chain(first, next) {
    const base = this.host.text();
    const a = this.host.apply(this.doc(), first);
    if (!a.ok) return void this.host.refused(a, first);
    const middle = this.host.seed(a.doc);
    a.map?.free();
    const second = next(this.host.view(middle));
    if (!second) return void this.host.refused({ ...first.at ?? first.corner, reason: "Nothing placed: there was no source to wire.", remedy: null }, null);
    const b = this.host.apply(this.host.doc(middle), second);
    if (!b.ok) return void this.host.refused({ ...b, reason: `Nothing placed: it couldn't be wired in (${b.reason})` }, null);
    // The map runs from the placed sheet, so the selection is dropped.
    this.selection = null;
    this.commit({ ...this.settle(b), tidied: a.tidied }, base);
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
      if ((this.held?.selection ?? this.selection)?.kind === "box") this.nudge(e.key, dr, dc);
      else this.moveCursor(dr, dc);
      return true;
    }
    if (e.repeat && this.held) return true;
    if (this.held) this.release();
    switch (e.key) {
      case "]":
      case "[":
        this.step(e.key === "[");
        return true;
      case "Enter":
        if (this.selection?.kind === "source" && this.selection.text) this.host.editText(this.selection);
        else if (this.selection?.kind === "text") this.host.editText(this.selection);
        else this.host.status("Enter edits a selected text: select one with ] or [.", "hint");
        return true;
      case "w":
      case "W":
        this.startWiring();
        return true;
      case "/":
        this.host.palette(this.cursor ?? { row: 0, col: 0 });
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
      this.nudging = null;
      this.release();
      return true;
    }
    return false;
  }

  moveCursor(dr, dc) {
    const c = this.cursor ?? { row: 0, col: 0 };
    this.cursor = { row: Math.max(0, c.row + dr), col: Math.max(0, c.col + dc) };
    this.selection = null;
    this.host.status(this.describe(), "hint");
    this.host.changed();
  }

  remove() {
    const target = this.selection;
    if (!target) return;
    const at = anchorOf(target);
    const edit =
      target.kind === "box" ? (target.box.kind === "node" ? { kind: "remove_node", at } : null)
      : target.kind === "branch" ? { kind: "disconnect", at }
      : target.kind === "wire" ? { kind: "remove_wire", at: target.at }
      : target.kind === "source" && target.text && target.text.kind !== "word" ? { kind: "remove_text", at }
      : null;
    if (!edit) return void this.host.status("Delete removes a selected node, wire, branch or constant.", "hint");
    this.selection = null;
    this.edit(edit);
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
  ["Enter", "Edit a selected text"],
  ["W", "Wire a selected source or arrowhead: ] and [ pick an end, Enter connects"],
  ["Delete", "Remove the selected node, wire, branch or constant"],
  ["/", "Open the palette at the cursor"],
  ["R, Shift+R", "Insert or delete a row at the cursor"],
  ["C, Shift+C", "Insert or delete a column at the cursor"],
  ["+ −", "Zoom in or out"],
  ["0", "Fit the sheet in the pane"],
  ["Ctrl or ⌘ + wheel", "Zoom about the pointer; a pinch does too"],
  ["Space + drag", "Pan"],
  ["Ctrl or ⌘ + Z", "Undo; with Shift, redo"],
  ["Escape", "Stop wiring, clear the selection, then the cursor"],
  ["Tab", "Leave the drawing"],
  ["?", "Show or hide this list"],
];
