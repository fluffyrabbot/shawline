// Runs programs off the main thread, so a program that never stops can be
// cut off without freezing the page. The same worker, as a second instance,
// traces sheets for the drawing's value badges.
import init, { run, trace } from "./pkg/shawline_wasm.js";

const ready = init();

onmessage = async ({ data }) => {
  await ready;
  if (data?.kind === "trace") {
    // A failed trace (a panic traps the module) must cost only the badges.
    try {
      postMessage({ ...trace(data.src), src: data.src, gen: data.gen });
    } catch (e) {
      postMessage({ failed: String(e), src: data.src, gen: data.gen });
    }
    return;
  }
  const src = data;
  const started = performance.now();
  const result = run(src);
  postMessage({ ...result, ms: performance.now() - started });
};
