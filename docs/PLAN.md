# Shawline implementation plan

How Shawline gets built: what is built in what order, who builds it, and how the work is verified. The language itself is defined in [SPEC.md](SPEC.md).

## Decisions

| Decision | Choice | Rationale |
|---|---|---|
| Implementation language | Rust | `char` is a whole code point (Shavian is outside the BMP); good tooling for the 2D lexer, LSP, WASM playground and font work. |
| Paradigm | Dataflow graph | The drawing carries the meaning: boxes are calls, wires are values. |
| Name | Shawline | Points at the Shavian alphabet. The `shawline-` crate prefix follows from it. |
| Execution | Bytecode VM (register-based) | Chosen up front. Its textual assembler also gives the VM an input that doesn't depend on any upstream crate. |
| Glyphs | Procedural stroke skeletons | One geometric source drives both the SVG renderer and the Shawline Mono font, so stroke weight and proportions always agree. |
| Formatter | Keeps the author's placement | Composition is the authored art; only local style is normalized (SPEC §11). `--relayout` comes later. |
| Source of truth | Grid text | Diffable, greppable and editable anywhere. Rendering is a view of the text, never what gets stored. |
| Character set | Closed repertoire, every character one cell wide | One font can cover everything, so alignment never depends on font fallback. |
| Verification | One golden corpus plus per-crate unit tests | A single oracle, checked once at each gate (see Orchestration). |
| Transliteration | Phonemic ASCII keys, no dictionary | Deterministic and license-free. A dictionary is an optional extra. |
| Reader | Programmers who take Shawline as an art piece or an art substrate, don't read Shavian, and learn it as a hobby | Chosen by the user. Surfaces explain the alphabet and the drawing's grammar, not programming. The pitch leads with mirrored letters naming duals (SPEC §1.2). Readings should teach toward reading unaided rather than stay a permanent translation. How a drawing looks and how it travels count as features. Studio is the front door, and the CLI and VS Code serve readers who stay. A hobbyist who is refused twice without a reason leaves, so every refusal names the rule of the drawing it enforces and a next step. |
| Teaching the alphabet | An English gloss layer, on by default, drawn beside the program but never part of it (P9). Readings are respellings, and the keys that type a word are taught where typing happens (Readings as respellings, below) | Every surface should teach Shavian to a newcomer. Because the layer is never source, turning it off costs fluent readers nothing. A reader learns sounds; only a typist needs `@`, `=` and `/`. |
| Phase order after P11 | Sharing (P12) before the toy (P13) | Chosen by the user (HISTORY.md, consolidation after P11). How a drawing travels is a feature for this reader, and today a link can't carry a reader's own drawing. Sharing needs no new oracle format, and challenges need a design pass first. |

## Architecture

```
.shaw text
  │ shawline-grid      closed repertoire → Grid of cells
  ▼
Grid
  │ shawline-shape     strokes → boxes, ports, dividers, wires, text runs
  ▼
Diagram  ──────────────► shawline-fmt     placement-preserving canonical form + lints
  │ shawline-syntax    + shawline-word (letter classes, primitives)
  ▼
AST (with positions)
  │ shawline-ir        name resolution, arity, switch lowering, checks
  ▼
IR ──► bytecode
  │ shawline-vm        register VM, heap frames, tail calls
  ▼
output

Grid ──► shawline-render (SVG) ◄── shawline-glyph (stroke skeletons) ──► shawline-font (TTF)
```

| Crate | Role | Built in | Contract file (lead-owned, read-only to agents) |
|---|---|---|---|
| `shawline-grid` | Repertoire, Grid, Pos/Dir/Rect, Diagnostic | P0 ✅ | the whole crate |
| `shawline-shape` | Stroke recognition | P0 seed ✅ → A1 | `src/wire.rs` (P1), `src/structure.rs` (P8) |
| `shawline-word` | Letter classes, mirrors, transliteration, readings | A2 → P9 | the signatures in `src/reading.rs` (P9) |
| `shawline-glyph` | Stroke skeletons, SVG paths, specimen sheet, gloss skeletons | A3 → P9 | the signatures in `src/gloss.rs` (P9) |
| `shawline-vm` | Bytecode VM and assembler | A4 | `src/bytecode.rs` (P1), `src/trace.rs` (P7) |
| `shawline-syntax` | Diagram → AST | B1 | `src/ast.rs` (P1) |
| `shawline-fmt` | Formatter and aesthetic lints | B2 | — |
| `shawline-render` | Grid → SVG, the gloss strip, geometry | B3 → P9 → P10 | `src/gloss.rs`, and the signatures of `render_with`, `glosses` and `row_pitch` in `src/lib.rs` (P9), `src/geometry.rs` (P10) |
| `shawline-font` | Shawline Mono TTF | B4 | — |
| `shawline-ir` | AST → IR → bytecode, checker | B5 | `src/ir.rs` (P1), `src/trace.rs` (P7) |
| `shawline-lsp` | Language server | C2 | — |
| `shawline-cli` | `shawline` binary over `shawline-doc` and `shawline-render` (no library since P8) | P0 ✅ → lead at each gate | — |
| `shawline-doc` | One revision of a sheet, each stage read once (`Doc`) | P8 (lead) | the whole crate |
| `shawline-edit` | Studio's checked edits, repairs and descriptions | P6 → P7 → P8 → P10 | the signatures in `src/lib.rs` |
| `shawline-wasm` | Studio's bindings: one `apply` for every edit, maps as queries, geometry | C3 → P10 | — |
| `shawline-share` | Links: a sheet's text to a URL fragment and back (SPEC §17) | P12 | the signatures in `src/lib.rs` (P12) |
| `shawline-corpus` | The oracle: stage registry and runner | P0 ✅ → lead | the whole crate |

### Entry points

These signatures are fixed so Gate B can wire the pipeline without negotiation. Each owning agent implements its row.

| Crate | Entry points |
|---|---|
| `shawline-word` | `class(char) -> Option<LetterClass>`, `mirror(char) -> Option<char>`, `Transliterator` (incremental: `push(&mut self, key: char) -> Edit`); P9: `reading(&str) -> String`, `annotate(&str) -> String` |
| `shawline-glyph` | `skeleton(char) -> Option<Skeleton>`, `specimen() -> String` (SVG); P9: `gloss_skeleton(char) -> Option<Skeleton>` |
| `shawline-vm` | `run(&Module) -> Result<Vec<Value>, Diagnostic>`, `assemble(&str) -> Result<Module, Diagnostic>`, `disassemble(&Module) -> String` |
| `shawline-syntax` | `parse(&Grid, &Diagram) -> (Sheet, Vec<Diagnostic>)`, and `impl Display for Sheet` (the `.ast` dump: one fact per line) |
| `shawline-ir` | `lower(&Sheet) -> (Option<Program>, Vec<Diagnostic>)`, `compile(&Program) -> Module` |
| `shawline-fmt` | `format(&Grid, &Diagram) -> String`, `fix(&Grid, &Diagram) -> String` (P5: also moves slipped cells), `lint(&Grid, &Diagram) -> Vec<Diagnostic>` |
| `shawline-render` | `render(&Grid, Theme) -> String` (SVG); P9: `render_with(&Grid, Options) -> String`, `glosses(&Grid) -> Vec<Gloss>`, `row_pitch(bool) -> f64` |
| `shawline-font` | `build() -> Vec<u8>` (TTF) |

## Orchestration

### Principles

1. **One author builds the spine first.** The grid, the spec, the contract types and the corpus are written sequentially by the lead (the main session). Conceptual integrity comes from one mind, and parallel agents only start once their interfaces are frozen.
2. **Ownership is disjoint.** Each agent owns one crate, works in its own git worktree, and touches no file outside that crate. Merges at a gate are therefore conflict-free by construction.
3. **There is one oracle.** The golden corpus (`corpus/`, run by `shawline-corpus`) plus each crate's own unit tests is the only verification. Expected corpus files are written or blessed by the lead and never edited by agents.
4. **Checking happens at gates, not between agents.** The lead verifies once, at the gate that ends each wave: merge, `cargo test --workspace`, `cargo clippy`, and reading the newly blessed corpus diffs. Nothing in between.
5. **Problems go up, never sideways.** An agent that thinks a contract or a corpus expectation is wrong stops and reports, with a proposed diff. The lead decides. Agents never coordinate with each other.
6. **Humans judge taste.** The user reviews the specimen sheet (Gate A) and the gallery (P5). No agent is asked whether something looks good.

### Deliberately not done

These prevent the cross-verification churn that swarms tend to accumulate:

- No agent reviews, re-tests or "double-checks" another agent's work.
- There are no verifier or reviewer agents inside or between waves.
- Agents build no harness crates, no mocks of sibling crates and no snapshot frameworks. The corpus is the only snapshot mechanism.
- No intermediate-form parsers exist just for tests. Dumps (`.diagram`, `.ast`) are output-only. A stage that needs upstream input either builds it in its own unit tests or waits for the gate. The VM's `.shawasm` assembler is the one exception, and it ships as a product feature.
- No wave is re-run for review. A gate failure is fixed by the lead, or by one targeted fix agent briefed with the failing corpus case, and the oracle is then re-run once.
- The audit fleet runs once per milestone (P5), with at most two lenses, and the lead triages the findings. Fixes are never applied automatically.
- No agent, audit agents included, runs generated or mutated programs outside `scripts/capped` (see Mechanics).

### Verification budget

| Check | Who | When |
|---|---|---|
| Crate unit tests, sized to the crate's branching logic | Owning agent | While working |
| Corpus stage owned by the agent (reads results only) | Owning agent | While working |
| Blessing corpus expectations | Lead | P1, and at each gate after reading the diff |
| `cargo test --workspace`, `cargo clippy --workspace --all-targets` | Lead | At gates only |
| Aesthetic judgment | User | Gate A (specimen sheet), P5 (gallery) |
| Fleet audit, at most two lenses | One agent per lens | P5 only |
| Property test: formatter idempotence and meaning preservation | Lead adds it at Gate B | The only property suite |

### Mechanics

- **Prerequisite:** the repo has a commit, because worktrees branch from `HEAD`. The lead commits at the end of P0 and at every gate.
- **Spawning:** a wave launches in a single message: one `Agent` call per brief, `isolation: "worktree"`, background, general-purpose agents. An agent may instead run through the `codex-call` skill with the same brief. At most 5 agents run at once; more agents would mean more seams than crates.
- **Done means:** the agent's own crate tests are green, clippy is clean for its crate, its own corpus stage passes where it has one, and the work is committed on its branch. It ends with a report of at most 200 words covering what landed, any friction with the contract, and any deviations from the brief.
- **After a gate:** the lead deletes each landed agent's branch and worktree, so no branch outlives the gate that merged it.
- **Contract change mid-wave:** if a change blocks an agent, the lead edits the contract on `main`, commits, and tells the affected agents (`SendMessage`) to rebase. Otherwise the change waits for the gate.
- **Resource caps:** every brief, audit briefs included, says to run generated or mutated programs (fuzzers, property loops, scratch harnesses) under `scripts/capped`, which kills the whole process tree past 2 GB or 120 s, and to keep such loops finite. macOS has no working `ulimit -v`.
- **Dependencies:** an agent may add crates.io dependencies to its own crate only. These are pre-approved: `kurbo` (glyph, font), `write-fonts`/`read-fonts` (font), `resvg`/`tiny-skia` (dev-only previews), `tower-lsp`/`tokio` (lsp), `wasm-bindgen` (playground), `miette` (cli). Anything else is mentioned in the report. Accepted at gates: `skrifa` (font, dev-only), `lsp-server`/`lsp-types` (lsp: maintained, and synchronous, so a typed key's edit always arrives before its transliteration request), `js-sys` (wasm, without its default features).

### Agent brief template

```
You are building <crate> for Shawline, a dataflow language whose source is a
drawing. Read CLAUDE.md, docs/SPEC.md §<sections>, and docs/PLAN.md "Orchestration".

Own: crates/<crate>/ except <contract file>. Touch nothing else.
Contracts (read-only): <files>. If one is wrong, stop and report a proposed diff.
Inputs available now: <what upstream exists>.
Deliver: <bullets>.
Your oracle: `cargo test -p <crate>` and corpus stage `<stage>`
(`cargo test -p shawline-corpus`; failures in other stages are expected, ignore them).
Do not edit corpus expectations, add harness crates, or review other crates.
Done = tests + clippy green for your crate, committed on your branch, ≤200-word report.
```

## Where it stands (2026-11-24)

P0 to P11 are finished, P10 included: it closed on the lead's gate record, and the user's by-hand pass moved to P12's gate. P12.0, the front door, is finished too: licenses, a README that draws its sheet, Studio fitted to the window and published at <https://fluffyrabbot.github.io/shawline/>. Every finished phase, with its decisions and gate record, is in [HISTORY.md](HISTORY.md); "Consolidation after P11" records how this plan's direction was chosen, and the last entry is P12.0.

- **The language** works end to end: recognition, checking, the VM, the formatter, the SVG renderer, Shawline Mono, the language server and the VS Code extension, which also runs in untrusted folders. `𐑗` chop and `𐑡` join are the first mirror pair added since v1.
- **Studio**'s edits rebuild every valid corpus program from an empty sheet (`studio_build`), and the whole language can be drawn by mouse, keyboard or finger. Studio makes room on request, places constants and switches, extracts definitions, and offers a repair for each of the 18 codes that has one.
- **Checks on `22c25e5`:** 424 workspace tests pass, and the four in `tests/links.rs` fail until wave 1's codec lands, as expected. Clippy is clean on host and wasm, and the playground passes its 114 Node checks.
- **Repository:** `main`, level with `origin/main`, and `gh-pages`, which only `scripts/publish-pages` writes. No other branches or worktrees.

```
P0–P11 ✅ ─► P12.0 Front door ✅ ─► P12 Sharing (contracts landed; wave 1 next) ─► P13 the toy layer
```

## Next

### P12: Sharing, how a drawing travels

**Goal:** a reader draws a sheet in Studio, sends it as a link or prints it as a poster, and whoever opens the link sees the same drawing, ready to edit. The parts come from the [Shawline Studio doc](https://claude.ai/code/artifact/923d6730-3f54-4722-8a46-ea15abf1dd1f)'s Phase 3 roadmap: share as a link, posters, and a remix gallery.

**What exists today** (`61e9ff0`):
- **Links name examples.** `#03-double` opens that example's slot (`app.js`: `enter` and the `hashchange` listener). The slots are the 19 corpus examples and the tour, each with a draft in browser storage (`drafts.js`). There is no blank sheet, and no way to open text that isn't an example except by pasting it over one.
- **Studio is hosted** at <https://fluffyrabbot.github.io/shawline/> since P12.0, published by `scripts/publish-pages`.
- **Drawings export, posters don't.** `shawline render` and `Doc.render` make a standalone SVG at 15 × 30 px per cell with a margin, readings optional. It has no title and no paper size.
- **`examples/` isn't in Studio.** `build.sh` copies corpus programs only, so `gray-code.shaw` can't be opened there.
- **Measured links** (raw DEFLATE, then base64url): 248 characters for the median corpus program, 454 for the largest (`09-length`), and 1,048 for `gray-code`. Unencoded, `gray-code`'s text is 31,824 characters percent-encoded.
- **Measured posters** (A4, 15 mm margins, a title band, no readings): a cell is 10.4 mm wide for `01-hello`, 2.4 mm for `11-even-odd` and 1.2 mm for `gray-code`.

**Decisions.**

| Decision | Choice | Rationale |
|---|---|---|
| Hosting | GitHub Pages at `https://fluffyrabbot.github.io/shawline/`, from a `gh-pages` branch that `scripts/publish-pages` writes from a local `playground/build.sh` and pushes. No GitHub Actions. `gh-pages` holds only the published build and is never developed on (project CLAUDE.md) | Chosen by the user. Free and static, and a link lives as long as the repository does. |
| Link format | `v1.` and then the base64url (no padding) of the raw DEFLATE of the sheet's UTF-8 bytes, byte-order mark included, at most 1 MiB decoded. SPEC §17 | Measured above: short enough to paste anywhere. Byte for byte, so the drawing arrives as the author placed it, errors included. The version lets a later format sit beside this one. The limit keeps a hostile link from inflating to gigabytes. |
| One implementation | A new crate, `shawline-share`: `encode(&str) -> String` and `decode(&str) -> Result<String, LinkError>`, with `miniz_oxide` for DEFLATE and base64url written out. Studio reaches it through wasm, and the CLI through `shawline link <file>`, which prints the Pages URL with the fragment | One encoder for every surface, checked by one corpus stage. The browser's `CompressionStream` would be JavaScript only, and asynchronous. A reader who stays in the CLI or VS Code can share too. |
| A link that can't be read | Opens nothing and overwrites nothing. The note names the cause and a next step: a newer version ("This link was made by a newer Studio (v2). Open it at fluffyrabbot.github.io/shawline."), a damaged payload ("This link is damaged: part of it is missing. Ask for the link again, or for the sheet's text."), or one too large | Every refusal names its rule and a next step (Decisions, Reader). |
| Your sheets | A new kind of slot beside the examples. **New sheet** in the picker opens an empty one. An opened link becomes a sheet of its own, named by a hash of its text, so the same link opened again reopens it, edits included. The picker lists them under **Yours**, each labelled by its first definition's name and reading (`·𐑛𐑳𐑚𐑩𐑤 dubəl`), or "Untitled sheet". **Remove sheet**, with a confirmation, takes Reset's place for them. Example slots don't change | Chosen by the user. Sharing your own drawing needs a place for it that isn't an edited example. |
| The link and the address bar | **Share** copies the link and says how long it is. The address bar keeps naming the open slot (`#03-double`, `#yours-…`), so a reload returns to it. Once a `#v1.` link has opened, the address becomes its sheet's slot | Sharing is a deliberate act, and a reload keeps the draft model it has today. An address that tracked every edit would also mean a 1,000-character address bar while drawing. |
| Clipboard refused | The link shows in a selected field, with "Copy it from here" | Clipboard access can be refused, and the link is still the thing being shared. |
| Poster | `shawline_render::poster(&Grid, &Poster) -> String`: an SVG whose width and height are in mm, for A4, A3 or Letter, in whichever orientation gives the larger cell. 15 mm margins. A title band at the top holds each definition's name drawn by the glyph pen with its reading beneath, and a colophon at the foot says "drawn in Shawline". The drawing is scaled to fit, at most 8 mm per cell width, in the given theme, with readings or without | Chosen by the user. The glyph pen keeps the poster font-free and in the renderer's hand. The cap keeps `01-hello` from turning into a billboard. |
| The poster's title | The caller passes the names, `Doc::definition_names()` (lead, `shawline-doc`), in sheet order. The poster draws each with its namer dot and its reading (`shawline_word::reading`) beneath, leaving out `𐑥𐑱𐑯` unless it is the only one | The sheet is parsed once, by `Doc`. `𐑥𐑱𐑯` is the harness on most sheets, not what the sheet is about. |
| PDF | No PDF library. Studio's **Print** opens the poster alone with a matching `@page` size, and the browser's dialog saves a PDF. The CLI writes SVG only | Chosen by the user with the paper page. An SVG sized in mm prints exactly. |
| Too small to read | Below 2 mm per cell, the Poster dialog says the letters will be hard to read and suggests A3. It still exports | `gray-code` comes out at 1.2 mm on A4. |
| Gallery | **Gallery** opens a grid of cards. Each shows its drawing, rendered when the gallery opens in the current theme and Readings setting, its label, and one line on what it does. One click opens it as the picker would. The picker stays | Chosen by the user. |
| Gallery content | `playground/gallery.json`, in teaching order: each valid corpus program by number, then `examples/`, as `{ "sheet": "03-double", "line": "…" }`. The lead writes the lines. `build.sh` copies `examples/*.shaw` beside the corpus programs | The corpus is lead-owned, and a comment row in a corpus sheet would move every position in every stage. The lines are reader-facing writing, which is the lead's. |
| Words | SPEC §16 gains **link**, **poster**, **gallery** and **your sheets** | One word per concept on every surface. |

**The oracle, written first by the lead:**
- SPEC §17, the link format, and the §16 words (landed with this design);
- `shawline-share` with its signatures, `LinkError` and doc comments, and its bodies left `todo!()`;
- `corpus/links/`: fixed links for `01-hello`, `09-length`, `11-even-odd` and `gray-code`, three edge cases (empty, exactly 1 MiB, a byte-order mark with CRLF) and eleven refusals, encoded independently of `shawline-share` (Python's `zlib`). `shawline-corpus/tests/links.rs` checks that each opens as its sheet byte for byte or fails with the error its name gives, and that every corpus program and every `examples/` sheet survives `decode(encode(text))`. A link made today must open in every later version;
- `shawline-render/src/poster.rs` with `Poster`, `Paper`, the three sizes in millimetres, and the signatures of `poster` and `cell_mm`. A `poster.svg` stage (A4, light, with readings), registered now. Its files for `01-hello` and `11-even-odd` are created and blessed at the wave 1 gate, after the user has looked at the posters (Principle 6). Until then no file exists, so the stage never runs an unwritten body;
- `Doc::definition_names()`;
- `playground/gallery.json`, with every line written.

**Contracts landed** ✅ (2026-11-22). `shawline-share` and `poster.rs` hold signatures with `todo!()` bodies, and `Doc::definition_names` is written and tested. Every fixture was checked against a strict reference decoder written from SPEC §17. Until S lands, `tests/links.rs` is the one failing test in the workspace, as expected. S, R and W1 can start.

**Wave 1: the pieces** (three agents at once):
- **S** owns `shawline-share`: the codec, its errors and their messages, and unit tests for damaged, truncated, oversized and unknown-version links. Oracle: `cargo test -p shawline-share` and `cargo test -p shawline-corpus --test links`.
- **R** owns `shawline-render`: `poster`. Unit tests check page sizes, orientation, the cell cap, and that the title band, the drawing and the colophon don't overlap. `resvg` previews of the two corpus posters, light and dark, go into the report for the gate.
- **W1** owns `shawline-wasm` and `playground/`: your sheets (New sheet, the Yours group, labels, Remove sheet, storage in `drafts.js`), the gallery from `gallery.json`, and `examples/` in `build.sh`. Node checks: every gallery entry opens, your sheets survive a reload, and removing one leaves the examples' drafts alone.

**Wave 1 gate** (lead): merge; `shawline link` and `shawline poster [--dark] [--plain] [--paper a4|a3|letter]` in the CLI; the user looks at the two posters before `poster.svg` is blessed; Studio republished with `scripts/publish-pages`.

**Wave 2: Studio shares** (one agent):
- **W2** owns `shawline-wasm` and `playground/`: wasm exports for `encode`, `decode`, `poster` and `definition_names`; Share and its clipboard fallback; opening a `#v1.` link into your sheets, with the three refusals; the Poster dialog (paper, the too-small warning, Download SVG and Print). Node checks: every `corpus/links/` fixture opens as a sheet of yours, and the same link twice gives one sheet.

**Gate** (P12's): workspace tests and clippy on host and wasm, the Node checks, and Studio at 320 px and 1,024 px for every new control. Then the lead publishes to Pages, opens the live Studio in the browser pane, and opens there a link made by the local CLI. Then the user's pass, which is also the first playtest of everything since P10:
1. build `03-double` with the keyboard alone, and it runs 42;
2. repair each of the 18 codes in the repair table, twelve in one click and six through "Fix…";
3. extract a node and its constant from `11-even-odd`'s default arm, and the output stays 0 and 1;
4. draw a definition with the Box tool;
5. in VS Code, point at `{ } Shawline` in the status bar and read the server's state, then apply the quick fix on `unused-input.shaw`'s warning, and the warning goes;
6. draw a sheet of your own, share it as a link, open the link in another browser, and print it as a poster.

Faults found in 1 to 5 are fixed in P12 if they're small, and otherwise go to the backlog with their steps.

### P13: Studio Phase 3, the toy layer

**Goal,** from the Studio doc's roadmap: a first-time player solves three challenges without help.

- **Challenges, Zachtronics style.** A challenge is a sheet with a locked `·𐑥𐑱𐑯` harness, fixed ports, and a table of inputs and expected outputs: reverse a list, sum the evens, test a palindrome. The player builds the definition in between. Solutions are scored on three histograms: cells used, VM steps, and beauty (lint warnings, bends and crossings).
- **The alphabet through its mirrors.** Play starts with `𐑐` (add) alone. Each solved challenge unlocks a mirror pair: `𐑚` subtract, then `𐑑` and `𐑛`, multiply and divide. The principle that shape follows meaning becomes the progression.
- **Step-through runs.** Deferred to this phase by the user during P10. A step control animates evaluation in the order SPEC §10 defines: a token travels along each wire, nodes light as they run, and a switch's untaken arms dim, labelled with why ("guard 1, selector 0"). The VM's trace (`shawline-vm/src/trace.rs`) is where this starts.

**How it starts.** A design pass by the lead in the same shape. It decides the challenge file format and whether challenges live in `corpus/` or a directory of their own. A challenge travels as a P12 link.

## Backlog

Not scheduled. Each was found or set aside at a gate. HISTORY.md has the context.

| Item | Why it waits |
|---|---|
| Tidy and `shawline fmt --relayout`: full automatic layout | A milestone of its own (SPEC §11). The router's tight wires (`│0`, a re-routed wire hugging a moved node) wait for it too: the user chose to keep them, since changing the router rewrites many corpus files. |
| Inline, the inverse of extract | It lays a body out inside the caller, where there is rarely room, so it waits for Tidy. |
| Dragging a wire's segments or bends, the knife, free typing on the canvas, copying as text | Outside Phase 2's chosen scope. The text pane covers free typing. |
| Sound, off by default: each primitive has a soft tone, with mirror pairs a fixed interval apart, so a running program is faintly audible | Moved out of Phase 3 by the user (HISTORY.md, consolidation after P11). It needs step-through's timing (P13). |
| A text outline of the drawing for screen readers | The drawing has a role and a name but no text equivalent of its boxes and wires. Its own design question, after touch. |
| One position search in `shawline-edit` | `make_room`, `place_connected` (which retries rows) and `feed` (which retries wire lengths) each search positions in private. |
| A constant that grows leftwards can end up against a border (`│21`) | Canonical, only tight. |
| "An arm starts at a cell of a switch's left border" after a successful `+ arm` | Seen once at the wave 2 gate and never reproduced. Record the steps if it comes back. |
| CLI messages that name a next step: one `G003` per run of Latin letters, saying to type the word's keys with transliteration, in place of one per letter; `C005` saying a run starts at a definition named `·𐑥𐑱𐑯`; `--version`; `--help` with an example and the dumps (`diagram`, `ast`, `asm`) listed apart | Found by the polish audit (P12.0, HISTORY.md). The CLI serves readers who stay, after the front door. |
| Words that drift from SPEC §16: the extension's `shawline.gloss` setting (§16 says readings), three names for the typing switch across Studio, the extension and its status item, "program" where §16 says sheet | Found by the polish audit (P12.0, HISTORY.md). Renaming a setting needs a migration for anyone who set it. |
| Releases: one version across the workspace (`0.0.1`) and the extension (`0.1.0`), tags, a changelog drawn from HISTORY.md, the extension on the Marketplace and Open VSX with the language server bundled per platform | Found by the polish audit (P12.0, HISTORY.md). Studio is the front door, and a published extension needs per-platform server builds. |
| Room for the drawing at 1,024 × 768 with the tour open: the header and the tour take 270 px, and the run state and inspector line under the drawing another 70 px, leaving the drawing about 200 px | Found at P12.0's gate. Larger windows have room. A one-row header or a tour folded into the drawing pane's head is a layout decision of its own. |
| Versioned file names in `scripts/publish-pages`, so a browser never runs a cached `app.js` beside a newer wasm | Found at P12.0's gate. GitHub Pages lets browsers cache each file for 10 minutes. It matters once a publish changes the wasm's bindings. |
| More primitives | The remaining single letters are reserved, and each pair is assigned as a pair (SPEC §9). |
| An English → Shavian dictionary | An optional extra beside the keys (Decisions). |

## Risks

| Risk | Mitigation |
|---|---|
| Editor line spacing leaves gaps in box drawing | The font has no line gap, with ascender + descender equal to the cell height; the docs recommend line-height 1.0. The SVG renderer is unaffected. |
| Terminals and editors fall back to other fonts for Shavian, breaking the grid | The closed repertoire means Shawline Mono covers everything; setup docs say to set it for `.shaw` files. |
| ASCII joints (`+ . '`) join an unrelated neighbouring stroke | SPEC §3: keep ASCII joints one cell from strokes they shouldn't meet. Canonical glyphs can't mis-join, and the formatter converts sketches to canonical form. |
| A large sheet makes a link too long for some places it's pasted | Measured: 1,048 characters for `gray-code`, the largest sheet there is. Share says each link's length, and pasting the text itself always works. |
| An old link stops opening | The fragment carries its version, a new version is added beside `v1` and never replaces it (SPEC §17), and the `link` stage's fixed links must keep decoding. |
| A published Studio goes stale behind `main` | `scripts/publish-pages` is run at each gate that changes Studio, and the gate's smoke test opens the live site. |
| `write-fonts` API churn | Pin the version. Fallback: emit a UFO and build it with `fontmake`. |
| Glosses crowd the drawing | Readings in glyphs half a cell wide are usually narrower than their word. A gloss that doesn't fit is dropped rather than squeezed in (P9), and the layer turns off with one switch. |
| No suitably licensed English → Shavian lexicon | v1 transliteration needs no dictionary (SPEC §13). |
| A challenge's beauty score feels arbitrary | It counts only what the linter and router already measure (lint warnings, bends, crossings), shown as a histogram beside other solutions rather than as a grade. |
| Making room moves the author's work | Only on request, as one undo step, and only by whole rows and columns, which keep every relative placement. |
| Tail-call detection through switch arms | Defined in SPEC §10; B5 tests it directly and A4 tests constant-stack execution from `.shawasm`. |
