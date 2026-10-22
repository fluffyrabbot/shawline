# Shawline

A dataflow programming language whose source is a drawing: Shavian-alphabet words, box-drawing boxes and wires. Language: [docs/SPEC.md](docs/SPEC.md). Build plan and orchestration rules: [docs/PLAN.md](docs/PLAN.md).

## Commands

- `cargo test --workspace`: unit tests plus the corpus oracle
- `cargo test -p shawline-corpus`: the corpus oracle and the formatter property test
- `SHAWLINE_BLESS=<stage>[,…] cargo test -p shawline-corpus`: rewrite expectations (lead only)
- `cargo run --bin shawline -- check|run|fmt|render [--dark]|diagram|ast <file.shaw>`
- `cargo run -p shawline-font --example build -- <out.ttf>`: build Shawline Mono
- `cargo clippy --workspace --all-targets`
- `scripts/capped [--mem MB] [--time S] -- <command>`: run a command under a memory and time cap

## Rules for agents

- Touch only the crate you were assigned. Contract files named in PLAN.md (`wire.rs`, `ast.rs`, `ir.rs`, `bytecode.rs`, the signatures in `shawline-edit/src/lib.rs`, `shawline-vm/src/trace.rs`, `shawline-ir/src/trace.rs`) and the whole of `shawline-grid`, `shawline-doc` and `shawline-corpus` are lead-owned.
- Never edit files under `corpus/`. If an expectation looks wrong, stop and report it with a proposed diff.
- Your oracle is your crate's tests and your corpus stage. Corpus failures in other stages are expected while a wave is in flight.
- Don't review or test other agents' crates, add harness or mock crates, or introduce snapshot frameworks.
- New crates.io dependencies go in your own crate's `Cargo.toml` only.
- Run anything that executes generated or mutated programs (fuzzers, property loops, scratch harnesses, `shawline run` on inputs you made up) under `scripts/capped`. It kills the whole process tree past 2 GB or 120 s. Keep such loops finite. macOS ignores `ulimit -v`, and an uncapped fuzzer once took this machine to 46 GB and froze it. The VM's limits (`R007`–`R009`) bound a running program, but nothing bounds parser, formatter or harness code. Run programs with `scripts/capped -- cargo run --bin shawline -- …`, never a prebuilt `target/` binary, which may be stale and lack the limits.

## Code conventions

- Every repertoire character is one cell, so columns are `char` indices. Never use byte or grapheme widths for layout.
- Positions are zero-based `Pos { row, col }` internally and one-based only in user-facing CLI output.
- Dump formats (`Display` impls checked by the corpus) are one fact per line in a stable order. Changing one means re-blessing, which the lead does.
- Comments are sparse and say why. Doc comments on public items.
