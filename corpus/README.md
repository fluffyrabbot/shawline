# Corpus

The single oracle for every pipeline stage. Each `*.shaw` program may have expected outputs beside it, one per stage. A stage is checked only where its file exists and the stage is registered in `shawline-corpus`.

| File | Stage | Registered |
|---|---|---|
| `.diagram` | Boxes, ports, dividers, text runs (`shawline-shape`) | P0 |
| `.wires` | Wire tracing (`wire.rs` dump) | P1; green at Gate A |
| `.ast` | Syntax dump | Gate B |
| `.check` | Every static diagnostic, as `ROW:COL CODE` (SPEC §14) | Gate B |
| `.out` | Program stdout, or `error ROW:COL CODE` | Gate B |
| `.fmt.shaw` | Formatter output for a non-canonical input | Gate B |
| `.studio` | How each Studio edit ends: `ok` with a hash of the result, or `refused` with the refusal's kind and cell (`shawline_corpus::studio`) | P8 |
| `.gloss` | The gloss layer: one line per Shavian word in row-major order, `ROW:COL WORD READING [GLOSS] shown\|dropped`, where `GLOSS` is a primitive's short name and the last field says whether the strip shows it (`shawline_render::glosses`) | P9 |
| `.repair` | How each repair `shawline_edit::repairs` offers ends, one line per repair: `ROW:COL CODE EDIT ok DIGEST`, `… refused KIND AT`, or `ROW:COL CODE` for a repair that is all follow-up, each ending `then FOLLOWUP ROW:COL` when it has one (`shawline_corpus::repair`) | P10 wave 3 |

A program with an empty `.check` and no `.fmt.shaw` is canonical: it must format to itself.

`.fmt.shaw`, `.svg`, `.studio`, `.gloss` and `.repair` are selective; `.studio` and `.gloss` are kept for every valid program, and `.repair` for every program with static diagnostics. Blessing only rewrites the files that exist; to add one, create it empty, then bless.

Numbering: `01`–`79` are valid programs, `80`–`89` fail at runtime, and `90`–`99` must be rejected statically. An ASCII twin of a canonical program has the same number and the suffix `-ascii`; its `.wires` and `.out` match the canonical program's, and its `.fmt.shaw` is the canonical program.

The lead writes and blesses expectations (`SHAWLINE_BLESS=<stage>`). Agents only read them. Never bless a stage whose implementation isn't merged: blessing writes actual output, which for an unimplemented stage is empty.

`corpus/static/` holds statically rejected programs, numbered on their own. Each shows one diagnostic (with a lint, or a second diagnostic that the same repair clears), so that every code in PLAN P10's repairs table has a case.

`corpus/fix/` holds inputs for `shawline fmt --fix`: near boxes with slipped border cells (SPEC §3). Each has a `.check` and a `.fix.shaw` (the repaired sheet, or the input unchanged where nothing qualifies as a slip). The `fix.shaw` stage is registered once `--fix` lands.
