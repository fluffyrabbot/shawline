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
| `shawline-shape` | Stroke recognition | P0 seed ✅ → A1 | `src/wire.rs` (P1) |
| `shawline-word` | Letter classes, mirrors, primitive table, transliteration | A2 | — |
| `shawline-glyph` | Stroke skeletons, SVG paths, specimen sheet | A3 | — |
| `shawline-vm` | Bytecode VM and assembler | A4 | `src/bytecode.rs` (P1) |
| `shawline-syntax` | Diagram → AST | B1 | `src/ast.rs` (P1) |
| `shawline-fmt` | Formatter and aesthetic lints | B2 | — |
| `shawline-render` | Grid → SVG | B3 | — |
| `shawline-font` | Shawline Mono TTF | B4 | — |
| `shawline-ir` | AST → IR → bytecode, checker | B5 | `src/ir.rs` (P1) |
| `shawline-lsp` | Language server | C2 | — |
| `shawline-cli` | `shawline` binary | P0 ✅ → lead at each gate | — |
| `shawline-corpus` | The oracle: stage registry and runner | P0 ✅ → lead | the whole crate |

### Entry points

These signatures are fixed so Gate B can wire the pipeline without negotiation. Each owning agent implements its row.

| Crate | Entry points |
|---|---|
| `shawline-word` | `class(char) -> Option<LetterClass>`, `mirror(char) -> Option<char>`, `Transliterator` (incremental: `push(&mut self, key: char) -> Edit`) |
| `shawline-glyph` | `skeleton(char) -> Option<Skeleton>`, `specimen() -> String` (SVG) |
| `shawline-vm` | `run(&Module) -> Result<Vec<Value>, Diagnostic>`, `assemble(&str) -> Result<Module, Diagnostic>`, `disassemble(&Module) -> String` |
| `shawline-syntax` | `parse(&Grid, &Diagram) -> (Sheet, Vec<Diagnostic>)`, and `impl Display for Sheet` (the `.ast` dump: one fact per line) |
| `shawline-ir` | `lower(&Sheet) -> (Option<Program>, Vec<Diagnostic>)`, `compile(&Program) -> Module` |
| `shawline-fmt` | `format(&Grid, &Diagram) -> String`, `fix(&Grid, &Diagram) -> String` (P5: also moves slipped cells), `lint(&Grid, &Diagram) -> Vec<Diagnostic>` |
| `shawline-render` | `render(&Grid, Theme) -> String` (SVG) |
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
1. **Document spine.** See P8 below; it precedes Phase 2.
2. **Keyboard and semantic selection.** A real palette combobox with phonemic search, zoom and pan, and a node inspector.
3. **Repair in place.** `fix` in the playground and as an LSP quick fix, and richer diagnostics.
4. **Onboarding.** A tour that tracks undo, README prerequisites, VS Code server status, and CLI `--help`.
5. **Phase 2 order.** Insert rows and columns, constants by mouse, switch outputs, drawing switches, extracting definitions, then scrubbing.

### P8: Document spine (before Studio Phase 2)

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

**Then Phase 2**, starting with inserting rows and columns. With wave 2 in place, insertion is a mapping that leaves structure equal, and needs no new check.

## Risks

| Risk | Mitigation |
|---|---|
| Editor line spacing leaves gaps in box drawing | The font has no line gap, with ascender + descender equal to the cell height; the docs recommend line-height 1.0. The SVG renderer is unaffected. |
| Terminals and editors fall back to other fonts for Shavian, breaking the grid | The closed repertoire means Shawline Mono covers everything; setup docs say to set it for `.shaw` files. |
| ASCII joints (`+ . '`) join an unrelated neighbouring stroke | SPEC §3: keep ASCII joints one cell from strokes they shouldn't meet. Canonical glyphs can't mis-join, and the formatter converts sketches to canonical form. |
| Jitter straightening in the formatter is the hardest algorithm | B2 builds character normalization, spacing and snapping first, and straightening last, behind the P5 jitter corpus. |
| `write-fonts` API churn | Pin the version. Fallback: emit a UFO and build it with `fontmake`. |
| No suitably licensed English → Shavian lexicon | v1 transliteration needs no dictionary (SPEC §13). |
| Tail-call detection through switch arms | Defined in SPEC §10; B5 tests it directly and A4 tests constant-stack execution from `.shawasm`. |
