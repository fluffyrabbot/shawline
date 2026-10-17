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
| `shawline-fmt` | `format(&Grid, &Diagram) -> String`, `lint(&Grid, &Diagram) -> Vec<Diagnostic>` |
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

### P5: Aesthetic and correctness gate (once)

- The user reviews the gallery (every corpus program rendered in the playground). Lint thresholds and canonical spacing get tuned from that review.
- One audit pass, with lenses picked via the `agent-fleet` skill (at most two; likely `dijkstra-correctness-auditor` on VM/IR and `data-integrity-architect` on formatter round-trips). The lead triages and batches the fixes.
- A jitter corpus: the canonical programs with random one-cell perturbations must format back to their canonical form.
- `shawline fmt --fix`: repairs ragged boxes, which plain formatting must leave alone (SPEC §11 rule 6).

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
