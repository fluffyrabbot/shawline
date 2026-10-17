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

A program with an empty `.check` and no `.fmt.shaw` is canonical: it must format to itself.

Numbering: `01`–`79` are valid programs, `80`–`89` fail at runtime, and `90`–`99` must be rejected statically. An ASCII twin of a canonical program has the same number and the suffix `-ascii`; its `.wires` and `.out` match the canonical program's, and its `.fmt.shaw` is the canonical program.

The lead writes and blesses expectations (`SHAWLINE_BLESS=<stage>`). Agents only read them. Never bless a stage whose implementation isn't merged: blessing writes actual output, which for an unimplemented stage is empty.
