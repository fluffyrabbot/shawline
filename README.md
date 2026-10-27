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

**Who it's for.** Programmers who want a language that is also a drawing, and who don't read Shavian yet. You learn the alphabet by using it: every word carries its English reading until you no longer need it.

**Why Shavian.** Each letter is one sound, and many letters come in mirrored pairs. Shawline names opposite operations with mirrored letters: `𐑐` adds and its mirror `𐑚` subtracts. Reading a program teaches the alphabet, and the alphabet tells you which operations are each other's opposites.

**Status.** The whole pipeline works end to end: recognition, syntax, checking, a bytecode VM, a placement-preserving formatter, an SVG renderer, the Shawline Mono font, a language server, a VS Code extension, and Studio, a web editor where programs are built by dragging boxes and wires. Readings are drawn under every word. Studio Phase 2 is next: making room, constants placed by mouse, keyboard editing and one-click repairs ([docs/PLAN.md](docs/PLAN.md)).

Ways in, easiest first:

- **Studio:** needs Rust with the `wasm32-unknown-unknown` target and `wasm-bindgen-cli`. Run `playground/build.sh`, then `python3 -m http.server -d playground/dist`, and open the page; the tour starts on a first visit.
- **CLI:** `cargo run --bin shawline -- check|run|fmt|render <file.shaw>`.
- **VS Code:** `cargo install --path crates/shawline-lsp`, then `npm install && npm run package` in `editors/vscode/`, and install the `.vsix`. See [editors/vscode/README.md](editors/vscode/README.md).

```bash
cargo run --bin shawline -- run corpus/08-factorial.shaw
```

The boxes only line up in a font that draws Shavian at the same width as box drawing. Build Shawline Mono with `cargo run -p shawline-font --example build -- ShawlineMono.ttf`, install it, and use it at line-height 1.0 for `.shaw` files.
