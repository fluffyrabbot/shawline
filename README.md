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

Status: Gate B done. The whole pipeline works end to end: recognition, syntax, checking, a bytecode VM, a placement-preserving formatter, an SVG renderer, and the Shawline Mono font. Editor support and a web playground are next ([docs/PLAN.md](docs/PLAN.md)).

```bash
cargo run --bin shawline -- run corpus/08-factorial.shaw
```

The boxes only line up in a font that draws Shavian at the same width as box drawing. Build Shawline Mono with `cargo run -p shawline-font --example build -- ShawlineMono.ttf`, install it, and use it at line-height 1.0 for `.shaw` files.
