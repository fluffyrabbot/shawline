// The page's readings of the sheet (PLAN P8): at most two wasm `Doc`s, by
// text. One is the editor's. The other is the sheet under a branch being
// dragged, or else the editor's previous text, which an undo returns to.
//
// A Doc lives in wasm memory, so one let go of here is freed at once rather
// than whenever the GC finalizes it. Nothing else keeps a Doc: callers ask
// by text each time they need one and use it before asking again, so a Doc
// freed here is never used after.

export class Docs {
  /** `read(text)` makes a revision of `text`: `new Doc(text)`. */
  constructor(read, size = 2) {
    this.read = read;
    this.size = size;
    /** Text → `{ doc, view }`, least recently used first. */
    this.entries = new Map();
  }

  /** The revision of `text`, read now unless it is kept. */
  get(text) {
    return this.entry(text).doc;
  }

  /** What `make(doc)` derives from `text`'s revision, made once and kept with it. */
  view(text, make) {
    const entry = this.entry(text);
    entry.view ??= make(entry.doc);
    return entry.view;
  }

  /**
   * Keeps `doc`, the revision an edit returned, so the refresh that follows
   * reads nothing again. A kept revision of the same text wins, and `doc`
   * is freed. Returns the text.
   */
  seed(doc) {
    const text = doc.text;
    if (this.entries.has(text)) {
      doc.free();
      this.entry(text);
    } else {
      this.keep(text, { doc, view: null });
    }
    return text;
  }

  entry(text) {
    const kept = this.entries.get(text);
    if (!kept) return this.keep(text, { doc: this.read(text), view: null });
    this.entries.delete(text);
    this.entries.set(text, kept);
    return kept;
  }

  keep(text, entry) {
    this.entries.set(text, entry);
    for (const [old, { doc }] of this.entries) {
      if (this.entries.size <= this.size) break;
      this.entries.delete(old);
      doc.free();
    }
    return entry;
  }
}
