# Shawline

A dataflow programming language you draw. Definitions are rounded boxes, calls are square boxes, values flow along wires, and every name is written in the [Shavian alphabet](https://en.wikipedia.org/wiki/Shavian_alphabet).

```
╭─ ·𐑛𐑳𐑚𐑩𐑤 ────────────────────╮
│                             │
│         ┌───┐               │
▶ 𐑯 ──┬───▶ 𐑐 ├───────────────▶ 𐑛
│     └───▶   │               │
│         └───┘               │
╰─────────────────────────────╯
```

The program above doubles a number: it feeds `𐑯` into both inputs of `𐑐` (add). Rough ASCII sketches (`+--+`, `-->`) parse to the same structure, and the formatter redraws them in canonical form.

Status: P0 scaffold. The grid and box recognition work; everything else is planned in [docs/PLAN.md](docs/PLAN.md).

```bash
cargo run --bin shawline -- diagram corpus/seed/02-abs.shaw
```

The boxes only line up in a font that draws Shavian at the same width as box drawing. Until Shawline Mono exists, alignment in your editor may drift.
