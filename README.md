# Shawline

A dataflow programming language you draw. Definitions are rounded boxes, calls are square boxes, values flow along wires, and every name is written in the [Shavian alphabet](https://en.wikipedia.org/wiki/Shavian_alphabet).

```
╭─ ·𐑛𐑳𐑚𐑩𐑤 ────────────╮
│                     │
│         ┌───┐       │
▶ 𐑯 ──┬───▶ 𐑐 ├───────▶ 𐑬
│     └───▶   │       │
│         └───┘       │
│                     │
╰─────────────────────╯
```

The definition above doubles a number: it feeds `𐑯` into both inputs of `𐑐` (add). Rough ASCII sketches (`+--+`, `-->`) parse to the same structure, and the formatter redraws them in canonical form.

Status: P1 done. The language is specified ([docs/SPEC.md](docs/SPEC.md)), the contracts between stages are written, and the 27-program corpus is the oracle. Grid and box recognition work; the build plan for everything else is in [docs/PLAN.md](docs/PLAN.md).

```bash
cargo run --bin shawline -- diagram corpus/04-abs.shaw
```

The boxes only line up in a font that draws Shavian at the same width as box drawing. Until Shawline Mono exists, alignment in your editor may drift.
