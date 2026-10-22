// Jobs on a worker, one at a time (PLAN P8). Every job and every result
// carries `rev`, the editor revision it was asked for, so the page can tell
// a result that no longer describes the text from one that does.
//
// - One job is in flight. A request made meanwhile waits, and a newer
//   request replaces it: only the latest text is worth running next.
// - Each job has its own time limit, set when it starts and cleared when it
//   ends. Past it the worker is killed, and the next job gets a fresh one,
//   so a newer request can neither extend an older job's limit nor leave it
//   running unwatched.
// - A worker whose module panicked (`failed`) is retired the same way: a
//   trapped module can't be trusted again.

export class Flight {
  /**
   * - `spawn()` makes a worker.
   * - `limit` is how long a job may run, in milliseconds.
   * - `settle(job, outcome)` hears how each job ended: the worker's result,
   *   `{ timedOut: true }` when it was killed at its limit, or `{ broken }`
   *   with a message when the worker itself failed (then `job` may be null,
   *   and the pending request is dropped, since a module that doesn't load
   *   would fail again at once).
   * - With `current()`, the current revision, a job asked for an earlier
   *   one settles silently: its result describes a text that is gone.
   */
  constructor({ spawn, limit, settle, current = null, timers = globalThis }) {
    Object.assign(this, { spawn, limit, settle, current, timers });
    this.worker = null;
    this.job = null;
    this.pending = null;
    this.timer = null;
    /** The job in flight when `cancel` was called, settled silently. */
    this.dropped = null;
  }

  get busy() {
    return this.job !== null;
  }

  /** Starts `job` now, or after the job in flight, replacing any job waiting. */
  request(job) {
    if (this.job) this.pending = job;
    else this.start(job);
  }

  /** Drops the waiting request, and the result of the job in flight. */
  cancel() {
    this.pending = null;
    this.dropped = this.job;
  }

  /** Kills the job in flight and drops the waiting one. Nothing settles. */
  stop() {
    this.pending = this.dropped = null;
    if (!this.job) return;
    this.timers.clearTimeout(this.timer);
    this.job = null;
    this.retire();
  }

  /** Starts a worker ahead of the next job, so it has loaded by then. */
  warm() {
    this.worker ??= this.hire();
  }

  start(job) {
    this.warm();
    this.job = job;
    this.worker.postMessage(job);
    this.timer = this.timers.setTimeout(() => this.end(job, { timedOut: true }, true), this.limit);
  }

  hire() {
    const worker = this.spawn();
    worker.onmessage = ({ data }) => {
      if (worker === this.worker && this.job) this.end(this.job, data, Boolean(data.failed));
    };
    worker.onerror = (e) => {
      e.preventDefault?.();
      if (worker !== this.worker) return;
      const { job, dropped } = this;
      this.timers.clearTimeout(this.timer);
      this.job = this.pending = this.dropped = null;
      this.retire();
      if (!job || job !== dropped) this.settle(job, { broken: e.message ?? "" });
    };
    return worker;
  }

  retire() {
    this.worker?.terminate();
    this.worker = null;
  }

  end(job, outcome, retire) {
    if (job !== this.job) return;
    this.timers.clearTimeout(this.timer);
    this.job = null;
    if (retire) this.retire();
    // The next job starts before this one settles, so whatever `settle`
    // requests waits its turn rather than starting a second job.
    const next = this.pending;
    this.pending = null;
    if (next) this.start(next);
    const dropped = job === this.dropped;
    if (dropped) this.dropped = null;
    if (!dropped && (!this.current || job.rev === this.current())) this.settle(job, outcome);
  }
}
