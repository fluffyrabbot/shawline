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
 * arrowhead is (Number, Text, 𐑯𐑦𐑤, then callees with an output),
 * `"place"` on blank sheet (Number, Text, 𐑯𐑦𐑤, then every callee), and
 * `"inline"` on a wire (the 1→1 callees, which can be spliced into it).
 */
export function offer(callees, mode) {
  const nodes = callees.map((c) => ({ kind: "node", ...c }));
  if (mode === "feed") return nodes.filter((c) => c.inputs > 0);
  if (mode === "inline") return nodes.filter((c) => c.inputs === 1 && c.outputs === 1);
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
 * The one edit a pick makes at blank cell `at`. Fed from the dragged
 * source `from` it is `place_connected`; feeding the unwired arrowhead
 * `to` it is `feed`, which places the source and wires it in (Number,
 * Text and 𐑯𐑦𐑤 as constants, any other pick as a node); on a wire it is
 * `place_inline`, its word starting at `at`; otherwise it places the pick
 * alone, a node's first port on the drop row.
 */
export function placing(choice, at, { mode, from = null, to = null }) {
  const { word, inputs, outputs } = choice;
  if (mode === "feed") return { kind: "place_connected", at, word, inputs, outputs, from };
  if (mode === "inline") return { kind: "place_inline", at: { row: at.row, col: at.col }, word };
  const constant = choice.kind !== "node" || word === NIL;
  if (to) {
    const source = constant ? { kind: "constant", content: word } : { kind: "node", word, inputs, outputs };
    return { kind: "feed", to: { row: to.row, col: to.col }, source };
  }
  if (choice.kind !== "node") return { kind: "place_text", at, content: word };
  return { kind: "place_node", corner: { row: Math.max(0, at.row - 1), col: at.col }, word, inputs, outputs };
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
