// The guided tour on 03-double: how to read the sheet, then six moves that
// show the drawing is an editor. Each step names what to do, rings the
// cells to look at or press on, and advances by itself once the step was
// done: a look at the gloss layer, or a sheet that shows the move, so it
// reads the program's state rather than the clicks that got there. It only
// moves forward: an undo that happens to satisfy a later step just skips
// ahead.

export const TOUR_EXAMPLE = "03-double";

const STEPS = [
  {
    // Written with its readings, so it is never annotated.
    plain: true,
    text: "Read the sheet. Under each word is its reading, the keys that type it: ·𐑥𐑱𐑯 reads =mayn, " +
      "and a primitive shows its name, so 𐑐 is add. Hold Alt (⌥ on a Mac) by itself to flip the readings " +
      "until you let go; the Readings switch sets them for good.",
    rings: (m) => [m.title, m.primitive].filter(Boolean),
    done: (m, event) => event === "looked",
  },
  {
    text: "Cut a wire. Click the wire that runs into ▶ 𐑬 at the right of ·𐑥𐑱𐑯, then press Delete.",
    rings: (m) => (m.outWire ? runs(m.outWire.cells) : []),
    done: (m) => m.sink && !m.sink.wired,
  },
  {
    text: "Draw it back. Press on the ├ notch at the right of the 𐑛𐑳𐑚𐑩𐑤 box and drag to the glowing ▶.",
    rings: (m) => [m.tee, m.sink].filter(Boolean).map(cell),
    done: (m) => m.sink?.wired,
  },
  {
    text: "Take the node out. Click the 𐑛𐑳𐑚𐑩𐑤 box and press Delete; its wires go with it.",
    rings: (m) => (m.node ? [m.node] : []),
    done: (m) => m.box && !m.node,
  },
  {
    text: "Add one back. Press on 21, drag into the empty space to its right, let go, and pick 𐑛𐑳𐑚𐑩𐑤, the palette’s last entry.",
    rings: (m) => (m.constant ? [cell(m.constant)] : []),
    done: (m) => m.node && m.constant?.wired,
  },
  {
    text: "Finish the wire. Drag from the new box’s ├ notch to ▶ 𐑬.",
    rings: (m) => [m.tee, m.sink].filter(Boolean).map(cell),
    done: (m) => m.sink?.wired,
  },
  {
    text: "Press Run. Every gesture rewrote the source text, so ⌘Z undoes any of them.",
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
 * node's output tee, and its constant; and the sheet's first node that
 * calls a primitive, 𐑐. Anything missing is undefined, and its step rings
 * nothing.
 */
function mainOf(view) {
  const lines = view.src.split("\n");
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
    this.host.hidden = false;
    this.update(view);
  }

  stop() {
    this.step = -1;
    this.host.hidden = true;
    this.changed();
  }

  /**
   * Reads a fresh sheet, advancing past every step it shows done. `event`
   * is what just happened besides: `"ran"` or `"looked"`.
   */
  update(view, event = null) {
    if (!this.active) return;
    this.parts = mainOf(view);
    while (!this.finished && STEPS[this.step].done(this.parts, event)) this.step++;
    this.show();
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
    this.step++;
    this.show();
  }

  show() {
    const total = STEPS.length;
    this.counter.textContent = this.finished ? "Done" : `${this.step + 1} of ${total}`;
    const step = STEPS[this.step];
    this.text.textContent = this.finished ? this.message(FINISHED) : step.plain ? step.text : this.message(step.text);
    this.next.textContent = this.finished ? "Close" : "Skip";
    this.changed();
  }

  /** Cell rectangles to ring on the drawing for the current step. */
  rings() {
    return this.active && !this.finished ? STEPS[this.step].rings(this.parts) : [];
  }
}
