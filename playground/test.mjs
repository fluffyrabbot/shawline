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
const cell = (row, col) => ({ row, col });
/** Each edit as the page writes it for `apply`, from the cells it names. */
const EDITS = {
  move_box: (row, col, dr, dc) => ({ kind: "move", corner: cell(row, col), dr, dc }),
  connect: (fr, fc, tr, tc) => ({ kind: "connect", from: cell(fr, fc), to: cell(tr, tc) }),
  remove_wire: (row, col) => ({ kind: "remove_wire", at: cell(row, col) }),
  disconnect: (row, col) => ({ kind: "disconnect", at: cell(row, col) }),
  place_node: (row, col, word, inputs, outputs) => ({ kind: "place_node", corner: cell(row, col), word, inputs, outputs }),
  remove_node: (row, col) => ({ kind: "remove_node", at: cell(row, col) }),
};
const edits = Object.fromEntries(
  Object.entries(EDITS).map(([name, make]) => [
    name,
    (src, ...args) => {
      const result = shawline.apply(sheet(src), make(...args));
      return result.ok ? { ...result, text: result.doc.text } : result;
    },
  ]),
);

let failed = 0;
let skipped = 0;
/**
 * Whether agent E1's edits (`set_text`, `insert`, `make_room`, …) have
 * landed: until they do, they refuse as `unsupported`, and the checks that
 * need them are skipped with a marker instead of failing.
 */
const E1 = (() => {
  const probe = shawline.apply(sheet(corpus("03-double.shaw")), { kind: "set_text", at: cell(14, 2), content: "22" });
  return probe.ok || probe.kind !== "unsupported";
})();
function pendingE1(name, body) {
  if (E1) return test(name, body);
  skipped++;
  console.log(`skip  ${name} (pending E1: its edits refuse as unsupported)`);
}

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
  const { path, joins_wire } = sheet(bare).preview(false, 5, 0, 5, 10);
  const cols = path.map((c) => c.col);
  assert.ok([4, 5, 6, 7, 8, 9].every((col) => cols.includes(col)), cols.join(" "));
  assert.ok(path.every((c) => c.row === 5));
  assert.equal(joins_wire, false, "the source had no wire");
  assert.equal(sheet(double).preview(false, 3, 3, 5, 10), undefined);
});

pendingE1("preview's path runs from the source to the arrowhead, ends included, and from an arrowhead too", () => {
  const bare = edits.remove_wire(double, 5, 4).text;
  const { path } = sheet(bare).preview(false, 5, 0, 5, 10);
  // From the source's own cell, the input's name 𐑯, to the arrowhead.
  assert.deepEqual([path[0], path[path.length - 1]].map((c) => c.col), [2, 10]);
  const back = sheet(bare).preview(true, 5, 10, 5, 0);
  assert.ok(back, "a drag from an unwired arrowhead has a preview");
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
  const { path } = sheet(cut).preview(false, 5, 0, 6, 10);
  const wire = sheet(cut).wires().find((w) => w.cells.some((c) => c.row === 5 && c.col === 4));
  const on = (end) => wire.cells.some((c) => c.row === end.row && c.col === end.col);
  assert.ok(on(path[0]) || on(path[path.length - 1]), "one end of the path is on the wire");
  assert.ok(!path.some((c) => c.row === 5 && c.col === 0), "it doesn't run on to the source");
});

// Revisions (PLAN P8): an edit takes a Doc and returns the next, and a
// refusal says what kind of obstacle stopped it.

const KINDS = ["unreadable", "target", "unsupported", "blocked", "gave-up", "changed"];

test("an edit returns the next Doc and leaves its own alone; a refusal has a kind and no Doc", () => {
  const start = sheet(double);
  const cut = shawline.apply(start, EDITS.disconnect(6, 10));
  assert.ok(cut.doc instanceof Doc);
  assert.equal(start.text, double);
  const back = shawline.apply(cut.doc, EDITS.connect(5, 0, 6, 10));
  assert.equal(back.ok, true, back.reason);
  const taken = shawline.apply(back.doc, EDITS.connect(5, 0, 5, 10));
  assert.deepEqual([taken.ok, "doc" in taken, taken.remedy], [false, false, null]);
  assert.ok(KINDS.includes(taken.kind), `kind ${taken.kind}`);
  [start, cut.doc, back.doc].forEach((d) => d.free());
  [cut.map, back.map].forEach((m) => m.free());
});

test("apply refuses an edit object written wrong, and a blocked move offers make-room", () => {
  for (const wrong of [{ kind: "teleport" }, { kind: "move", corner: cell(4, 10) }, { kind: "connect", from: 3, to: cell(5, 10) }]) {
    const refused = shawline.apply(sheet(double), wrong);
    assert.deepEqual([refused.ok, refused.kind], [false, "target"], JSON.stringify(wrong));
  }
  const blocked = shawline.apply(sheet(double), EDITS.move_box(4, 10, 0, 20));
  assert.deepEqual([blocked.ok, blocked.kind, blocked.remedy], [false, "blocked", "make-room"]);
});

pendingE1("place_connected is one edit: a refused connect places nothing", () => {
  const gone = sheet(edits.remove_node(double, 13, 8).text);
  const before = gone.text;
  const placing = (from) => ({ kind: "place_connected", at: cell(14, 8), word: "𐑛𐑳𐑚𐑩𐑤", inputs: 1, outputs: 1, from });
  // 21 at 14:2 feeds the new node's input, as the tour's palette step does.
  const wired = shawline.apply(gone, placing(cell(14, 2)));
  assert.equal(wired.ok, true, wired.reason);
  assert.ok(wiredSinks(wired.doc.text).includes("14:8"));
  // ·𐑛𐑳𐑚𐑩𐑤's input at 5:0 can't reach a node in ·𐑥𐑱𐑯, so nothing is placed.
  const unwired = shawline.apply(gone, placing(cell(5, 0)));
  assert.deepEqual([unwired.ok, "doc" in unwired], [false, false]);
  assert.equal(gone.text, before, "the sheet is still the Doc it started from");
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
  const moved = shawline.apply(docs.get(double), EDITS.move_box(4, 10, 1, 0));
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
  assert.ok(tour.rings().length > 0, "step 1 rings 𐑐");
  tour.looked(view(src));
  assert.equal(tour.step, 0, "step 1 waits for Next");
  tour.advance();
  assert.ok(tour.rings().length > 0, "step 2 rings what to read");
  tour.looked(view(src));
  assert.equal(tour.step, 2, "a look at the layer is step 2");
  step(({ outWire }) => edits.remove_wire(src, outWire.cells[0].row, outWire.cells[0].col));
  step(({ tee, sink }) => edits.connect(src, tee.row, tee.col, sink.row, sink.col));
  step(({ node }) => edits.remove_node(src, node.top, node.left));
  // The palette's pick, as two edits: the tour reads the sheet, not the gesture.
  step(({ constant }) => {
    const placed = edits.place_node(src, constant.row - 1, constant.col + 6, "𐑛𐑳𐑚𐑩𐑤", 1, 1);
    assert.equal(placed.ok, true, placed.reason);
    return edits.connect(placed.text, constant.row, constant.col, constant.row, constant.col + 6);
  });
  step(({ tee, sink }) => edits.connect(src, tee.row, tee.col, sink.row, sink.col));
  assert.deepEqual(outputsOf(src), ["42"]);
  tour.ran(view(src));
  assert.equal(tour.finished, true);
  assert.deepEqual(tour.rings(), []);
});

test("the tour steps back on undo and forward again on redo", () => {
  const tour = new Tour(fakeHost(), () => {});
  tour.start(view(double));
  tour.advance();
  tour.looked(view(double));
  assert.equal(tour.step, 2, "at Cut a wire");
  const { outWire } = tour.parts;
  const cut = edits.remove_wire(double, outWire.cells[0].row, outWire.cells[0].col).text;
  tour.update(view(cut));
  assert.equal(tour.step, 3, "at Draw it back");
  tour.undone(view(double));
  assert.equal(tour.step, 2, "undoing the cut goes back to Cut a wire");
  tour.update(view(cut));
  assert.equal(tour.step, 3, "redoing it comes forward again");
  const { tee, sink } = tour.parts;
  const drawn = edits.connect(cut, tee.row, tee.col, sink.row, sink.col).text;
  tour.update(view(drawn));
  assert.equal(tour.step, 4, "drawing the wire back by hand moves on, whatever text it gives");
  tour.update(view(`${drawn}\n`));
  tour.undone(view(drawn));
  assert.equal(tour.step, 4, "an edit undone on the text a step started on stays there");
  tour.undone(view(cut));
  assert.equal(tour.step, 3, "the undo of the redrawn wire goes back to Draw it back");
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

test("the tour's first step shows 𐑐's mirror, with its reading written in, and waits for Next", () => {
  const host = fakeHost();
  const tour = new Tour(host, () => {}, (text) => `${text} [read]`);
  tour.start(view(double));
  const text = host.querySelector(".tour-text").textContent;
  assert.match(text, /𐑐 \(p\) adds, and its mirror 𐑚 \(b\) subtracts/);
  assert.doesNotMatch(text, /\[read\]/);
  assert.equal(host.querySelector(".tour-next").textContent, "Next");
  const [primitive] = tour.rings();
  assert.deepEqual([primitive.top, primitive.left, primitive.bottom, primitive.right], [4, 10, 7, 14]);
  tour.update(view(edits.remove_wire(double, 14, 18).text));
  assert.equal(tour.step, 0, "an edit isn't Next");
});

test("the tour's second step rings ·𐑥𐑱𐑯 with its strip and the 𐑐 node, and waits for a look", () => {
  const host = fakeHost();
  const tour = new Tour(host, () => {}, (text) => `${text} [read]`);
  tour.start(view(double));
  tour.advance();
  const [title, primitive] = tour.rings();
  assert.deepEqual({ ...title }, { top: 11, left: 3, bottom: 11, right: 6, under: true });
  assert.deepEqual([primitive.top, primitive.left, primitive.bottom, primitive.right], [4, 10, 7, 14]);
  const text = () => host.querySelector(".tour-text").textContent;
  assert.match(text(), /·𐑥𐑱𐑯 reads mayn/);
  assert.doesNotMatch(text(), /\[read\]/, "it writes its readings itself");
  tour.update(view(edits.remove_wire(double, 14, 18).text));
  assert.equal(tour.step, 1, "an edit isn't a look");
  tour.looked(view(double));
  assert.equal(tour.step, 2);
  assert.match(text(), /\[read\]$/, "the steps after it are shown through the layer");
});

// The gloss layer (PLAN P9): the Readings switch, the lone-Alt peek, and
// the drawing's geometry at either row pitch.

const { Layer } = await import("./layer.js");

function fakeLayer({ on = true, annotate = shawline.annotate } = {}) {
  const timers = new Map();
  let ids = 0;
  const flips = [];
  const layer = new Layer({
    on,
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

// Where cells sit (PLAN P10): wasm's Geometry at the drawing's zoom, and
// the page's zoom about the pointer (view.js).

const { CELL_MIN_PX, ZOOM_MIN, ZOOM_MAX, defaultZoom, fitZoom, zoomAbout, cellUnder } = await import("./view.js");

test("hit-testing finds the cell drawn under the pointer at three zooms, glossed and plain", () => {
  const d = sheet(double);
  for (const gloss of [false, true]) {
    const unit = d.geometry(gloss, 1).size();
    for (const zoom of [0.5, 1, 4]) {
      const g = d.geometry(gloss, zoom);
      const size = g.size();
      assert.ok(Math.abs(size.width - unit.width * zoom) < 1e-6 && Math.abs(size.height - unit.height * zoom) < 1e-6);
      // The drawn <svg> somewhere on the page, scrolled or not.
      const box = { left: 130.5, top: -40, right: 130.5 + size.width, bottom: -40 + size.height };
      for (const row of [0, 5, 14]) {
        for (const col of [0, 10, 22]) {
          const r = g.cell_rect(row, col, row, col, false, false);
          const at = (x, y) => cellUnder(g, box.left + x, box.top + y, box);
          assert.deepEqual({ ...at(r.x + 0.01, r.y + 0.01) }, { row, col }, `top-left of ${row}:${col} at ${zoom}`);
          assert.deepEqual({ ...at(r.x + r.width - 0.01, r.y + r.height - 0.01) }, { row, col });
          const { x, y } = g.cell_centre(row, col);
          assert.deepEqual({ ...at(x, y) }, { row, col });
          // A point in the strip under a row is in that row.
          const strip = g.cell_rect(row, col, row, col, true, false);
          assert.equal(strip.height > r.height, gloss, "a strip only with the layer on");
          if (gloss) assert.deepEqual({ ...at(x, strip.y + strip.height - 0.01) }, { row, col });
        }
      }
      assert.equal(cellUnder(g, box.left - 1, box.top + 5, box), null, "off the drawing");
      assert.equal(cellUnder(g, box.left + 1, box.top + 1, box), null, "in the margin, above and left of the grid");
    }
  }
});

test("the geometry at zoom 1 is the drawing's own size, glossed or plain", () => {
  const d = sheet(double);
  for (const gloss of [false, true]) {
    const svg = d.render(false, gloss);
    const [, w, h] = svg.match(/width="([\d.]+)" height="([\d.]+)"/).map(Number);
    const size = d.geometry(gloss, 1).size();
    assert.ok(Math.abs(size.width - w) < 0.01 && Math.abs(size.height - h) < 0.01, `${size.width}×${size.height} against ${w}×${h}`);
  }
  assert.equal(sheet("\t").geometry(true, 1), undefined);
});

test("zoom opens with cells at least 16 px wide, fits between 50% and 400%, and keeps the pointer's point still", () => {
  const g = sheet(double).geometry(true, 1);
  const cellWidth = g.cell_rect(0, 0, 0, 0, false, false).width;
  assert.ok(defaultZoom(cellWidth) * cellWidth >= CELL_MIN_PX - 1e-9);
  const size = g.size();
  assert.equal(fitZoom(size, { width: size.width * 100, height: size.height * 100 }), ZOOM_MAX);
  assert.equal(fitZoom(size, { width: 1, height: 1 }), ZOOM_MIN);
  assert.equal(fitZoom(size, { width: size.width * 2, height: size.height * 3 }), 2);
  const scroll = { left: 40, top: 10 };
  const pointer = { x: 100, y: 60 };
  const after = zoomAbout(1, 2, scroll, pointer);
  assert.deepEqual([(after.left + pointer.x) / 2, (after.top + pointer.y) / 2], [scroll.left + pointer.x, scroll.top + pointer.y]);
});

test("positions convert between cells and the textarea's UTF-16 offsets, byte-order mark and all", () => {
  const text = "﻿※ 𐑛𐑳𐑚\n▶ 𐑯 ─";
  assert.equal(shawline.offset_of_cell(text, 0, 0), 1, "the mark is no cell");
  assert.equal(shawline.offset_of_cell(text, 0, 3), 1 + 2 + 2, "Shavian letters are two units");
  assert.equal(shawline.offset_of_cell(text, 1, 2), text.indexOf("𐑯"));
  for (const [row, col] of [[0, 0], [0, 4], [1, 0], [1, 2], [1, 4]]) {
    assert.deepEqual({ ...shawline.cell_of_offset(text, shawline.offset_of_cell(text, row, col)) }, { row, col });
  }
});

test("diagnostics take their severity from wasm, and describe says what a word is", () => {
  const found = sheet(corpus("93-arity.shaw")).diagnostics();
  assert.ok(found.every((d) => ["error", "warning", "lint"].includes(d.severity)));
  const said = sheet(double).describe(5, 12, true);
  assert.equal(said.row, 5);
  assert.match(said.text, /𐑐/);
  assert.match(said.text, /add/);
  assert.equal(sheet(double).describe(9, 5, false), undefined);
});

test("box kinds come from shawline-edit, and branch names the cells a disconnect clears", () => {
  const kinds = sheet(corpus("04-abs.shaw")).boxes().map((b) => b.kind);
  assert.deepEqual([...new Set(kinds)].sort(), ["definition", "node", "switch"]);
  const cells = sheet(double).branch(6, 10).map((c) => `${c.row}:${c.col}`);
  assert.ok(cells.includes("6:6") && cells.includes("6:9"), cells.join(" "));
  assert.ok(!cells.includes("5:4"), "the other branch's stroke stays");
  assert.deepEqual(sheet(double).branch(3, 3), []);
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
  assert.deepEqual([at(11, 3).word, at(11, 3).reading, at(11, 3).keys, at(11, 3).span, at(11, 3).gloss], ["·𐑥𐑱𐑯", "mayn", "=mayn", 4, null]);
  assert.deepEqual([at(5, 12).word, at(5, 12).reading, at(5, 12).keys, at(5, 12).gloss], ["𐑐", "p", "p", "add"]);
  const title = at(2, 3);
  assert.deepEqual([title.word, title.reading, title.keys], ["·𐑛𐑳𐑚𐑩𐑤", "dubəl", "=dub@l"]);
  assert.ok(words.every((g) => typeof g.shown === "boolean"));
  assert.ok(!words.some((g) => /^\d+$/.test(g.word)), "numbers have no reading");
  assert.deepEqual(sheet("\t").glosses(), []);
  assert.deepEqual([shawline.reading("·𐑥𐑱𐑯"), shawline.reading("𐑣𐑧𐑤𐑴"), shawline.reading("𐑑𐑣")], ["mayn", "heloh", "t-h"]);
  assert.deepEqual([shawline.keys("·𐑥𐑱𐑯"), shawline.keys("𐑑𐑣"), shawline.keys("𐑝")], ["=mayn", "t/h", "/v"]);
  const peep = shawline.table().find(([, letter]) => letter === "𐑐");
  assert.deepEqual(peep, ["p", "𐑐", "peep"], "the key panel's entry names a word the letter sounds in");
});

test("a wire's cells say which join it southward, through the strip", () => {
  const input = sheet(double).wires().find((w) => w.cells.some((c) => c.row === 5 && c.col === 4));
  const down = (row, col) => input.cells.find((c) => c.row === row && c.col === col)?.down;
  assert.deepEqual([down(5, 4), down(5, 6), down(6, 6), down(6, 7)], [false, true, false, false]);
});

// The drawing as an editor (studio.js) driven by its keys and gestures over
// a fake page: each `write` is one undo step in the editor.

const { Studio, readView } = await import("./studio.js");
const { Choices, offer, placing } = await import("./palette.js");

function studioOn(text) {
  const docs = new Map();
  const views = new Map();
  const host = {
    current: text,
    steps: [],
    refusals: [],
    statuses: [],
    editing: [],
    /** Every edit sent to wasm, trials included. */
    sent: [],
    /** The texts whose inspector field was opened for naming. */
    naming: [],
    palettes: [],
    apply: (doc, edit) => (host.sent.push(edit), shawline.apply(doc, edit)),
    text: () => host.current,
    write: (t) => {
      host.steps.push(t);
      host.current = t;
    },
    doc: (t) => docs.get(t) ?? (docs.set(t, new Doc(t)), docs.get(t)),
    view: (t) => views.get(t) ?? (views.set(t, readView(host.doc(t))), views.get(t)),
    seed: (d) => {
      if (!docs.has(d.text)) docs.set(d.text, d);
      return d.text;
    },
    refused: (r, edit, then) => host.refusals.push({ ...r, edit, then }),
    status: (said, kind) => host.statuses.push([said, kind]),
    changed: () => {},
    palette: (at, mode) => host.palettes.push({ at, mode }),
    editText: (target) => host.editing.push(target),
    name: (target) => host.naming.push(target),
    help() {},
    zoom() {},
    gloss: () => false,
  };
  const studio = new Studio(host);
  const press = (key, more = {}) =>
    studio.keydown({ key, repeat: false, shiftKey: false, ctrlKey: false, metaKey: false, altKey: false, ...more });
  /** Presses `]` until `found(selection)`, at most once round the sheet. */
  const seek = (found) => {
    for (let i = 0; i < 300; i++) {
      press("]");
      if (studio.selection && found(studio.selection)) return studio.selection;
    }
    throw new Error("] never reached it");
  };
  return { studio, host, press, seek };
}

const sameCell = (a, b) => a.row === b.row && a.col === b.col;

test("a keyboard-only rewire of 03-double: ] and Delete take every wire off, W and Enter put them back", () => {
  const { studio, host, press, seek } = studioOn(double);
  const editable = () => sheet(host.current).wires().filter((w) => w.source).length;
  const removed = editable();
  while (editable()) {
    seek((s) => s.kind === "wire");
    assert.equal(press("Delete"), true);
  }
  assert.deepEqual(wiredSinks(host.current), []);
  // The .studio file's rewire order: source, then arrowhead.
  for (const [from, to] of [[cell(5, 0), cell(5, 10)], [cell(5, 14), cell(5, 22)], [cell(5, 0), cell(6, 10)], [cell(14, 2), cell(14, 8)], [cell(14, 16), cell(14, 22)]]) {
    press("Escape");
    press("Escape");
    seek((s) => s.kind === "source" && sameCell(s.source, from));
    press("w");
    assert.ok(studio.wiring, `W wires from ${from.row}:${from.col}`);
    for (let i = 0; i < 20 && !sameCell(studio.end, to); i++) press("]");
    assert.ok(sameCell(studio.end, to), `${to.row}:${to.col} glows`);
    press("Enter");
  }
  assert.deepEqual(host.refusals.map((r) => r.reason), []);
  assert.equal(host.steps.length, removed + 5, "one undo step per wire");
  assert.deepEqual(wiredSinks(host.current), wiredSinks(double));
  assert.deepEqual(outputsOf(host.current), outputsOf(double));
});

test("a palette pick made with the arrow keys places what is highlighted", () => {
  const gone = edits.remove_node(double, 13, 8).text;
  const callees = sheet(gone).palette();
  callees.forEach((c) => ((c.reading = shawline.reading(c.word)), (c.keys = shawline.keys(c.word))));
  const choices = new Choices(offer(callees, "place"));
  assert.deepEqual(choices.items.slice(0, 3).map((c) => c.gloss), ["Number", "Text", "empty list"], "constants first");
  choices.filter("d");
  assert.ok(choices.items.length > 1, "a query keeps several");
  assert.ok(choices.items.every((c) => [c.word, c.reading, c.keys, c.gloss.toLowerCase()].some((s) => s.includes("d"))));
  let moves = 0;
  while (choices.chosen.word !== "𐑛𐑳𐑚𐑩𐑤") {
    assert.equal(choices.key("ArrowDown"), "move");
    assert.ok(++moves < choices.items.length, "ArrowDown reaches every entry");
  }
  assert.ok(moves > 0, "the pick isn't the first entry");
  assert.equal(choices.key("ArrowUp"), "move");
  assert.equal(choices.key("ArrowDown"), "move");
  assert.equal(choices.key("Enter"), "pick");
  const { studio, host } = studioOn(gone);
  const edit = placing(choices.chosen, cell(14, 8), { mode: "place" });
  assert.equal(edit.kind, "place_node");
  assert.equal(studio.edit(edit), true, host.refusals[0]?.reason);
  assert.ok(sheet(host.current).boxes().some((b) => b.kind === "node" && b.top === 13 && b.left === 8));
  assert.equal(choices.key("Escape"), "close");
  assert.deepEqual(offer(callees, "source").filter((c) => c.kind === "node").every((c) => c.outputs > 0), true);
  assert.ok(offer(callees, "feed").every((c) => c.inputs > 0));
});

/**
 * Whether `feed` has landed: until it does it refuses as `unsupported`,
 * and the check that needs it is skipped with a marker.
 */
const unfed = edits.disconnect(double, 14, 8).text.replace("│ 21 ", "│    ");
const FEED = (() => {
  const probe = shawline.apply(sheet(unfed), { kind: "feed", to: cell(14, 8), source: { kind: "constant", content: "0" } });
  return probe.ok || probe.kind !== "unsupported";
})();
function pendingFeed(name, body) {
  if (FEED) return test(name, body);
  skipped++;
  console.log(`skip  ${name} (pending feed: it refuses as unsupported)`);
}

pendingFeed("an arrowhead fed from the palette gets a number as one feed edit and one undo step", () => {
  const { host, studio } = studioOn(unfed);
  const [number] = offer([], "source");
  assert.equal(number.gloss, "Number");
  const edit = placing(number, cell(15, 2), { mode: "source", to: cell(14, 8) });
  assert.deepEqual(edit, { kind: "feed", to: cell(14, 8), source: { kind: "constant", content: "0" } });
  assert.equal(studio.edit(edit), true, host.refusals[0]?.reason);
  assert.equal(host.steps.length, 1);
  assert.ok(wiredSinks(host.current).includes("14:8"));
});

test("feed is the one edit for a callee too, 𐑯𐑦𐑤 is fed as a constant, and wasm reads both", () => {
  const callees = sheet(double).palette().map((c) => ({ ...c, reading: "", keys: "" }));
  const [nil, show] = ["𐑯𐑦𐑤", "𐑖"].map((w) => offer(callees, "source").find((c) => c.word === w));
  const to = cell(14, 8);
  assert.deepEqual(placing(nil, cell(14, 3), { mode: "source", to }).source, { kind: "constant", content: "𐑯𐑦𐑤" });
  const fed = placing(show, cell(14, 3), { mode: "source", to });
  assert.deepEqual(fed.source, { kind: "node", word: "𐑖", inputs: 1, outputs: 1 });
  const unread = /^(no source|the edit has no|.* isn't)/;
  for (const edit of [fed, placing(nil, cell(14, 3), { mode: "source", to })]) {
    const result = shawline.apply(sheet(unfed), edit);
    assert.ok(result.ok || !unread.test(result.reason), result.reason);
  }
  const wrong = shawline.apply(sheet(unfed), { kind: "feed", to, source: { kind: "pipe" } });
  assert.deepEqual([wrong.ok, wrong.kind, wrong.reason], [false, "target", "no source pipe"]);
});

test("a selection survives a move: a held nudge is one undo step, and the box stays selected", () => {
  const { studio, host, press, seek } = studioOn(double);
  seek((s) => s.kind === "box" && s.box.top === 4 && s.box.left === 10);
  assert.equal(press("ArrowDown"), true);
  assert.equal(press("ArrowDown", { repeat: true }), true);
  assert.equal(host.steps.length, 0, "nothing is written while the key is held");
  assert.equal(host.refusals.length, 1, "the second row would cross the border, and says so");
  assert.equal(studio.keyup({ key: "ArrowDown" }), true);
  assert.equal(host.steps.length, 1);
  assert.deepEqual([studio.selection.kind, studio.selection.box.top, studio.selection.box.left], ["box", 5, 10]);
  assert.deepEqual({ ...studio.cursor }, { row: 5, col: 10 });
  assert.deepEqual(outputsOf(host.current), outputsOf(double));
});

test("a held nudge refused at the end offers its last step again on release, for Make room", () => {
  const { studio, host, press, seek } = studioOn(double);
  seek((s) => s.kind === "box" && s.box.top === 4 && s.box.left === 10);
  for (let k = 0; k < 12 && host.refusals.length === 0; k++) press("ArrowRight", { repeat: k > 0 });
  assert.ok(host.refusals.length > 0, "the box runs out of room");
  studio.keyup({ key: "ArrowRight" });
  const last = host.refusals.at(-1);
  const box = studio.selection.box;
  assert.deepEqual([last.edit.kind, last.edit.corner.row, last.edit.corner.col, last.edit.dr, last.edit.dc], ["move", box.top, box.left, 0, 1]);
  assert.equal(host.steps.length, 1, "the steps that fit are one undo step");
});

test("a branch selection survives a move", () => {
  const { studio, host, seek } = studioOn(double);
  seek((s) => s.kind === "branch" && sameCell(s.sink, cell(6, 10)));
  assert.equal(studio.edit(EDITS.move_box(4, 10, 1, 0)), true);
  const chosen = studio.selection;
  assert.deepEqual([chosen.kind, chosen.sink.row, chosen.sink.col], ["branch", 7, 10]);
  assert.deepEqual(chosen.cells.map((c) => `${c.row}:${c.col}`), sheet(host.current).branch(7, 10).map((c) => `${c.row}:${c.col}`));
  assert.ok(chosen.cells.length > 0);
});

pendingE1("a selection survives an insertion", () => {
  const { studio, host, press, seek } = studioOn(double);
  seek((s) => s.kind === "box" && s.box.top === 13 && s.box.left === 8);
  studio.cursor = cell(12, 4);
  assert.equal(press("r"), true);
  assert.equal(host.steps.length, 1, host.refusals[0]?.reason);
  assert.deepEqual([studio.selection.kind, studio.selection.box.top, studio.selection.box.left], ["box", 14, 8]);
  // The cursor followed the box down; the inserted row is still 12.
  studio.cursor = cell(12, 4);
  press("R");
  assert.equal(host.current, double, "deleting the row again gives the sheet back");
});

pendingE1("a selection survives a retext", () => {
  const { studio, host, press, seek } = studioOn(double);
  seek((s) => s.kind === "source" && s.text?.content === "21");
  assert.equal(press("Enter"), true);
  assert.equal(host.editing.length, 1, "Enter opens the selected text for editing");
  assert.equal(studio.edit({ kind: "set_text", at: cell(14, 2), content: "22" }), true, host.refusals[0]?.reason);
  assert.deepEqual([studio.selection.kind, studio.selection.text.content], ["source", "22"]);
});

pendingE1("a scrub commits one undo step, and stops at the last value that fits", () => {
  const { studio, host, seek } = studioOn(double);
  seek((s) => s.kind === "source" && s.text?.content === "21");
  assert.equal(studio.scrubbable, true);
  assert.equal(studio.scrubStart(), true);
  studio.scrubMove(8 * 3);
  studio.scrubMove(8 * 5 + 3);
  studio.scrubMove(-8 * 2, true);
  studio.scrubMove(8 * 2);
  assert.equal(host.steps.length, 0, "steps edit a preview, not the text");
  assert.match(studio.text, /│ 23 ─/);
  studio.scrubEnd();
  assert.equal(host.steps.length, 1);
  assert.match(host.current, /│ 23 ─/);
  assert.deepEqual([studio.selection.kind, studio.selection.text.content], ["source", "23"]);
});

test("a first edit on 03-double-ascii leaves two undo steps: the format, then the edit", () => {
  const ascii = corpus("03-double-ascii.shaw");
  const { studio, host } = studioOn(ascii);
  studio.select(studio.targetAt(cell(4, 10)));
  assert.equal(studio.selection.kind, "box");
  assert.equal(studio.edit(EDITS.move_box(4, 10, 1, 0)), true, host.refusals[0]?.reason);
  assert.equal(host.steps.length, 2);
  assert.equal(host.steps[0], corpus("03-double-ascii.fmt.shaw"));
  assert.equal(host.steps[1], edits.move_box(host.steps[0], 4, 10, 1, 0).text);
  assert.ok(host.statuses.some(([said]) => said === "Formatted the sheet to edit it"));
  assert.deepEqual([studio.selection.box.top, studio.selection.box.left], [5, 10]);
});

test("Tab leaves the drawing from any selection, and while wiring", () => {
  const { studio, press, seek } = studioOn(double);
  const kinds = new Set();
  for (let i = 0; i < 40; i++) {
    press("]");
    const before = studio.selection;
    kinds.add(before.kind);
    assert.equal(press("Tab"), false, `Tab from a ${before.kind}`);
    assert.equal(press("Tab", { shiftKey: true }), false);
    assert.equal(studio.selection, before, "Tab changes nothing");
  }
  assert.deepEqual([...kinds].sort(), ["box", "branch", "source", "text", "wire"]);
  // Every arrowhead of 03-double is wired, so W rewires a branch.
  seek((s) => s.kind === "branch");
  press("w");
  assert.ok(studio.wiring);
  assert.equal(press("Tab"), false);
  press("Escape");
  press("Escape");
  press("Escape");
  assert.deepEqual([studio.selection, studio.cursor], [null, null]);
  assert.equal(press("Tab"), false);
});

test("the live region says what the selection is, with its description", () => {
  const { studio, seek } = studioOn(double);
  seek((s) => s.kind === "box" && s.box.top === 4);
  assert.match(studio.describe(), /^Node at 5:11\..*𐑐.*Arrow keys nudge it/);
  seek((s) => s.kind === "source" && s.text?.content === "21");
  assert.match(studio.describe(), /^Number 21 at 15:3\./);
});

// Studio Phase 2, wave 2 (PLAN P10): drawing boxes, ports and arms, port
// counts, inline nodes and renaming, each gesture one edit.

/**
 * Whether agent E2's edits (`place_definition`, `add_port`, `set_ports`,
 * …) have landed: until they do, they refuse as `unsupported`, and the
 * checks that need them are skipped with a marker instead of failing.
 */
const E2 = (() => {
  const rect = { top: 0, left: 0, bottom: 4, right: 12 };
  const probe = shawline.apply(sheet(""), { kind: "place_definition", rect, name: "𐑥𐑱𐑯" });
  return probe.ok || probe.kind !== "unsupported";
})();
function pendingE2(name, body) {
  if (E2) return test(name, body);
  skipped++;
  console.log(`skip  ${name} (pendingE2: its edits refuse as unsupported)`);
}

const { KEYS, PORT_NAMES, portSpot, armsOf, switchLike, portCounts } = await import("./studio.js");

const abs = corpus("04-abs.shaw");
/** 04-abs without its comment, which no drawing gesture writes. */
const absBare = abs.split("\n").slice(2).join("\n");
const boxOf = (src, row, col) => sheet(src).boxes().find((b) => b.top === row && b.left === col);
const lastSent = (host) => JSON.parse(JSON.stringify(host.sent.at(-1)));
const unreadable = /^(no edit|the edit has no|.* isn't (a number|text)|.* is (out of range|negative))/;

/**
 * A main holding an empty square box guarded 0, as `place_switch` draws
 * one before its selector: three blank rows inside.
 */
const squareBox = (() => {
  const width = 19;
  const row = (inner) => `│${inner.padEnd(width - 2)}│`;
  return [
    `╭─ ·𐑥𐑱𐑯 ${"─".repeat(width - 9)}╮`,
    row(""),
    row(`  ┌─ 0 ${"─".repeat(5)}┐`),
    row(`  │${" ".repeat(9)}│`),
    row(`  │${" ".repeat(9)}│`),
    row(`  │${" ".repeat(9)}│`),
    row(`  └${"─".repeat(9)}┘`),
    row(""),
    `╰${"─".repeat(width - 2)}╯`,
    "",
  ].join("\n");
})();

test("boxes give their ports, dividers, titles and words, so the page classifies nothing", () => {
  const [definition, node, switchBox] = [boxOf(abs, 2, 0), boxOf(abs, 4, 14), boxOf(abs, 9, 12)];
  assert.deepEqual([definition.parent, definition.title.content, definition.word], [null, "·𐑨𐑚𐑕", null]);
  const ports = definition.ports.map((p) => `${p.row}:${p.col} ${p.side} ${p.kind} ${p.name.content}`);
  assert.deepEqual(ports, ["5:0 west arrowhead 𐑯", "13:34 east arrowhead 𐑬"]);
  assert.deepEqual([node.word.content, node.word.row, node.word.col, node.parent], ["𐑤", 5, 16, 0]);
  assert.deepEqual(node.ports.map((p) => `${p.side} ${p.kind}`), ["west arrowhead", "east tee", "west arrowhead"]);
  assert.deepEqual([switchBox.kind, switchBox.title.content, switchBox.gap.col, switchBox.word], ["switch", "0", 14, null]);
  assert.deepEqual(switchBox.dividers.map((d) => [d.row, d.guard]), [[11, null]]);
  assert.ok(switchBox.ports.some((p) => p.row === 9 && p.col === 20 && p.side === "north"), "the selector");
  const arms = armsOf(switchBox).map((a) => [a.top, a.bottom, a.guard?.content ?? null, a.divider]);
  assert.deepEqual(arms, [[10, 10, "0", null], [12, 15, null, 11]]);
  // The square box place_switch draws reads as a node with no word.
  const empty = sheet(squareBox).boxes().find((b) => b.parent !== null);
  assert.deepEqual([empty.kind, empty.word, empty.title.content, switchLike(empty)], ["node", null, "0", true]);
  assert.equal(switchLike(node), false);
});

test("apply reads remove_port, remove_arm, remove_box and set_ports", () => {
  for (const edit of [
    { kind: "remove_port", at: cell(9, 20) },
    { kind: "remove_arm", at: cell(11, 15) },
    { kind: "remove_box", at: cell(9, 12) },
    { kind: "set_ports", corner: cell(4, 14), inputs: 3, outputs: 1 },
  ]) {
    const result = shawline.apply(sheet(abs), edit);
    assert.ok(result.ok || !unreadable.test(result.reason), `${edit.kind}: ${result.reason}`);
  }
  const wrong = shawline.apply(sheet(abs), { kind: "set_ports", corner: cell(4, 14), inputs: 3 });
  assert.deepEqual([wrong.ok, wrong.kind, wrong.reason], [false, "target", "the edit has no outputs"]);
});

test("the Box tool sends place_definition at top level, named 𐑥𐑱𐑯 while there is no main, and place_switch inside one", () => {
  const empty = studioOn("");
  const rect = { top: 0, left: 0, bottom: 4, right: 12 };
  empty.studio.placeBox(rect);
  assert.deepEqual(lastSent(empty.host), { kind: "place_definition", rect, name: "𐑥𐑱𐑯" });
  const { studio, host, press } = studioOn(double);
  const below = { top: 19, left: 0, bottom: 23, right: 16 };
  studio.placeBox(below);
  assert.deepEqual(lastSent(host), { kind: "place_definition", rect: below, name: "𐑯𐑿" }, "a sheet with a main gets 𐑯𐑿");
  const inside = { top: 3, left: 16, bottom: 8, right: 20 };
  studio.placeBox(inside);
  assert.deepEqual(lastSent(host), { kind: "place_switch", rect: inside, guard: "0" });
  // The keyboard path: B at one corner, the arrow keys to the other, Enter.
  const keyed = studioOn(double);
  keyed.studio.cursor = cell(19, 0);
  keyed.press("b");
  for (let i = 0; i < 4; i++) keyed.press("ArrowDown");
  for (let i = 0; i < 16; i++) keyed.press("ArrowRight");
  assert.deepEqual(keyed.studio.sketched, below);
  assert.equal(keyed.host.sent.length, 0, "nothing is sent while the box is drawn");
  keyed.press("Enter");
  assert.deepEqual([keyed.host.sent.length, lastSent(keyed.host)], [1, { kind: "place_definition", rect: below, name: "𐑯𐑿" }]);
  assert.equal(keyed.studio.sketched, null);
  keyed.press("b");
  keyed.press("Escape");
  assert.deepEqual([keyed.studio.sketched, keyed.host.sent.length], [null, 1], "Escape stops it");
});

test("a click on a definition's border adds a port named 𐑨, or the next free letter; a node's takes no name", () => {
  const view = readView(sheet(double));
  assert.equal(portSpot(view, cell(7, 0))?.kind, "definition", "plain left border");
  for (const at of [cell(2, 0), cell(5, 0), cell(2, 4), cell(9, 5), cell(5, 12)]) {
    assert.equal(portSpot(view, at), null, `no port at ${at.row}:${at.col}`);
  }
  const { studio, host, press } = studioOn(double);
  studio.addPort(cell(7, 0));
  assert.deepEqual(lastSent(host), { kind: "add_port", at: cell(7, 0), name: "𐑨" });
  const taken = studioOn(double.replace("▶ 𐑯 ──", "▶ 𐑨 ──"));
  taken.studio.addPort(cell(7, 0));
  assert.deepEqual(lastSent(taken.host), { kind: "add_port", at: cell(7, 0), name: PORT_NAMES[1] }, "𐑨 is taken, so 𐑚");
  studio.addPort(cell(6, 14));
  assert.deepEqual(lastSent(host), { kind: "add_port", at: cell(6, 14), name: null }, "𐑐's right border");
  // P at the cursor is the same gesture.
  const keyed = studioOn(double);
  keyed.studio.cursor = cell(7, 0);
  keyed.press("p");
  assert.deepEqual([keyed.host.sent.length, lastSent(keyed.host)], [1, { kind: "add_port", at: cell(7, 0), name: "𐑨" }]);
  // A refused port carries its follow-up, so Make room can open the name field too.
  const corner = studioOn(double);
  corner.studio.addPort(cell(2, 0));
  assert.equal(typeof corner.host.refusals.at(-1)?.then, "function");
});

test("+ arm adds a default arm under a guarded one, and a guarded arm above the default", () => {
  const { studio, host, press } = studioOn(squareBox);
  const empty = readView(sheet(squareBox)).boxes.find((b) => b.parent !== null);
  studio.select({ kind: "box", box: empty });
  studio.addArm();
  assert.deepEqual(lastSent(host), { kind: "add_arm", at: cell(4, 3), guard: null }, "under the first row, for the default");
  const keyed = studioOn(squareBox);
  keyed.studio.select({ kind: "box", box: empty });
  keyed.press("a");
  assert.deepEqual([keyed.host.sent.length, lastSent(keyed.host)], [1, { kind: "add_arm", at: cell(4, 3), guard: null }]);
  // 04-abs's switch has a default arm, so the new arm is guarded 1 and
  // splits the arm above it; that arm is full, so the edit is the one to say so.
  const full = studioOn(abs);
  full.studio.select({ kind: "box", box: boxOf(abs, 9, 12) });
  full.press("A");
  assert.deepEqual(lastSent(full.host), { kind: "add_arm", at: cell(10, 12), guard: "1" });
  const none = studioOn(double);
  none.studio.select(none.studio.targetAt(cell(4, 10)));
  none.press("a");
  assert.equal(none.host.sent.length, 0, "a node has no arms");
});

test("the inspector's steppers, and I and O, send one set_ports for the selected node", () => {
  const node = boxOf(double, 4, 10);
  const { studio, host } = studioOn(double);
  studio.setPorts(node, 3, 1);
  assert.deepEqual(lastSent(host), { kind: "set_ports", corner: cell(4, 10), inputs: 3, outputs: 1 });
  for (const [key, inputs, outputs] of [["i", 3, 1], ["I", 1, 1], ["o", 2, 2], ["O", 2, 0]]) {
    const keyed = studioOn(double);
    keyed.studio.select({ kind: "box", box: node });
    keyed.press(key, { shiftKey: key === key.toUpperCase() });
    assert.deepEqual([keyed.host.sent.length, lastSent(keyed.host)], [1, { kind: "set_ports", corner: cell(4, 10), inputs, outputs }], key);
  }
  const definition = studioOn(double);
  definition.studio.select({ kind: "box", box: boxOf(double, 2, 0) });
  definition.press("i");
  assert.equal(definition.host.sent.length, 0, "a definition's ports are drawn one by one");
});

test("a double-click on a straight stretch of wire opens the 1→1 palette, and the pick is one place_inline", () => {
  const { studio, host, press } = studioOn(double);
  assert.equal(studio.inlineAt(cell(5, 18)), true);
  for (const at of [cell(5, 6), cell(6, 6), cell(3, 5), cell(5, 12)]) assert.equal(studio.inlineAt(at), false, `${at.row}:${at.col}`);
  const callees = sheet(double).palette().map((c) => ({ ...c, reading: "", keys: "" }));
  const offered = offer(callees, "inline");
  assert.ok(offered.every((c) => c.inputs === 1 && c.outputs === 1));
  assert.deepEqual(["𐑖", "𐑠", "𐑛𐑳𐑚𐑩𐑤"].map((w) => offered.some((c) => c.word === w)), [true, true, true]);
  assert.ok(!offered.some((c) => c.word === "𐑐"), "𐑐 takes two");
  const show = offered.find((c) => c.word === "𐑖");
  studio.pick(placing(show, cell(5, 18), { mode: "inline" }));
  assert.deepEqual(lastSent(host), { kind: "place_inline", at: cell(5, 18), word: "𐑖" });
  studio.cursor = cell(5, 18);
  press("/");
  studio.cursor = cell(3, 5);
  press("/");
  assert.deepEqual(host.palettes.map((p) => p.mode), ["inline", "place"], "/ on a wire splices, elsewhere it places");
});

test("a node is renamed with one set_text on its word, from the inspector or in place; a definition keeps its dot", () => {
  const { studio, host, press } = studioOn(double);
  studio.select({ kind: "box", box: boxOf(double, 4, 10) });
  const word = studio.editable();
  assert.deepEqual([word.text.content, word.at.row, word.at.col], ["𐑐", 5, 12]);
  press("Enter");
  assert.deepEqual([host.editing.length, host.editing.at(-1).text.content], [1, "𐑐"], "Enter opens its word in place");
  studio.retext(word, "𐑚");
  assert.deepEqual(lastSent(host), { kind: "set_text", at: cell(5, 12), content: "𐑚" });
  // A definition's name is renamed today (wave 1), its namer dot kept.
  const named = studioOn(double);
  named.studio.select({ kind: "box", box: boxOf(double, 2, 0) });
  const title = named.studio.editable();
  assert.equal(title.text.content, "·𐑛𐑳𐑚𐑩𐑤");
  assert.equal(named.studio.retext(title, "𐑑𐑢𐑲𐑕"), true, named.host.refusals[0]?.reason);
  assert.deepEqual([named.host.steps.length, boxOf(named.host.current, 2, 0).title.content], [1, "·𐑑𐑢𐑲𐑕"]);
  assert.deepEqual([named.studio.selection.kind, named.studio.selection.box.top], ["box", 2], "the definition stays selected");
});

test("Delete removes a selected definition, switch, arm or port with one edit each", () => {
  const removal = (src, pick) => {
    const { studio, host, press } = studioOn(src);
    studio.select(pick(studio));
    assert.ok(studio.selection, "something is selected");
    press("Delete");
    assert.equal(host.sent.length, 1);
    return lastSent(host);
  };
  const box = (row, col) => (s) => ({ kind: "box", box: s.view().boxes.find((b) => b.top === row && b.left === col) });
  const at = (row, col) => (s) => s.targetAt(cell(row, col));
  assert.deepEqual(removal(abs, box(2, 0)), { kind: "remove_box", at: cell(2, 0) });
  assert.deepEqual(removal(abs, box(9, 12)), { kind: "remove_box", at: cell(9, 12) });
  assert.deepEqual(removal(abs, box(4, 14)), { kind: "remove_node", at: cell(4, 14) });
  assert.deepEqual(removal(abs, at(11, 15)), { kind: "remove_arm", at: cell(11, 12) });
  assert.deepEqual(removal(abs, at(5, 18)), { kind: "remove_port", at: cell(5, 18) }, "𐑤's output tee");
  assert.deepEqual(removal(abs, at(5, 2)), { kind: "remove_port", at: cell(5, 0) }, "an input, by its name");
  assert.deepEqual(removal(abs, at(13, 36)), { kind: "remove_port", at: cell(13, 34) }, "an output, by its name");
  const unwired = edits.disconnect(abs, 9, 20).text;
  assert.deepEqual(removal(unwired, at(9, 20)), { kind: "remove_port", at: cell(9, 20) }, "an unwired selector");
  assert.deepEqual(removal(abs, at(9, 20)), { kind: "disconnect", at: cell(9, 20) }, "a wired one loses its branch first");
  // ] reaches the arm.
  const { seek } = studioOn(abs);
  assert.equal(seek((s) => s.kind === "arm").divider, 11);
});

test("every new gesture's key is in the ? overlay", () => {
  const keys = KEYS.map(([k]) => k);
  for (const k of ["B", "P", "A", "I, Shift+I", "O, Shift+O", "Enter", "Delete", "/"]) assert.ok(keys.includes(k), k);
  assert.match(KEYS.find(([k]) => k === "/")[1], /wire/);
});

pendingE2("the Box tool draws a definition, and typing in its open field renames it", () => {
  const { studio, host } = studioOn("");
  assert.equal(studio.placeBox({ top: 0, left: 0, bottom: 4, right: 16 }), true, host.refusals[0]?.reason);
  const made = boxOf(host.current, 0, 0);
  assert.deepEqual([made.kind, made.title.content], ["definition", "·𐑥𐑱𐑯"]);
  assert.equal(host.naming.at(-1).text.content, "·𐑥𐑱𐑯", "its name's field opens");
  assert.equal(studio.retext(host.naming.at(-1), "𐑛𐑳𐑚𐑩𐑤"), true, host.refusals[0]?.reason);
  assert.equal(boxOf(host.current, 0, 0).title.content, "·𐑛𐑳𐑚𐑩𐑤");
  assert.equal(host.steps.length, 2, "one undo step each");
});

pendingE2("a port added to a definition is named 𐑨, its field opens, and typing renames it", () => {
  const { studio, host } = studioOn(double);
  assert.equal(studio.addPort(cell(7, 0)), true, host.refusals[0]?.reason);
  const port = boxOf(host.current, 2, 0).ports.find((p) => p.row === 7);
  assert.deepEqual([port.side, port.name.content], ["west", "𐑨"]);
  assert.deepEqual([host.naming.length, host.naming.at(-1).text.content], [1, "𐑨"]);
  assert.equal(studio.retext(host.naming.at(-1), "𐑥"), true, host.refusals[0]?.reason);
  assert.equal(boxOf(host.current, 2, 0).ports.find((p) => p.row === 7).name.content, "𐑥");
});

pendingE2("+ arm splits an empty box into a guarded arm and a default arm, and keeps it selected", () => {
  const { studio, host } = studioOn(squareBox);
  studio.select({ kind: "box", box: readView(sheet(squareBox)).boxes.find((b) => b.parent !== null) });
  assert.equal(studio.addArm(), true, host.refusals[0]?.reason);
  const split = sheet(host.current).boxes().find((b) => b.parent !== null);
  assert.deepEqual(split.dividers.map((d) => [d.row, d.guard]), [[4, null]]);
  assert.deepEqual([studio.selection.kind, studio.selection.box.top, studio.selection.box.left], ["box", 2, 3]);
});

pendingE2("a stepper gives a node another input, and the node stays selected", () => {
  const { studio, host } = studioOn(double);
  studio.select({ kind: "box", box: boxOf(double, 4, 10) });
  assert.equal(studio.setPorts(studio.selection.box, 3, 1), true, host.refusals[0]?.reason);
  const node = boxOf(host.current, 4, 10);
  assert.deepEqual([node.ports.filter((p) => p.side === "west").length, studio.selection.box.bottom], [3, node.bottom]);
  assert.ok(wiredSinks(host.current).includes("5:10") && wiredSinks(host.current).includes("6:10"), "ports 0 and 1 keep their wires");
});

pendingE2("a 1→1 call is spliced into a wire, and its word is selected", () => {
  const { studio, host } = studioOn(double);
  assert.equal(studio.pick({ kind: "place_inline", at: cell(5, 18), word: "𐑖" }), true, host.refusals[0]?.reason);
  assert.ok(sheet(host.current).texts().some((t) => t.row === 5 && t.col === 18 && t.content === "𐑖"));
  assert.deepEqual([studio.selection.text.content, studio.selection.at.col], ["𐑖", 18]);
  assert.ok(wiredSinks(host.current).includes("5:22"), "the definition's output keeps its wire");
});

pendingE2("a node renamed in the inspector keeps its box selected", () => {
  const { studio, host } = studioOn(double);
  studio.select({ kind: "box", box: boxOf(double, 4, 10) });
  assert.equal(studio.retext(studio.editable(), "𐑚"), true, host.refusals[0]?.reason);
  assert.equal(boxOf(host.current, 4, 10).word.content, "𐑚");
  assert.deepEqual([studio.selection.kind, studio.selection.box.word.content], ["box", "𐑚"]);
});

pendingE2("Delete takes out an arm, a port and a switch, one undo step each", () => {
  const { studio, host, press } = studioOn(abs);
  studio.select(studio.targetAt(cell(11, 15)));
  press("Delete");
  assert.deepEqual([host.steps.length, boxOf(host.current, 9, 12)?.dividers.length], [1, 0], host.refusals[0]?.reason);
  studio.select(studio.targetAt(cell(5, 18)));
  press("Delete");
  assert.equal(host.steps.length, 2, host.refusals[0]?.reason);
  assert.equal(portCounts(boxOf(host.current, 4, 14)).outputs, 0);
  studio.select({ kind: "box", box: boxOf(host.current, 9, 12) });
  press("Delete");
  assert.equal(host.steps.length, 3, host.refusals[0]?.reason);
  assert.equal(boxOf(host.current, 9, 12), undefined);
});

pendingE2("a drawing edit refused for room carries its edit, so Make room is offered when it would fit", () => {
  const { studio, host } = studioOn(double);
  studio.select({ kind: "box", box: boxOf(double, 4, 10) });
  // Seven rows tall, the node would cross ·𐑛𐑳𐑚𐑩𐑤's bottom border.
  assert.equal(studio.setPorts(studio.selection.box, 5, 1), false);
  const refusal = host.refusals.at(-1);
  assert.deepEqual([refusal.kind, refusal.remedy], ["blocked", "make-room"]);
  assert.deepEqual(JSON.parse(JSON.stringify(refusal.edit)), { kind: "set_ports", corner: cell(4, 10), inputs: 5, outputs: 1 });
  // The page's offer: make_room of the refused edit, shown only if it succeeds, applied as one step.
  const roomed = shawline.apply(sheet(host.current), { kind: "make_room", edit: refusal.edit });
  if (roomed.ok) {
    studio.commit(studio.settle(roomed), host.current, refusal.then);
    assert.equal(host.steps.length, 1);
    assert.equal(portCounts(boxOf(host.current, studio.selection.box.top, studio.selection.box.left)).inputs, 5);
  } else {
    assert.equal(roomed.remedy, null, "a refused make_room offers nothing");
  }
});

pendingE2("04-abs builds from an empty sheet through the page's own gestures", () => {
  const { studio, host } = studioOn("");
  const ok = (done, what) => assert.equal(done, true, `${what}: ${host.refusals.at(-1)?.reason}`);
  /** Types a name into the field the last gesture opened. */
  const name = (content) => ok(studio.retext(host.naming.at(-1), content), `name ${content}`);
  // Definitions and their ports, each named as its field opens.
  ok(studio.placeBox({ top: 0, left: 0, bottom: 16, right: 34 }), "·𐑨𐑚𐑕");
  name("𐑨𐑚𐑕");
  ok(studio.addPort(cell(3, 0)), "input");
  name("𐑯");
  ok(studio.addPort(cell(11, 34)), "output");
  name("𐑬");
  ok(studio.placeBox({ top: 18, left: 0, bottom: 27, right: 30 }), "·𐑥𐑱𐑯");
  assert.equal(host.naming.at(-1).text.content, "·𐑥𐑱𐑯", "the sheet had no main again");
  ok(studio.addPort(cell(21, 30)), "output");
  name("𐑢𐑳𐑯");
  ok(studio.addPort(cell(24, 30)), "output");
  name("𐑑𐑵");
  // Nodes from the palette, the switch with the Box tool, then its
  // selector, its through-ports and `+ arm`.
  const node = (word, corner) => {
    const callee = offer(sheet(host.current).palette(), "place").find((c) => c.word === word);
    ok(studio.pick(placing(callee, cell(corner.row + 1, corner.col), { mode: "place" })), word);
  };
  node("𐑤", cell(2, 14));
  ok(studio.placeBox({ top: 7, left: 12, bottom: 14, right: 26 }), "switch");
  for (const at of [cell(7, 20), cell(8, 12), cell(8, 26), cell(11, 26), cell(12, 12)]) ok(studio.addPort(at), `port ${at.row}:${at.col}`);
  ok(studio.addArm(boxOf(host.current, 7, 12)), "+ arm");
  node("𐑚", cell(10, 18));
  node("𐑚", cell(20, 6));
  node("𐑨𐑚𐑕", cell(20, 16));
  node("𐑨𐑚𐑕", cell(23, 16));
  // Constants: each arrowhead fed a Number from the palette, valued in the inspector.
  const [number] = offer([], "source");
  for (const [to, value] of [[cell(4, 14), "0"], [cell(11, 18), "0"], [cell(21, 6), "0"], [cell(22, 6), "7"], [cell(24, 16), "5"]]) {
    ok(studio.pick(placing(number, to, { mode: "source", to })), `feed ${to.row}:${to.col}`);
    if (value === "0") continue;
    const constant = sheet(host.current).texts().filter((t) => t.kind === "number" && t.row === to.row && t.col < to.col).at(-1);
    ok(studio.retext({ kind: "source", at: constant, text: constant }, value), `value ${value}`);
  }
  // Every connection, arrowheads in row-major order. The switch's merged
  // output is named by its first arm's right through-port.
  const links = [
    [cell(3, 0), cell(3, 14)], [cell(3, 18), cell(7, 20)], [cell(3, 0), cell(8, 12)], [cell(8, 12), cell(8, 26)],
    [cell(11, 22), cell(11, 26)], [cell(8, 26), cell(11, 34)], [cell(3, 0), cell(12, 12)], [cell(12, 12), cell(12, 18)],
    [cell(21, 10), cell(21, 16)], [cell(21, 22), cell(21, 30)], [cell(24, 22), cell(24, 30)],
  ];
  for (const [from, to] of links) ok(studio.connect(from, to), `connect ${from.row}:${from.col} ${to.row}:${to.col}`);
  assert.deepEqual(host.refusals.map((r) => r.reason), []);
  assert.deepEqual(sheet(host.current).diagnostics().map((d) => d.code), sheet(absBare).diagnostics().map((d) => d.code));
  assert.deepEqual(outputsOf(host.current), outputsOf(absBare));
  assert.deepEqual(wiredSinks(host.current), wiredSinks(absBare));
  assert.equal(sheet(host.current).formatted(), host.current);
});

if (skipped) console.log(`${skipped} skipped until E1's or E2's edits land`);
process.exit(failed ? 1 : 0);
