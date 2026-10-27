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
| Teaching the alphabet | An English gloss layer, on by default, drawn beside the program but never part of it (P9) | Every surface should teach Shavian to a newcomer. Because the layer is never source, turning it off costs fluent readers nothing. |

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
| `shawline-vm` | Bytecode VM and assembler | A4 | `src/bytecode.rs` (P1) |
| `shawline-syntax` | Diagram → AST | B1 | `src/ast.rs` (P1) |
| `shawline-fmt` | Formatter and aesthetic lints | B2 | — |
| `shawline-render` | Grid → SVG, the gloss strip | B3 → P9 | `src/gloss.rs`, and the signatures of `render_with`, `glosses` and `row_pitch` in `src/lib.rs` (P9) |
| `shawline-font` | Shawline Mono TTF | B4 | — |
| `shawline-ir` | AST → IR → bytecode, checker | B5 | `src/ir.rs` (P1) |
| `shawline-lsp` | Language server | C2 | — |
| `shawline-cli` | `shawline` binary over `shawline-doc` and `shawline-render` (no library since P8) | P0 ✅ → lead at each gate | — |
| `shawline-doc` | One revision of a sheet, each stage read once (`Doc`) | P8 (lead) | the whole crate |
| `shawline-edit` | Studio's checked edits, repairs and descriptions | P6 → P7 → P8 → P10 | the signatures in `src/lib.rs` |
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
- **Spawning:** a wave launches in a single message: one `Agent` call per brief, `isolation: "worktree"`, background, general-purpose agents. An agent may instead run through the `codex-56-implement` or `grok-45-implement` skill with the same brief. At most 5 agents run at once; more agents would mean more seams than crates.
- **Done means:** the agent's own crate tests are green, clippy is clean for its crate, its own corpus stage passes where it has one, and the work is committed on its branch. It ends with a report of at most 200 words covering what landed, any friction with the contract, and any deviations from the brief.
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

## Phases

The critical path runs **P1 → A1 (wires) → B1 (syntax) → Gate B → C**. The glyph → render/font track and the VM run alongside it, off the critical path.

```
P0 spine ✅ ─► P1 spec+contracts ✅ ─► Wave A ✅ ─────────► Gate A ✅ ─► Wave B ✅ ───────► Gate B ✅ ─► Wave C ✅ ─► Gate C ✅ ─► P5
             (lead)               A1 shape-wires                B1 syntax                   C2 lsp
                                  A2 word                       B2 fmt                      C3 playground
                                  A3 glyph                      B3 render                   C4 vscode
                                  A4 vm                         B4 font                     (lead: cli)
                                                                B5 ir
```

### P0: Spine scaffold (lead) ✅

- [x] Cargo workspace with all 13 crates. Lints forbid `unsafe`. No external dependencies yet.
- [x] `shawline-grid`: the closed repertoire, Grid, Pos/Dir/Rect, byte offsets, Diagnostic.
- [x] `shawline-shape` seed: the stroke connectivity table with ASCII joint resolution, box recognition (titles, ports, dividers, nesting, overlap diagnostics), text runs, and the stable `.diagram` dump.
- [x] `shawline-corpus`: the stage registry, runner, and `SHAWLINE_BLESS`.
- [x] `shawline` CLI: `check` and `diagram`, with caret diagnostics.
- [x] Seed corpus: canonical double, its ASCII twin (identical diagram), the abs switch, and two negative cases.

### P1: Spec freeze and contracts (lead, sequential) ✅

Done by one mind, because this is where conceptual integrity is set.

- [x] SPEC v1: how text meets wires (adjacency across spaces on one row), the source/sink role of every wire end, the touching rule (canonical strokes may run side by side), the output format, diagnostic codes with fixed positions, and `.shawasm`.
- [x] `shawline-grid`: diagnostics carry stable codes (`Diagnostic { at, code, message }`); the corpus checks codes and positions, never wording.
- [x] `shawline-shape/src/wire.rs`: `Wire`, `WireEnd`, `EndKind` and the `.wires` dump (a separate stage, so `.diagram` stays stable). Guard gaps in dividers (`├┄ 1 ┄┤`) are recognized.
- [x] `shawline-syntax/src/ast.rs`: Sheet, Definition, Node (call, inline, constant, switch), Arm, Guard, Literal, and Edge/Endpoint (one edge per wire), with geometry on every item.
- [x] `shawline-ir/src/ir.rs`: Program, Function, and Block in SSA form; Op is Const, Prim, Call or Switch (arms see only their through-ports).
- [x] `shawline-vm/src/bytecode.rs`: register machine `Instr`, `Prim` (the letter, arity and mirror table: the single source for primitives), `Value` with its output `Display`, the source map, and the `.shawasm` grammar.
- [x] Corpus v1, 27 programs: hello, arithmetic, double (and its ASCII twin), abs, inline chains, pairs with a crossing, lists, factorial (and its ASCII twin), list length, 100 000-deep tail recursion, mutual recursion, guards (and text results), fan-out, 3 runtime errors and 9 static errors. Expectations: `.diagram` (blessed from the reviewed seed recognizer), `.wires`, `.out` and `.check` (hand-written), `.fmt.shaw` for the ASCII twins. The `.wires` files come from a scratch reference tracer written independently of A1, then hand-checked on six programs.
- [x] Gate P1: contracts compile; clippy is clean; the corpus is green except `.wires`, which A1 owns.

### Wave A: Leaves (4 agents in parallel) ✅

| Agent | Owns | Delivers | Oracle |
|---|---|---|---|
| **A1** shape-wires | `shawline-shape` (not `wire.rs`) | Wire tracing per the `wire.rs` contract: components, crossings (one wire per axis), tees, end classification; ASCII parity (twins must dump identically); `S010`/`S011`. | Corpus `.wires`, with `.diagram` staying green |
| **A2** word | `shawline-word` | Letter table, classes and `mirror()`; the phonemic ASCII transliteration table, plus an incremental transliterator for the LSP; a license check on the ReadLex lexicon, reported with a recommendation. Primitives are not A2's: they live in `bytecode.rs`. | Unit tests against the SPEC §4 table |
| **A3** glyph | `shawline-glyph` | A skeleton model (kurbo paths on a 600×1200-unit cell, with x-height, ascender and descender for the short, tall and deep classes); all 48 letters, with partners derived by the true rotation or reflection; every stroke character drawn edge-to-edge so lines join across cells; digits, marks, namer dot; `specimen.svg` and a PNG preview for self-inspection. | Self-inspection of the PNG, then the user at Gate A |
| **A4** vm | `shawline-vm` (not `bytecode.rs`) | The dispatch loop, values, heap-allocated frames, tail calls, runtime errors mapped to source positions; `.shawasm` assembler and disassembler; a 100k-deep recursion test. | Crate tests driven by `.shawasm` programs |

**Gate A (lead) ✅:** merge four branches; run workspace tests and clippy; the `.diagram` and `.wires` stages must be fully green. The user reviews `specimen.svg`. The lead copies the transliteration table into SPEC §13 and commits.

- [x] Four branches merged without conflicts. All 54 corpus checks green, 58 crate tests green, clippy clean.
- [x] Contract fixes from the reports. `wire.rs`: tie order for wires starting at a shared crossing. `bytecode.rs`: comma form of `prim` in the example, unused prim slots are 0, pair/list constants, use `shawline_vm::equal` rather than `==` on `Value`. SPEC §14: `S011` scope, `R001` for oversized reads, `A001`.
- [x] SPEC §13: transliteration table, `=` for the namer dot, and transliteration only inside word runs (resolves the clash between `v` and `▼`). ReadLex (MIT) adopted as an optional language-server dictionary.
- [x] The user reviewed `crates/shawline-glyph/specimen.svg` and passed it.
- [x] SPEC §14 gains `Y017` (number literal too large for an Int), closing a gap B1 would otherwise have hit.

### Wave B: Middle (5 agents in parallel) ✅

| Agent | Owns | Delivers | Oracle |
|---|---|---|---|
| **B1** syntax | `shawline-syntax` (not `ast.rs`) | Diagram + words → AST; every `Y` diagnostic at its SPEC §14 position; the `.ast` dump. | One crate test comparing its `Y` codes with the `Y` lines of every corpus `.check`; `.ast` blessed by the lead after reading |
| **B2** fmt | `shawline-fmt` | The placement-preserving formatter over Grid + Diagram (SPEC §11) and the lint table (SPEC §12). | Corpus `.fmt.shaw` |
| **B3** render | `shawline-render` | Grid → SVG from glyph skeletons; light and dark themes; no dependence on installed fonts. | PNG self-inspection; `.svg` corpus for 3 programs |
| **B4** font | `shawline-font` | Shawline Mono TTF built from skeletons (kurbo stroke expansion → `write-fonts`); metrics with no line gap so box drawing joins; read-back validation. | Read-back plus a rasterized test string |
| **B5** ir | `shawline-ir` (not `ir.rs`) | AST → IR (name resolution, arity, switch lowering, cycle detection); `C` and `W` diagnostics; IR → bytecode, including tail-call detection. | Unit tests on hand-built ASTs; corpus `.check` and `.out` come online at the gate |

**Gate B (lead) ✅:** merge; wire `shawline run` and `shawline fmt`; register the corpus stages `ast`, `check`, `out`, `fmt.shaw` and `svg`. A program with an empty `.check` and no `.fmt.shaw` must format to itself. Add the formatter property test; fix integration seams personally. The whole corpus must be green. Commit.

- [x] Five branches merged; only `Cargo.lock` overlapped, and it merged automatically.
- [x] The pipeline lives in the `shawline_cli` library and is shared by the binary and the corpus. The CLI gains `run`, `fmt`, `render [--dark]` and `ast`.
- [x] Corpus stages `ast`, `check`, `out`, `fmt.shaw` (falling back to the program itself when its `.check` is empty) and `svg` (selective: `01-hello`, `04-abs`, `06-pair`) are registered. First integration: every `.out` and `.check` matched with no seam fixes. B1 and B5 met only through `ast.rs`.
- [x] `.ast` dumps blessed after reading `03`, `04`, `06` and `12` against their drawings; `.svg` blessed after viewing the light and dark renders.
- [x] Gate decisions:
  - `Y018` (square box at top level) is reported by syntax and left alone by fmt, with a new negative case `99-square-top`.
  - `L003` is now more than 4 bends.
  - SPEC §11 rule 6 becomes `fmt --fix` (P5).
  - Font arrowheads carry the border through them.
  - SPEC §10 tail calls require "in the same order".
  - SPEC §14 rewords `Y005`/`Y016`, adds positions for `C004`/`Y008`/`Y014`/`Y015`/`Y018`, and runs the checker only on structurally clean sheets.

### Wave C: Surfaces (3 agents in parallel, plus the lead) ✅

| Agent | Delivers |
|---|---|
| **lead** | CLI polish (`run`, `fmt`, `check`, `render`, `asm`), `miette` diagnostics, release build. |
| **C2** lsp | Diagnostics, formatting, semantic tokens, on-type transliteration, hover showing arity and the mirror partner. |
| **C3** playground | A `wasm-bindgen` build of the pipeline; a static page with a grid editor in Shawline Mono (as a webfont), live SVG and run output. |
| **C4** vscode | A VS Code extension: LSP client, bundled font and setup instructions, a transliteration toggle. |

**Gate C (lead) ✅:** merge; smoke-test the playground in the browser pane and the extension in an editor; commit.

- [x] Three branches merged; `Cargo.lock` conflicted once and was regenerated.
- [x] Lead's share: `shawline asm` (compiled bytecode as `.shawasm`) and `shawline fmt --write`. `miette` was dropped: the caret diagnostics already land on the right cell because every character is one cell wide.
- [x] Gate fix: `/v` starts a word with 𐑝 in the language server, matching the playground (SPEC §13). Without it a word beginning with 𐑝 couldn't be typed.
- [x] One check run:
  - 161 Rust tests and clippy (host and `wasm32`) clean.
  - `playground/build.sh` and its 7 Node checks pass.
  - The `.vsix` compiles and packages (377 KB).
- [x] Playground seen in the browser pane: source and drawing side by side, Run gives `08-factorial`'s output in 2 ms, `/vow` gives 𐑝𐑬.
- [ ] The extension inside VS Code: the `code` CLI isn't on this machine, so installing the `.vsix` is the user's step.
- Decided: no custom `semanticTokenColors`. The server uses standard token types (`function`, `parameter`, `number`, `string`, `enumMember`, `comment`), which every theme already colours.

### P5: Aesthetic and correctness gate (once) ✅

- The user reviews the gallery (every corpus program rendered in the playground). Lint thresholds and canonical spacing get tuned from that review.
- One audit pass, with lenses picked via the `agent-fleet` skill (at most two; likely `dijkstra-correctness-auditor` on VM/IR and `data-integrity-architect` on formatter round-trips). The lead triages and batches the fixes.
- A jitter corpus: the canonical programs with random one-cell perturbations must format back to their canonical form.
- `shawline fmt --fix`: repairs ragged boxes, which plain formatting must leave alone (SPEC §11 rule 6).

Progress:
- [x] Jitter corpus (`crates/shawline-corpus/tests/jitter.rs`): 15 canonical programs × 24 seeds. Perturbations are glyph swaps to ASCII (never the `+ . '` joints), trailing spaces, doubled blank lines, text gaps filled with wire (rule 4), and one-cell wire bumps (rule 5, wire cells only). All 360 format back exactly. The test asserts that bumps and fills actually fired, so it can't pass vacuously.
- [x] Incident and fix: the formatter-integrity audit agent built a scratch fuzzer that also *ran* random mutants. A mutant whose base case was never reached recursed without bound, because the VM's heap frames had no limit. It peaked at 46 GB, and macOS froze. Fixes:
  - the VM caps calls at 1 000 000 frames and 16 M live registers, stopping with the new `R007` (SPEC §10, §14);
  - corpus case `83-runaway`;
  - `scripts/capped` and the rule above for every agent.
- [x] VM/IR audit (`jackson-counterexample-auditor`), re-run under the cap.
  - **No miscompile** in about 310 000 programs differentially fuzzed against a reference interpreter.
  - **Ten defects**, triaged into SPEC (`Y019`, a reserved `𐑯𐑦𐑤`, a defined runtime-error order, `R008` heap and `R009` output limits) and fixed by agent V. New cases: `14-nil-box`, `84-heap-runaway` and `static/01`–`02`.
  - **The lead added `I001`:** `check` guarantees a program whenever there are no errors, so the CLI and playground can't panic.
  - **Deviation accepted:** `·𐑥𐑱𐑯`'s own calls are never tail calls.
  - **Lesson:** the lead ran case 84 through a stale prebuilt binary, uncapped, and it reached 5.1 GB. The rule now requires `cargo run` under `scripts/capped`.
- [x] Formatter audit (`data-integrity-architect`), re-run under the cap.
  - All 18 earlier fuzzed candidates were false positives. Across about 400 000 inputs, meaning, determinism and text integrity held.
  - Five findings were fixed by agent F:
    - corners keep their roundedness (SPEC §11 rules 1–2);
    - the phases run to a fixpoint (two idempotence bugs);
    - each blank-line edit is verified separately;
    - the fingerprint gains box style and syntax codes.
  - The lead made `fmt --write` atomic and added `fmt_mutations` (seeded mutants, never run).
- [x] Design for `fmt --fix`, recorded in SPEC §3 (near boxes), §11 rule 6 and §14 (`S012`), with cases in `corpus/fix/`. While designing it, it turned out that one slipped border cell made the whole box unrecognizable and produced 24 cascading diagnostics, none at the cause. Near boxes therefore become an error-recovery feature of the shape stage: the box is still recognized, its slipped cells are recorded, and the user gets one `S012` at the slipped cell. `fmt --fix` then moves recorded slips into place.
- [x] Wave P5 (after audit triage). The formatter keeps a single writer.
  - **F**, one agent owning `shawline-shape` (outside `wire.rs`) and `shawline-fmt`, does three things: near-box recognition (slips recorded on `BoxShape`, excluded from wire tracing, `S012`), `fix` and `fmt --fix`, and the confirmed formatter audit findings.
  - **V** fixes the confirmed VM/IR findings in `shawline-vm`/`shawline-ir`, if there are any.
  - The lead then registers the `fix.shaw` stage, blesses the `fix/` dumps after reading them, and runs the gate.
- [x] `fmt --fix` and P5 gate.
  - Agent F built near boxes (`BoxShape::slips`, `S012`) and `shawline_fmt::fix`. The lead added `shawline fmt --fix` and the `fix.shaw` stage, and reworded `S012` to give a direction instead of a zero-based coordinate.
  - Proof of the recovery: each near box's wires and syntax tree are identical to its intact original.
  - Gate: 195 tests, clippy clean, playground build and Node checks green, all under `scripts/capped`.
- [x] Gallery review: the user passed every corpus program's render (light and dark), the near-box before/after pairs and the font sample. No lint thresholds changed. **P5 closed.**

### P6: Shawline Studio spike (after P5 closes) ✅

The interaction model for a visual editor is designed in the [Shawline Studio doc](https://claude.ai/code/artifact/923d6730-3f54-4722-8a46-ea15abf1dd1f). It calls for a grid-native drag-and-drop editor whose every gesture is one checked edit to the `.shaw` text. Its first step, approved by the user, is a spike that proves the model before any UI is built around it:

- [x] Lead: the `shawline-edit` contract (`move_box`, `route`, `Refusal`; checked-edit semantics on the signatures) and the gate test `crates/shawline-corpus/tests/edit_moves.rs`. The gate is red until the spike lands.
- [x] `crates/shawline-edit`, a new crate for wasm: a best-first router over free cells, costed by bends, crossings and clearance, with a search budget. Plus the move gesture, which moves a box and re-routes only the wire stretches that must change. Every edit is validated with the formatter's recognize-and-fingerprint check.
- [x] Wired into the existing playground: drag a box on the drawing, watch its wires follow.
- [x] Spike gate: every node box in the corpus's valid programs can be moved one cell in each direction, and the result keeps every corpus check, or is refused with a reason.
  - Agent E: of 236 moves, 200 succeed and 36 (15%) are refused with reasons. The most common reason is a box crossing into another switch arm.
  - Router weights, in half-cells: cell 2, bend 6, crossing 60, neighbouring stroke 1, new tee 4. Budget: 40 000 expansions.
  - Gate run: 211 tests, clippy clean on host and wasm, playground build and its 8 Node checks green, all under `scripts/capped`.
  - Lead's own trial in the browser with real mouse drags:
    - dragging `𐑐` in `03-double` three cells right re-routes its wires and keeps the author's tee;
    - undo restores the original;
    - dragging it into the border is refused with "the box would leave its definition" shown beside it.
  - The grid-native model is proven, and Studio Phase 1 can build on it.

### P7: Studio Phase 1, hands on ✅

**Goal.** Whole programs editable mostly by mouse: wire by dragging, a context palette, and live value badges. It builds on P6's router and checked edits, and follows Phase 1 of the [Shawline Studio doc](https://claude.ai/code/artifact/923d6730-3f54-4722-8a46-ea15abf1dd1f).

**In scope:**
- **Wire by dragging.** Press on a source and drag. The router previews the path live and compatible sinks glow. What happens on release depends on where:
  - on a sink: connect;
  - on an existing wire: join it as a fan-out with a tee;
  - on empty canvas: open the context palette.
- **Context palette.** Primitives (with their SPEC §9 meaning and mirror partner) and definitions (with their port names), filtered to callees with a free input. Picking one places the node where the drag ended and wires it in.
- **Delete a node** (the `Delete` key), with its wires.
- **Live value badges.** Every wire shows the value it carried during the first call of its definition, refreshed after each edit.

**Out of scope, for Phase 2 or later:** placing constants by mouse (typing covers them), drawing switches and their arms, re-wiring a switch's merged outputs, inserting rows and columns, extracting definitions, and scrubbing.

**Contracts.** The lead writes these first; they are frozen for the wave.
- **Naming endpoints by cell:**
  - A source is named by its cell: a node's output tee, a definition input's arrowhead, a constant's first cell, or an arm's left through-port.
  - A sink is named by its arrowhead.
  - A switch's merged-output wire isn't editable in Phase 1; any edit to it is refused with that reason.
- **`shawline-edit`.** Every function is a checked edit that returns new canonical text or a `Refusal` (P6 semantics):

| Function | Semantics |
|---|---|
| `connect(src, from, to)` | Adds sink `to` to source `from`'s wire, creating the wire or joining the existing one with a tee. The edge set becomes the old set plus that pair, and nothing else changes. |
| `remove_wire(src, at)` | Removes the whole wire through cell `at`, with every branch. |
| `disconnect(src, to)` | Removes only the branch reaching sink `to`, back to its tee, and turns a tee left with two arms back into a plain stroke. Split from `remove_wire` when the contract was written: one function whose effect depended on which cell was clicked would have made the gates ambiguous. |
| `place_node(src, corner, word, inputs, outputs)` | A canonical node box with the given ports, inside a definition. Refused if it would overlap something or leave the definition. |
| `remove_node(src, corner)` | Removes the box, the wires it sources, and the branches into its inputs. |
| `palette(src, from: Option<Pos>) -> Vec<Candidate>` | `Candidate { word, inputs, outputs, gloss, mirror }`. Primitives first, then definitions, in a fixed order. With `from`, only candidates with at least one input. |

- **Trace:**
  - `shawline_vm::run_traced(&Module, steps) -> Traced` records the outputs of every instruction during the first activation of each function. When it runs out of `steps` it stops cleanly, with `stopped: true` rather than an error code.
  - `shawline_vm::show(&Value, max_chars)` prints a value in the SPEC §9 format, shortened with `…`.
  - `shawline_ir::trace(&Sheet, &Program, steps) -> SheetTrace { wires: Vec<(usize, Value)>, error, stopped }` maps those outputs to `Diagram` wire indices. A definition's parameter wires take the values its first call received.
  - `shawline_cli::trace(src)` is the lead's glue for the CLI and playground.

- [x] Lead: contracts written (`shawline-edit` P7 section, `shawline-vm/src/trace.rs`, `shawline-ir/src/trace.rs`, `shawline_cli::trace`), and the gates in `crates/shawline-corpus/tests/studio.rs`. All three gates are red until the wave lands.

**Construction.** One wave, two agents, disjoint:
- **W** owns `shawline-edit` (behind the contract), the `shawline-wasm` exports and the playground UI: dragging wires, the palette, `Delete`, and badges. W builds the badges last. If T hasn't merged by then, the lead resumes W after the gate merges T, for a short second round.
- **T** owns `run_traced` and `show` in `shawline-vm`, and `trace` in `shawline-ir`.

**Gate tests.** The lead writes these in `shawline-corpus` before the wave, so the agents can't adjust their own oracle:
- **`studio_rewire`.** For each valid corpus program:
  - remove every wire except switch-output wires;
  - reconnect each original (source, sink) pair, sinks in row-major order;
  - a program whose connects all succeed must keep its diagnostic codes, its output and its edge set, and be canonical;
  - at most one program in five may end with a refusal, and every refusal has a reason.
- **`studio_rebuild`.** For each node box in each valid program:
  - `palette` offers the node's word with its arity;
  - `remove_node`, then `place_node` at the original corner, then reconnect the node's original edges;
  - the same requirements apply, with at most a fifth of nodes refused.
- **`studio_trace`.** For each valid program:
  - every wire in `·𐑥𐑱𐑯` has a value;
  - the values reaching `·𐑥𐑱𐑯`'s outputs print exactly the `.out` lines;
  - runtime-error programs report the `.out` error;
  - every valid program traces to completion within 5 s in a debug build (budget `TRACE_STEPS` = 5 000 000). A runtime-error program may instead stop at the budget.
  - Lead decision at merge: T proposed raising the budget to 30M so `84-heap-runaway` (about 25M steps) reaches its error. The budget stays 5M, because a runaway would otherwise freeze the badges for about 4 s per edit.
  - Known effect: recorded values keep their cells alive, so a traced run can hit `R008` slightly sooner than `run`.
- **By hand**, first the lead and then the user, in the playground:
  - wire `03-double` from scratch by dragging;
  - add a node by dropping a wire on empty canvas;
  - delete a node and undo;
  - watch the badges change after each edit.

**Gate (done).**

- **Agent T, tracing:**
  - `run` and `run_traced` share one dispatch loop. Untraced speed is unchanged: 1147→1161 ms on a 30M-iteration loop and 42.9→41.3 ms on 1M-deep recursion, both noise.
  - Instruction outputs map to wires through a private `compile_mapped`.
- **Agent W, editing:** the six edits, plus UI helpers outside the contract (`endpoints`, `preview`, `wire_sources`). `studio_rewire`: 16 programs, 0 refused. `studio_rebuild`: 57 nodes, 0 refused.
- **Gate run:** 240 tests, clippy clean on host and wasm, playground build (536 KB wasm) and its Node checks green, all under `scripts/capped`.
- **Lead's trial,** with synthetic pointer events through the UI's own handlers, because the browser pane wasn't on screen. On `03-double`:
  - delete `main`'s node;
  - drag from `21` to empty space, which opens the palette (primitives with gloss, arity and mirror; `𐑯𐑦𐑤` correctly absent);
  - pick `𐑛𐑳𐑚𐑩𐑤`, then drag its output to `main`'s output;
  - result: no diagnostics, Run prints `42`, badges read `21`/`42`, undo reverts.
- W had already tried the same flow with real mouse drags.
- **Follow-up (Phase 2 polish):** a node placed from the palette lands with its input one row off the source, so its wire jogs (`21 ┘` into `┌─▶`). Placing the node so its first input lines up with the source's row would keep the wire straight.
  - Done after the gate: a drop within two rows of the source snaps level with it first, and falls back to the drop row.
- **First-run tour, after the gate.** The user's own trial found no obvious first move, because every wire in `03-double` is already connected. Changes:
  - A first visit with no link opens `03-double` with a six-step tour (`playground/tour.js`): cut and redraw `·𐑥𐑱𐑯`'s output, delete its node, add one back from the palette, wire it, Run.
  - Each step rings the cells to press. It advances when the sheet shows the step done, not on clicks, so undo and alternative routes still work. The header's Tour button restarts it.
  - Its Node check walks the six steps through the wasm edits.
  - The trial with real mouse input found two UI bugs, both fixed:
    - a drag that no click followed swallowed the next real click;
    - the palette was clipped inside the drawing's scroll box. It is now a fixed overlay that opens above the drop point when there's no room below, and closes on scroll.

**Gate honesty.** The roadmap's Phase 1 gate says "every corpus program can be built with the mouse alone". Rewire plus rebuild covers every wire and every node box. Constants are still typed and switches still drawn by hand, both deferred to Phase 2.

### Review after P7: UX and architecture

Two read-only Codex audits, one on UX (checked against WAI, MDN, Blender and Unreal conventions) and one on architecture, gave 33 findings. The lead merged them into 30 items in six batches, reproducing the worst three in the browser and checking the rest in the code. One claim was dropped: muted text measures 4.74:1 (light) and 4.93:1 (dark), which passes AA.

**Batch 1 landed** (two sessions, disjoint files):
- **Playground** (`playground/` only):
  - every example and the tour keep their own draft, so switching examples no longer destroys edits, and Reset restores the original;
  - dragging a wired arrowhead away disconnects that branch, and Delete on a selected arrowhead calls `disconnect`;
  - the trace status shows complete, stopped at the budget, runtime error or unavailable;
  - a palette drop places nothing unless its wire connects;
  - drags track their pointer, and releasing off the sheet or pressing Escape cancels;
  - refusals persist in a status line;
  - focus rings and reduced motion are respected;
  - runtime errors show one-based;
  - worker, fetch and storage failures are handled;
  - also fixed: Chrome's redo dropped a line from whole-text replacements, and the tour lost `21` once main moved.
- **`shawline-edit`:**
  - targets resolve on the text as given and carry over to the canonical sheet by identity (`canon.rs`). Before this, a leading or doubled blank line made every edit below it miss;
  - one router budget covers a whole edit, and running out is refused as "gave up";
  - a definition can't be moved past the sheet's reachable area.
- **`shawline-lsp`:** positions past a line's end are computed arithmetically.

**Gate.** `studio_rewire` and `studio_rebuild` also start from one jittered copy of each program (`shawline_corpus::jitter`), and must end exactly as the canonical start does. On `099b8ed`'s `shawline-edit`, seed 0 fails both with the bug's own refusals. One seed rather than three keeps the studio tests at about 26 s.
- 250 tests, all under `scripts/capped`.
- Clippy clean on host and wasm.
- The playground build (540 875-byte wasm) and its 28 Node checks are green.
- By hand in the browser: drafts survive a switch, and a real drag disconnects one branch and undoes.

**Batch 2 landed: hardening.**
- **`C007`.** A sheet whose definition needed more than 65,535 registers passed `check` and then panicked in codegen. The checker now bounds every frame (SPEC §14, §15), and the generated test `limits.rs` covers 65,535 and 65,536 inputs.
- **Both suspected quadratic paths were real** (release builds, under `scripts/capped`):
  - near-box recognition on a 256×256 corner grid went from killed at 2 GB to 237 ms and 19 MB; each corner now walks only to the first stroke a border can't hold;
  - the cycle check on a 30,000-node chain went from 1.0 GB to 4 ms; it is now an iterative Tarjan pass.
  
  Both are tested against the old algorithms on thousands of seeded inputs, and diagnostics are unchanged.
- **Gate:** 259 tests, clippy clean on host and wasm, and the playground build (546 865-byte wasm) with 28 Node checks green.
- **Found, not fixed:** box parent and overlap checks are quadratic in the number of boxes (3.9 s at 65,536 boxes), and the CLI's diagnostic printer rescans the source for each diagnostic.

**Next, in order:**
1. **Document spine** ✅. See P8 below.
2. **Gloss layer** ✅. See P9 below.
3. **Onboarding** (batch 6, items 28–30): a tour that tracks undo, README prerequisites, VS Code server status, and CLI `--help`. It runs as its own session and merges before P10's wave 1 agents start, because both touch the playground.
4. **Studio Phase 2** (P10 below), with the other two batches folded in:
   - batch 4 (keyboard and semantic selection, the palette combobox, zoom and pan, the inspector) opens its wave 1;
   - batch 5 (repair in place, richer diagnostics, transliteration parity) closes its wave 3.

### P8: Document spine (before Studio Phase 2) ✅

**Goal.** Each text revision is read once, and every consumer shares that one reading: the CLI, the corpus, the language server, the playground and the edits. Edits return the revision they already checked. They refuse with a kind the UI can act on, and stop re-deriving facts that recognition already has. In wave 2, the three "did this edit change only what it meant to" checks become one structural comparison.

**What exists today** (`8d99495`):
- **One playground refresh reads the same text six times:** `check`, `endpoints`, `wires`, `wire_sources`, `boxes` and `render`.
- **Every preview cell formats the sheet again.** Every edit parses its input twice, given and canonical, and its result twice.
- **`shawline-edit` and `shawline-lsp` depend on `shawline-cli`** for its pipeline functions, which brings render, glyph and kurbo into both.
- **Run output carries no revision.** Edit the text after Run and the output still describes the old sheet, and its error link points into the new text.
- **Editing re-infers which axis of a crossing a wire owns** (`Model::own_mask`), although the tracer already knew. A wire that crosses itself owns both axes, so removing it leaves one axis behind.
- **Three fingerprints encode the same structure separately:** `fmt::verify::Fingerprint`, `edit::check::Print` for moves, and `edit::model::Print` for wiring.

**Decisions.**

| Decision | Choice | Rationale |
|---|---|---|
| Where the reading lives | A new crate `shawline-doc`, lead-owned like the old pipeline glue in `shawline_cli` | Edit, LSP, wasm and the corpus need the reading without the renderer. `shawline-cli` becomes a binary over `shawline-doc` and `shawline-render`. |
| How stages are computed | `Doc` is immutable. Each stage is computed on first use and kept (`std::cell::OnceCell`) | `boxes` needs only the diagram and `check` needs everything, so nothing is computed that isn't asked for. Every consumer is single-threaded. |
| What an edit returns | The `Doc` it checked | Its result is already parsed to verify the edit, so the refresh after an edit reads nothing again. |
| Compound edits | Composition over immutable `Doc`s. `place_node` returns the new node's port cells | A refused step leaves the caller holding the original, which makes compound edits atomic without a transaction mechanism. The returned ports let the next step name the new node without guessing coordinates. |
| Refusal kinds | `Refusal { at, kind, reason }`, where `kind` is one of `Unreadable`, `Target`, `Unsupported`, `Blocked`, `GaveUp` or `Changed` | The UI picks a remedy by kind: Phase 2 offers row and column insertion on `Blocked`. The corpus checks kinds and cells, never wording, as it does for diagnostics. |
| Crossing ownership | `Wire::links`: for each cell, the directions in which it joins this wire. It is not dumped | It falls out of the tracer's existing nodes. `.wires` stays byte-identical, so nothing is re-blessed. |
| Wasm memory | `Doc` is a wasm-bindgen class. The playground caches at most two (the editor's, and a dragged branch's detached view) and calls `free()` when evicting | It is explicit and bounded, and needs no reliance on the GC's finalization timing. |
| Preview off the main thread | No. Measure first | With a cached reading, a preview costs one route search under a fixed budget. If p95 on the largest corpus sheet exceeds 16 ms, a worker comes later. |
| Stale Run output | Dimmed, labelled "from an earlier version", with the error link disabled | The last result stays readable, but nothing points into text it doesn't describe. |
| The oracle for edit behaviour | A blessed `.studio` dump per valid program, captured from `main` before the refactor | The one-in-five refusal allowance can hide a regression; a dump diff can't. |

**Oracle first** (lead, before any refactor):
- **Corpus stage `studio`:** one `.studio` dump per valid program. Each line is one fact, in the gates' order:
  - `rewire SRC -> SINK ok|refused AT`
  - `rebuild CORNER WORD ok|refused AT`
  - `move CORNER N|E|S|W ok|refused AT`
  
  It is blessed from `8d99495` as the baseline. When wave 1 adds kinds, it is re-blessed after reading a diff in which only the `refused` lines gain a kind.
- **Gate `studio_sequences`:** a fixed multi-step sequence per valid program, run on the canonical text and on one jittered copy. It removes a wire, reconnects it, removes a node, places it again, connects it and moves it. Every intermediate result must:
  - be canonical;
  - have exactly the edge set the step declares;
  - keep the diagnostic codes the step allows.
- **Regression test:** a fan-out whose branch crosses another branch of the same wire. `remove_wire` must leave no stroke. It is written first and fails on `8d99495`.

Oracle baseline (done):
- **Shared sequences.** `shawline_corpus::studio` holds the rewire, rebuild and move sequences. The `studio` stage and the Studio gates now walk the same steps.
- **The `.studio` baseline.** It covers all 17 valid programs, 863 lines, blessed from `8d99495`'s edit code.
  - On 03-double, both the rewire and the 𐑐 rebuild end on the original text's hash.
  - It records 37 refusals: 36 moves, the same count P6's spike recorded, and the case below.
- **New corpus program `15-fanout-cross`.** One wire fans out and its branches cross, so the wire owns both axes of `4:6`.
  - Its `.wires` lists the crossing once, in one wire.
  - Its baseline records `remove_wire 2:4 refused 2:4`.
  - The regression test `removing_a_wire_that_crosses_itself_leaves_no_stroke` fails today with "it would change what another wire connects". It is `#[ignore]`d until wave 1.
- **`studio_sequences`.** It checks 117 intermediate states over 17 programs, and 16 sequences run to the end; `15-fanout-cross` stops at its first step. The only codes gained mid-sequence are `Y019` and `Y012`, so those two are the whole allowance. A jittered start must produce the same texts and be refused at the same step.
- **Gate:** 260 tests (one ignored), clippy clean on host and wasm, and the playground build with 28 Node checks green. The playground's example list gains `15-fanout-cross`.

**Wave 1 contracts** (lead):
- **`shawline-doc`.** Contract and implementation, since it is the old glue moved:
  - `Doc::new(text)`, `text()`;
  - `grid() -> Result<&Grid, &[Diagnostic]>`, `diagram()`, `sheet()`;
  - `diagnostics()` (all static diagnostics, sorted: today's `check`), `has_errors()`;
  - `program()`, `module()` (compiled once);
  - `run() -> Option<Result<Vec<Value>, Diagnostic>>`, `trace(steps)`;
  - `formatted()`, `fixed()`, and `canonical() -> Option<&Doc>`, which is itself when the text is already canonical.
  
  The corpus, CLI and LSP move to it, and `shawline-cli`'s library loses its pipeline functions.
- **`Wire::links`** in `wire.rs`, filled by the tracer.
- **`shawline-edit` signatures:**
  - every edit takes `&Doc`;
  - `move_box`, `connect`, `remove_wire`, `disconnect` and `remove_node` return `Result<Doc, Refusal>`;
  - `place_node` returns `Result<Placed { doc, inputs, outputs }, Refusal>`;
  - `palette`, `endpoints`, `preview`, `wire_sources` and `route` read a `&Doc`;
  - `RefusalKind` is added.
  
  The lead lands these with thin adapters over the current internals, so the contracts compile and stay green from the first commit.

Wave 1 contracts (done):
- **`shawline-doc`** holds the old glue, and `shawline-cli` is a binary only, so nothing depends on it. The corpus, the CLI, the language server (one `Doc` per document version) and the wasm crate read through it.
  - `run()` runs again on each call rather than keeping its result, and `trace(steps)` is `None` when there is no program; the errors are in `diagnostics()`.
  - The canonical revision of an untidy text is seeded as its own canonical form, because the formatter is idempotent. A check of canonical form must therefore format a fresh `Doc::new(text)`, and every gate now does.
- **`Wire::links`** is the OR of the tracer's node masks per cell. Two tracer tests cover a crossing's single axis and a self-crossing wire's both axes. Every `.wires` file is unchanged.
- **`shawline-edit`** takes and returns `Doc`s through adapters.
  - `RefusalKind::name()` (`unreadable`, `target`, `unsupported`, `blocked`, `gave-up`, `changed`) is the one spelling for the dump and for JS, so neither matches on variants.
  - Until E sets kinds, `Refusal::new` fills `Changed`.
  - The `Placed` adapter reports the ports of the one box that the input's canonical form lacks.
  - The crate's own tests reach the new API through a text shim in `tests.rs`.
- **Proof the adapters change nothing.** Every `.studio` dump is byte-identical without re-blessing. 267 tests pass, with one ignored, which still fails under `--ignored` with the same refusal. Clippy is clean on host and wasm, and the playground build (554,946-byte wasm) and its 28 Node checks are green.

**Wave 1 construction.** Two agents with disjoint files:
- **E** owns `shawline-edit`:
  - its internals read the `Doc`, and its canonical form is `doc.canonical()`, so nothing parses its input again;
  - every refusal site sets a kind;
  - `own_mask` gives way to `Wire::links`;
  - `Placed` reports the new ports;
  - the adapters are removed.
- **W** owns `shawline-wasm` and `playground/`:
  - the `Doc` class, with the playground's two-entry cache seeded by every edit's returned `Doc`;
  - `rev` on every worker request and result;
  - one trace in flight plus one replaceable pending request, with the kill timer belonging to the job in flight;
  - stale Run output, as decided above;
  - the palette's place-and-connect composed in Rust through `Placed`.
  
  W works against the lead's adapters, so it doesn't wait for E.

**Wave 1 gate:**
- The workspace tests, the `.studio` dumps re-blessed with kinds, and `studio_sequences` pass.
- Clippy is clean on host and wasm.
- The playground build and its Node checks pass, all under `scripts/capped`.
- No crate except `shawline-cli` depends on `shawline-cli`, and its library exports no pipeline function.
- The self-crossing regression test passes.
- **By hand:** time a refresh and a 20-cell preview drag on the largest corpus sheet, before and after, and record it here. Then edit during a long Run and see the output dim.

Wave 1 gate (done). E (`92e202a`) and W (`2fe35c9`) merged without conflicts.
- **E, `shawline-edit`:**
  - edits borrow the `Doc`'s grid, diagram and canonical revision, and each returns the revision it checked, formatted afresh;
  - a kind is set at every refusal site; the reason-to-kind table is in the P8 notes;
  - `Wire::links` replaces `own_mask`, and `Placed` reports the ports it drew;
  - the crate no longer depends on `shawline-fmt`.
  
  Judgment calls accepted:
  - look-alike wires on untidy text are `unreadable`;
  - a corner in no box is `target`, but a node sticking out of its definition is `blocked`;
  - a constant with no blank cell after it is `blocked`;
  - too many ports and a wire looping back into its own box are `unsupported`.
  
  New: a result that formats differently a second time is refused as `changed`; nothing in the corpus reaches it.
- **W, `shawline-wasm` and `playground/`:**
  - a `Doc` class, whose methods replace the text-taking exports; `run` and `trace` still take text, because the workers can't share wasm memory;
  - `docs.js`, the two-entry cache;
  - `flight.js`, which runs worker jobs one at a time, each with its `rev` and its own kill timer;
  - stale Run output;
  - `place_and_connect`, composed in Rust through `Placed`.
- **Re-blessed `.studio`:** a refusal reads `refused KIND AT`. The diff was read before keeping it:
  - the 36 refused moves gain `blocked`, with their cells unchanged;
  - `15-fanout-cross`'s rewire now runs to its end. It ends on the same fan-out drawn without the self-crossing, a different text but the same program.
  
  The regression test is no longer ignored and passes.
- **Gate:**
  - 271 tests;
  - clippy clean on host and wasm;
  - the playground build (554,475-byte wasm) and 37 Node checks green.
  
  `cargo metadata` shows no crate depends on `shawline-cli`, which has no library.
- **Timing,** median / p95 in ms, release build. Both builds were loaded into one page and timed interleaved. A refresh reads diagnostics, endpoints, wires, boxes and the drawing; a preview drag is 20 cells along a blank row.

  | Sheet | Refresh `59952d2` | Refresh, new text | Refresh, after an edit | Preview `59952d2` | Preview |
  |---|---|---|---|---|---|
  | `11-even-odd` | 3.1 / 3.9 | 2.3 / 2.5 | 2.0 / 2.2 | 3.0 / 3.3 | 0.3 / 0.5 |
  | `09-length` | 2.3 / 2.4 | 1.8 / 1.9 | 1.5 / 1.7 | 2.2 / 2.6 | 0.3 / 0.6 |

  The browser pane was hidden, so a background page's timers could drift; one early run read 7 ms for a refresh. The same calls in Node agree with the table:
  - `11-even-odd`: refresh 3.5 → 2.5 (2.2 after an edit), preview 3.7 → 0.34;
  - `09-length`: refresh 2.6 → 1.9 (1.7 after an edit), preview 2.8 → 0.32.
  
  The preview's p95 stays under 1 ms, so no worker is needed.
- **Stale Run, by hand:** Run on `84-heap-runaway`, with an edit 0.3 s in. The result arrived dimmed (opacity 0.45) and labelled "1757 ms · from an earlier version", and its `R008` link was a disabled button. The console showed no errors.

**Wave 2: one structural comparison.**
- **Lead contract:** `shawline-shape/src/structure.rs`, a `Structure::of(&Grid, &Diagram)` that ignores where things sit. It records:
  - boxes: style, titled, slips, parent, ports with what each meets, guarded dividers;
  - texts: kind, content, owner, title and divider flags;
  - wires: as sorted term lists;
  - shape codes;
  - plus `Mapping`, which relabels box and text indices.
- **How each check uses it:** a formatting step expects equality. A move or a row/column insertion also expects equality after relabelling, because positions are not part of the structure. A wiring edit expects the old structure plus the graph change it declares.
- **F** moves `fmt::verify` to it; **E** moves `edit::check` and `edit::model`'s `Print` to it. Both old fingerprint types are deleted.
- **Gate:** every existing gate stays green, including jitter, `fmt_mutations`, the formatter property test, `studio`, `edit_moves` and `studio_sequences`. `grep` finds no second fingerprint type.

Wave 2 contract (done): `shawline-shape/src/structure.rs`, lead-owned, with 12 unit tests.
- **What it records:**
  - **boxes:** style, title, slips, parent, ports (side, kind, direction, what an arrowhead meets) and guarded dividers;
  - **texts:** kind, content, owner, and the boxes they title or guard;
  - **wires:** sorted term lists, each term a facing and what it meets (a port by box and port index, or a free arrowhead, a loose end or a blocked stroke, with what lies across);
  - **shape codes,** without `S012` and `S010`: each box counts its own slips, and each wire names its own blocked ends.
- **`Mapping` relabels box and text indices.** `Mapping::by_place` builds one from where a step should have put each box and text. Relabelling drops what maps to nothing:
  - a wire loses its terms at a dropped box;
  - a wire left with fewer than two terms goes, as when a node is removed;
  - a reference across a dropped text becomes `Nothing`.
- **One comparison, `expect(found, routing)`,** reports the first part that differs (boxes, texts, wires or codes). Wiring edits declare their graph change with `connect`, `remove_wire` and `disconnect`. A wire that no term names alone, such as one of two look-alike stray strokes, is read with `Structure::wire` and declared removed with `remove_part`.
- **Deviations from the design above**, each decided while writing it:
  - **Wires keep tee and crossing counts, and `Routing` says how far a step may change them.** `Kept` is for formatting and insertion. `Rerouted` lets crossings change, because today's move check ignores them and a re-routed stretch may cross a wire the old one didn't. `Ignored` is for wiring edits. Without this, moving the move check onto the structure would refuse moves that pass today.
  - **Texts name the box they title or guard,** rather than carrying flags. The formatter's net already compared these by index, so flags would weaken it.
  - **Syntax codes stay outside the structure,** because `shawline-shape` sits upstream of syntax. The formatter compares them beside it, as before.

Wave 2 gate (done). F (`2c23150`) and E (`d1d09ad`) merged without conflicts.
- **F, `shawline-fmt`:**
  - the safety net is `Structure` at `Routing::Kept`, with the syntax codes compared beside it as a sorted list;
  - a repair expects the structure with that box's slips at 0.
- **E, `shawline-edit`:**
  - a move relabels the input's structure by place and expects the result at `Rerouted`;
  - each wiring edit declares its change and expects the result at `Ignored`, its old diagram mapped by unchanged places, so a stray move still fails;
  - `place_node` inserts the expected box and word at their row-major indices.
  
  Each `Part` keeps the reason its old check gave, so no `.studio` line changed.
- **Contract changes at the gate,** both proposed by E with diffs:
  - **`S010` left out of the codes.** The tracer emits one per blocked end, and each is already an `End::Blocked` term, so the formatter's net is exactly as strict. Without the change, removing a stroke that runs into another was refused as a changed diagnostic. The new crate test `a_stroke_that_runs_into_another_can_be_removed` fails with `S010` put back.
  - **`Structure::wire` and `remove_part`.** These replace `Model::part`, a private copy of how the structure reads a wire. Its consistency test moved into `structure.rs`, which now also checks a look-alike pair and blocked ends.
- **Gate:**
  - 284 tests;
  - clippy clean on host and wasm;
  - the playground build (533,837-byte wasm, down from 554,475 now the duplicate checks are gone) and its 37 Node checks green;
  - no corpus file re-blessed;
  - `fmt` and `fmt --fix` give byte-identical output to `7c71b4e` on all 37 corpus programs;
  - no `Fingerprint`, `Print`, `BoxPrint`, `TextPrint`, `WirePrint`, `EndPrint` or `Target` type is left in any crate.
- **P8 closed.**

**Then Phase 2**, starting with inserting rows and columns. With wave 2 in place, insertion is a mapping that leaves structure equal, and needs no new check.

### P9: Gloss layer (after P8, before Studio Phase 2) ✅

**Goal.** Every surface teaches the Shavian alphabet to a newcomer. English sits beside the drawing, on by default, and one toggle turns it off. The rule that keeps it minimal: **English goes in the margin, never in the grid.** A gloss is never source text, never takes up a column, and never changes a program's meaning. SPEC §2's closed repertoire and one-cell rule hold exactly as before.

**What exists today:**
- **Primitive meanings are written out three times.** There is `gloss()` in `shawline-edit/src/wiring.rs` for the palette, `meaning()` in `shawline-lsp/src/analysis.rs` for hover, and SPEC §9's table.
- **Transliteration goes one way.** `shawline-word` turns keys into letters. Nothing turns a word back into its keys.
- **Nothing outside the palette shows any English** for a word on the sheet. Hover shows arity and the mirror partner, but not how a word is read.
- **The renderer draws only the repertoire.** Every glyph is a path from `shawline-glyph`, and the SVG needs no fonts.

**Two tiers, both exact.**
1. **The reading** of a word is the keys you would type to produce it. `·𐑥𐑱𐑯` reads `=mayn`, `𐑣𐑧𐑤𐑴` reads `heloh`, and `𐑑𐑣` reads `t/h`. Shavian is phonemic, so for most words the reading already tells you the word. It also teaches typing.
2. **The gloss** of a primitive is its English name: 𐑐 `add`, 𐑚 `sub`. Each pair of duals is named as a pair, so the strip teaches the mirror principle (SPEC §1.2) wherever a primitive appears.

**Decisions.**

| Decision | Choice | Rationale |
|---|---|---|
| Where English appears | Only in a view layer: the SVG strip, hover, the palette, and diagnostic messages. It never appears in `.shaw` text or any dump except `.gloss` | The source of truth stays Shavian. With the layer off, every output is byte-identical to today's. |
| What a reading is | `shawline_word::reading(word)`, the inverse of `transliterate`. It inserts `/` only where the longest match would read otherwise (`t/h`, `a/y`, a leading `/v`), and writes the namer dot as `=` | There is one form to learn, and it is exactly what you type. A round-trip test makes it exact: `transliterate(reading(w)) == w`. |
| Meanings for user-defined words | None. User words show their reading only, and ReadLex is not bundled | Chosen by the user. A phonemic reading already conveys the word. A lexicon would add wasm size and homophone ambiguity, and a learning aid that is sometimes wrong teaches distrust. |
| Primitive names | One table in the contract: `Prim::gloss()`, at most 6 lowercase letters, and `Prim::meaning()`, SPEC §9's phrase. Both private copies are deleted | The short names are what the strip shows and what the palette lists. The phrase goes in hover. |
| The short names | add/sub, mul/div, pair/unpair, cons/uncons, less/more, same/differ, min/max, show/read | Each mirror pair reads as a pair. At most 6 letters at half a cell each fits the 3-cell interior of a node box. |
| What the strip shows per word | A primitive's node word shows its gloss. Every other Shavian word shows its reading, including names, ports, words inside `«»` text, and comments. Numbers show nothing | A primitive's letter is a sign rather than a word. Its key (`p`) teaches typing but says nothing about what the node does. Hover shows both. |
| Layout | With glosses on, a strip half a cell high opens under every row. Glosses sit in the strip, centred under their word, in Latin glyphs half a cell wide, drawn in the comment ink. Vertical strokes continue through the strip | The grid opens to make room for English and closes again when the layer is off. Nothing is ever drawn over a stroke. |
| A gloss that doesn't fit | A gloss is shown only if it lies entirely within its strip's free run (bounded by vertical strokes) and is at least one gloss character away from any gloss already placed in row-major order. Otherwise it is dropped from the strip, and hover still has it | Glosses are never truncated, shifted or drawn over a stroke. Each gate records how many were dropped, and placement is revisited if many are. |
| Gloss glyphs | Lowercase `a`–`z`, `@`, `=` and `/` as procedural skeletons in `shawline-glyph`. They are not added to Shawline Mono | One geometric source keeps the stroke weight consistent, and the SVG still needs no fonts. Editors never draw a gloss, so the font doesn't need these glyphs. |
| Renderer API | `render(grid, theme)` is unchanged and plain. New: `render_with(grid, Options { theme, gloss })`, `glosses(grid) -> Vec<Gloss>`, and `row_pitch(gloss) -> f64` | The corpus `svg` stage and its blessed files are unaffected. The playground computes hit-testing from the pitch rather than from a hard-coded `CELL_H`. |
| Diagnostics | `shawline_word::annotate(message)` appends `(reading)` after each Shavian run, as in `𐑥𐑱𐑯 (mayn)`. It is applied where messages are printed (the CLI, the LSP, wasm `check`), not where they are written | No message site changes, and the corpus checks codes rather than wording. |
| Editors | Hover gains a `Reading:` line for every word. There are no inlay hints and no CodeLens | Inlay hints shift columns and would break the drawing. The Studio is where the alphabet is taught. |
| The default and the toggle | On everywhere. The playground has a "Readings" checkbox, kept in `store`. The CLI has `--plain` on `check`, `run` and `render`. The LSP has the setting `shawline.gloss`, which defaults to true | This is what the thesis asks for: guardrails by default, one switch to turn them off. |
| Peek | In the playground, holding Alt by itself flips the layer until Alt is released. Pressing any other key cancels the peek | Readers learn to read without glosses by checking only when unsure. Only a lone Alt counts, so Option-typing on macOS doesn't flash the layer. |
| The `.gloss` dump | A new corpus stage with one line per Shavian word in row-major order: `ROW:COL WORD READING [GLOSS] shown\|dropped` | Readings, glosses and placement are checked facts, in house dump style. The lead blesses it after reading every line. |
| Order relative to Phase 2 | Before | Every Phase 2 overlay is written against the pitch-aware coordinates from the start. |

**Contracts** (lead):
- `Prim::gloss()` and `Prim::meaning()` in `shawline-vm/src/bytecode.rs`. The lead switches the palette and hover call sites to them and deletes both private copies. Palette and hover output stays the same except for the new short names.
- Signatures, with thin first implementations: `shawline_word::reading` and `annotate`, and `shawline_render::{Options, Gloss, render_with, glosses, row_pitch}`.
- Corpus stage `gloss` (`glosses` behind the `Display` impl of `Gloss`), and gate `gloss_clear`: for each valid program, no shown gloss overlaps a stroke, its strip's bounds, or another gloss.
- SPEC §13 gains a "Reading" paragraph, and §9's primitive table gains the short-name column.
- The CLI's `--plain` flag, its annotated diagnostics, and its glossed `render`.

Contracts (done):
- **`Prim::gloss()` and `Prim::meaning()`** are in `bytecode.rs`, and the palette's and hover's private copies are gone. Hover reads exactly as before. The palette shows the short names, and one Node check that pinned "subtract" now reads `sub`. A new `shawline-vm` test keeps SPEC §9's table, which gains the short-name column, in step with both methods.
- **Signatures, each with a thin first body:**
  - `shawline_word::{reading, annotate}` in `src/reading.rs`. `reading` joins each letter's key and never inserts `/`; `annotate` returns its input.
  - `shawline_glyph::{gloss_skeleton, GLOSS_WIDTH, GLOSS_HEIGHT, GLOSS_CHARS}` in `src/gloss.rs`, where every character is drawn as an x-height box.
  - `shawline_render::{Options, Gloss, render_with, glosses, row_pitch}`. `Options` and `Gloss` (with its `Display`) are in `src/gloss.rs`.
    - `render(grid, theme)` is now `render_with(grid, Options { theme, gloss: false })`.
    - With `gloss`, rows sit `row_pitch(true)` = 1800 glyph units apart, so W works against the real pitch from the start. Vertical strokes still break at the strip and no gloss is drawn yet.
    - `glosses` finds the words, readings and short names, and drops every gloss.
- **Corpus:**
  - the selective stage `gloss`, which has no files until the gate;
  - the gate `gloss_clear`, with a bite test on `03-double` that breaks each rule once;
  - the `.gloss` row in `corpus/README.md`.
- **SPEC:** §13 gains "Reading", and §9 gains the short-name column.
- **CLI:**
  - `render` is glossed by default;
  - every printed diagnostic and runtime error message goes through `annotate`;
  - `--plain` turns both off.
- **Decisions taken while writing the contracts:**

  | Decision | Choice | Rationale |
  |---|---|---|
  | Coverage of the short names | PLAN's 16 names cover the 16 `Prim` variants exactly. `𐑯𐑦𐑤` gets none | It is a literal (`Literal::Nil`), not a `Prim`, and its reading `nil` already names it. The palette keeps "empty list". |
  | What counts as a word | A maximal run of Shavian letters, namer dots and digits that isn't all digits, found by scanning rows | This is SPEC §4's word, read inside `«»` and comments as well, with numbers left out as decided. |
  | Which word is a primitive's | A boxed or inline call whose name is one primitive letter, read from the syntax tree | A port named `𐑯` is not the primitive `𐑯` (SPEC §4), and only the parser knows inline nodes. `shawline-render` now depends on `shawline-syntax`, `shawline-vm` and `shawline-word`. |
  | Units of `Gloss::span` | Columns from the grid's left edge, as `(left, right)` | This is how the gate checks placement. The dump says only `shown` or `dropped`, so tuning the scale changes no line unless a gloss drops. |
  | Blocked column | A box's side running through the strip, or the cell above connecting south or the cell below connecting north | A border runs through stacked port arrowheads, which connect only westward in the stroke table, so the cells alone would miss it (`03-double`'s 𐑐 node). |
  | What `gloss_clear` checks | Shown glosses are centred on their word, lie within `0..width`, cover no blocked column, and keep one advance apart in each strip. The advance is read from each span | It reads strokes from the diagram rather than the renderer, and it still holds if the gloss scale is tuned. |
  | Strip under the last row | Yes. A glossed drawing is `rows × pitch` tall plus its margins | Every row gets a strip, so the pitch is uniform and hit-testing is a single division. |
  | `--plain` scope | Accepted by every command | Every command can print a diagnostic. PLAN named the three where English shows most. |

- **Proof the contracts change nothing:**
  - 288 tests pass (284, plus two in `shawline-vm` and two in `gloss_clear`), and no corpus file was re-blessed;
  - `shawline render --plain` is byte-identical to `38862bf`'s `render` on all 37 corpus programs, the three blessed `.svg` files included;
  - clippy is clean on host and wasm;
  - the playground build (535,273-byte wasm) and its 37 Node checks are green.

**Construction.** Four agents, with disjoint files:
- **K** owns `shawline-word`: `reading` and `annotate`, plus the round-trip test over every pair of letters and a finite set of seeded random words (run under `scripts/capped`).
- **G** owns `shawline-glyph`: the gloss skeletons, at half the width and height of the Shavian letters, and a specimen sheet.
- **R** owns `shawline-render`: the strip, gloss placement, `row_pitch`, and drawing glosses through G's skeletons. It works against the contract's stub skeletons, so it doesn't wait for G.
- **W** owns `shawline-wasm` and `playground/`: the Readings toggle, peek, pitch-aware coordinates for every overlay and hit test, annotated messages, and a tour step that introduces the layer.
- The LSP changes (the hover reading line, `annotate`, and `shawline.gloss`) are small enough to go to whichever agent finishes first, as its own crate assignment.

**Gate:**
- The workspace tests pass. Every existing `.svg` is byte-identical without re-blessing. `.gloss` is blessed. `gloss_clear` and the round-trip test pass.
- Clippy is clean on host and wasm. The playground build and its Node checks pass, plus new checks that toggling the layer and peeking leave the editor text unchanged. Everything runs under `scripts/capped`.
- The number of dropped glosses across the corpus is recorded here.
- **By hand:** screenshots of `01-hello` and the largest corpus sheet, glossed and plain, in light and dark. The layer passes if it reads as annotation and never as clutter. If it doesn't, the strip height and gloss scale are tuned here.

Gate (done). K (`673695c`), G (`01140bd`), R (`460211e`) and W (`5b96f85`) merged without conflicts, each branch touching only its own crate.
- **K, `shawline-word`:**
  - `reading` puts a `/` only where the longest match from one of the two keys before would run past that key's end, as late as possible (`𐑨𐑦𐑮` reads `ai/r`);
  - `annotate` follows each run of Shavian letters and namer dots with its reading;
  - the tests cover every word of up to three letters (120,099), 5,000 seeded random words, and the minimality of every `/`.
  
  Accepted deviation: a run of namer dots alone gets no reading, since `Y001`'s message uses the dot as punctuation ("one ·-word").
- **G, `shawline-glyph`:**
  - a monoline geometric lowercase for `a`–`z`, `@`, `=` and `/`, with bowls drawn as ellipses;
  - `d`, `p`, `q` and `u` are derived by transform;
  - the `@` ring's right side doubles as the inner `a`'s stem;
  - the specimen gains the gloss alphabet and a sample strip, and the tracked `specimen.svg` is regenerated.
- **R, `shawline-render`:**
  - a line bridges a strip only where the plain drawing joins strokes across that row boundary in a blocked column, so box sides and wires stay one chained path;
  - placement follows `Gloss::span`, counting covered columns the way `gloss_clear` does;
  - the gloss box is centred in the strip, 93 units clear of both rows' ink;
  - `STRIP` and `GLOSS_SCALE` are named constants.
- **W, `shawline-wasm` and `playground/`:**
  - `layer.js` holds the switch, the peek and each pitch's cell geometry, and `CELL_H` is gone;
  - wasm passes messages through as written and exports `annotate`, `reading` and `row_pitch`, plus `Doc.render(dark, gloss)` and `Doc.glosses`. The page annotates only while the layer shows, so a flip never needs a second reading or a second run;
  - the tour opens with a reading step.
  
  Accepted beyond the brief:
  - the peek waits 250 ms of lone Alt before it flips, so Option-typing never flashes the layer;
  - the palette shows readings and searches by them;
  - wire cells carry a `down` flag, so a selected wire's highlight runs on through the strips.
- **Lead diffs:**
  - `reading.rs`'s contract header loses its "thin first versions" note, as K proposed;
  - `gloss_clear` fails when a valid program has no `.gloss`. Moving one aside makes it fail.
- **`.gloss` blessed** for the 17 valid programs after reading all 176 lines. Every reading round-trips, for example `@/rithm@tik` and `faktori/@l`. Every primitive node carries its short name, and port names such as `15-fanout-cross`'s `𐑚` carry only their reading. **Dropped glosses across the corpus: 0.**
- **Gate:**
  - 309 tests;
  - clippy clean on host and wasm;
  - the playground build (548,447-byte wasm, up from 535,273) and 45 Node checks green, the 8 new ones covering toggle and peek leaving the text unchanged, hit-testing at each pitch, and plain messages with the layer off;
  - every `.svg` byte-identical without re-blessing, and `render --plain` byte-identical to `38862bf` on all 37 corpus programs.
- **By hand:** `01-hello` and the first definition of `11-even-odd`, glossed and plain, light and dark, rasterized with `resvg`. Strokes run unbroken through every strip. The user judged the layer to read as annotation, not clutter, so the strip (half a cell) and the gloss scale (half a letter) stay as they are.
- **The LSP piece** went to K, the first agent done, as its own crate assignment.

LSP gate (done). K's `17ec32c` merged without conflicts, touching only `shawline-lsp` and `editors/vscode`.
- **Hover:** every Shavian word gets a `Reading:` line, read off the grid, so port names, words in `«»` and comment words count, as do unknown names and `𐑯𐑦𐑤`, which had no hover before. A primitive's line adds its short name (``Reading: `p` · add``). Call words and titles keep their description above it.
- **Diagnostics:** published messages go through `annotate`.
- **`shawline.gloss`:**
  - a boolean, default true, declared in the extension's `package.json` and sent as `initializationOptions`;
  - a change is pushed as `workspace/didChangeConfiguration`, and the server then republishes every open document's diagnostics;
  - with it off, hover is exactly as before P9 and messages are plain.
  
  There are no inlay hints and no CodeLens.
- **Gate:**
  - 312 tests, the three new ones covering hover on every kind of word, the setting off from the start, and the setting changed while running;
  - clippy clean on host and wasm;
  - the extension's `tsc` build green;
  - the playground build (548,447-byte wasm, unchanged) and its 45 Node checks green.
- **P9 closed.**

### Review after P9: UX and the backend behind it

Five read-only audits (interaction mechanics, product legibility, conceptual integrity, information hiding, error handling) looked at Studio, the CLI and the language server, and at how `shawline-edit`, `shawline-doc` and the wasm layer shape them. The lead traced each finding used here in the code and reproduced the fixed ones in the browser.

**Landed** (`playground/` and `shawline-wasm`):
- `boxes()` names switches, so pressing a switch moves the switch, not its definition;
- a click on blank sheet only deselects, and focus stays on the drawing. Before, it moved focus into the text, where Delete then edited the source;
- dropping a dragged arrowhead on blank sheet says the branch was removed, and that Undo puts it back;
- a sheet whose drawing never finished is set aside on the next load, which opens the original and offers the draft back or as a download. Before, a sheet that froze the page froze every reload;
- uncaught errors and wasm panics show in the status line, with the panic's message (`panic_message`);
- a leading byte-order mark no longer shifts row 0's columns, and a paste of some 100,000 lines no longer overflows `Math.max`'s arguments.
- **Gate:** the playground's 47 Node checks, clippy on host and wasm, and each fix by hand in the browser.

**Folded into P10:** refusal remedies, edits on untidy text, box kinds, compound gestures as `Edit` variants, wires in `Edited.map`, whole previews, geometry and positions, one severity and one description, Make room's offer, the scrub's preview and badges, the keyboard model's Tab, Enter and key repeat, and the repair `Followup`.

**Found, not fixed:**
- the CLI prints `run` errors zero-based (SPEC §9) and `check` one-based;
- the language server has no panic containment or logging, and ignores `$/cancelRequest`;
- touch: no `touch-action`, and the palette opens only by double-click;
- the drawing has no role, name or text outline for screen readers;
- a drag doesn't scroll the pane at its edges;
- the product's reader isn't named on any surface, nothing says why the alphabet is Shavian (SPEC §1.2 has the reason), and the place a wire starts is called a tee, a source, a notch and an output tee. A glossary of the words P10 brings to labels (through-port, merged output, selector, arm) is due before wave 2.

### P10: Studio Phase 2, forgiving

**Goal.** Studio forgives. When there's no room, the editor can make room. Every corpus program can be built from an empty sheet with the mouse or the keyboard alone. Every diagnostic with a known fix offers that fix in one click.
- This makes P7's gate honest: constants and switches are still typed today.
- It meets the Studio doc's Phase 2 gate: "every fix one click".
- Review batches 4 and 5 are folded in. Batch 6 runs on its own first.

**What exists today** (`4cb7e36`):
- **Edits:** `move_box`, `connect`, `remove_wire`, `disconnect`, `place_node` and `remove_node`. Each takes a `Doc` and returns either the `Doc` it checked or a `Refusal` with a kind. The corpus's `Step` (`shawline-corpus/src/studio.rs`) is the only list of edits as data.
- **No room is a dead end.** All 36 refusals in the `.studio` dumps are moves refused as `blocked`.
  - `RefusalKind::Blocked`'s doc comment says that inserting rows or columns may help, but nothing can insert them.
  - `structure.rs` already describes insertion: a relabelling through `Mapping::by_place` that keeps `Routing::Kept`.
- **Two kinds of wire are refused as unsupported:** a switch's merged outputs ("can't be wired by mouse yet"), and any wire touching an inline node.
- **Constants and switches are typed.** A drag can start from an unwired arrowhead, but releasing it on blank canvas is refused ("Drop on a source or a wire"). Switches appear in 7 of the 17 valid programs and inline nodes in two (`05-inline` and `13-fanout`), so P7's mouse-only gate covered only wires and node boxes.
- **Keyboard and view:**
  - the drawing handles only undo, Delete and Escape;
  - the palette filters by meaning, letter or reading, Enter takes the first match, and arrow keys do nothing;
  - there's no zoom, no pan and no inspector.
- **Selection:** `app.js` ties the selection to the text it was made on, so any edit drops it.
- **Repair:** `Doc::fixed()` (P5's `fmt --fix`) exists, but wasm doesn't export it and the language server has no code actions. Hover text is built inside `shawline-lsp/src/analysis.rs`, so Studio can't show it.

**Decisions.**

| Decision | Choice | Rationale |
|---|---|---|
| Scope | PLAN's six, in three waves: rows and columns, constants, switch outputs and inline nodes, drawing switches and definitions, extracting a definition, and scrubbing. Ghost repairs are added | Chosen by the user. Definitions and inline nodes join the list because building every corpus program from empty needs them. |
| Step-through runs | Phase 3 | Chosen by the user. Nothing in Phase 2 needs them, and they are a VM trace feature with their own UI. |
| Order with the review batches | Batch 4's keyboard and selection model opens wave 1. Batch 5's repairs close wave 3. Batch 6 runs first in its own session | Chosen by the user. Extraction needs multiple selection, and Studio principle 8 asks for a keyboard path for every new gesture, so the model comes first. Batch 6 edits `playground/tour.js`, which W1 also edits. |
| No room | A refusal whose remedy is `MakeRoom` offers **Make room**. The page computes `make_room` when the refusal arrives, and shows the button by the refused cell only if it succeeds, with the shifted layout drawn as a ghost. One click applies it as one undo step. Inserting and deleting a row or column are also explicit commands | Chosen by the user. Nothing grows unless the user asks (Studio principle 6), and the retry is checked like any other edit. Offering only what will succeed, where the drop failed, keeps the button from being a second refusal (review after P9). |
| Where rows and columns go | A row is inserted or deleted only between a top-level box's top and bottom borders. A column is inserted or deleted only across the rows of the top-level box that holds it, so other definitions stay where they are | SPEC §11 rule 7 allows exactly one blank line between top-level items, so a row between them can't be canonical. Definitions are stacked in the corpus, never side by side, and `place_definition` keeps it so. |
| How a cut stretches | The inserted cell continues any stroke that joins across the cut (`─`, `┄` or `│`), and is blank elsewhere. A cut never splits text. Where it would fall in the gap between a text and the stroke it meets, it moves to the stroke's side of the gap. Deleting is the exact inverse, refused where a cell is neither blank nor a plain stretch of stroke | Strokes stretch like a spreadsheet's borders (Studio doc). Spacing stays canonical, so the result needs no tidying. |
| How room is found | `make_room(doc, &Edit)` tries candidates in a fixed order: first growing the parent by exactly what a move overshoots, then 1 to 3 rows or columns at the refusal's cell. It makes at most 12 attempts, each a checked compound edit, and the first that succeeds wins. It re-addresses the `Edit`'s cells through each insertion's `Mapping`. `Roomed { edited, inserted }` publishes only the outcome, `inserted` as `{ axis, index, count }`; the order and the budget stay private, and neither appears in a reason or a `.studio` line | It is deterministic and bounded, like the router's budget. The list lets the UI show what was added. A published search order would be frozen by the blessed dumps. |
| Edits as data | `shawline_edit::Edit` has one variant per edit, and `apply(doc, &Edit)`. Its `Display` is the `.studio` step syntax. It replaces the corpus's `Step`. Compound gestures are variants too, applied in Rust: `Rewire` (a branch off one source and onto another), `PlaceConnected` (today's wasm `place_and_connect`, with the palette's corner offset, row snap and retries that `app.js` does now) and `MakeRoom`. The page sends one `Edit` per gesture | Make room and repairs both need an edit they can hold and replay. One enum names edits the same way in the dumps, the gates and the UI. Today gestures are composed in wasm, in JS and (planned) in `make_room`, each with its own atomicity and refusal priority (review after P9). |
| Selection across edits | Every edit returns `Edited { doc, map, tidied }`. `map` is P8's `Mapping` from the old diagram's boxes, texts and wires to the new one's. Wasm exposes it as queries (`map_box`, `map_text`, `map_wire`, `map_cell`), never as the table. `Placed` gains `map` too. A query `branch(doc, sink) -> Vec<Pos>` gives the cells `disconnect` would clear | The check already builds this mapping, and `Structure` keys wires by their terms. The page maps its selection through it instead of re-deriving geometry in JS, the duplicated editing logic the Studio doc rejected. Without wires, branch selection would stay the text diff in `selectBranch`. |
| Text edits | `set_text(doc, at, content)`, `place_text(doc, at, content)` and `remove_text(doc, at)`. A constant grows or shrinks at its left end, so its wire stays put. A name, title, guard or comment grows or shrinks at its right end. A text keeps its role: a constant stays a literal, a name a word, and a title a `·`-word | One edit covers constants, scrubbing, renaming ports and titles, guards, and comments. Growing away from the wire leaves the wire untouched. |
| Placing text | `place_text` refuses a text that would meet a stroke across spaces, so a new constant is stray (`Y012`) until a `connect` wires it. `remove_text` removes a stray text or a comment, or a constant together with its wire, as `remove_node` does for a node. It refuses names, titles and guards | Wiring stays one edit, `connect`, whatever the source is. |
| Constants by mouse | Release a drag from an unwired arrowhead on blank canvas and a palette of sources opens. It lists Number, Text and `𐑯𐑦𐑤` first, then callees with an output. The pick is placed and wired as one edit. The palette opened on blank canvas without a drag also offers those three | This is Unreal Blueprints' convention in both directions: Phase 1 drags from sources, this drags from sinks. |
| Scrubbing | Click a number constant to select it, then drag its digits sideways. A selected constant shows a sideways-arrow cursor and a scrub outline, so the mode is visible. Each 8 px is one step, Shift makes a step 10, and the value stays between 0 and the largest Int. The inspector's number field scrubs the same way. Each step edits a preview revision held like a branch drag's `drag.view`, and the textarea is written once, on release, as one undo step. The trace follows the latest preview immediately, without the 250 ms debounce, and badges keep their last values, dimmed, until the new ones arrive. If a step is refused for lack of room for another digit, the value stops at the last one that fit. Selection survives the commit through `map` | Chosen by the user. An unselected constant still starts a wire, so the two drags never clash. Today every edit bumps the revision and hides all badges until a trace lands, so a scrub would never show them (review after P9). |
| Keyboard | A cell cursor, `[` and `]` to select, arrow keys to nudge, Enter to edit, `W` to wire, `/` for the palette, and a `?` overlay listing every key (details below). Tab leaves the drawing. Clicking a source and then a sink also connects with the mouse | Studio principle 8 and review item 21, with the Studio doc's "arrow keys nudge the selection". Tab kept for the page, so the drawing is no keyboard trap (WCAG 2.1.2); Enter and `W` split, since a constant is both text and a source. |
| Palette | A combobox: arrow keys move through the list, Enter picks the highlighted entry, and search matches meaning, letters or reading (P9) | Review item 22. |
| Zoom and pan | Ctrl or Cmd with the scroll wheel, or a pinch, zooms about the pointer, from 50% to 400%. A Fit command fits the sheet. The default zoom never shrinks a cell below 16 px, so wide sheets scroll | Review item 23. Hit-testing is still a division, now by zoom times pitch (P9's `layer.js`). |
| Inspector | A panel describing the selection in the same words as editor hover. It has a field for the selection's text (constant, name, title or guard), and each field commits one `set_text` | Review item 24. Hover and Studio share one description, `shawline_edit::describe`, moved there from `shawline-lsp`. |
| Inline nodes | `place_inline` splices a 1→1 call into a straight stretch of wire, and removing the node joins the wire up again (details below) | An inline node exists only on a wire, so it is placed onto one. |
| Switch outputs | A switch's merged output is a source, named by its first arm's right through-port at that ordinal. Connecting from it routes each arm's port at that ordinal into one merge, and from there to the sink. The merge goes when its last sink does | One cell names the whole merged output, as P7 names every other source. |
| Drawing boxes | One edit per Box-tool gesture: an empty box first, then ports and arms added at border cells (details below) | The build gate can then replay any corpus program's geometry exactly, and `C002`'s repair is `set_ports`. |
| Extracting a definition | `extract` moves a pick into a new definition below the sheet and leaves a call node in its place (details below) | It is the only automatic layout in Studio (Studio doc). It lays out nothing the author drew, since the pick keeps its own composition. |
| Inline, the inverse | Not in Phase 2 | Chosen by the user. It would have to lay a whole body out inside the caller, where there is rarely room, so it waits for Tidy and `--relayout`. |
| Repairs | `repairs(doc) -> Vec<Repair { at, code, label, edit, then }>`, where `edit` is an `Option<Edit>` and `then` a `Followup`: `None`, `NameField(at)` or `DrawWire(at)`. A repair with a follow-up is labelled "Fix…", since it isn't one click. Each diagnostic row has a Fix button; hovering or focusing it, after 150 ms, draws the repaired drawing as a ghost overlay, and pressing it applies. A click on the row still jumps to the cell. The language server offers the repairs that have an `edit` as quick fixes and ignores `then`. The code-to-repair table stays inside `shawline-edit`. The table below is the contract | Studio principle 5 and the Studio doc's Phase 2 gate. One table serves Studio and editors, since the Studio doc already has the language server calling the same crate. `Name` and `Wire` as edit actions would publish UI gestures in a core type that the language server can't apply. |
| Richer messages | Review item 26: which input is wrong, expected against actual counts, and a cycle's path. Wording only, in wave 3 | The corpus checks codes and positions, so no expectation changes. |
| Refusal remedies | `Refusal` gains `remedy: Option<Remedy>`, where `Remedy` is `MakeRoom` or `Format`. `Blocked` splits by remedy: `NoRoom` (inserting room helps) against `Edge` and `Spacing` (it can't). The page acts on `remedy`, never on a kind's name or a reason's words. Every reason names only gestures that exist, and the reasons that state an internal rule ("it would change the sheet's diagnostics", "formatting would undo this move") say what to try instead | Today `kind` reaches the page and is never read, and reasons offer "make room" before it exists. Keeping the inference in Rust keeps one mind on what helps (review after P9). |
| Edits on untidy text | An edit on text that isn't canonical still succeeds, on the canonical sheet. `Edited.tidied` says so, and the page applies the formatted text first as its own undo step, with the status "Formatted the sheet to edit it", then the edit. `move_box`'s doc comment says "nothing else moves" holds on a canonical sheet | P7's jitter gates require editing untidy text. Today the first gesture on an ASCII or sketched sheet rewrites every character in one undo step, unannounced (review after P9). |
| Box kinds | `shawline_edit::BoxKind` (`Definition`, `Switch`, `Node`) becomes public with `box_kind(doc, corner)`. Wasm's `boxes()` reports it and classifies nothing itself | Wasm copied `Model::kind` and left switches out until the review after P9, so pressing a switch dragged its definition. |
| Previews | `preview` returns `Preview { path, joins_wire }` with the whole polyline `connect` would draw, ends included. A drag anchored on an unwired sink is a separate variant, not a second meaning of the same call | `app.js` stitches the sink and the source tail back on today, using what it knows about fan-out tees. |
| Geometry and positions | `shawline-render` exports a `Geometry` per drawing: `cell_at(x, y)`, `cell_rect(rect)`, `cell_centre(at)` and `size`, at a given zoom. `row_pitch` and `CELL_W` leave JS. The offset-to-cell conversion in `shawline-lsp/src/position.rs` moves into `shawline-doc`, with the encoding as a parameter; wasm exposes `cell_of_offset` and `offset_of_cell` in UTF-16, and the page's `offsetOf` and `posOf` go | Zoom divides by zoom times pitch, so geometry must have one owner. The language server converts positions correctly (BOM, surrogates); the page had its own copy, which miscounted a leading BOM until the review after P9. |
| One severity, one description | `Diagnostic::severity() -> Severity` (`Error`, `Warning`, `Lint`) in `shawline-grid` is used by the CLI, the language server (`Lint` as `Hint`), wasm and the page. `describe` also gives the palette's gloss and the drawing's hover title | Severity is read from a code's first letter in five places that disagree, and one word has three descriptions today. |
| Not in Phase 2 | Step-through runs (Phase 3), Inline, free typing on the canvas (the text pane does it), dragging a wire's segments or bends, the knife, copying as text, and Tidy | Each is outside the chosen scope, and none blocks a gate. |

**Gesture details.**
- **Keyboard:**
  - the drawing keeps a cell cursor while it has focus;
  - `]` and `[` select the next and previous object in row-major order, and Tab and Shift+Tab leave the drawing;
  - arrow keys nudge a selected node (`move_box`) and otherwise move the cursor. A held key moves by its repeats but commits one undo step when released, and a refused nudge shows the refusal by the box, as a drag does;
  - Enter edits a selected text in place. `W` starts a connection from a selected source or sink: `]` and `[` then pick among the glowing ends, and Enter commits;
  - `/` opens the palette at the cursor, and Escape clears the selection, then the cursor;
  - every other gesture has a key, listed in a `?` overlay;
  - a live region reads out the description of the selection.
- **Inline nodes:**
  - `place_inline(doc, at, word)` splices a 1→1 call into the straight stretch of wire through `at`;
  - `remove_node` on its word splices it out and joins the wire up again;
  - its output is a source named by its word's first cell, and its input arrowhead is a sink that always has its wire;
  - double-clicking a straight stretch of wire opens the palette of 1→1 callees.
- **Drawing boxes:**
  - `place_definition(doc, rect, name)` and `place_switch(doc, rect, guard)` draw an empty box. A definition must share no rows with another top-level item;
  - `add_port(doc, at, name)` adds a port at a border cell: a definition's named input or output, a node's port, or a switch's selector or through-port;
  - `add_arm(doc, at, guard)` draws a divider;
  - `set_ports(doc, corner, inputs, outputs)` resizes a node and keeps its wires by port index. Renaming a node resizes it the same way;
  - `remove_port`, `remove_arm` and `remove_box` undo these.
- **Extracting a definition,** `extract(doc, picks, name)`:
  - the pick is node boxes, inline nodes and constants, all in one scope. A switch can only be picked whole;
  - the pick moves into a new definition, placed one blank line below the sheet, and keeps its relative placement;
  - each wire into the pick becomes an input, one per source. Each wire out of it becomes an output, one per source inside. Both are ordered row-major by their first cell;
  - a port takes the name of the definition input it comes from, and otherwise 𐑨, 𐑚, 𐑒, … in order;
  - a call node takes the pick's top-left corner, and its wires are re-routed the way a move re-routes them.

**Repairs** (wave 3's contract; any code not listed has no known fix in Phase 2 and gets a richer message instead):

| Code | Repair | Edit or follow-up |
|---|---|---|
| `S012` | Move the slipped cell home | `Edit::Fix` (`Doc::fixed`) |
| `Y003` | Connect to the nearest free sink in scope, or remove the wire; the label says which | `Connect` or `RemoveWire` |
| `Y004` | Remove the dangling wire | `RemoveWire` |
| `Y009`, `Y013`, `C004` | Name it (the inspector field opens) | `NameField` |
| `Y012` | Remove the stray text; a literal can be wired instead | `RemoveText`, or `DrawWire` |
| `Y019` | Wire it: a connection starts from the arrowhead | `DrawWire` |
| `Y001` | Add the namer dot | `SetText` |
| `Y014`, `Y015`, `Y018` | Square or round its corners | `Restyle` |
| `C001` | "Did you mean …?" with the known name nearest by edit distance, offered only at distance 1 and only when that name is unique. Shavian words are 2 to 4 letters, so distance 2 matches nearly anything | `SetText` |
| `C002` | Match the callee's ports | `SetPorts` |
| `C005` | Add `·𐑥𐑱𐑯` below the sheet | `PlaceDefinition` |
| `C006` | Remove main's inputs | `RemovePort` |
| `W001` | Add a default arm | `AddArm` |
| `W002` | Remove the unused input | `RemovePort` |

**Oracle first** (lead, before wave 1):
- **`Edit` and `apply`** move into `shawline-edit`'s contract, and the corpus's `Step` is removed. Every `.studio` file must stay byte-identical without re-blessing.
- **`Edited { doc, map }`,** with adapters building the maps by place: unchanged places for wiring edits, and the shifted box for a move. `studio_sequences` checks that every successful step's map sends each kept box and text where the step says.
- **New `.studio` sections.** The stubs refuse as `unsupported`, so the first bless records that. Each gate re-blesses after reading the diff.
  - `room`: each blocked move, retried with `make_room`;
  - `insert`: for each definition, a row before its bottom border, a column at its right border, and a row and a column through its middle, each one deleted again;
  - `retext`: each number constant gets +1, then ×10, then is set back.
- **Gate `studio_room`:**
  - inserting and then deleting, or retexting and then setting back, returns the original text exactly;
  - every success is canonical, keeps the codes and the output, and matches the original structure relabelled by the shift at `Routing::Kept` (for retext, with that one text's content changed);
  - a `make_room` success leaves the moved box where the move asked, relative to its parent.
- **Gate `studio_build`, with a `build` section in `.studio`.** Each valid program is rebuilt from an empty sheet, in this order:
  - top-level comments;
  - each definition: `place_definition` at its rectangle, then `add_port` for each port;
  - inside each definition, boxes in row-major order: `place_node` at a node's corner, or `place_switch` at a switch's rectangle followed by `add_port` for its selector and through-ports and `add_arm` for each divider;
  - constants and comments inside boxes, with `place_text`;
  - every connection, sinks in row-major order;
  - inline nodes last. Each inline node's wire is first connected straight through, then the node is placed on it.
  
  Every intermediate state must be canonical. It may gain only codes from an allowance list, measured on the first run as `WHILE_UNWIRED` was. The final sheet must have the original's codes, output and wiring. The gate stays red until wave 2.

**Wave 1: room, text and the keyboard model.**
- **Contracts (lead):**
  - `Edit` and `Edited`, as above;
  - stubs for `insert`, `delete`, `make_room` (returning `Roomed { edited, inserted }`), `set_text`, `place_text` and `remove_text`;
  - `describe`, moved verbatim from `shawline-lsp`'s hover. The language server calls it, and its hover output stays byte-identical;
  - `preview` returns `Preview`, with a variant for a drag anchored on an unwired sink;
  - `Remedy` and the split of `Blocked`; `Edited.tidied`; `BoxKind` and `box_kind`; `branch`; the `Rewire` and `PlaceConnected` variants;
  - `Diagnostic::severity()` in `shawline-grid`; the position conversion moved into `shawline-doc`; `Geometry` in `shawline-render`.
- **E1** owns `shawline-edit`: the six new edits, exact maps for every edit (wires included), previews from a sink, `branch`, `Rewire`, `PlaceConnected`, and refusal reasons rewritten to name a next step.
- **W1** owns `shawline-wasm` and `playground/`.
  - It builds these first, against today's edits: the keyboard model, selection carried through maps, the palette combobox, zoom, pan and Fit through `Geometry`, the inspector, the `?` overlay, and a live verdict on box drags (a trial `move_box` per cell change, the ghost tinted and the reason in the status line while it would be refused). It deletes the page's own copies: `offsetOf` and `posOf`, the branch text diff, the preview stitching, the palette's retry, and severity by first letter.
  - It builds these last: Make room, constants by mouse, scrubbing, and the insert and delete commands. They get a short second round once E1 merges, as W's badges did after T in P7.
- **Gate:**
  - the workspace tests pass;
  - `.studio` is re-blessed after reading the diff. Only the `room`, `insert` and `retext` lines may change, from `refused unsupported` to their outcomes;
  - `studio_room` passes;
  - clippy is clean on host and wasm;
  - the playground build and its Node checks pass, with new checks for:
    - a keyboard-only rewire of `03-double`;
    - a palette pick made with the arrow keys;
    - hit-testing at three zooms and both pitches;
    - a scrub committing one undo step;
    - a selection surviving a move, an insertion and a retext;
    - a branch selection surviving a move;
    - a first edit on `03-double-ascii` leaving two undo steps, the format and the edit;
    - Tab leaving the drawing from any selection;
  - **by hand:** Make room on a blocked move in the browser, and scrubbing `03-double`'s `21` with the badges following;
  - **timing:** the p95 of a scrub step (edit and refresh) on `11-even-odd` is recorded here. Over 16 ms, scrubbing moves to a worker.

**Wave 2: the whole language by mouse.**
- **Contracts (lead):**
  - `place_definition`, `place_switch`, `add_port`, `remove_port`, `add_arm`, `remove_arm`, `remove_box`, `set_ports` and `place_inline`;
  - `set_text` on node words;
  - the P7 contract's two unsupported kinds of wire are lifted, with the semantics above;
  - `studio_build`'s allowance list.
- **E2** owns `shawline-edit`: those edits, plus merged outputs and the wires around inline nodes.
- **W2** owns `shawline-wasm` and `playground/`:
  - the Box tool: drag a rectangle at top level for a definition, or inside one for a switch;
  - clicking a border adds a port, and its name field opens;
  - the `+ arm` handle on a switch's lower edge;
  - port-count steppers in the inspector;
  - placing an inline node by double-click;
  - renaming a node.
- **Gate:** `studio_build` passes on all 17 valid programs, and its `.studio` lines are blessed after reading. **By hand:** build `04-abs` from an empty sheet with the mouse.

**Wave 3: extract and repair.**
- **Contracts (lead):**
  - `extract`, returning `Extracted { edited, definition, call }`;
  - `restyle(doc, corner, rounded)`;
  - `repairs`, with `Repair`, `Followup` and the table above;
  - the corpus stage `repair`: one line per offered repair, in the form `ROW:COL CODE EDIT ok DIGEST` or `… refused KIND AT`;
  - new negative cases under `corpus/static/` for the table's codes the corpus lacks: `Y001`, `Y004`, `Y009`, `Y012`, `Y014`, `Y015`, `C004`, `C006`, `W001` and `W002`;
  - an `extract` section in `.studio`, extracting every node box on its own.
- **E3** owns `shawline-edit`: `extract`, `restyle` and `repairs`.
- **W3** owns `shawline-wasm` and `playground/`:
  - multiple selection, by Shift-click and marquee, and Make definition;
  - ghost repairs: a Fix button per diagnostic, previewed on hover or focus, applied on press;
  - the faded drawing labelled "last valid drawing" (review item 27).
- **L** owns `shawline-lsp` and `editors/vscode`: code actions from `repairs`, and review item 27's VS Code part (uppercase keys, and a convert action for pasted phonemic text).
- **I** owns the messages of `shawline-ir` and `shawline-syntax`: review item 26.
- **Gate (Phase 2's gate):**
  - every code in the table offers its repair on its corpus case, in Studio and as an LSP quick fix. Applying the repair clears that diagnostic without adding an error;
  - every single-node extraction in `.studio` keeps the program's codes and output, or is refused with a kind;
  - every earlier gate stays green;
  - **by hand, the lead and then the user:**
    - build `04-abs` from an empty sheet with the mouse;
    - build `03-double` with the keyboard alone;
    - fix one case of each repairable code in one click;
    - extract a definition from `11-even-odd`.

**Then Phase 3**, the toy layer, with step-through runs.

## Risks

| Risk | Mitigation |
|---|---|
| Editor line spacing leaves gaps in box drawing | The font has no line gap, with ascender + descender equal to the cell height; the docs recommend line-height 1.0. The SVG renderer is unaffected. |
| Terminals and editors fall back to other fonts for Shavian, breaking the grid | The closed repertoire means Shawline Mono covers everything; setup docs say to set it for `.shaw` files. |
| ASCII joints (`+ . '`) join an unrelated neighbouring stroke | SPEC §3: keep ASCII joints one cell from strokes they shouldn't meet. Canonical glyphs can't mis-join, and the formatter converts sketches to canonical form. |
| Jitter straightening in the formatter is the hardest algorithm | B2 builds character normalization, spacing and snapping first, and straightening last, behind the P5 jitter corpus. |
| `write-fonts` API churn | Pin the version. Fallback: emit a UFO and build it with `fontmake`. |
| Glosses crowd the drawing | Readings in glyphs half a cell wide are usually narrower than their word. A gloss that doesn't fit is dropped rather than squeezed in (P9), and the layer turns off with one switch. |
| No suitably licensed English → Shavian lexicon | v1 transliteration needs no dictionary (SPEC §13). |
| Studio Phase 2 is the largest phase yet | Three waves, each with its oracle written first and its own gate. `studio_build` says exactly what "done" means. |
| Making room moves the author's work | Only on request, as one undo step, and only by whole rows and columns, which keep every relative placement. |
| Tail-call detection through switch arms | Defined in SPEC §10; B5 tests it directly and A4 tests constant-stack execution from `.shawasm`. |
