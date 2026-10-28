// The guided tour on 03-double: why the alphabet is Shavian, how to read
// the sheet, then six moves that show the drawing is an editor. Each step
// names what to do, rings the cells to look at or press on, and advances by
// itself once the step was done: a look at the readings, or a sheet that
// shows the move, so it reads the program's state rather than the clicks
// that got there. It also follows undo: an undo back to the text a step
// started on takes the tour back to that step, and redo moves it forward
// again. Only an undo goes back, since redrawing a cut wire by hand can give
// back the very text the cut started on.

export const TOUR_EXAMPLE = "03-double";

const STEPS = [
  {
    // The steps that teach reading write their readings in, so they are
    // never annotated and read the same with the readings off.
    plain: true,
    next: true,
    text: "Shawline is written in Shavian, an alphabet with one letter per sound. Many letters come in mirrored " +
      "pairs, and Shawline names opposite operations with them: 𐑐 (p) adds, and its mirror 𐑚 (b) subtracts. " +
      "Mirrored letters are paired sounds too: p and b.",
    rings: (m) => [m.primitive].filter(Boolean),
    done: () => false,
  },
  {
    plain: true,
    text: "Read the sheet. Under each word is its reading, how it sounds: ·𐑥𐑱𐑯 reads mayn, and a primitive shows " +
      "its name, so 𐑐 shows add. Hover a word for the keys that type it. Hold Alt (⌥ on a Mac) by itself to flip " +
      "the readings until you let go; the Readings switch sets them for good.",
    rings: (m) => [m.title, m.primitive].filter(Boolean),
    done: (m, event) => event === "looked",
  },
  {
    text: "Cut a wire. Click the wire that runs into the ▶ 𐑬 arrowhead at the right of ·𐑥𐑱𐑯, then press Delete.",
    rings: (m) => (m.outWire ? runs(m.outWire.cells) : []),
    done: (m) => m.sink && !m.sink.wired,
  },
  {
    text: "Draw it back. A wire starts at a source: press on the ├ source at the right of the 𐑛𐑳𐑚𐑩𐑤 box and " +
      "drag to the glowing arrowhead.",
    rings: (m) => [m.tee, m.sink].filter(Boolean).map(cell),
    done: (m) => m.sink?.wired,
  },
  {
    text: "Take the node out. Click the 𐑛𐑳𐑚𐑩𐑤 box and press Delete; its wires go with it.",
    rings: (m) => (m.node ? [m.node] : []),
    done: (m) => m.box && !m.node,
  },
  {
    text: "Add one back. Press on the source 21, drag into the empty space to its right and let go. Then type its " +
      "keys, dub, to find 𐑛𐑳𐑚𐑩𐑤, and pick it.",
    rings: (m) => (m.constant ? [cell(m.constant)] : []),
    done: (m) => m.node && m.constant?.wired,
  },
  {
    text: "Finish the wire. Drag from the new box’s ├ source to the ▶ 𐑬 arrowhead.",
    rings: (m) => [m.tee, m.sink].filter(Boolean).map(cell),
    done: (m) => m.sink?.wired,
  },
  {
    text: "Press Run to see what ·𐑥𐑱𐑯 returns; the value on each wire is already showing. Every gesture rewrote " +
      "the text, so Undo (⌘Z or Ctrl+Z) takes any of them back, and the tour steps back with it.",
    rings: () => [],
    done: (m, event) => event === "ran",
  },
];

const FINISHED =
  "That’s the loop. Drag a box to move it, or double-click empty space for a node. Try it on any example.";

/** A one-cell rectangle, spanning a source's whole text. */
const cell = ({ row, col, span = 1 }) => ({ top: row, left: col, bottom: row, right: col + span - 1 });
/** A wire's cells as one rectangle per horizontal run, so its ring reads
 * as one outline rather than a chain of cells. */
function runs(cells) {
  const out = [];
  for (const c of cells) {
    const last = out[out.length - 1];
    if (last && last.top === c.row && last.right + 1 === c.col) last.right = c.col;
    else out.push(cell(c));
  }
  return out;
}
const inside = (r, { row, col }) => row > r.top && row < r.bottom && col > r.left && col < r.right;

/**
 * What the tour needs of ·𐑥𐑱𐑯: its title with the strip under it, its box,
 * its output arrowhead and the wire into it, its first node with that
 * node's source, and its constant; and the sheet's first node that
 * calls a primitive, 𐑐. Anything missing is undefined, and its step rings
 * nothing.
 */
function mainOf(view) {
  // Without a leading byte-order mark, which isn't a cell.
  const lines = view.src.replace(/^\ufeff/, "").split("\n");
  // Columns count code points, and a Shavian letter is two UTF-16 units.
  const inner = (b, row) => [...(lines[row] ?? "")].slice(b.left + 1, b.right).join("").trim();
  const primitive = view.boxes.find(
    (b) => b.kind === "node" && Array.from({ length: b.bottom - b.top - 1 }, (_, i) => inner(b, b.top + 1 + i)).includes("𐑐"),
  );
  const box = view.boxes.find((b) => b.kind === "definition" && lines[b.top]?.includes("·𐑥𐑱𐑯"));
  if (!box) return { primitive };
  const left = [...lines[box.top].slice(0, lines[box.top].indexOf("·𐑥𐑱𐑯"))].length;
  const title = { top: box.top, left, bottom: box.top, right: left + 3, under: true };
  const sink = view.sinks.find((s) => s.col === box.right && s.row > box.top && s.row < box.bottom);
  const outWire = sink && view.wires.find((w) => w.cells.some((c) => c.row === sink.row && c.col === sink.col - 1));
  const node = view.boxes.find((b) => b.kind === "node" && inside(box, { row: b.top, col: b.left }));
  const tee = node && view.sources.find((s) => s.col === node.right && s.row > node.top && s.row < node.bottom);
  // Not a node's tee: a source on no node's cells, in any definition.
  const onNode = (s) =>
    view.boxes.some((b) => b.kind === "node" && s.row >= b.top && s.row <= b.bottom && s.col >= b.left && s.col <= b.right);
  const constant = view.sources.find((s) => inside(box, s) && !onNode(s));
  return { title, primitive, box, sink, outWire, node, tee, constant };
}

export class Tour {
  /**
   * `host` is the strip that shows the step; `changed` is called whenever
   * the rings may differ, so the drawing can redraw its overlays.
   * `message(text)` is how a step's text is shown: with readings while the
   * gloss layer shows.
   */
  constructor(host, changed, message = (text) => text) {
    this.host = host;
    this.changed = changed;
    this.message = message;
    this.step = -1;
    this.parts = {};
    /** The text each step reached so far started on, by step. */
    this.entered = [];
    /** The text last read. */
    this.src = null;
    this.counter = host.querySelector(".tour-count");
    this.text = host.querySelector(".tour-text");
    this.next = host.querySelector(".tour-next");
    this.next.addEventListener("click", () => (this.finished ? this.stop() : this.advance()));
    host.querySelector(".tour-close").addEventListener("click", () => this.stop());
  }

  get active() {
    return this.step >= 0;
  }

  get finished() {
    return this.step >= STEPS.length;
  }

  start(view) {
    this.step = 0;
    this.entered = [view.src];
    this.host.hidden = false;
    this.update(view);
  }

  stop() {
    this.step = -1;
    this.host.hidden = true;
    this.changed();
  }

  /**
   * Reads a fresh sheet and advances past every step it shows done.
   * `event` is what just happened besides: `"ran"`, `"looked"`, or
   * `"undone"`, which first goes back to the latest earlier step that
   * started on this text, unless the current step started on it too.
   */
  update(view, event = null) {
    if (!this.active) return;
    this.parts = mainOf(view);
    this.src = view.src;
    if (event === "undone" && this.step > 0 && !this.finished && this.entered[this.step] !== view.src) {
      const back = this.entered.lastIndexOf(view.src, this.step - 1);
      if (back >= 0) this.step = back;
    }
    while (!this.finished && STEPS[this.step].done(this.parts, event)) this.enter(this.step + 1);
    this.show();
  }

  /** An undo moved the text; a redo is an ordinary update. */
  undone(view) {
    this.update(view, "undone");
  }

  /** Makes `step` current, as started on the text last read. */
  enter(step) {
    this.step = step;
    this.entered.length = step;
    this.entered.push(this.src);
  }

  /** A run finished cleanly. */
  ran(view) {
    this.update(view, "ran");
  }

  /** The gloss layer was looked at: a peek ended, or the switch was turned. */
  looked(view) {
    this.update(view, "looked");
  }

  advance() {
    this.enter(this.step + 1);
    this.show();
  }

  show() {
    const total = STEPS.length;
    this.counter.textContent = this.finished ? "Done" : `${this.step + 1} of ${total}`;
    const step = STEPS[this.step];
    this.text.textContent = this.finished ? this.message(FINISHED) : step.plain ? step.text : this.message(step.text);
    this.next.textContent = this.finished ? "Close" : step.next ? "Next" : "Skip";
    this.changed();
  }

  /** Cell rectangles to ring on the drawing for the current step. */
  rings() {
    return this.active && !this.finished ? STEPS[this.step].rings(this.parts) : [];
  }
}
