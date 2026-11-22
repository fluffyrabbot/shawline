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

## Where it stands (2026-11-22)

P0 to P11 are finished, P10 included: it closed on the lead's gate record, and the user's by-hand pass moved to P12's gate. Every finished phase, with its decisions and gate record, is in [HISTORY.md](HISTORY.md); the last entry, "Consolidation after P11", records how this plan's direction was chosen.

- **The language** works end to end: recognition, checking, the VM, the formatter, the SVG renderer, Shawline Mono, the language server and the VS Code extension, which also runs in untrusted folders. `𐑗` chop and `𐑡` join are the first mirror pair added since v1.
- **Studio**'s edits rebuild every valid corpus program from an empty sheet (`studio_build`), and the whole language can be drawn by mouse, keyboard or finger. Studio makes room on request, places constants and switches, extracts definitions, and offers a repair for each of the 18 codes that has one.
- **Checks on `db04614`:** 422 workspace tests pass with none ignored, clippy is clean on host, and the playground passes its 113 Node checks.
- **Repository:** `main` only, level with `origin/main`. No other branches or worktrees.

```
P0–P11 ✅ ─► P12 Sharing ─► P13 the toy layer
```

## Next

### P12: Sharing, how a drawing travels

**Goal:** a reader draws a sheet in Studio, sends it as a link or prints it as a poster, and whoever opens the link sees the same drawing, ready to edit. The parts come from the [Shawline Studio doc](https://claude.ai/code/artifact/923d6730-3f54-4722-8a46-ea15abf1dd1f)'s Phase 3 roadmap, copied here so this plan is the one place that schedules them.

- **Share as a link.** The sheet's text, compressed into the URL fragment. No server is needed, and pasting the text anywhere still works. Today the fragment names a corpus example (`app.js`), and both kinds of link have to keep working.
- **Posters.** Export any sheet as a print-ready SVG or PDF in the renderer's hand, light or dark, titled with its definition names and their readings.
- **A remix gallery.** The valid corpus programs and `examples/`, each with a one-line explanation, open with one click as starting points.

**How it starts.** A design pass by the lead, in P10's shape: a decisions table, then the oracle first, then waves with disjoint crates. The design pass decides the fragment's encoding and version marker (the text round-trips byte for byte, which a Node check can test), where the poster is made (the renderer, so the CLI can make the same poster), and where each gallery entry's explanation lives.

**Gate:** the lead's usual checks, then the user's pass, which is also the first playtest of everything since P10:
1. build `03-double` with the keyboard alone, and it runs 42;
2. repair each of the 18 codes in the repair table, twelve in one click and six through "Fix…";
3. extract a node and its constant from `11-even-odd`'s default arm, and the output stays 0 and 1;
4. draw a definition with the Box tool;
5. in VS Code, point at `{ } Shawline` in the status bar and read the server's state, then apply the quick fix on `unused-input.shaw`'s warning, and the warning goes;
6. share a drawing of your own as a link, open it in a fresh window, and print it as a poster.

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
| More primitives | The remaining single letters are reserved, and each pair is assigned as a pair (SPEC §9). |
| An English → Shavian dictionary | An optional extra beside the keys (Decisions). |

## Risks

| Risk | Mitigation |
|---|---|
| Editor line spacing leaves gaps in box drawing | The font has no line gap, with ascender + descender equal to the cell height; the docs recommend line-height 1.0. The SVG renderer is unaffected. |
| Terminals and editors fall back to other fonts for Shavian, breaking the grid | The closed repertoire means Shawline Mono covers everything; setup docs say to set it for `.shaw` files. |
| ASCII joints (`+ . '`) join an unrelated neighbouring stroke | SPEC §3: keep ASCII joints one cell from strokes they shouldn't meet. Canonical glyphs can't mis-join, and the formatter converts sketches to canonical form. |
| A large sheet makes a link too long for some places it's pasted | The text is compressed, and pasting the text itself always works. P12's design pass measures the corpus's largest sheet. |
| An old share link stops opening after the text format changes | The fragment carries a version marker, and a link Studio can't read still shows its text. |
| `write-fonts` API churn | Pin the version. Fallback: emit a UFO and build it with `fontmake`. |
| Glosses crowd the drawing | Readings in glyphs half a cell wide are usually narrower than their word. A gloss that doesn't fit is dropped rather than squeezed in (P9), and the layer turns off with one switch. |
| No suitably licensed English → Shavian lexicon | v1 transliteration needs no dictionary (SPEC §13). |
| A challenge's beauty score feels arbitrary | It counts only what the linter and router already measure (lint warnings, bends, crossings), shown as a histogram beside other solutions rather than as a grade. |
| Making room moves the author's work | Only on request, as one undo step, and only by whole rows and columns, which keep every relative placement. |
| Tail-call detection through switch arms | Defined in SPEC §10; B5 tests it directly and A4 tests constant-stack execution from `.shawasm`. |
