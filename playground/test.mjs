// Checks the wasm bindings against the corpus. Run `playground/build.sh`
// first, then `node playground/test.mjs`.
import assert from "node:assert/strict";
import { readFileSync, existsSync } from "node:fs";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";

const here = (p) => fileURLToPath(new URL(p, import.meta.url));
const corpus = (name) => readFileSync(here(`../corpus/${name}`), "utf8");

const pkg = here("dist/node/shawline_wasm.js");
if (!existsSync(pkg)) {
  console.error("missing dist/node: run playground/build.sh first");
  process.exit(1);
}
const shawline = createRequire(import.meta.url)(pkg);
const { Doc } = shawline;

// The page holds each text as a Doc and every edit takes one and returns the
// next. Most checks follow texts, so these read a text as a fresh Doc and
// give an edit's new text back as `text`. Node exits when the checks end, so
// they free nothing.
const sheet = (src) => new Doc(src);
const edits = Object.fromEntries(
  ["move_box", "connect", "remove_wire", "disconnect", "place_node", "place_and_connect", "remove_node"].map((name) => [
    name,
    (src, ...args) => {
      const result = shawline[name](sheet(src), ...args);
      return result.ok ? { ...result, text: result.doc.text } : result;
    },
  ]),
);

let failed = 0;
function test(name, body) {
  try {
    body();
    console.log(`ok    ${name}`);
  } catch (e) {
    failed++;
    console.log(`FAIL  ${name}\n${e.message}`);
  }
}

for (const name of ["08-factorial", "12-guards"]) {
  test(`run ${name}`, () => {
    const result = shawline.run(corpus(`${name}.shaw`));
    assert.equal(result.error, null);
    assert.equal(result.outputs.map((line) => `${line}\n`).join(""), corpus(`${name}.out`));
  });
}

test("run 80-div-zero reports the runtime error", () => {
  const result = shawline.run(corpus("80-div-zero.shaw"));
  assert.deepEqual(result.outputs, []);
  assert.equal(result.error.split(":").slice(0, 2).join(":"), corpus("80-div-zero.out").trim());
});

test("check 93-arity", () => {
  const [row, col, code] = corpus("93-arity.check").trim().split(/[: ]/);
  const found = sheet(corpus("93-arity.shaw")).diagnostics();
  const d = found.find((d) => d.code === "C002");
  assert.ok(d, `no C002 in ${JSON.stringify(found)}`);
  assert.deepEqual([d.row, d.col, d.code, d.severity], [Number(row), Number(col), code, "error"]);
  assert.equal(typeof d.message, "string");
});

test("format 03-double-ascii", () => {
  assert.equal(sheet(corpus("03-double-ascii.shaw")).formatted(), corpus("03-double-ascii.fmt.shaw"));
});

test("render follows the theme", () => {
  const src = corpus("01-hello.shaw");
  assert.match(sheet(src).render(false), /#faf9f6/i);
  assert.match(sheet(src).render(true), /#17191e/i);
  assert.equal(sheet("\t").render(false), undefined);
});

test("transliterator revises th and keeps v inside words", () => {
  const t = new shawline.Transliterator();
  assert.deepEqual({ ...t.push("t") }, { delete: 0, insert: "𐑑" });
  assert.deepEqual({ ...t.push("h") }, { delete: 1, insert: "𐑔" });
  t.reset();
  assert.equal(t.push("v").insert, "𐑝");
});

test("move_box moves a box in 03-double", () => {
  const src = corpus("03-double.shaw");
  const node = sheet(src).boxes().find((b) => b.kind === "node");
  assert.deepEqual([node.top, node.left, node.bottom, node.right], [4, 10, 7, 14]);
  const moved = edits.move_box(src, node.top, node.left, 1, 0);
  assert.equal(moved.ok, true, moved.reason);
  assert.ok(sheet(moved.text).boxes().some((b) => b.kind === "node" && b.top === 5 && b.left === 10));
  assert.equal(sheet(moved.text).formatted(), moved.text);
  assert.deepEqual(shawline.run(moved.text).outputs, shawline.run(src).outputs);
  const refused = edits.move_box(src, node.top, node.left, 0, 20);
  assert.equal(refused.ok, false);
  assert.deepEqual([refused.row, refused.col], [4, 10]);
  assert.match(refused.reason, /leave its definition/);
});

// Studio Phase 1: wiring edits, the palette, and what the drawing needs to
// drag wires and show values. 03-double: 𐑯 enters at 5:0, 𐑐's inputs are
// 5:10 and 6:10, its output tee 5:14, and ·𐑛𐑳𐑚𐑩𐑤's output 5:22.

const double = corpus("03-double.shaw");
const outputsOf = (src) => shawline.run(src).outputs;

test("boxes names switches, so pressing one moves the switch, not its definition", () => {
  const abs = corpus("04-abs.shaw");
  const kinds = sheet(abs).boxes().map((b) => `${b.top}:${b.left} ${b.kind}`);
  assert.ok(kinds.includes("9:12 switch"), kinds.join(", "));
  assert.ok(kinds.includes("2:0 definition") && kinds.includes("4:14 node"), kinds.join(", "));
  const moved = edits.move_box(abs, 9, 12, 0, 1);
  if (moved.ok) {
    assert.ok(sheet(moved.text).boxes().some((b) => b.kind === "switch" && b.top === 9 && b.left === 13));
    assert.ok(sheet(moved.text).boxes().some((b) => b.kind === "definition" && b.top === 2 && b.left === 0));
  } else {
    assert.ok(moved.reason, "a refused switch move says why");
  }
});

test("remove_wire and connect rebuild 03-double's fan-out", () => {
  const bare = edits.remove_wire(double, 5, 4);
  assert.equal(bare.ok, true, bare.reason);
  const once = edits.connect(bare.text, 5, 0, 5, 10);
  assert.equal(once.ok, true, once.reason);
  const twice = edits.connect(once.text, 5, 0, 6, 10);
  assert.equal(twice.ok, true, twice.reason);
  assert.match(twice.text, /┬/);
  assert.equal(sheet(twice.text).formatted(), twice.text);
  assert.deepEqual(outputsOf(twice.text), outputsOf(double));
  const taken = edits.connect(double, 5, 0, 5, 10);
  assert.deepEqual([taken.ok, taken.row, taken.col], [false, 5, 10]);
  assert.match(taken.reason, /already has a wire/);
});

test("disconnect leaves a plain stroke where the tee was", () => {
  const cut = edits.disconnect(double, 6, 10);
  assert.equal(cut.ok, true, cut.reason);
  assert.equal(cut.text.split("\n")[5], "▶ 𐑯 ──────▶ 𐑐 ├───────▶ 𐑬");
});

test("remove_node and place_node take a node out and put it back", () => {
  const gone = edits.remove_node(double, 4, 10);
  assert.equal(gone.ok, true, gone.reason);
  assert.ok(!sheet(gone.text).boxes().some((b) => b.kind === "node" && b.top === 4));
  const back = edits.place_node(gone.text, 4, 10, "𐑐", 2, 1);
  assert.equal(back.ok, true, back.reason);
  assert.ok(sheet(back.text).boxes().some((b) => b.kind === "node" && b.top === 4 && b.left === 10));
  const onto = edits.place_node(double, 4, 3, "𐑐", 2, 1);
  assert.equal(onto.ok, false);
  assert.match(onto.reason, /land on/);
});

test("palette lists primitives, nil, then definitions", () => {
  const all = sheet(double).palette();
  assert.deepEqual(all.slice(0, 2).map((c) => [c.word, c.gloss, c.mirror]), [["𐑐", "add", "𐑚"], ["𐑚", "sub", "𐑐"]]);
  assert.deepEqual({ ...all[16] }, { word: "𐑯𐑦𐑤", inputs: 0, outputs: 1, gloss: "empty list", mirror: null });
  assert.deepEqual([all[17].word, all[17].gloss], ["𐑛𐑳𐑚𐑩𐑤", "𐑯 → 𐑬"]);
  const dragging = sheet(double).palette(5, 0);
  assert.ok(dragging.every((c) => c.inputs > 0));
});

test("endpoints name sources and sinks with their scopes", () => {
  const { sources, sinks } = sheet(double).endpoints();
  const input = sources.find((s) => s.row === 5 && s.col === 0);
  assert.deepEqual([input.span, input.wired], [3, true]);
  const tee = sources.find((s) => s.row === 5 && s.col === 14);
  const into = sinks.find((s) => s.row === 6 && s.col === 10);
  assert.equal(tee.scope, into.scope);
  assert.equal(into.wired, true);
  const constant = sources.find((s) => s.row === 14);
  assert.deepEqual([constant.col, constant.span], [2, 2]);
});

test("preview shows the path connect would draw", () => {
  const bare = edits.remove_wire(double, 5, 4).text;
  const path = sheet(bare).preview(5, 0, 5, 10);
  assert.deepEqual(path.map((c) => c.col), [9, 8, 7, 6, 5, 4]);
  assert.ok(path.every((c) => c.row === 5));
  assert.equal(sheet(double).preview(3, 3, 5, 10), undefined);
});

test("wires gives each wire's cells and editable source", () => {
  const list = sheet(double).wires();
  const input = list.find((w) => w.cells.some((c) => c.row === 5 && c.col === 4));
  assert.deepEqual({ ...input.source }, { row: 5, col: 2 });
  assert.ok(input.cells.some((c) => c.row === 6 && c.col === 9));
  const inline = sheet(corpus("13-fanout.shaw")).wires().find((w) => w.cells.some((c) => c.row === 5 && c.col === 4));
  assert.equal(inline.source, null);
});

test("trace returns values per wire, or no badges with the reason", () => {
  const broken = shawline.trace("╭─ ·𐑥𐑱𐑯 ─╮\n│ 𐑐 │\n╰───────╯\n");
  assert.deepEqual(broken.wires, []);
  assert.equal(typeof broken.error.code, "string");
  const found = shawline.trace(double);
  assert.equal(found.error, null);
  assert.equal(found.stopped, false);
  const output = sheet(double).wires().find((w) => w.cells.some((c) => c.row === 14 && c.col === 19));
  assert.equal(found.wires.find((v) => v.wire === output.wire)?.value, "42");
});

test("trace reports a runtime error where it happened, and a run cut off at its budget", () => {
  const failed = shawline.trace(corpus("80-div-zero.shaw"));
  const [, row, col, code] = corpus("80-div-zero.out").trim().match(/^error (\d+):(\d+) (\w+)/);
  assert.deepEqual([failed.error.row, failed.error.col, failed.error.code], [Number(row), Number(col), code]);
  assert.equal(failed.stopped, false);
  assert.ok(failed.wires.length > 0, "wires before the error keep their values");
  const runaway = shawline.trace(corpus("83-runaway.shaw"));
  assert.deepEqual([runaway.error, runaway.stopped], [null, true]);
});

test("run's error position is the at object's, so the page can count from one", () => {
  const { error, at } = shawline.run(corpus("80-div-zero.shaw"));
  assert.equal(error, `error ${at.row}:${at.col} ${at.code}: ${at.message}`);
});

// Dragging a wired arrowhead off is `disconnect`, then `connect` from
// wherever it was dropped; the page applies the pair as one edit.

const wiredSinks = (src) =>
  sheet(src).endpoints().sinks.filter((s) => s.wired).map((s) => `${s.row}:${s.col}`);

test("disconnect takes off one branch and keeps the others", () => {
  assert.deepEqual(wiredSinks(double), ["5:10", "5:22", "6:10", "14:8", "14:22"]);
  const cut = edits.disconnect(double, 6, 10);
  assert.equal(cut.ok, true, cut.reason);
  assert.deepEqual(wiredSinks(cut.text), ["5:10", "5:22", "14:8", "14:22"]);
  assert.equal(sheet(cut.text).formatted(), cut.text);
  const input = sheet(cut.text).wires().find((w) => w.cells.some((c) => c.row === 5 && c.col === 4));
  assert.ok(input.cells.some((c) => c.row === 5 && c.col === 9), "the other branch still reaches 5:10");
  const back = edits.connect(cut.text, 5, 0, 6, 10);
  assert.equal(back.ok, true, back.reason);
  assert.deepEqual(outputsOf(back.text), outputsOf(double));
});

test("a branch can come off and go on again at another source", () => {
  const cut = edits.disconnect(double, 6, 10);
  const moved = edits.connect(cut.text, 5, 14, 6, 10);
  assert.equal(moved.ok, true, moved.reason);
  assert.deepEqual(wiredSinks(moved.text), wiredSinks(double));
  assert.equal(sheet(moved.text).formatted(), moved.text);
});

test("the last branch's disconnect removes the whole wire", () => {
  const cut = edits.disconnect(double, 14, 22);
  assert.equal(cut.ok, true, cut.reason);
  assert.deepEqual(wiredSinks(cut.text), ["5:10", "5:22", "6:10", "14:8"]);
  assert.equal(sheet(cut.text).wires().length, sheet(double).wires().length - 1);
});

test("wires the mouse can't edit refuse disconnect with the reason", () => {
  const fanout = corpus("13-fanout.shaw");
  const locked = sheet(fanout).wires().find((w) => w.source === null);
  assert.ok(locked, "13-fanout has an inline node's wire");
  const refused = edits.disconnect(fanout, 5, 10);
  assert.deepEqual([refused.ok, refused.row, refused.col], [false, 5, 10]);
  assert.match(refused.reason, /can't be edited by mouse/);
});

test("preview from a wired source ends on its wire, not at the source", () => {
  const cut = edits.disconnect(double, 6, 10).text;
  const path = sheet(cut).preview(5, 0, 6, 10);
  const wire = sheet(cut).wires().find((w) => w.cells.some((c) => c.row === 5 && c.col === 4));
  const end = path[path.length - 1];
  assert.ok(wire.cells.some((c) => c.row === end.row && c.col === end.col), `path ends at ${end.row}:${end.col}`);
});

// Revisions (PLAN P8): an edit takes a Doc and returns the next, and a
// refusal says what kind of obstacle stopped it.

const KINDS = ["unreadable", "target", "unsupported", "blocked", "gave-up", "changed"];

test("an edit returns the next Doc and leaves its own alone; a refusal has a kind and no Doc", () => {
  const start = sheet(double);
  const cut = shawline.disconnect(start, 6, 10);
  assert.ok(cut.doc instanceof Doc);
  assert.equal(start.text, double);
  const back = shawline.connect(cut.doc, 5, 0, 6, 10);
  assert.equal(back.ok, true, back.reason);
  const taken = shawline.connect(back.doc, 5, 0, 5, 10);
  assert.deepEqual([taken.ok, "doc" in taken], [false, false]);
  assert.ok(KINDS.includes(taken.kind), `kind ${taken.kind}`);
  [start, cut.doc, back.doc].forEach((d) => d.free());
});

test("place_and_connect is one edit: a refused connect places nothing", () => {
  const gone = sheet(edits.remove_node(double, 13, 8).text);
  const before = gone.text;
  // 21 at 14:2 feeds the new node's input at 14:8, as the tour's palette step does.
  const wired = shawline.place_and_connect(gone, 13, 8, "𐑛𐑳𐑚𐑩𐑤", 1, 1, 14, 2);
  assert.equal(wired.ok, true, wired.reason);
  assert.ok(wiredSinks(wired.doc.text).includes("14:8"));
  // ·𐑛𐑳𐑚𐑩𐑤's input at 5:0 can't reach a node in ·𐑥𐑱𐑯: the node alone
  // would fit, but wired from there it is refused, and nothing is placed.
  assert.equal(shawline.place_node(gone, 13, 8, "𐑛𐑳𐑚𐑩𐑤", 1, 1).ok, true);
  const unwired = shawline.place_and_connect(gone, 13, 8, "𐑛𐑳𐑚𐑩𐑤", 1, 1, 5, 0);
  assert.deepEqual([unwired.ok, unwired.step, "doc" in unwired], [false, "connect", false]);
  assert.ok(KINDS.includes(unwired.kind), `kind ${unwired.kind}`);
  assert.equal(gone.text, before, "the sheet is still the Doc it started from");
  const unplaced = shawline.place_and_connect(sheet(double), 4, 3, "𐑐", 2, 1, 5, 0);
  assert.deepEqual([unplaced.ok, unplaced.step], [false, "place"]);
  assert.match(unplaced.reason, /land on/);
});

// The page's Docs: at most two, by text, each freed when it is dropped.

const { Docs } = await import("./docs.js");

/** A cache over real Docs that notes each text it reads and each Doc freed. */
function counted() {
  const read = [];
  const freed = [];
  const spy = (d) => {
    const { text } = d;
    const free = d.free.bind(d);
    d.free = () => (freed.push(text), free());
    return d;
  };
  const docs = new Docs((text) => (read.push(text), spy(new Doc(text))));
  return { docs, read, freed, spy };
}

const factorial = corpus("08-factorial.shaw");
const fanout = corpus("13-fanout.shaw");

test("the Doc cache keeps two revisions and frees the one it drops", () => {
  const { docs, read, freed } = counted();
  const first = docs.get(double);
  assert.equal(docs.get(double), first);
  const second = docs.get(fanout);
  docs.get(double);
  docs.get(factorial);
  assert.deepEqual(read, [double, fanout, factorial]);
  assert.deepEqual(freed, [fanout], "the least recently used goes");
  assert.throws(() => second.text, "a freed Doc is gone from wasm memory");
  assert.equal(docs.get(double), first);
});

test("an edit's Doc seeds the cache, so the refresh after it reads nothing", () => {
  const { docs, read } = counted();
  const moved = shawline.move_box(docs.get(double), 4, 10, 1, 0);
  assert.equal(moved.ok, true, moved.reason);
  const text = docs.seed(moved.doc);
  assert.equal(docs.get(text), moved.doc);
  let made = 0;
  const view = () => docs.view(text, (d) => (made++, d.boxes()));
  assert.equal(view(), view());
  assert.deepEqual([read, made], [[double], 1]);
});

test("a changed text misses the cache, and a second Doc of a kept text is freed", () => {
  const { docs, read, freed, spy } = counted();
  const kept = docs.get(double);
  const edited = double.replace("21", "22");
  assert.notEqual(docs.get(edited), kept);
  assert.deepEqual(read, [double, edited]);
  assert.equal(docs.seed(spy(new Doc(double))), double);
  assert.deepEqual(freed, [double]);
  assert.equal(docs.get(double), kept);
});

// Worker jobs: one in flight, one replaceable request waiting, a kill timer
// per job, and results that carry the revision they describe.

const { Flight } = await import("./flight.js");

function fakeFlight(current) {
  const workers = [];
  const timers = new Map();
  let ids = 0;
  const settled = [];
  const flight = new Flight({
    spawn: () => {
      const worker = { posted: [], terminated: false };
      worker.postMessage = (job) => worker.posted.push(job.rev);
      worker.terminate = () => (worker.terminated = true);
      workers.push(worker);
      return worker;
    },
    limit: 1000,
    current,
    settle: (job, outcome) => settled.push([job?.rev, outcome]),
    timers: { setTimeout: (fn) => (timers.set(++ids, fn), ids), clearTimeout: (id) => timers.delete(id) },
  });
  const trace = (rev) => flight.request({ kind: "trace", src: `text ${rev}`, rev });
  const answer = (worker, rev, data = {}) => worker.onmessage({ data: { kind: "trace", rev, wires: [], ...data } });
  /** Fires the one kill timer pending. */
  const expire = () => {
    assert.equal(timers.size, 1);
    const [[id, fn]] = timers;
    timers.delete(id);
    fn();
  };
  return { flight, workers, timers, settled, trace, answer, expire };
}

test("rev protocol: a result for a text since edited is ignored", () => {
  let rev = 1;
  const f = fakeFlight(() => rev);
  f.trace(1);
  rev = 2;
  f.answer(f.workers[0], 1);
  assert.deepEqual(f.settled, []);
  f.trace(2);
  f.answer(f.workers[0], 2);
  assert.deepEqual(f.settled.map(([r]) => r), [2]);
});

test("one trace in flight; a newer request replaces the one waiting", () => {
  let rev = 1;
  const f = fakeFlight(() => rev);
  f.trace(1);
  f.trace((rev = 2));
  f.trace((rev = 3));
  const [worker] = f.workers;
  assert.deepEqual(worker.posted, [1]);
  f.answer(worker, 1);
  assert.deepEqual(worker.posted, [1, 3], "2 was replaced, and 3 starts once 1 is done");
  f.answer(worker, 3);
  assert.deepEqual(f.settled.map(([r]) => r), [3]);
  assert.deepEqual([f.flight.busy, f.timers.size], [false, 0]);
});

test("each job keeps its own kill timer, which a newer request never resets", () => {
  let rev = 1;
  const f = fakeFlight(() => rev);
  f.trace(1);
  const [timer] = f.timers.keys();
  f.trace((rev = 2));
  assert.deepEqual([...f.timers.keys()], [timer], "still job 1's timer");
  f.expire();
  const [first, second] = f.workers;
  assert.equal(first.terminated, true, "job 1 was killed, not left running");
  assert.deepEqual(second.posted, [2], "job 2 runs on a fresh worker");
  f.answer(first, 1);
  assert.deepEqual([f.settled, f.flight.busy], [[], true], "the killed worker's late answer is ignored");
  f.expire();
  assert.deepEqual(f.settled, [[2, { timedOut: true }]]);
  assert.equal(second.terminated, true);
});

test("a panicked worker is retired; cancel drops what was asked; a broken worker drops the queue", () => {
  const f = fakeFlight(() => 1);
  f.trace(1);
  f.answer(f.workers[0], 1, { failed: "unreachable" });
  assert.equal(f.workers[0].terminated, true);
  assert.equal(f.settled[0][1].failed, "unreachable");
  f.trace(1);
  f.trace(1);
  f.flight.cancel();
  f.answer(f.workers[1], 1);
  assert.deepEqual([f.settled.length, f.workers[1].posted, f.flight.busy], [1, [1], false]);
  f.trace(1);
  f.trace(1);
  f.workers[1].onerror({ message: "no module" });
  assert.deepEqual(f.settled[1], [1, { broken: "no module" }]);
  assert.deepEqual([f.workers[1].terminated, f.flight.busy, f.workers.length], [true, false, 2]);
});

// Drafts: each example and the tour keep their own text, and storage
// that refuses never throws.

const { Store, TOUR_SLOT } = await import("./drafts.js");

function memory({ full = false } = {}) {
  const map = new Map();
  return {
    map,
    getItem: (k) => map.get(k) ?? null,
    setItem: (k, v) => {
      if (full) throw new DOMException("quota", "QuotaExceededError");
      map.set(k, v);
    },
    removeItem: (k) => map.delete(k),
  };
}

test("drafts are per slot, and the tour's is its own", () => {
  const store = new Store(memory());
  assert.equal(store.saveDraft("03-double", "mine", "original"), true);
  assert.equal(store.saveDraft(TOUR_SLOT, "tour edits", "original"), true);
  assert.equal(store.draft("03-double"), "mine");
  assert.equal(store.draft(TOUR_SLOT), "tour edits");
  assert.equal(store.draft("08-factorial"), null);
  store.saveDraft("03-double", "original", "original");
  assert.equal(store.draft("03-double"), null, "back to the original drops the draft");
  store.saveDraft("08-factorial", "offline", undefined);
  assert.equal(store.draft("08-factorial"), "offline", "without an original the text is kept");
});

test("full or missing storage refuses without throwing", () => {
  const full = new Store(memory({ full: true }));
  assert.equal(full.saveDraft("03-double", "mine", "original"), false);
  assert.equal(full.set("theme", "dark"), false);
  const none = new Store(null);
  assert.equal(none.get("theme"), null);
  assert.equal(none.saveDraft("03-double", "mine", "original"), false);
});

test("a draft whose drawing never finished is set aside on load, and can come back", () => {
  const store = new Store(memory());
  store.saveDraft("03-double", "frozen", "original");
  assert.equal(store.markDrawing("03-double"), true);
  store.unmarkDrawing();
  assert.equal(store.setAsideStopped(), null, "a finished drawing leaves nothing to set aside");
  assert.equal(store.draft("03-double"), "frozen");
  store.markDrawing("03-double");
  assert.equal(store.setAsideStopped(), "03-double");
  assert.equal(store.draft("03-double"), null, "the slot opens its original");
  assert.equal(store.stopped("03-double"), "frozen");
  assert.equal(store.get("drawing"), null, "the mark is cleared");
  assert.equal(store.restoreStopped("03-double"), true);
  assert.equal(store.draft("03-double"), "frozen");
  assert.equal(store.stopped("03-double"), null);
  assert.equal(store.restoreStopped("03-double"), false);
  store.markDrawing("08-factorial");
  assert.equal(store.setAsideStopped(), null, "a slot without a draft opens its original anyway");
});

test("the single pre-draft sheet moves into its example's draft", () => {
  const storage = memory();
  storage.setItem("shawline.source", "old edits");
  storage.setItem("shawline.example", "03-double");
  const store = new Store(storage);
  store.migrate(["03-double"]);
  assert.equal(store.draft("03-double"), "old edits");
  assert.equal(store.get("source"), null);
  storage.setItem("shawline.source", "older");
  store.migrate(["03-double"]);
  assert.equal(store.draft("03-double"), "old edits", "an existing draft wins");
  const stuck = memory();
  stuck.setItem("shawline.source", "kept");
  stuck.setItem("shawline.example", "03-double");
  const fullAfter = new Store({ ...stuck, setItem: () => { throw new Error("full"); } });
  fullAfter.migrate(["03-double"]);
  assert.equal(stuck.getItem("shawline.source"), "kept", "the old key stays until the move succeeds");
});

// The guided tour reads the sheet, not the clicks: drive 03-double through
// the edits each step asks for and check that it advances to the end.

const { Tour } = await import("./tour.js");

const view = (src) => {
  const d = sheet(src);
  return { src, boxes: d.boxes(), wires: d.wires(), ...d.endpoints() };
};

function fakeHost() {
  const part = () => ({ textContent: "", addEventListener() {} });
  const parts = {};
  return { hidden: true, querySelector: (sel) => (parts[sel] ??= part()) };
}

test("the tour walks 03-double to the end", () => {
  const tour = new Tour(fakeHost(), () => {});
  let src = double;
  const step = (edit) => {
    assert.ok(tour.rings().length > 0, `step ${tour.step + 1} rings nothing`);
    const result = edit(tour.parts);
    assert.equal(result.ok, true, result.reason);
    src = result.text;
    const before = tour.step;
    tour.update(view(src));
    assert.equal(tour.step, before + 1, `step ${before + 1} didn't advance`);
  };
  tour.start(view(src));
  assert.equal(tour.step, 0);
  assert.ok(tour.rings().length > 0, "step 1 rings what to read");
  tour.looked(view(src));
  assert.equal(tour.step, 1, "a look at the layer is step 1");
  step(({ outWire }) => edits.remove_wire(src, outWire.cells[0].row, outWire.cells[0].col));
  step(({ tee, sink }) => edits.connect(src, tee.row, tee.col, sink.row, sink.col));
  step(({ node }) => edits.remove_node(src, node.top, node.left));
  step(({ constant }) =>
    edits.place_and_connect(src, constant.row - 1, constant.col + 8, "𐑛𐑳𐑚𐑩𐑤", 1, 1, constant.row, constant.col),
  );
  step(({ tee, sink }) => edits.connect(src, tee.row, tee.col, sink.row, sink.col));
  assert.deepEqual(outputsOf(src), ["42"]);
  tour.ran(view(src));
  assert.equal(tour.finished, true);
  assert.deepEqual(tour.rings(), []);
});

test("the tour finds main's constant wherever main is moved", () => {
  const main = sheet(double).boxes().find((b) => b.kind === "definition" && b.top === 11);
  // Moved right, the constant shares columns with ·𐑛𐑳𐑚𐑩𐑤's node.
  const moved = edits.move_box(double, main.top, main.left, 0, 8);
  assert.equal(moved.ok, true, moved.reason);
  const tour = new Tour(fakeHost(), () => {});
  tour.start(view(moved.text));
  assert.deepEqual([tour.parts.constant?.row, tour.parts.constant?.col], [14, 10]);
});

test("the tour's first step rings ·𐑥𐑱𐑯 with its strip and the 𐑐 node, and waits for a look", () => {
  const host = fakeHost();
  const tour = new Tour(host, () => {}, (text) => `${text} [read]`);
  tour.start(view(double));
  const [title, primitive] = tour.rings();
  assert.deepEqual({ ...title }, { top: 11, left: 3, bottom: 11, right: 6, under: true });
  assert.deepEqual([primitive.top, primitive.left, primitive.bottom, primitive.right], [4, 10, 7, 14]);
  const text = () => host.querySelector(".tour-text").textContent;
  assert.match(text(), /·𐑥𐑱𐑯 reads =mayn/);
  assert.doesNotMatch(text(), /\[read\]/, "it writes its readings itself");
  tour.update(view(edits.remove_wire(double, 14, 18).text));
  assert.equal(tour.step, 0, "an edit isn't a look");
  tour.looked(view(double));
  assert.equal(tour.step, 1);
  assert.match(text(), /\[read\]$/, "the steps after it are shown through the layer");
});

// The gloss layer (PLAN P9): the Readings switch, the lone-Alt peek, and
// the drawing's geometry at either row pitch.

const { Layer, CELL_W } = await import("./layer.js");

function fakeLayer({ on = true, annotate = shawline.annotate } = {}) {
  const timers = new Map();
  let ids = 0;
  const flips = [];
  const layer = new Layer({
    on,
    rowPitch: shawline.row_pitch,
    annotate,
    changed: () => flips.push(layer.shown),
    timers: { setTimeout: (fn) => (timers.set(++ids, fn), ids), clearTimeout: (id) => timers.delete(id) },
  });
  /** Lets the peek's beat pass, as if Alt were held on. */
  const wait = () => [...timers].forEach(([id, fn]) => (timers.delete(id), fn()));
  return { layer, flips, timers, wait };
}

/** A key event as the window's listener sees it, noting a preventDefault. */
const key = (name, more = {}) => ({
  key: name, repeat: false, altKey: name === "Alt", shiftKey: false, ctrlKey: false, metaKey: false,
  prevented: false,
  preventDefault() {
    this.prevented = true;
  },
  ...more,
});

/** A textarea behind the layer: each key the layer lets through types itself. */
function typist(layer, text) {
  return {
    text,
    down(name, more) {
      const e = key(name, more);
      layer.keydown(e);
      if (!e.prevented && [...name].length === 1) this.text += name;
      return e;
    },
    up(name, more) {
      const e = key(name, more);
      layer.keyup(e);
      return e;
    },
  };
}

const desc = (svg) => svg.match(/<desc>([\s\S]*)<\/desc>/)[1];

test("toggling Readings and peeking leave the editor text unchanged", () => {
  const { layer, flips, wait } = fakeLayer();
  const editor = typist(layer, double);
  const doc = sheet(double);
  const drawn = [];
  const draw = () => drawn.push(doc.render(false, layer.shown));
  draw();
  layer.set(false);
  draw();
  layer.set(true);
  draw();
  for (const on of [true, false]) {
    layer.set(on);
    editor.down("Alt");
    wait();
    draw();
    editor.up("Alt");
    draw();
  }
  assert.deepEqual(flips, [false, true, false, true, false, true, false]);
  assert.deepEqual([editor.text, doc.text], [double, double]);
  assert.equal(new Set(drawn.map(desc)).size, 1, "every drawing carries the same source");
  assert.equal(new Set(drawn).size, 2, "the layer alone decides the drawing: glossed or plain");
  assert.equal(sheet(double).formatted(), double);
});

test("a lone Alt peeks after a beat; another key cancels it, so Option-typing never flashes", () => {
  const { layer, flips, timers, wait } = fakeLayer();
  const editor = typist(layer, "");
  editor.down("Alt");
  assert.equal(timers.size, 1, "a lone Alt waits a beat");
  const typed = editor.down("e", { altKey: true });
  assert.equal(timers.size, 0, "the next key cancels the peek before it shows");
  wait();
  editor.up("Alt");
  assert.deepEqual([flips, layer.shown, typed.prevented, editor.text], [[], true, false, "e"]);
  // Held past the beat, the peek shows; another key ends it at once, and
  // the key still types. Alt's release then changes nothing.
  editor.down("Alt");
  wait();
  assert.equal(layer.shown, false);
  editor.down("x", { altKey: true });
  assert.equal(layer.shown, true);
  editor.up("Alt");
  assert.deepEqual([flips, editor.text], [[false, true], "ex"]);
  // Alt with another modifier isn't alone, and a held Alt's repeats keep its beat.
  editor.down("Shift", { shiftKey: true });
  editor.down("Alt", { shiftKey: true });
  assert.equal(timers.size, 0);
  editor.up("Alt");
  editor.down("Alt");
  const beat = [...timers.keys()];
  editor.down("Alt", { repeat: true });
  assert.deepEqual([...timers.keys()], beat);
  // Losing focus ends a peek, and so does a pointer that says Alt is up.
  wait();
  assert.equal(layer.shown, false);
  layer.cancel();
  assert.equal(layer.shown, true);
  editor.down("Alt");
  wait();
  layer.pointer({ altKey: true });
  assert.equal(layer.shown, false);
  layer.pointer({ altKey: false });
  assert.equal(layer.shown, true);
  // Turning the switch mid-peek shows what the switch says.
  editor.down("Alt");
  wait();
  layer.set(false);
  assert.deepEqual([layer.on, layer.peeking, layer.shown], [false, false, false]);
  assert.equal(editor.text, "ex");
});

test("the hit test puts a strip's y in the row above it, with the layer on and off", () => {
  const { layer } = fakeLayer();
  const on = layer.geometry;
  layer.set(false);
  const off = layer.geometry;
  const cell = shawline.row_pitch(false);
  const pitch = shawline.row_pitch(true);
  assert.deepEqual([off.pitch, off.cell, on.pitch, on.cell], [cell, cell, pitch, cell]);
  assert.ok(pitch > cell, "a strip opens under every row");
  for (const row of [0, 1, 5, 14]) {
    const strip = row * pitch + (cell + pitch) / 2;
    assert.deepEqual(on.cellAt(3.5 * CELL_W, strip), { row, col: 3 }, "the middle of the strip");
    assert.deepEqual(on.cellAt(0, row * pitch + cell), { row, col: 0 }, "the strip's top");
    assert.deepEqual(on.cellAt(0, (row + 1) * pitch - 0.5), { row, col: 0 }, "the strip's bottom");
    assert.deepEqual(on.cellAt(0, (row + 1) * pitch), { row: row + 1, col: 0 }, "the next row's top");
    assert.deepEqual(off.cellAt(0, (row + 1) * cell - 0.5), { row, col: 0 });
    assert.deepEqual(off.cellAt(0, strip), { row: Math.floor(strip / cell), col: 0 }, "plain, that y is rows further down");
    for (const g of [on, off]) {
      const { x, y } = g.centre({ row, col: 2 });
      assert.deepEqual(g.cellAt(x, y), { row, col: 2 });
      const r = g.rect({ top: row, left: 2, bottom: row + 1, right: 4 }, { under: true, over: row > 0 });
      const above = g === on && row > 0 ? row - 1 : row;
      assert.deepEqual(g.cellAt(r.x, r.y), { row: above, col: 2 }, "over starts in the strip above, if there is one");
      assert.deepEqual(g.cellAt(r.x + r.width - 0.5, r.y + r.height - 0.5), { row: row + 1, col: 4 });
      assert.deepEqual(g.cellAt(r.x, r.y + r.height), { row: row + 2, col: 2 }, "under ends with the strip");
    }
  }
  assert.deepEqual([on.cellAt(-1, 0), on.cellAt(0, -1)], [null, null]);
});

test("the drawing's rows are row_pitch apart, glossed or plain", () => {
  const doc = sheet(double);
  const view = (svg) => svg.match(/viewBox="([^"]+)"/)[1].split(" ").map(Number);
  const [x0, y0, w0, plain] = view(doc.render(false, false));
  const [x1, y1, w1, glossed] = view(doc.render(true, true));
  assert.deepEqual([x1, y1, w1], [x0, y0, w0], "the same margin and width");
  const rows = (plain + 2 * y0) / shawline.row_pitch(false);
  assert.equal(rows, double.split("\n").length - 1);
  assert.equal(glossed + 2 * y1, rows * shawline.row_pitch(true));
});

test("messages are plain with the layer off, annotated while it shows, and flip with a peek", () => {
  const message = "unknown name 𐑓𐑵";
  const { layer, wait } = fakeLayer({ annotate: (text) => `${text} [read]` });
  assert.equal(layer.message(message), `${message} [read]`);
  const node = { dataset: {}, textContent: "", title: "" };
  const tip = { dataset: {}, textContent: "", title: "" };
  const root = { querySelectorAll: (sel) => ({ "[data-plain]": [node], "[data-plain-title]": [tip] })[sel] };
  layer.say(node, message);
  layer.say(tip, message, { title: true });
  layer.set(false);
  assert.equal(layer.message(message), message);
  layer.resay(root);
  assert.deepEqual([node.textContent, tip.title, tip.textContent], [message, message, ""]);
  layer.keydown(key("Alt"));
  wait();
  layer.resay(root);
  assert.deepEqual([node.textContent, tip.title], [`${message} [read]`, `${message} [read]`], "a peek shows readings");
  layer.keyup(key("Alt"));
  layer.resay(root);
  assert.equal(node.textContent, message);
  // wasm hands messages over as written, and its annotate only adds
  // readings: take each bracket after a Shavian word away and the message
  // is back.
  const [unknown] = sheet(corpus("95-unknown.shaw")).diagnostics();
  assert.equal(unknown.message, message);
  const { at, error } = shawline.run(corpus("80-div-zero.shaw"));
  for (const text of [message, at.message, error, "·𐑥𐑱𐑯 has no outputs."]) {
    const read = shawline.annotate(text);
    assert.equal(read.replace(/(?<=[\u{10450}-\u{1047F}]) \([^()]*\)/gu, ""), text);
  }
});

test("glosses give each word its reading and each primitive node its name", () => {
  const words = sheet(double).glosses();
  const at = (row, col) => words.find((g) => g.row === row && g.col === col);
  assert.deepEqual([at(11, 3).word, at(11, 3).reading, at(11, 3).span, at(11, 3).gloss], ["·𐑥𐑱𐑯", "=mayn", 4, null]);
  assert.deepEqual([at(5, 12).word, at(5, 12).reading, at(5, 12).gloss], ["𐑐", "p", "add"]);
  assert.ok(words.every((g) => typeof g.shown === "boolean"));
  assert.ok(!words.some((g) => /^\d+$/.test(g.word)), "numbers have no reading");
  assert.deepEqual(sheet("\t").glosses(), []);
  assert.deepEqual([shawline.reading("·𐑥𐑱𐑯"), shawline.reading("𐑣𐑧𐑤𐑴")], ["=mayn", "heloh"]);
});

test("a wire's cells say which join it southward, through the strip", () => {
  const input = sheet(double).wires().find((w) => w.cells.some((c) => c.row === 5 && c.col === 4));
  const down = (row, col) => input.cells.find((c) => c.row === row && c.col === col)?.down;
  assert.deepEqual([down(5, 4), down(5, 6), down(6, 6), down(6, 7)], [false, true, false, false]);
});

process.exit(failed ? 1 : 0);
