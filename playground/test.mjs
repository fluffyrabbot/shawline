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
  const found = shawline.check(corpus("93-arity.shaw"));
  const d = found.find((d) => d.code === "C002");
  assert.ok(d, `no C002 in ${JSON.stringify(found)}`);
  assert.deepEqual([d.row, d.col, d.code, d.severity], [Number(row), Number(col), code, "error"]);
  assert.equal(typeof d.message, "string");
});

test("format 03-double-ascii", () => {
  assert.equal(shawline.format(corpus("03-double-ascii.shaw")), corpus("03-double-ascii.fmt.shaw"));
});

test("render follows the theme", () => {
  const src = corpus("01-hello.shaw");
  assert.match(shawline.render(src, false), /#faf9f6/i);
  assert.match(shawline.render(src, true), /#17191e/i);
  assert.equal(shawline.render("\t", false), undefined);
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
  const node = shawline.boxes(src).find((b) => b.kind === "node");
  assert.deepEqual([node.top, node.left, node.bottom, node.right], [4, 10, 7, 14]);
  const moved = shawline.move_box(src, node.top, node.left, 1, 0);
  assert.equal(moved.ok, true, moved.reason);
  assert.ok(shawline.boxes(moved.text).some((b) => b.kind === "node" && b.top === 5 && b.left === 10));
  assert.equal(shawline.format(moved.text), moved.text);
  assert.deepEqual(shawline.run(moved.text).outputs, shawline.run(src).outputs);
  const refused = shawline.move_box(src, node.top, node.left, 0, 20);
  assert.equal(refused.ok, false);
  assert.deepEqual([refused.row, refused.col], [4, 10]);
  assert.match(refused.reason, /leave its definition/);
});

process.exit(failed ? 1 : 0);
