# Shawline language specification

Status: **v1, frozen at Gate P1.** Changes go through the lead only (see [PLAN.md](PLAN.md)). The contracts that implement it are `shawline-shape/src/wire.rs`, `shawline-syntax/src/ast.rs`, `shawline-ir/src/ir.rs` and `shawline-vm/src/bytecode.rs`.

Shawline is a dataflow language whose source is a drawing. Definitions are rounded boxes, calls are square boxes, values travel along wires, and every name is written in the Shavian alphabet.

## 1. Principles

1. **The drawing is the program.** Boxes, wires and arrowheads carry the meaning. They are not decoration around the text.
2. **Shape follows meaning.** Shavian's mirrored letter pairs name dual operations, the namer dot marks definitions, and rounded corners mean "definition" while square corners mean "use".
3. **Rough input, canonical output.** Recognition is structural, so a lopsided ASCII sketch parses. The formatter then gives it the canonical glyphs and spacing while keeping the author's composition (§11).
4. **One cell, one character.** The repertoire is closed, and Shawline Mono draws every character in it at the same width. Column arithmetic is code-point arithmetic everywhere: in the grid, in diagnostics and in the renderer.

## 2. Source text

A source file (`.shaw`) is UTF-8. It may use `\n` or `\r\n` line endings and may start with a byte-order mark. Tabs are errors. Every character must be in the repertoire; anything else is an error that reports its position (`shawline-grid`).

| Class | Characters |
|---|---|
| Shavian | U+10450–U+1047F (48 letters, §4) |
| Namer dot | `·` U+00B7 |
| Digits | `0`–`9` |
| Strokes, canonical | `─ │ ┌ ┐ └ ┘ ├ ┤ ┬ ┴ ┼ ╭ ╮ ╯ ╰ ┄ ┆ ▶ ◀ ▲ ▼` |
| Strokes, ASCII fallback | `- \| + . ' ~ : > < ^ v` |
| Marks | `« » ※` |
| Space | U+0020 |

The ASCII fallbacks map as follows: `-`→`─`, `|`→`│`, `~`→`┄`, `:`→`┆`, `> < ^ v`→`▶ ◀ ▲ ▼`. The joints `+ . '` take their shape from their neighbours: `+` draws square corners, tees and crossings, `.` draws rounded top corners and `'` draws rounded bottom corners.

Plain English is outside the repertoire by construction. The only Latin letter allowed is `v`, and only as a stroke.

## 3. Strokes

Every stroke cell connects in a fixed set of directions. The table lives in `shawline-shape/src/stroke.rs`.

- **Lines** (`─ │`) carry a wire straight through.
- **Corners** (`┌ ┐ └ ┘` square, `╭ ╮ ╰ ╯` rounded) turn a wire or close a box.
- **Tees** (`├ ┤ ┬ ┴`) split a wire into branches (fan-out). On a box border, a tee is the point where a wire leaves the box.
- **Crossing** (`┼`) is two wires passing over each other **without connecting**. Four-way junctions do not exist; draw two tees instead. This follows the schematic convention.
- **Arrowheads** (`▶ ◀ ▲ ▼`) mark where a wire **ends**. Every wire ends in an arrowhead at each of its sinks.
- **Dashed** strokes (`┄ ┆`) appear only in switch dividers (§8).

**Touching.** Two adjacent stroke cells join only where each connects toward the other. Canonical glyphs say exactly where they connect, so strokes may run side by side: two wires on adjacent rows, or a wire hugging a box border, stay separate. The ASCII joints `+ . '` are the exception, because they join every neighbour that could join them. Keep an ASCII joint one cell away from strokes it shouldn't meet.

A wire that runs into a stroke that doesn't join it, and isn't a port, is an error (`S010`).

**Near boxes.** A box whose border is almost closed is still recognized as a box, so one misplaced cell doesn't make everything inside it meaningless. A near box needs:

- at least three of its four corners in place, which fixes its rectangle;
- every other border cell either in place, or **slipped**: exactly one cell off its line, perpendicular to its edge (a `│` one column out, a `─` one row out, the fourth corner one cell out). The cell where it belongs must be blank, and the slipped cell must join no other stroke.

Each slipped cell is an error (`S012`), so the program doesn't run. Everything else parses as if the box were whole. `shawline fmt --fix` moves slipped cells into place (§11). A cell two or more cells out is not a slip, and the box isn't recognized.

**Arrowheads in Shawline Mono.** A font glyph can't see the rows above and below it, so the font draws `▶ ◀` with a vertical border line through them and `▲ ▼` with a horizontal one. Nearly every arrowhead sits in a box border; an inline node's input arrowhead shows a small tick instead. The SVG renderer knows each arrowhead's neighbours and draws both kinds exactly.

## 4. Words

A **word** is a maximal horizontal run of Shavian letters, namer dots and digits.

- A word made only of digits is a **number**: a non-negative integer literal. Negative values come from `𐑚` (subtract).
- A word that mixes digits and letters is an error.
- A word that starts with `·` is a **definition name** and appears only in a definition's title. Shavian prose marks proper names with the namer dot; Shawline uses it the same way for the names of definitions.
- `«…»` is a **text literal**. It may contain Shavian letters, digits, the namer dot and spaces.
- `※` starts a **comment** that runs to the end of the line. Comments are still restricted to the repertoire, so they are written in Shavian.
- A comment ends early at the first stroke character on its line (a box-drawing character, ASCII stroke or joint, or arrowhead), so a comment can sit inside a box without swallowing its border.

**Single-letter words and `𐑯𐑦𐑤` are reserved for primitives** (§9). User definitions need names of two letters or more, other than `𐑯𐑦𐑤`.

**Port names** (§5) are a separate namespace from callable names. A port named `𐑯` does not shadow the primitive `𐑯`.

### The Shavian alphabet

The letters are paired. In each pair, the second letter is the first one rotated or reflected, and usually the voiced counterpart of its sound. Shawline gives the two letters of a pair **dual meanings** (§9).

| Tall | Deep | | Short | Short |
|---|---|---|---|---|
| 𐑐 peep | 𐑚 bib | | 𐑤 loll | 𐑮 roar |
| 𐑑 tot | 𐑛 dead | | 𐑥 mime | 𐑯 nun |
| 𐑒 kick | 𐑜 gag | | 𐑦 if | 𐑰 eat |
| 𐑓 fee | 𐑝 vow | | 𐑧 egg | 𐑱 age |
| 𐑔 thigh | 𐑞 they | | 𐑨 ash | 𐑲 ice |
| 𐑕 so | 𐑟 zoo | | 𐑩 ado | 𐑳 up |
| 𐑖 sure | 𐑠 measure | | 𐑪 on | 𐑴 oak |
| 𐑗 church | 𐑡 judge | | 𐑫 wool | 𐑵 ooze |
| 𐑘 yea | 𐑢 woe | | 𐑬 out | 𐑶 oil |
| 𐑙 hung | 𐑣 ha-ha | | 𐑭 ah | 𐑷 awe |

There are also eight compound letters (U+10478–1047F): 𐑸 are, 𐑹 or, 𐑺 air, 𐑻 err, 𐑼 array, 𐑽 ear, 𐑾 ian, 𐑿 yew. Compounds are unpaired.

In code points, each tall letter's partner is +10 (U+10450 → U+1045A), and each short letter's partner is also +10 (U+10464 → U+1046E).

## 5. Definitions

A sheet (one source file) holds definitions and comments. A **definition** is a rounded box with the definition's name in its top border:

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

This is `double n = n 𐑐 n`, from [corpus/03-double.shaw](../corpus/03-double.shaw).

- **Inputs** are `▶` arrowheads on the left border. The input's name follows the arrowhead inside the box, and the input's wire starts after the name.
- **Outputs** are `▶` arrowheads on the right border. The output's name follows the arrowhead outside the box.
- **A port's name always follows its arrowhead**, on both sides.
- Ports are ordered top to bottom. That order is the definition's parameter order and result order.
- The entry point is `·𐑥𐑱𐑯` ("main"). It has no inputs. Running a program prints main's outputs top to bottom, one per line.
- Definitions may be recursive and mutually recursive. Their order on the sheet does not matter.

## 6. Nodes

A **node** is a call. It is drawn as a square box containing exactly one word: the name of a definition or a primitive.

- **Inputs** are `▶` arrowheads on the node's left border. Their top-to-bottom order is the argument order.
- **Outputs** are `├` tees on the node's right border. Their top-to-bottom order is the result order. An output nobody needs is still drawn, as a tee with no wire (see `𐑕` in [corpus/09-length.shaw](../corpus/09-length.shaw)).
- The number of input and output ports must match the callee's arity exactly.
- Every port arrowhead (a node's input, a definition's output, a switch's selector or through-port) must have a wire reaching it (`Y019`). An output tee may be left unwired; an arrowhead may not.

Two shorthands avoid drawing a box:

- **Inline node:** `─▶ 𐑖 ──`. A word that sits on a horizontal wire, between an arrowhead and a continuing wire, is a call with one input and one output.
- **Constant:** `0 ──▶` or `«𐑣𐑦» ──▶`. A number or text literal followed by a wire is a source of that value. `𐑯𐑦𐑤` (the empty list) works the same way.

### Text meets wires across spaces

Text and wires interact only **horizontally, on the same row, across spaces**. Nothing else may come between them. The canonical gap is exactly one space; rough input may use any number of spaces, including none. In the rules below, "open end" means a wire end that faces a non-stroke cell (`EndKind::Open`), and "free arrowhead" means an arrowhead that is not on a box border.

| Pattern | Meaning |
|---|---|
| `▶` on a definition's left border, word, open end facing W | Input port: the word names it, and the wire is its source. |
| `▶` on a definition's right border, then a word outside | Output port: the word names it. |
| Literal (or `𐑯𐑦𐑤`), then an open end facing W, and no free arrowhead west of the literal | Constant. |
| Free arrowhead pointing E, word, open end facing W | Inline node. |
| Any other open end | Dangling wire (`Y003`). |
| Any other free arrowhead | Dangling arrowhead (`Y004`). |
| Any other word, number or literal | Stray text (`Y012`). Comments and titles are never stray. |

Inline nodes are horizontal only. A vertical wire can't carry one.

## 7. Wires

A **wire** is a connected set of stroke cells outside box borders. It links exactly one **source** to one or more **sinks**.

Each wire end is a source or a sink, decided by what it meets:

| The end meets | Role |
|---|---|
| An open end next to an input port name, a constant, or an inline node's word | Source |
| A tee on a node's right border | Source |
| A node's input `▶`, a definition's output `▶`, a selector `▼` | Sink |
| A free arrowhead (an inline node's input) | Sink |
| A switch's left through-port, from outside / from inside | Sink / source |
| A switch's right through-port, from inside / from outside | Sink / source |

- Every wire has exactly one source and at least one sink. Every sink is an arrowhead.
- **Fan-out** is a tee in the middle of a wire.
- **Merges** (two sources on one wire) are errors (`Y006`), with one exception: the outer sides of the right through-ports of the same switch, at the same result ordinal, one per arm. Together they are that switch's output (§8).
- A definition's wire graph must be acyclic (`C003`). Repetition comes from recursion.

## 8. Switches

A **switch** is a square box divided into **arms** by dashed dividers (`├┄┄┄┤`). It evaluates exactly one arm.

```
╭─ ·𐑨𐑚𐑕 ──────────────────────────╮
│                                 │
│             ┌───┐               │
▶ 𐑯 ──┬──┬────▶ 𐑤 ├─┐             │
│     │  │ 0 ─▶   │ │             │
│     │  │    └───┘ │             │
│     │  │          │             │
│     │  │  ┌─ 0 ───▼─────┐       │
│     │  └──▶─────────────▶──┐    │
│     │     ├┄┄┄┄┄┄┄┄┄┄┄┄┄┤  │    │
│     │     │     ┌───┐   │  │    │
│     │     │  0 ─▶ 𐑚 ├───▶──┴────▶ 𐑬
│     └─────▶─────▶   │   │       │
│           │     └───┘   │       │
│           └─────────────┘       │
│                                 │
╰─────────────────────────────────╯
```

This is `abs n`, from [corpus/04-abs.shaw](../corpus/04-abs.shaw). `n 𐑤 0` is the selector. When it is `0` (that is, n ≥ 0), the first arm passes `n` through unchanged. Otherwise the default arm computes `0 𐑚 n`.

- **The selector** is a wire that ends in a `▼` on the switch's top border.
- **Guards** are written the same way as titles. An arm's guard is a literal in a gap in the line just above the arm: the top border for the first arm, and the divider for each later arm (`├┄ 1 ┄┄┤`). An arm with no guard is the **default arm**. There is at most one default arm, and it must come last.
- **Evaluation:** the selector is evaluated first and compared for equality with each guard, top to bottom. Only the first matching arm runs; if nothing matches, the default arm runs. If nothing matches and there is no default arm, that is a runtime error. The checker warns when a switch has no default arm.
- **Through-ports** carry values across the border. A `▶` on the left border inside an arm's rows ends the outer wire and starts a wire inside that arm. A `▶` on the right border does the opposite, carrying an arm's result out. A value that several arms need is fanned out outside the switch and fed in once per arm.
- Every arm must have the same number of output through-ports. Outputs at the same ordinal merge outside the switch (the `┴` in the example above).

## 9. Values and primitives

**Values:**

| Type | Notes |
|---|---|
| Int | `i64`. Overflow is a runtime error. Truth is `1`/`0`. |
| Text | A Shavian string. |
| Pair | Two values. |
| List | Either empty (`𐑯𐑦𐑤`) or a head and a tail. |

Checking is dynamic in v1, so a type mismatch is a runtime error.

**Output.** Running a program prints main's outputs top to bottom, one per line:

| Value | Printed as |
|---|---|
| Int | Decimal, with `-` when negative |
| Text | Bare at top level; `«quoted»` inside a pair or list |
| Pair | `(a b)` |
| List | `[a b c]`; the empty list is `[]` |

A runtime error prints nothing on stdout. It prints `error ROW:COL CODE: message` on stderr and exits with status 1. The position is the failing node's word (or the switch's selector), zero-based as in every dump. The corpus `.out` files hold stdout, or, for a program that fails, the single line `error ROW:COL CODE`.

**Primitives.** Each mirrored pair names a pair of duals.

| Letter | Meaning | Ports in→out | Its mirror | Meaning | Ports in→out |
|---|---|---|---|---|---|
| 𐑐 | add | 2→1 | 𐑚 | subtract | 2→1 |
| 𐑑 | multiply | 2→1 | 𐑛 | divide (truncating; ÷0 is an error) | 2→1 |
| 𐑒 | pair | 2→1 | 𐑜 | unpair | 1→2 |
| 𐑟 | prepend (head, tail → list) | 2→1 | 𐑕 | split (list → head, tail; empty is an error) | 1→2 |
| 𐑤 | less than | 2→1 | 𐑮 | greater than | 2→1 |
| 𐑦 | equal | 2→1 | 𐑰 | not equal | 2→1 |
| 𐑥 | minimum | 2→1 | 𐑯 | maximum | 2→1 |
| 𐑖 | show (Int → Text) | 1→1 | 𐑠 | read (Text → Int) | 1→1 |

`𐑯𐑦𐑤` is the empty list (0→1), whether written as a constant (`𐑯𐑦𐑤 ──▶`) or boxed. The other single letters are reserved for future primitives, and each pair is still to be assigned as a pair.

## 10. Evaluation

- Each definition is a pure function, and evaluation is strict.
- Nodes run in dependency order; among nodes that don't depend on each other, the one whose word comes first in row-major order runs first. The exception is a tail call, which always runs last in its definition, because it replaces the definition's frame.
- That order only shows when a program hits more than one runtime error: the error reported is the first one hit in this order. So the reported error is the same on every run of a sheet, though an edit that turns a call into, or out of, a tail call can change which one it is.
- A switch evaluates only the arm it selects.
- Calls must not consume the host stack, and recursion at least 100 000 deep must work.
- Memory is bounded, so a program whose base case is never reached stops with an error instead of exhausting the machine:
  - **Calls** (`R007`, at the call that went too deep): more than 1 000 000 calls nested below `·𐑥𐑱𐑯`, or more than 16 777 216 values held by live calls together (about 400 MB). Tail calls don't nest, so they never reach the first limit.
  - **Heap** (`R008`, at the node that allocated): more than 8 388 608 pairs and list cells alive at once. This catches a tail loop that keeps growing a list, which the call limit can't see.
  - **Output** (`R009`, at `·𐑥𐑱𐑯`'s namer dot): a result whose printed form would exceed 1 000 000 characters. Pairs can share parts, so a few cells can describe an astronomically large value.
- A call whose outputs are exactly the enclosing definition's outputs, all of them, in the same order, and read by nothing else (directly, or through switch arm outputs), is a **tail call**, and it must run in constant stack. A call whose results reach the outputs reordered, duplicated or only partly is an ordinary call. Calls inside `·𐑥𐑱𐑯` itself are never tail calls, so its results can be measured (`R009`) before the program ends.

## 11. Canonical form

The formatter **keeps the author's placement**: every box and wire stays where it was drawn. It normalizes only local style:

1. Canonical characters replace ASCII. An ASCII corner becomes the canonical corner it draws: `.` and `'` rounded, `+` square.
2. A canonical corner never changes roundedness. Definitions are drawn rounded and nodes and switches square, but the formatter doesn't enforce it: a wrongly styled box is an error (`Y014`, `Y015`, `Y018`), and the corners of a drawing that isn't a box yet keep the style the author is drawing toward.
3. Titles and guards are written `─ name ─`: exactly one rule, one space, the name, one space.
4. A port name sits exactly one space after its arrowhead. A wire starts one space after a word or literal.
5. Jitter is straightened. A wire that wanders by one cell becomes straight when straightening changes no connections.
6. Slipped border cells (§3, near boxes) stay where they are. Moving one changes the program from an error into a valid one, so that is a repair, not formatting. `shawline fmt --fix` does it: it applies every rule here and also moves each slipped cell into place. Nothing else differs between the two modes.
7. Trailing spaces are removed. Exactly one blank line separates top-level items.

The formatter is idempotent: `fmt(fmt(x)) == fmt(x)`. It repeats its rules until nothing changes, because one fix can make room for another. It also preserves meaning: `parse(fmt(x))` is equivalent to `parse(x)`.

Full automatic layout (`shawline fmt --relayout`) is a later milestone.

## 12. Aesthetic lints

These are measurable stand-ins for taste. The formatter fixes what it can; the checker reports the rest as warnings.

- **Mixed box style:** a box with some rounded and some square corners.
- **Wire too short:** an arrowhead with no line cell before it (`0 ▶`).
- **Too many bends:** a wire with more than 4 bends between its source and a sink. Four is the minimum for a U-shaped detour into a lower input, as in `06-pair` and `08-factorial`.
- **Too many crossings:** more than 2 crossings in a definition. Most of these are better solved by re-arranging.
- **Ink too dense:** more than 60% non-space cells in a definition's interior.
- **Asymmetric padding:** a node box whose word is not centred, when centring is possible.

The thresholds live in one table in `shawline-fmt`. They get tuned against the corpus at the P5 aesthetic gate.

## 13. Typing it

Shawline is typed through **phonemic ASCII transliteration**. Each Shavian letter has a short ASCII key sequence, and the language server replaces the sequence as you type. The mapping is one-to-one and needs no dictionary. Keys follow the sounds, spelled the way English pronunciation respellings spell them, with `@` for the schwa. Mirror pairs are voiceless/voiced key pairs (`p`/`b`, `th`/`dh`, `sh`/`zh`).

| Letter | Key | Letter | Key |
|---|---|---|---|
| 𐑐 peep | `p` | 𐑚 bib | `b` |
| 𐑑 tot | `t` | 𐑛 dead | `d` |
| 𐑒 kick | `k` | 𐑜 gag | `g` |
| 𐑓 fee | `f` | 𐑝 vow | `v` |
| 𐑔 thigh | `th` | 𐑞 they | `dh` |
| 𐑕 so | `s` | 𐑟 zoo | `z` |
| 𐑖 sure | `sh` | 𐑠 measure | `zh` |
| 𐑗 church | `ch` | 𐑡 judge | `j` |
| 𐑘 yea | `y` | 𐑢 woe | `w` |
| 𐑙 hung | `ng` | 𐑣 ha-ha | `h` |
| 𐑤 loll | `l` | 𐑮 roar | `r` |
| 𐑥 mime | `m` | 𐑯 nun | `n` |
| 𐑦 if | `i` | 𐑰 eat | `ee` |
| 𐑧 egg | `e` | 𐑱 age | `ay` |
| 𐑨 ash | `a` | 𐑲 ice | `ie` |
| 𐑩 ado | `@` | 𐑳 up | `u` |
| 𐑪 on | `o` | 𐑴 oak | `oh` |
| 𐑫 wool | `uu` | 𐑵 ooze | `oo` |
| 𐑬 out | `ow` | 𐑶 oil | `oy` |
| 𐑭 ah | `ah` | 𐑷 awe | `aw` |
| 𐑸 are | `ar` | 𐑹 or | `or` |
| 𐑺 air | `air` | 𐑻 err | `ur` |
| 𐑼 array | `@r` | 𐑽 ear | `eer` |
| 𐑾 ian | `i@` | 𐑿 yew | `ew` |
| · namer dot | `=` | | |

- **Longest match wins.** `th` is 𐑔 and `ar` is 𐑸. Type `/` between keys for the shorter reading: `t/h` is 𐑑𐑣. The `/` itself produces nothing.
- **The namer dot is `=`,** because `.` is a stroke.
- **Only word runs are transliterated.** Inside a word run, `v` is 𐑝. Anywhere else `v` is left alone, so it stays the `▼` arrowhead fallback. To start a word with 𐑝, type `/v`. Other characters (digits, spaces, marks, strokes) always pass through unchanged.

`shawline-word` owns the table (`TABLE`), and a test keeps this copy in step with it. The language server may also offer whole-word suggestions from the ReadLex lexicon (MIT-licensed; only its Latin→Shavian columns are bundled), but source text never depends on it.

## 14. Diagnostics

Every diagnostic has a stable code. The corpus checks codes and positions, never message wording, so implementers own the messages. The first letter of a code names the stage that reports it; `W` (warning) and `L` (lint) codes don't stop a program from running.

| Code | Stage | Meaning |
|---|---|---|
| G001 | grid | Tab character |
| G002 | grid | Stray carriage return |
| G003 | grid | Character outside the repertoire |
| S001 | shape | Boxes overlap without nesting |
| S002 | shape | `«` without `»` |
| S003 | shape | `»` without `«` |
| S004 | shape | Digits and letters in one word |
| S010 | shape | Wire runs into a stroke that doesn't join it |
| S011 | shape | Crossing on exactly one box's border. Where two overlapping borders cross, `S001` covers it. A wire running into such a crossing is also blocked and gets `S010` as well. |
| S012 | shape | A near box's border cell slipped one cell off its line (§3) |
| Y001 | syntax | Definition title is not a `·`-word |
| Y002 | syntax | Node box doesn't hold exactly one word |
| Y003 | syntax | Dangling wire end |
| Y004 | syntax | Dangling arrowhead |
| Y005 | syntax | Wire with no source or no sink |
| Y006 | syntax | Two sources on one wire (a merge outside a switch) |
| Y007 | syntax | Guard after the default arm |
| Y008 | syntax | More than one default arm |
| Y009 | syntax | Port without a name |
| Y010 | syntax | Switch without a selector |
| Y011 | syntax | Arms with different numbers of results |
| Y012 | syntax | Stray text |
| Y013 | syntax | Reserved definition name: a single letter, or `𐑯𐑦𐑤` |
| Y014 | syntax | Rounded box inside a definition (definitions don't nest) |
| Y015 | syntax | Box with mixed corner styles |
| Y016 | syntax | A guard on a box that is not a switch (no selector, no dividers) |
| Y017 | syntax | Number literal too large for an Int |
| Y018 | syntax | Square box outside any definition (definitions are rounded) |
| Y019 | syntax | Port arrowhead with no wire reaching it |
| C001 | check | Unknown name |
| C002 | check | Arity mismatch |
| C003 | check | Cycle in a definition's wires |
| C004 | check | Two definitions with one name |
| C005 | check | No `·𐑥𐑱𐑯` |
| C006 | check | `·𐑥𐑱𐑯` has inputs |
| C007 | check | Definition too large for one frame (§15) |
| W001 | check | Switch without a default arm |
| W002 | check | Unused definition input |
| R001 | runtime | Integer overflow, including `𐑠` of a number too large for an Int |
| R002 | runtime | Division by zero |
| R003 | runtime | Split of an empty list |
| R004 | runtime | Type mismatch |
| R005 | runtime | No arm matches |
| R006 | runtime | `𐑠` (read) of text that isn't a number |
| R007 | runtime | Call stack exhausted: calls nested too deep (§10) |
| R008 | runtime | Heap exhausted: too many pairs and list cells alive (§10) |
| R009 | runtime | Result too large to print (§10) |
| A001 | asm | Malformed `.shawasm` or bytecode |
| I001 | internal | A sheet with no errors failed to compile. Always a bug in Shawline, never in the program. |
| L001–L006 | fmt | The lints of §12, in table order |

**Where a diagnostic points.** Positions are fixed so the corpus can check them:

| Diagnostic is about | Position |
|---|---|
| A node or inline node (`Y002`, `C001`, `C002`, `R001`–`R004`, `R006`, `R007`) | Its word |
| A constant or literal (`Y017`) | The literal's first cell |
| A switch (`Y010`, `Y011`, `W001`, `R005`) | Its selector `▼`; for `Y010`, its top-left corner |
| A guard (`Y007`) | The guard's first cell |
| A definition (`Y001`, `Y013`, `C006`, `C007`) | Its namer dot |
| A duplicate definition (`C004`) | The namer dot of each later duplicate |
| A box's style or placement (`Y014`, `Y015`, `Y018`) | Its top-left corner |
| A second default arm (`Y008`) | Its divider's left `├` |
| A port (`Y009`, `W002`) | Its arrowhead |
| A wire end (`Y003`, `Y004`, `S010`) | The end's cell |
| A slipped border cell (`S012`) | The cell where it sits, not where it belongs |
| A whole wire (`Y005`, `Y006`) | The wire's first cell in row-major order |
| A cycle (`C003`) | Of the words of the nodes on the cycle, the first in row-major order |
| A port with no wire (`Y019`) | Its arrowhead |
| Heap exhausted (`R008`) | The word of the node that allocated |
| Result too large (`R009`) | The namer dot of `·𐑥𐑱𐑯` |
| No main (`C005`) | `0:0` |

A wire with a dangling end gets `Y003` (or `Y004`) only, never also `Y005`.

The checker (`C` and `W` codes) only runs on sheets free of shape and syntax errors, so a broken drawing doesn't also produce phantom name or arity errors.

The corpus `.check` files list every static diagnostic (grid, shape, syntax, check, warnings), one per line as `ROW:COL CODE`, sorted.

## 15. Bytecode

The VM is a register machine with heap-allocated frames. Instructions, values, primitives and the `.shawasm` text format are specified in `shawline-vm/src/bytecode.rs`. Tail calls compile to `TailCall` (§10). Switch guards compile to `JumpIfNe` against a constant, falling through to the default arm or to `NoMatch`.

A frame has at most 65,535 registers. The checker bounds the registers a definition needs by counting its inputs and outputs plus, for every node, constant and switch inside it at any depth, the larger of its number of inputs and its number of outputs. A constant has one output. A switch's inputs are its selector and every arm's input through-ports; its outputs are one arm's output through-ports. A count over 65,535 is `C007`. The count is generous, since registers are reused, but only a definition of tens of thousands of nodes reaches it. It is checked after every other check passes, so it never appears beside another error.
