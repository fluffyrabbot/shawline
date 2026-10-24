// The gloss layer (PLAN P9): the Readings switch, a peek while Alt is held
// by itself, and where cells sit in a drawing, whose rows are further apart
// while the layer shows. The layer changes how the sheet and its messages
// are shown and never the text: nothing here edits, and the only key it
// holds back is the release of an Alt that peeked.

/** One cell's width in the renderer's SVG units (shawline-glyph CELL_WIDTH). */
export const CELL_W = 600;

/**
 * How long Alt must be held alone before the layer flips. Option-typing on
 * macOS presses its second key well within this, so it never flashes the
 * layer.
 */
export const PEEK_MS = 250;

/**
 * Where cells sit in a drawing whose rows are `pitch` apart, in the
 * renderer's SVG units. A row's cell is `cell` high, at the top of its
 * pitch; the rest of the pitch is the gloss strip under it.
 */
export class Geometry {
  constructor(pitch, cell) {
    this.pitch = pitch;
    this.cell = cell;
  }

  /**
   * The cell under a point, or null left of or above the sheet. A point in
   * a strip is in the row above it, whose words the strip glosses.
   */
  cellAt(x, y) {
    return x >= 0 && y >= 0 ? { row: Math.floor(y / this.pitch), col: Math.floor(x / CELL_W) } : null;
  }

  /**
   * An inclusive cell rectangle's area, from its top row's top to its
   * bottom row's cell. `under` takes it on through the strip below, and
   * `over` back up through the strip above, so a highlight of cells that
   * join vertically doesn't break at the strip between them.
   */
  rect({ top, left, bottom, right }, { under = false, over = false } = {}) {
    const strip = this.pitch - this.cell;
    const y = top * this.pitch - (over ? strip : 0);
    const end = bottom * this.pitch + this.cell + (under ? strip : 0);
    return { x: left * CELL_W, y, width: (right - left + 1) * CELL_W, height: end - y };
  }

  /** A cell's centre. */
  centre({ row, col }) {
    return { x: col * CELL_W + CELL_W / 2, y: row * this.pitch + this.cell / 2 };
  }
}

export class Layer {
  /**
   * - `on` is the Readings switch.
   * - `rowPitch(gloss)` and `annotate(text)` are the wasm exports.
   * - `changed()` hears every flip of what is shown.
   * - `timers` stands in for the page's in tests.
   */
  constructor({ on = true, rowPitch, annotate, changed = () => {}, timers = globalThis }) {
    Object.assign(this, { on, annotate, changed, timers });
    const cell = rowPitch(false);
    this.geometries = [new Geometry(cell, cell), new Geometry(rowPitch(true), cell)];
    this.peeking = false;
    /** The timer that starts a peek, while a lone Alt is held. */
    this.hold = null;
  }

  /** Whether the layer shows: the switch, flipped while a peek lasts. */
  get shown() {
    return this.on !== this.peeking;
  }

  /** Where cells sit in a drawing made as the layer now shows. */
  get geometry() {
    return this.geometries[Number(this.shown)];
  }

  /** Sets the switch. A peek ends, so what shows is what the switch says. */
  set(on) {
    const before = this.shown;
    this.on = on;
    this.disarm();
    this.peeking = false;
    if (this.shown !== before) this.changed();
  }

  /**
   * A key went down anywhere on the page. Alt alone, with no other
   * modifier, starts a peek once held for `PEEK_MS`; any other key ends
   * one, held or pending, and that Alt peeks no more until pressed again.
   */
  keydown(e) {
    if (e.key === "Alt" && e.repeat) return;
    this.cancel();
    if (e.key === "Alt" && !e.shiftKey && !e.ctrlKey && !e.metaKey) {
      this.hold = this.timers.setTimeout(() => {
        this.hold = null;
        this.flip(true);
      }, PEEK_MS);
    }
  }

  /** A key came up: Alt's release ends the peek. */
  keyup(e) {
    if (e.key !== "Alt") return;
    // Firefox would otherwise open its menu bar on a lone Alt's release.
    if (this.peeking) e.preventDefault?.();
    this.cancel();
  }

  /** A pointer event: one without Alt held means Alt went up unseen. */
  pointer(e) {
    if (!e.altKey) this.cancel();
  }

  /** Ends a peek, pending or showing: the page lost focus, or a key said so. */
  cancel() {
    this.disarm();
    this.flip(false);
  }

  disarm() {
    if (this.hold === null) return;
    this.timers.clearTimeout(this.hold);
    this.hold = null;
  }

  flip(peeking) {
    if (this.peeking === peeking) return;
    this.peeking = peeking;
    this.changed();
  }

  /** A message as the page shows it: annotated with readings while the layer shows. */
  message(text) {
    return this.shown ? this.annotate(text) : text;
  }

  /**
   * Shows `text` in `node` as a message, and remembers it as written so
   * `resay` can show it again after a flip. With `title`, it goes in the
   * node's tooltip instead.
   */
  say(node, text, { title = false } = {}) {
    if (title) {
      node.dataset.plainTitle = text;
      node.title = this.message(text);
    } else {
      node.dataset.plain = text;
      node.textContent = this.message(text);
    }
    return node;
  }

  /** Shows every message `say` put under `root` as the layer now shows. */
  resay(root) {
    for (const node of root.querySelectorAll("[data-plain]")) node.textContent = this.message(node.dataset.plain);
    for (const node of root.querySelectorAll("[data-plain-title]")) node.title = this.message(node.dataset.plainTitle);
  }
}
