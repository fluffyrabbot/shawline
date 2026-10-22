// Runs programs off the main thread, so a program that never stops can be
// cut off without freezing the page. The same worker, as a second instance,
// traces sheets for the drawing's value badges.
//
// A request is `{ kind, src, rev }`, where `kind` is "run" or "trace". The
// page's memory isn't shared, so it sends the text rather than its Doc. The
// result echoes `kind` and `rev`, the editor revision it describes.
import init, { run, trace } from "./pkg/shawline_wasm.js";

const ready = init();

onmessage = async ({ data: { kind, src, rev } }) => {
  // A panic traps the module, and a module that didn't load can't run
  // anything; the page retires this worker on `failed`.
  try {
    await ready;
    const started = performance.now();
    const result = kind === "trace" ? trace(src) : run(src);
    postMessage({ ...result, kind, rev, ms: performance.now() - started });
  } catch (e) {
    postMessage({ failed: String(e), kind, rev });
  }
};
