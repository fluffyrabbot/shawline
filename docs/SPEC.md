# Shawline language specification

Status: **draft v0**. The lead completes and freezes it at Gate P1 (see [PLAN.md](PLAN.md)). After the freeze, changes go through the lead only.

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

## 4. Words

A **word** is a maximal horizontal run of Shavian letters, namer dots and digits.

- A word made only of digits is a **number**: a non-negative integer literal. Negative values come from `𐑚` (subtract).
- A word that mixes digits and letters is an error.
- A word that starts with `·` is a **definition name** and appears only in a definition's title. Shavian prose marks proper names with the namer dot; Shawline uses it the same way for the names of definitions.
- `«…»` is a **text literal**. It may contain Shavian letters, digits, the namer dot and spaces.
- `※` starts a **comment** that runs to the end of the line. Comments are still restricted to the repertoire, so they are written in Shavian.

**Single-letter words are reserved for primitives** (§9). User definitions need names of two letters or more.

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
╭─ ·𐑛𐑳𐑚𐑩𐑤 ────────────────────╮
│                             │
│         ┌───┐               │
▶ 𐑯 ──┬───▶ 𐑐 ├───────────────▶ 𐑛
│     └───▶   │               │
│         └───┘               │
╰─────────────────────────────╯
```

This is `double n = n 𐑐 n`, from [corpus/seed/01-double.shaw](../corpus/seed/01-double.shaw).

- **Inputs** are `▶` arrowheads on the left border. The input's name follows the arrowhead inside the box, and the input's wire starts after the name.
- **Outputs** are `▶` arrowheads on the right border. The output's name follows the arrowhead outside the box.
- **A port's name always follows its arrowhead**, on both sides.
- Ports are ordered top to bottom. That order is the definition's parameter order and result order.
- The entry point is `·𐑥𐑱𐑯` ("main"). It has no inputs. Running a program prints main's outputs top to bottom, one per line.
- Definitions may be recursive and mutually recursive. Their order on the sheet does not matter.

## 6. Nodes

A **node** is a call. It is drawn as a square box containing exactly one word: the name of a definition or a primitive.

- **Inputs** are `▶` arrowheads on the node's left border. Their top-to-bottom order is the argument order.
- **Outputs** are `├` tees on the node's right border. Their top-to-bottom order is the result order.
- The number of input and output ports must match the callee's arity exactly.

Two shorthands avoid drawing a box:

- **Inline node:** `─▶ 𐑖 ──`. A word that sits on a horizontal wire, between an arrowhead and a continuing wire with one space on each side, is a call with one input and one output.
- **Constant:** `0 ──▶` or `«𐑣𐑦» ──▶`. A number or text literal followed by a space and then a wire is a source of that value. The zero-input primitive `𐑯𐑦𐑤` (the empty list) can be used the same way.

## 7. Wires

A **wire** is a connected set of stroke cells outside box borders. It links exactly one **source** to one or more **sinks**.

- **Sources:** a definition input (after its name), a node output tee, a constant, an inline node's output, and the inner or outer side of a switch through-port (§8).
- **Sinks:** a node input arrowhead, a definition output arrowhead, a switch selector `▼`, an inline node's input, and switch through-ports.
- **Fan-out** is a tee in the middle of a wire.
- **Merges** (two wires flowing into one) are errors, with one exception: the outputs of different arms of the same switch, at the same ordinal, may merge (§8).
- A definition's wire graph must be acyclic. Repetition comes from recursion.

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
│     │     │  0 ─▶ 𐑚 ├───▶──┴────▶ 𐑮
│     └─────▶─────▶   │   │       │
│           │     └───┘   │       │
│           └─────────────┘       │
╰─────────────────────────────────╯
```

This is `abs n`, from [corpus/seed/02-abs.shaw](../corpus/seed/02-abs.shaw). `n 𐑤 0` is the selector. When it is `0` (that is, n ≥ 0), the first arm passes `n` through unchanged. Otherwise the default arm computes `0 𐑚 n`.

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

`𐑯𐑦𐑤` is the empty list (0→1). The other single letters are reserved for future primitives, and each pair is still to be assigned as a pair.

## 10. Evaluation

- Each definition is a pure function, and evaluation is strict.
- Nodes run in dependency order. The order among independent nodes is unobservable.
- A switch evaluates only the arm it selects.
- Calls must not consume the host stack, and recursion at least 100 000 deep must work.
- A call whose outputs go only to the enclosing definition's outputs (directly, or through switch arm outputs) is a **tail call**, and it must run in constant stack.

## 11. Canonical form

The formatter **keeps the author's placement**: every box and wire stays where it was drawn. It normalizes only local style:

1. Canonical characters replace ASCII.
2. Definitions get rounded corners. Nodes and switches get square ones.
3. Titles and guards are written `─ name ─`: exactly one rule, one space, the name, one space.
4. A port name sits exactly one space after its arrowhead. A wire starts one space after a word or literal.
5. Jitter is straightened. A wire that wanders by one cell becomes straight when straightening changes no connections.
6. A box edge snaps to the nearest column where all of its rows agree.
7. Trailing spaces are removed. Exactly one blank line separates top-level items.

The formatter is idempotent: `fmt(fmt(x)) == fmt(x)`. It also preserves meaning: `parse(fmt(x))` is equivalent to `parse(x)`.

Full automatic layout (`shawline fmt --relayout`) is a later milestone.

## 12. Aesthetic lints

These are measurable stand-ins for taste. The formatter fixes what it can; the checker reports the rest as warnings.

- **Mixed box style:** a box with some rounded and some square corners.
- **Wire too short:** a wire with fewer than 2 cells before its arrowhead.
- **Too many bends:** a wire with more than 3 bends between its source and a sink.
- **Too many crossings:** more than 2 crossings in a definition. Most of these are better solved by re-arranging.
- **Ink too dense:** more than 60% non-space cells in a definition's interior.
- **Asymmetric padding:** a node box whose word is not centred, when centring is possible.

The thresholds live in one table in `shawline-fmt`. They get tuned against the corpus at the P5 aesthetic gate.

## 13. Typing it

Shawline is typed through **phonemic ASCII transliteration**. Each Shavian letter has a short ASCII key sequence, and the language server replaces the sequence as you type. The mapping is one-to-one and needs no dictionary.

`shawline-word` owns the table, and it is reproduced here at Gate A. An English-word → Shavian dictionary is an optional extra, adopted only if a suitably licensed lexicon exists; the A2 brief covers that check.
