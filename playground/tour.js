// The guided tour: six moves on 03-double that show the drawing is an
// editor. Each step names what to do, rings the cells to press on, and
// advances by itself once the sheet shows the step was done, so it reads
// the program's state rather than the clicks that got there. It only moves
// forward: an undo that happens to satisfy a later step just skips ahead.

export const TOUR_EXAMPLE = "03-double";

const STEPS = [
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
    done: (m, ran) => ran,
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
 * What the tour needs of ·𐑥𐑱𐑯: its box, its output arrowhead and the wire
 * into it, its first node with that node's output tee, and its constant.
 * Anything missing is undefined, and its step rings nothing.
 */
function mainOf(view) {
  const lines = view.src.split("\n");
  const box = view.boxes.find((b) => b.kind === "definition" && lines[b.top]?.includes("·𐑥𐑱𐑯"));
  if (!box) return {};
  const sink = view.sinks.find((s) => s.col === box.right && s.row > box.top && s.row < box.bottom);
  const outWire = sink && view.wires.find((w) => w.cells.some((c) => c.row === sink.row && c.col === sink.col - 1));
  const node = view.boxes.find((b) => b.kind === "node" && inside(box, { row: b.top, col: b.left }));
  const tee = node && view.sources.find((s) => s.col === node.right && s.row > node.top && s.row < node.bottom);
  // Not a node's tee: a source on no node's cells, in any definition.
  const onNode = (s) =>
    view.boxes.some((b) => b.kind === "node" && s.row >= b.top && s.row <= b.bottom && s.col >= b.left && s.col <= b.right);
  const constant = view.sources.find((s) => inside(box, s) && !onNode(s));
  return { box, sink, outWire, node, tee, constant };
}

export class Tour {
  /**
   * `host` is the strip that shows the step; `changed` is called whenever
   * the rings may differ, so the drawing can redraw its overlays.
   */
  constructor(host, changed) {
    this.host = host;
    this.changed = changed;
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

  /** Reads a fresh sheet, advancing past every step it shows done. */
  update(view, ran = false) {
    if (!this.active) return;
    this.parts = mainOf(view);
    while (!this.finished && STEPS[this.step].done(this.parts, ran)) this.step++;
    this.show();
  }

  /** A run finished cleanly. */
  ran(view) {
    this.update(view, true);
  }

  advance() {
    this.step++;
    this.show();
  }

  show() {
    const total = STEPS.length;
    this.counter.textContent = this.finished ? "Done" : `${this.step + 1} of ${total}`;
    this.text.textContent = this.finished ? FINISHED : STEPS[this.step].text;
    this.next.textContent = this.finished ? "Close" : "Skip";
    this.changed();
  }

  /** Cell rectangles to ring on the drawing for the current step. */
  rings() {
    return this.active && !this.finished ? STEPS[this.step].rings(this.parts) : [];
  }
}
