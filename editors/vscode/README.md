# Shawline for VS Code

Editor support for Shawline, the dataflow language you draw. You get diagnostics, formatting, semantic highlighting and hover from the `shawline-lsp` language server, plus typing Shavian from plain ASCII keys and the Shawline Mono font.

## Install

1. Install the language server from the repository root:

   ```bash
   cargo install --path crates/shawline-lsp
   ```

   The extension looks for `shawline-lsp` on `PATH` and in `~/.cargo/bin`. To use another build, set `shawline.serverPath` (a relative path is resolved against the first workspace folder, e.g. `target/debug/shawline-lsp`).

2. Build and install the extension:

   ```bash
   cd editors/vscode
   npm install
   npm run package          # builds fonts/ShawlineMono.ttf, then shawline-0.1.0.vsix
   code --install-extension shawline-0.1.0.vsix
   ```

   Or choose **Extensions: Install from VSIX…** in the Command Palette.

## Font setup

Boxes and wires line up only when Shavian and box drawing have the same width, so `.shaw` files need **Shawline Mono** at line height 1. VS Code can't install fonts, so the extension ships the font and points you at it:

1. Run **Shawline: Install Shawline Mono Font**. It reveals `ShawlineMono.ttf` in your file manager.
2. Install it: double-click it and choose *Install Font* on macOS, right-click and choose *Install* on Windows, or copy it to `~/.local/share/fonts` and run `fc-cache -f` on Linux.
3. Reload the window.

The extension already sets the font and line height for the `shawline` language; you don't need to change your other editors.

## Typing Shavian

With transliteration on, the language server turns ASCII key sequences into Shavian letters as you type (SPEC §13). Keys follow the sounds, with `@` for the schwa. Mirror pairs are voiceless and voiced key pairs.

| Letter | Key | Letter | Key | Letter | Key |
|---|---|---|---|---|---|
| 𐑐 peep | `p` | 𐑚 bib | `b` | 𐑩 ado | `@` |
| 𐑑 tot | `t` | 𐑛 dead | `d` | 𐑳 up | `u` |
| 𐑔 thigh | `th` | 𐑞 they | `dh` | 𐑰 eat | `ee` |
| 𐑕 so | `s` | 𐑟 zoo | `z` | 𐑸 are | `ar` |
| 𐑖 sure | `sh` | 𐑠 measure | `zh` | 𐑯 nun | `n` |

The full table is in `docs/SPEC.md` §13 of the Shawline repository.

- **Longest match wins:** `th` is 𐑔. Type `/` between keys for the shorter reading: `t/h` is 𐑑𐑣. The `/` itself leaves nothing behind.
- **The namer dot `·` is `=`,** because `.` is a stroke. `=dub@l` gives `·𐑛𐑳𐑚𐑩𐑤`.
- Only word runs are transliterated. Outside a word, `v` stays the `▼` arrowhead fallback, and digits, spaces and strokes pass through.

Transliteration is `editor.formatOnType` for the `shawline` language. The status bar shows **Shavian** (on) or **ASCII** (off); click it, or run **Shawline: Toggle Shavian Transliteration**, to switch.

## Settings

| Setting | Default | Purpose |
|---|---|---|
| `shawline.serverPath` | `shawline-lsp` | Language server binary |

Defaults applied to `[shawline]` only, each to keep the drawing on its grid:

| Setting | Value | Why |
|---|---|---|
| `editor.fontFamily` | `Shawline Mono` | One cell per character |
| `editor.lineHeight` | `1` | Line height 1 × font size, so vertical strokes join |
| `editor.letterSpacing`, `editor.fontLigatures` | `0`, `false` | Nothing may shift or merge cells |
| `editor.formatOnType` | `true` | Transliteration on |
| `editor.renderWhitespace`, `editor.guides.indentation` | `none`, `false` | No dots or guide lines across the drawing |
| `editor.wordWrap` | `off` | A wrapped row breaks every box it crosses |
| `editor.insertSpaces`, `editor.detectIndentation` | `true`, `false` | Tabs are errors (G001) |
| `editor.inlayHints.enabled`, `editor.codeLens` | `off`, `false` | Inserted hints and lens rows push cells out of line |
| `editor.unicodeHighlight.*` | `false` | No boxes drawn around Shavian or `·` |
| `editor.semanticHighlighting.enabled` | `true` | Colour comes from the server, not a grammar |
| `editor.wordBasedSuggestions` | `off` | Stops half-typed ASCII keys being offered as completions |

Override any of them under `"[shawline]"` in your settings.

**Commands:** Toggle Shavian Transliteration, Install Shawline Mono Font, Restart Language Server.
