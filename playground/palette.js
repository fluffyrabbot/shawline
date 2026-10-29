// The context palette as a combobox (PLAN P10, Decisions: Palette), without
// the page: what it offers, what a query keeps, and which entry is
// highlighted. Arrow keys move the highlight, Enter picks it, and a query
// matches an entry's meaning, its letters, its reading or its keys.

/**
 * The sources a constant can be (Decisions: Constants by mouse), offered
 * first when the palette places something to feed an arrowhead, or on
 * blank sheet without a drag. 𐑯𐑦𐑤 follows them, from the callees.
 */
export const CONSTANTS = [
  { kind: "number", word: "0", gloss: "Number", inputs: 0, outputs: 1, mirror: null, reading: "", keys: "" },
  { kind: "literal", word: "«»", gloss: "Text", inputs: 0, outputs: 1, mirror: null, reading: "", keys: "" },
];

const NIL = "𐑯𐑦𐑤";

/**
 * What the palette offers. `callees` is `Doc.palette()`, each with its
 * `reading` and `keys`. `mode` is `"feed"` while a wire from a source is
 * dropped (callees with an input), `"source"` while one from an unwired
 * arrowhead is (Number, Text, 𐑯𐑦𐑤, then callees with an output), and
 * `"place"` on blank sheet (Number, Text, 𐑯𐑦𐑤, then every callee).
 */
export function offer(callees, mode) {
  const nodes = callees.map((c) => ({ kind: "node", ...c }));
  if (mode === "feed") return nodes.filter((c) => c.inputs > 0);
  const nil = nodes.filter((c) => c.word === NIL);
  const rest = nodes.filter((c) => c.word !== NIL && (mode !== "source" || c.outputs > 0));
  return [...CONSTANTS, ...nil, ...rest];
}

/** Whether an entry matches a lowercase query by meaning, letters, reading or keys. */
export function matches(entry, query) {
  return !query || entry.word.includes(query) || entry.reading.includes(query) || entry.keys.includes(query) ||
    entry.gloss.toLowerCase().includes(query);
}

/**
 * The edit a pick makes at blank cell `at`. Fed from the dragged source
 * `from` it is one `place_connected`. Feeding the unwired arrowhead `to`,
 * it is `first` and then the `connect` that `then(view)` names on the
 * placed sheet, which the Studio writes as one undo step (`Studio.chain`):
 * no single `Edit` places a source and wires it onward yet. Otherwise it is
 * the placing alone. A node's first port, or a constant, lands on the drop
 * row; a drop a row or two off the arrowhead's row lands level with it.
 */
export function placing(choice, at, { mode, from = null, to = null }) {
  const { word, inputs, outputs } = choice;
  if (mode === "feed") return { edit: { kind: "place_connected", at, word, inputs, outputs, from } };
  const row = to && Math.abs(to.row - at.row) <= 2 ? to.row : at.row;
  const corner = { row: Math.max(0, row - 1), col: at.col };
  const first = choice.kind === "node"
    ? { kind: "place_node", corner, word, inputs, outputs }
    : { kind: "place_text", at: { row, col: at.col }, content: word };
  if (!to) return { edit: first };
  const then = (view) => {
    const box = choice.kind === "node" && view.boxes.find((b) => b.top === corner.row && b.left === corner.col);
    const source = box
      ? view.sources.find((s) => s.col === box.right && s.row > box.top && s.row < box.bottom)
      : view.sources.find((s) => s.row === row && s.col === at.col);
    return source ? { kind: "connect", from: { row: source.row, col: source.col }, to: { row: to.row, col: to.col } } : null;
  };
  return { edit: first, then };
}

export class Choices {
  constructor(offered) {
    this.offered = offered;
    this.items = offered;
    this.index = 0;
  }

  /** Keeps the entries matching `query`; the first is highlighted. */
  filter(query) {
    const q = query.trim().toLowerCase();
    this.items = this.offered.filter((c) => matches(c, q));
    this.index = 0;
    return this.items;
  }

  /** The highlighted entry, or null when nothing matches. */
  get chosen() {
    return this.items[this.index] ?? null;
  }

  /**
   * A key in the palette's field. Arrow keys, Home and End move the
   * highlight; Enter answers `"pick"` and Escape `"close"`. Anything else
   * answers null and is typed into the field.
   */
  key(name) {
    const n = this.items.length;
    if (name === "Enter") return this.chosen ? "pick" : null;
    if (name === "Escape") return "close";
    if (!n) return null;
    const moves = { ArrowDown: this.index + 1, ArrowUp: this.index - 1, Home: 0, End: n - 1 };
    if (!(name in moves)) return null;
    this.index = (moves[name] + n) % n;
    return "move";
  }
}
