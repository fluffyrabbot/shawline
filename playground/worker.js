// Runs programs off the main thread, so a program that never stops can be
// cut off without freezing the page.
import init, { run } from "./pkg/shawline_wasm.js";

const ready = init();

onmessage = async ({ data: src }) => {
  await ready;
  const started = performance.now();
  const result = run(src);
  postMessage({ ...result, ms: performance.now() - started });
};
