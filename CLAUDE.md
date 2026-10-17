# Shawline

A dataflow programming language whose source is a drawing: Shavian-alphabet words, box-drawing boxes and wires. Language: [docs/SPEC.md](docs/SPEC.md). Build plan and orchestration rules: [docs/PLAN.md](docs/PLAN.md).

## Commands

- `cargo test --workspace`: unit tests plus the corpus oracle
- `cargo test -p shawline-corpus`: the corpus only (its `.wires` checks stay red until A1 lands at Gate A)
- `SHAWLINE_BLESS=<stage>[,…] cargo test -p shawline-corpus`: rewrite expectations (lead only)
- `cargo run --bin shawline -- check|diagram <file.shaw>`
- `cargo clippy --workspace --all-targets`

## Rules for agents

- Touch only the crate you were assigned. Contract files named in PLAN.md (`wire.rs`, `ast.rs`, `ir.rs`, `bytecode.rs`) and the whole of `shawline-grid` and `shawline-corpus` are lead-owned.
- Never edit files under `corpus/`. If an expectation looks wrong, stop and report it with a proposed diff.
- Your oracle is your crate's tests and your corpus stage. Corpus failures in other stages are expected while a wave is in flight.
- Don't review or test other agents' crates, add harness or mock crates, or introduce snapshot frameworks.
- New crates.io dependencies go in your own crate's `Cargo.toml` only.

## Code conventions

- Every repertoire character is one cell, so columns are `char` indices. Never use byte or grapheme widths for layout.
- Positions are zero-based `Pos { row, col }` internally and one-based only in user-facing CLI output.
- Dump formats (`Display` impls checked by the corpus) are one fact per line in a stable order. Changing one means re-blessing, which the lead does.
- Comments are sparse and say why. Doc comments on public items.
