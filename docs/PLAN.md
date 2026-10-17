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
- **Dependencies:** an agent may add crates.io dependencies to its own crate only. These are pre-approved: `kurbo` (glyph, font), `write-fonts`/`read-fonts` (font), `resvg`/`tiny-skia` (dev-only previews), `tower-lsp`/`tokio` (lsp), `wasm-bindgen` (playground), `miette` (cli). Anything else is mentioned in the report.

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
P0 spine ─► P1 spec+contracts ─► Wave A ───────────► Gate A ─► Wave B ──────────► Gate B ─► Wave C ─► Gate C ─► P5
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

### P1: Spec freeze and contracts (lead, sequential)

Done by one mind, because this is where conceptual integrity is set.

- [ ] Finish SPEC v1: exact rules for wire endpoints next to text (inline nodes, constants, port names), the minimum gap between parallel wires, the `.shawasm` grammar, and the display format for values.
- [ ] `shawline-shape/src/wire.rs`: `Wire { cells, ends }` and `WireEnd { pos, kind }`, plus `wire` lines in the `.diagram` dump. Guard gaps in dividers (`├┄ 1 ┄┤`) recognized in the seed.
- [ ] `shawline-syntax/src/ast.rs`: Sheet, Definition, Node, InlineNode, Constant, Switch/Arm/Guard, Port, Edge, with source positions on every item (the formatter needs geometry).
- [ ] `shawline-ir/src/ir.rs`: Program, Function, Value nodes, Prim/Call/Const/Switch, and stable IDs.
- [ ] `shawline-vm/src/bytecode.rs`: `Module`, `Function`, `Instr` (register machine: `Const`, `Prim`, `Call`, `TailCall`, `Switch`/`Jump`, `Return`), `Value`, and a source map from bytecode to `Pos`.
- [ ] Corpus v1, 20–25 programs: every construct from SPEC §5–9, ASCII twins, recursion (factorial, list length, deep recursion), and negative cases (overlap, merge outside a switch, arity mismatch, cycle, unknown name, a guard after the default arm). Hand-written expectations: `.diagram` including wires, `.out` for program output, and `.fmt.shaw` for non-canonical inputs.
- [ ] Gate P1: contracts compile, the workspace builds, commit. The corpus is expected to be red only on stages that later waves own.

### Wave A: Leaves (4 agents in parallel)

| Agent | Owns | Delivers | Oracle |
|---|---|---|---|
| **A1** shape-wires | `shawline-shape` (not `wire.rs`) | Wire tracing over stroke components outside borders; endpoint classification; crossings; guard gaps in dividers; ASCII parity for every construct; diagnostics for dangling ends and wires touching. | Corpus `.diagram` |
| **A2** word | `shawline-word` | Letter table, classes and `mirror()`; the primitive table (name, arity); the phonemic ASCII transliteration table, plus an incremental transliterator for the LSP; a license check on the ReadLex lexicon, reported with a recommendation. | Unit tests against the SPEC §4 and §9 tables |
| **A3** glyph | `shawline-glyph` | A skeleton model (kurbo paths on a 600×1200-unit cell, with x-height, ascender and descender for the short, tall and deep classes); all 48 letters, with partners derived by the true rotation or reflection; every stroke character drawn edge-to-edge so lines join across cells; digits, marks, namer dot; `specimen.svg` and a PNG preview for self-inspection. | Self-inspection of the PNG, then the user at Gate A |
| **A4** vm | `shawline-vm` (not `bytecode.rs`) | The dispatch loop, values, heap-allocated frames, tail calls, runtime errors mapped to source positions; `.shawasm` assembler and disassembler; a 100k-deep recursion test. | Crate tests driven by `.shawasm` programs |

**Gate A (lead):** merge four branches; run workspace tests and clippy; the `.diagram` stage must be fully green. The user reviews `specimen.svg`. The lead copies the transliteration table into SPEC §13 and commits.

### Wave B: Middle (5 agents in parallel)

| Agent | Owns | Delivers | Oracle |
|---|---|---|---|
| **B1** syntax | `shawline-syntax` (not `ast.rs`) | Diagram + words → AST; every structural diagnostic from SPEC §5–8; an `.ast` dump. | Corpus `.ast` (lead blesses after reading), negative cases |
| **B2** fmt | `shawline-fmt` | The placement-preserving formatter over Grid + Diagram (SPEC §11) and the lint table (SPEC §12). | Corpus `.fmt.shaw` |
| **B3** render | `shawline-render` | Grid → SVG from glyph skeletons; light and dark themes; no dependence on installed fonts. | PNG self-inspection; `.svg` corpus for 3 programs |
| **B4** font | `shawline-font` | Shawline Mono TTF built from skeletons (kurbo stroke expansion → `write-fonts`); metrics with no line gap so box drawing joins; read-back validation. | Read-back plus a rasterized test string |
| **B5** ir | `shawline-ir` (not `ir.rs`) | AST → IR (name resolution, arity, switch lowering, cycle detection); the checker; IR → bytecode including tail-call detection. | Unit tests on hand-built ASTs; corpus `.out` comes online at the gate |

**Gate B (lead):** merge; wire `shawline run` and `shawline fmt`; register the corpus stages `ast`, `fmt.shaw`, `out` and `svg`; add the formatter property test; fix integration seams personally. The whole corpus must be green. Commit.

### Wave C: Surfaces (3 agents in parallel, plus the lead)

| Agent | Delivers |
|---|---|
| **lead** | CLI polish (`run`, `fmt`, `check`, `render`, `asm`), `miette` diagnostics, release build. |
| **C2** lsp | Diagnostics, formatting, semantic tokens, on-type transliteration, hover showing arity and the mirror partner. |
| **C3** playground | A `wasm-bindgen` build of the pipeline; a static page with a grid editor in Shawline Mono (as a webfont), live SVG and run output. |
| **C4** vscode | A VS Code extension: LSP client, bundled font and setup instructions, a transliteration toggle. |

**Gate C (lead):** merge; smoke-test the playground in the browser pane and the extension in an editor; commit.

### P5: Aesthetic and correctness gate (once)

- The user reviews the gallery (every corpus program rendered in the playground). Lint thresholds and canonical spacing get tuned from that review.
- One audit pass, with lenses picked via the `agent-fleet` skill (at most two; likely `dijkstra-correctness-auditor` on VM/IR and `data-integrity-architect` on formatter round-trips). The lead triages and batches the fixes.
- A jitter corpus: the canonical programs with random one-cell perturbations must format back to their canonical form.

## Risks

| Risk | Mitigation |
|---|---|
| Editor line spacing leaves gaps in box drawing | The font has no line gap, with ascender + descender equal to the cell height; the docs recommend line-height 1.0. The SVG renderer is unaffected. |
| Terminals and editors fall back to other fonts for Shavian, breaking the grid | The closed repertoire means Shawline Mono covers everything; setup docs say to set it for `.shaw` files. |
| Parallel wires one cell apart are ambiguous to trace | SPEC v1 requires a gap between parallel wires; A1 diagnoses wires that touch. |
| Jitter straightening in the formatter is the hardest algorithm | B2 builds character normalization, spacing and snapping first, and straightening last, behind the P5 jitter corpus. |
| `write-fonts` API churn | Pin the version. Fallback: emit a UFO and build it with `fontmake`. |
| No suitably licensed English → Shavian lexicon | v1 transliteration needs no dictionary (SPEC §13). |
| Tail-call detection through switch arms | Defined in SPEC §10; B5 tests it directly and A4 tests constant-stack execution from `.shawasm`. |
