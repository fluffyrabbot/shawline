// What the playground keeps in localStorage, under `shawline.`. Every
// example has a draft of its own, and so does the tour, so opening an
// example or starting the tour never overwrites edits made to another. A
// draft equal to its original is dropped, so having one means "edited".
//
// Storage can be full, disabled or missing: every call degrades to doing
// nothing rather than throwing, so typing never breaks.

/** The tour's slot: a draft of its example kept apart from the example's own. */
export const TOUR_SLOT = "tour";

export class Store {
  /** `storage` is a `Storage` (or anything with its three methods), or null. */
  constructor(storage) {
    this.storage = storage;
  }

  get(key) {
    try {
      return this.storage?.getItem(`shawline.${key}`) ?? null;
    } catch {
      return null;
    }
  }

  /** Writes a key; false when storage refused it. */
  set(key, value) {
    try {
      this.storage.setItem(`shawline.${key}`, String(value));
      return true;
    } catch {
      return false;
    }
  }

  remove(key) {
    try {
      this.storage.removeItem(`shawline.${key}`);
      return true;
    } catch {
      return false;
    }
  }

  /** The saved text of an example or the tour, or null when it has none. */
  draft(slot) {
    return this.get(`draft.${slot}`);
  }

  /**
   * Saves `text` as the slot's draft, or drops the draft when `text` is
   * the original. Without an original (it didn't load) the text is kept.
   * False when storage refused.
   */
  saveDraft(slot, text, original) {
    return text === original ? this.remove(`draft.${slot}`) : this.set(`draft.${slot}`, text);
  }

  /**
   * Moves the one sheet saved before drafts existed (`source`, with the
   * example it came from) into that example's draft. An existing draft
   * wins, and the old key stays until the move has succeeded.
   */
  migrate(names) {
    const source = this.get("source");
    const example = this.get("example");
    if (source === null || !names.includes(example)) return;
    if (this.draft(example) === null && !this.set(`draft.${example}`, source)) return;
    this.remove("source");
  }
}
