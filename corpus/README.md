# Corpus

The single oracle for every pipeline stage. Each `*.shaw` program may have expected outputs beside it, one per stage: `<name>.diagram`, and later `.ast`, `.fmt.shaw`, `.out` and `.svg`. A stage is checked only where its file exists.

Numbering: `0x`–`8x` are valid programs and `9x` are programs that must be diagnosed. An ASCII twin of a canonical program has the same number and the suffix `-ascii`.

The lead writes and blesses expectations (`SHAWLINE_BLESS=<stage>`). Agents only read them.
