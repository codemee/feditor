**English** | [繁體中文](README.md)

# FEditor

A cross-platform text editor with scripting macros, modeled after EmEditor. Written in Rust.

## Project layout

| Path | Description |
|---|---|
| `core/` | `feditor-core`: text buffer (ropey) with undo/redo, file encodings, search, line wrapping, search in files. Independent of the UI and macro engines |
| `script/` | `feditor-script`: language-agnostic macro API (`EditorApi`, `ScriptEngine`) |
| `script-js/` | `feditor-script-js`: JavaScript macro engine (QuickJS via rquickjs) |
| `ui/` | `feditor`: egui front end, including the custom-drawn editor area (`ui/src/editor/`), tabs, macro library, syntax highlighting, and session restore |
| `examples/` | Example macros (`macros/`) and test files in various encodings (`files/`) |
| `proto/` | Standalone IME verification prototypes (egui, iced, Slint); not part of the main workspace |

## Features

Operations are **toolbar buttons**; there are no menus. Buttons show icons only; the text description (with shortcut) is in the tooltip. The icons are drawn as vectors in `ui/src/icons.rs`, with no icon-font dependency, and follow the light/dark theme.

| Button | Shortcut | Description |
|---|---|---|
| New / Open / Save / Save As | Ctrl+N / O / S / Shift+S | Basic file operations; unsaved changes prompt before closing or switching documents |
| Find & Replace | Ctrl+F (replace: Ctrl+H), F3 / Shift+F3 | Opens the find bar with regular expression, case-sensitive, and whole-word options |
| Find in Files | Ctrl+Shift+F | Search all text files under a folder; clicking a result opens it and selects the match (see below) |
| Settings (⋯, far right) | | Appearance (follow system / light / dark), a separate color scheme for light and for dark, font (monospace fonts only), font size, display options (line numbers, current line, whitespace, syntax coloring, word wrap) |
| Font size | Ctrl +/−, Ctrl+wheel, Ctrl+0 | Adjusts the editor font size; synced with the settings panel |
| Macro panel / Rerun | F5 (rerun) | The panel lists your macros: run, set a shortcut, edit, delete; all changes are rolled back if a macro fails |
| Record Macro | | Start/stop recording: keys and typing are recorded into a macro that can be replayed |
| Tab bar | Ctrl+W close, Ctrl+Tab / Ctrl+Shift+Tab switch | Shown only when two or more files are open; click to switch, middle-click or × to close (the × only appears when the pointer is over the tab), **double-click a tab to rename the file**; unsaved tabs have a "●" after the name |
| Clear Output (top right of the output pane) | | Clear macro output and messages |
| Language, Encoding (with BOM), Line ending (right side of the status bar) | | Shows and changes the save format. In the encoding list, "with BOM" for UTF-8 and UTF-16 is its own entry: UTF-8, UTF-8 BOM, Big5, GBK, Shift_JIS, EUC-KR, UTF-16LE BOM, UTF-16LE, UTF-16BE BOM, UTF-16BE |
| Reload (right side of the status bar) | | Re-read the current file with a specific encoding when auto-detection is wrong |

The left side of the status bar shows the file path and whether it is unsaved; the right side holds the format settings of this file. Together they make up the file status. You can also open a file from the command line, `feditor <path>`, or drag files from the file manager into the window (several at once, one tab each; a file that is already open is switched to; folders are skipped).

Above the status bar is the **output panel**, which holds file-opening messages, warnings, errors, and macro output. It is collapsed to a single line by default (showing only the latest message); use the arrow on the left to expand or collapse it. The expanded height can be dragged, and the expanded/collapsed state is remembered. If an error or macro output arrives while it is collapsed, the panel pops open temporarily so you do not miss it.

### Tabs, session, and external changes

- New and Open create a new tab; if the current document is blank and unmodified it is reused, and a file that is already open is switched to instead of being opened twice. Each tab has its own caret, selection, and undo history. When closing the window, every unsaved tab is asked about in turn.
- **Double-click a tab to rename the file**: a dialog opens with only the base name selected (the extension is not overwritten); Enter or "Rename" actually renames the file on disk (it stays in the same folder). If the name is invalid (empty, contains `\ / : * ? " < > |`, or is a reserved Windows name), the folder already has a file with that name, or another tab already has that file open, the dialog explains why and nothing is overwritten. A tab with unsaved changes can be renamed too; its content and unsaved state are unchanged. For a tab that is not saved to a file yet, a double-click opens "Save As".
- **Session**: when started without a file argument, the files that were open last time (and their caret positions) are reopened. It is stored in `session.conf` in the settings folder; blank never-saved documents are not recorded.
- **External change detection**: when another program modifies a file (checked when the window regains focus and once per second for the active file), a document without unsaved changes is reloaded automatically with the caret kept; otherwise you are asked whether to reload, and choosing to keep your version means you are not asked again.

### Appearance

The "⋯" button at the far right of the toolbar opens **Settings**: appearance and color schemes, font, and font size.

- **Appearance**: a single three-state button; its icon changes with the current state (◐ follow system, ☀ light, ☾ dark) and each click advances to the next one (follow system → light → dark → follow system).
- **Color scheme**: **light and dark each get their own scheme**, independent of each other (for example GitHub Light for light and Dracula for dark); the one for the current appearance is used, also when following the system. Your choice is remembered. Built-in schemes:
  - Dark: EVA (the default, the *Neon Genesis Evangelion* palette: purple-tinted near-black, Unit-01 purple, neon green, NERV orange), Dracula, Monokai, Solarized Dark, One Dark, Nord, Gruvbox Dark, Tokyo Night, Catppuccin Mocha / Macchiato / Frappé.
  - Light: Default Light (the original look), Solarized Light, GitHub Light, One Light, Gruvbox Light, VS Code Light+, Catppuccin Latte.
  - The built-in schemes are adapted from these well-known palettes, with the contrast between text and every background checked (at least 4.5:1 for body text), so they are not necessarily identical to the originals.
- **Custom schemes**: "Edit current scheme…" in the settings panel opens an editor for the scheme selected for the **appearance currently in use** (dark or light; switch the appearance first to edit the other one). Every color (background, text, buttons and lines, accents and status, syntax coloring; 25 in all) has a swatch, changes preview immediately, and "Revert" discards them. Built-in schemes cannot be overwritten: use "Save as…" to give it a name and store it as a custom scheme (it is also selected for that appearance); a custom scheme can be saved in place.
  Custom schemes live in the `themes` folder inside the settings folder (Windows `%APPDATA%\feditor\themes\`; the "Scheme folder" button in the settings panel opens it), one `*.conf` file per scheme, and **they are re-read whenever the settings panel opens**, so you can also write one in a text editor or drop in one from someone else. The format is `key = value` (whole lines starting with `#` are comments, and a color may be followed by `  # note`):

  | Key | Meaning |
  |---|---|
  | `name` | Scheme name (required). A custom scheme overrides a built-in one with the same name |
  | `mode` | `dark` or `light`: which list the scheme appears in (default `dark`) |
  | `base` | Optional: colors you leave out are inherited from this existing scheme (built-in or another custom one); without `base` the default scheme of that mode fills the gaps |
  | `editor_bg`, `panel_bg`, `popup_bg`, `faint_bg`, `code_bg` | Backgrounds: editor, panels, popups, faint (striping), code |
  | `text`, `text_strong`, `text_weak` | Normal, strong, and secondary text |
  | `button`, `button_hover`, `button_active`, `selection`, `line`, `line_hover` | Buttons (normal, hovered, pressed), selection, lines (normal, hovered) |
  | `accent`, `cursor`, `link`, `warning`, `error` | Accent, caret, hyperlinks, warnings (also search-result highlights), errors |
  | `syn_comment`, `syn_string`, `syn_number`, `syn_keyword`, `syn_type`, `syn_function` | Syntax coloring: comments, strings, numbers, keywords, types, functions |

  Write colors as `#RRGGBB` (or `#RGB`). Broken files (missing `name`, unknown keys, bad colors, an unknown `base`) are skipped and the reason is shown in the output panel. See `examples/themes/midnight.conf`. If the scheme named in the settings no longer exists (for example the file was deleted), the default scheme of that mode is used.
- **Font**: lists **monospace fonts only**. A font counts as monospace when `i`, `m`, `W`, `.`, etc. all have the same advance width; the monospace flag inside font files is not used because it is often wrong. The default is the **operating system's default monospace font** (queried from the OS, not hard-coded; Consolas on Windows).
  Some old fonts (MingLiU, MS Gothic, Lucida Console, ...) have embedded *color bitmap* Latin glyphs, which egui does not tint with the text color and therefore draws invisibly, so such fonts are left out of the list; if the settings file names one, the editor falls back to the system default and the panel shows the font that is actually in effect.
  Building the list means opening every font file, so it is scanned in the background; until it finishes the panel shows "Finding monospace fonts on the system…". Chinese glyphs come from Microsoft JhengHei. Fonts differ in ascent/descent proportions, which makes egui draw Latin letters and Chinese characters on different baselines by default, so at startup and whenever the font changes the gap is measured with egui's real layout and compensated; tests verify that every font in the list puts both on the same baseline.
- **Font size**: the size field in the panel and the shortcuts share one setting, so they are always in sync. Shortcuts: **Ctrl +/−** to enlarge/shrink, **Ctrl+mouse wheel**, and **Ctrl+0** to reset (range 8–48 pt). This replaces egui's built-in "Ctrl +/− zooms the whole UI".
- **Display options**: five toggle buttons in the settings panel (icon plus tooltip): line numbers, highlight the current line, show whitespace (dots for spaces, arrows for tabs, boxes for full-width spaces, ↵ at line ends), syntax coloring, and word wrap.
- **Settings file** location: Windows `%APPDATA%\feditor\settings.conf`, macOS `~/Library/Application Support/feditor/settings.conf`, Linux `$XDG_CONFIG_HOME/feditor/settings.conf` (`~/.config/feditor/settings.conf` when unset). It is plain `key = value`: `theme` (`system`/`light`/`dark`), `font_family` (no line means the system default), `font_size`, and `show_line_numbers` / `highlight_line` / `show_whitespace` / `syntax` / `word_wrap` (`true`/`false`). A missing or corrupt file, or an unreasonable value, falls back to the default and never prevents startup. While the font size is changing continuously the file is written only after 0.4 s of inactivity, and always on close.
- The unsaved marker is in the title bar (the "●" after the file name); it is not repeated in the status bar.

### Editor area

The editor area is **custom-drawn** (`ui/src/editor/`) instead of using egui's built-in text box: the text lives in a rope buffer and each frame lays out and paints only the visible lines, so the cost depends on the window height, not the file size (verified with a 1,000,000-line document).

- The line-number gutter is fixed on the left; the current line has a subtle background; tabs are displayed expanded to 4-column stops (the file still contains tab characters).
- **Column-grid layout**: every character occupies exactly its column width × the half-width glyph width, so Chinese and full-width characters are always exactly two columns, whatever the advance width of the CJK font is, and Latin text on neighboring lines stays aligned (a monospace Latin font alone cannot guarantee this because the Chinese glyphs come from a different font). Runs of ASCII are laid out together; every other character is laid out individually and centered in its own cell.
- The caret moves by user-perceived character: combining marks and emoji (including ZWJ sequences) count as one, and full-width CJK characters take two columns. Moving up/down remembers the original column.
- Keys: arrows, Home/End (first press goes to the first non-blank character, the second to the line start), Ctrl+Home/End, Page Up/Down, Ctrl+arrows (by word), Shift with any of these (select), Ctrl+A, Ctrl+Z, Ctrl+Y (or Ctrl+Shift+Z), Ctrl+X/C/V, Tab/Shift+Tab. Enter keeps the previous line's indentation. Mouse: click, drag to select, double-click for a word, triple-click for a line, Shift+click to extend the selection.
- **Line operations**: Alt+↑/↓ move lines, Shift+Alt+↑/↓ duplicate lines, Ctrl+Shift+K delete lines, Ctrl+/ toggle comment (the symbol depends on the extension; if every line is already commented they are uncommented), Ctrl+] / Ctrl+[ indent/unindent; with a multi-line selection Tab/Shift+Tab indent the whole block. Each is one undo step and the selection moves with the text.
- **Multiple carets and rectangular selection**: Ctrl+click adds (or removes) a caret, Ctrl+Alt+↑/↓ adds a caret on the line above/below, Ctrl+D selects the word under the caret and then adds the next identical text each time, **Alt+drag** makes a rectangular selection (by display column; Chinese counts as two), and Esc returns to a single caret. Typing, deleting, Enter, paste, movement, copy, and cut apply to every caret, and one action is one undo step; when pasting, a clipboard with exactly as many lines as carets goes one line per caret. External changes such as undo, replace, and macros discard the extra carets.
- **Word wrap**: a toggle in the settings panel. Lines wrap by display column (English words are not split, wrapping is allowed between CJK characters, and kinsoku punctuation never starts a row); line numbers appear only on the first row of each line, and Up/Down, Home/End, and the mouse all work on screen rows; at a wrap point the caret can sit at the end of the upper row or the start of the lower one (clicking at the end of a row or pressing End keeps it on the upper row). Files over 100,000 lines do not offer word wrap (every edit would have to recompute the row count of the whole document).
- **Syntax coloring**: by file extension (Rust, C/C++, Java/C#, JavaScript/TypeScript, Python, Go, Shell, SQL, CSS, Lua, JSON, TOML/INI/YAML, HTML/XML, Markdown), coloring comments, strings, numbers, keywords, types, and function names; block comments, triple-quoted strings, and template strings can span lines. Embedded expressions inside strings are colored as code too: `${…}` in JavaScript/TypeScript template strings, `{…}` in Python f-strings (`{{` is an escape and does not count), and `${…}` in Shell double-quoted strings; strings and nested template strings inside an expression are matched, and an expression may span lines. Triple-quoted f-strings are not handled yet. It is a lightweight tokenizer, not a full parser. Files over 20,000 lines estimate the cross-line state by looking back only 1000 lines.
- **Choosing the language**: the status bar (left of the encoding) has a "language" dropdown listing the built-in and custom languages, plus "Auto" and "Plain text". The choice belongs to this tab only, is not written to the file, and survives a reload. "Auto" works like this: with a file name it uses the extension; for an **unsaved document without a file name** it guesses from the beginning of the content: a `#!` interpreter line (Python, Node, Lua, various shells), `<?php`, `<!DOCTYPE html` / `<html` / `<?xml` / `<svg`, or JSON starting with `{` / `[`; if it cannot tell, nothing is colored (for example a pasted Rust snippet with no telltale start, which you pick from the list yourself). When you pick a language manually, the comment marker used by Ctrl+/ follows it.
- **Custom languages**: put `*.conf` files in the `syntax` folder under the config directory (Windows `%APPDATA%\feditor\syntax\`), one language per file; they take effect after a restart and take priority over the built-in languages (so you can override a built-in keyword list). Broken files are skipped and the reason is shown in the output panel. See `examples/syntax/php.conf`. The format is `key = value`; whole lines starting with `#` are comments:

  | Key | Meaning |
  |---|---|
  | `name` | Language name (required) |
  | `extensions` | File extensions separated by commas or spaces; a full file name such as `makefile` also works (required) |
  | `line_comment` | Line comment markers, possibly several, e.g. `//, #` |
  | `block_comment` | Start and end of a block comment, e.g. `/*, */` |
  | `quotes` | String quote characters, e.g. `", '` (default: `"` and `'`) |
  | `keywords` | Keywords separated by commas or spaces; may appear on several lines (merged) |
  | `capital_types` | `true` colors identifiers starting with a capital letter as types |
  | `case_insensitive` | `true` makes keywords case-insensitive (SQL) |
  | `triple_quotes` / `backtick_strings` | Triple-quoted strings (Python) and backtick strings (JavaScript); they can span lines |
  | `interpolation` | Start and end of an embedded expression in a string, e.g. `${, }` (JavaScript), `#{, }` (Ruby), `{, }` (Python f-string) |
  | `interpolation_in` | Which strings contain embedded expressions, comma-separated: `backtick` (backtick strings), `double` (double-quoted strings), `fstring` (strings with an `f` or `$` prefix; `{{` is an escape). Default: `backtick, double` |
  | `mode` | `code` (default), `markup` (HTML/XML), `markdown`, `config` (`[section]` and `key =` in ini/toml) |
- **Undo** works per operation: consecutive typing and consecutive deleting are one step each; a single replace, replace-all, and a whole macro run are one step each.
- **Input method**: composition text is drawn at the caret with an underline and is **not in the buffer**, so composing never pollutes the undo history; the candidate window is placed at the composition. Arrow keys during composition belong to the IME (candidate selection), and clicking elsewhere in the editor ends the composition; clicking somewhere in the window that cannot take input (blank areas, buttons) keeps composing, and when another input box takes focus the composition text is written into the editor directly. The caret position is inferred from the IME's behavior (the built-in Windows Bopomofo IME only reports the highlighted clause, so the caret is derived from text changes and repeated events). If an IME misbehaves, start with the environment variable `FEDITOR_IME_LOG=<log file path>` and the editor appends the IME events with the caret and composition state to that file (nothing is written when it is not set).
- Current limits: a single extremely long line (hundreds of thousands of characters) gets slow; vertical scroll positions beyond roughly 1,000,000 lines are limited by f32 precision (a measured 4,000,000-line, 346 MB file opens in about 4.5 s and uses about 840 MB; it is usable, but the spacing between rows visibly jitters); and while a Windows IME has focus, some Ctrl+Alt+letter combinations are swallowed by the IME and never reach the editor.
### Find & Replace

- Three options: regular expression, case-sensitive (off by default), and whole word. Search is live while you type and shows "n / total"; when nothing is left in one direction it wraps around and shows "wrapped".
- Hovering the "regular expression" button shows a syntax cheat sheet and a link to the full documentation (<https://docs.rs/regex/latest/regex/#syntax>); the tooltip stays open when the pointer moves onto it, so the link can be clicked.
- **Regular expressions** use the Rust `regex` syntax (guaranteed linear time, so a badly written pattern cannot hang the editor); `^` and `$` work per line. In the replacement, `$1` and `${name}` insert capture groups, `\n` and `\t` mean newline and tab, and a literal `$` is written `$$`.
- **Whole word**: the match must not be directly adjacent to a letter, digit, or underscore. Chinese has no spaces between words, so adjacent Han characters block each other; this option is usually not useful for Chinese words.
- Changes made by replace and by macros can be undone with Ctrl+Z and redone with Ctrl+Y: a single replace, replace-all, and a whole macro run are one undo step each.
- For files over 8 MiB the total count is not computed (shown as "—") so that typing does not rescan a huge file; the count is capped at 9999.
- The found match becomes the editor selection (typing then replaces it); "next" continues from the end of the selection and "previous" from its start, so clicking somewhere yourself and pressing F3 continues from that caret. After closing the find bar with Esc the caret returns to the editor.

### Find in Files

Ctrl+Shift+F opens the panel (pre-filled with the selected text and the folder of the current file): search text, three options (case-sensitive, whole word, regular expression), the folder, and a file-name filter (for example `*.rs;*.toml`). The search runs in the background and can be stopped at any time; clicking a result opens the file (or switches to it if it is already open) and selects the match.

- Files are decoded with automatic encoding detection (Big5, UTF-16, ... are searchable); binary files containing NUL, files over 20 MiB, and `.git` / `target` / `node_modules` folders are skipped.
- Only the first match per line is listed; at most 5000 results, after which you are asked to narrow the scope.

The application icon (a text document with a pencil) is generated by `ui/assets/make_icon.py`: `uv run ui/assets/make_icon.py`.

- **Encoding detection**: BOM → strict UTF-8 → `chardetng` (biased toward Traditional Chinese Big5).
- **Line endings**: the original CRLF/LF/CR is remembered and restored on save; mixed endings produce a warning and are unified.
- **Save safety**: writes to a temp file, then renames; characters the target encoding cannot represent make the save **fail** instead of being silently replaced; if bytes failed to decode on load, overwriting the original file is blocked.

## Design decisions

- **Rust core**: performance, single-binary distribution, cross-platform builds.
- **Custom-rendered UI with egui**: Traditional Chinese IME has been verified on egui (see `proto/IME-TEST.en.md`). The editor area does not use egui's `TextEdit`; it is custom-drawn (rope buffer plus painting only the visible lines) because `TextEdit` is not fast enough for large files and gives no fine control over undo and IME composition.
- **JavaScript first for macros** (`rquickjs`): lightweight, statically linkable, and easy to cap in time and memory. It sits behind the `ScriptEngine` abstraction, so adding Python or another language later does not touch the core.
- **File size target**: up to a few hundred MB. Buffer and rendering will be optimized later if larger files are needed.

## Macros

### Library, recording, and shortcuts

- Your macros live in the `macros` folder inside the settings folder (Windows `%APPDATA%\feditor\macros`), one `.js` file per macro. The toolbar macro button opens the macro panel: ▶ runs, the keyboard icon sets a shortcut, the pencil edits the source in a new tab, the I-beam icon renames (double-clicking the name works too), the trash can deletes; at the bottom you can run any file or open the macro folder.
- Comments at the top of a file can carry tags: `// @name Display name`, `// @shortcut Ctrl+Alt+U`. When you set a shortcut by pressing the combination, the program rewrites the `@shortcut` in the header (Esc cancels, Delete clears). It needs Ctrl or Alt (function keys F1–F12 excepted); duplicates of built-in shortcuts or of another macro are rejected. Running a macro with its shortcut (for example Alt+M) does not type that letter. Renaming a macro rewrites the `@name` in the header; the file name stays and other tags such as the shortcut are kept.
- **Recording**: press the record button on the toolbar to start; from then on **keys and typing** (typing, text committed by an IME, paste, arrows, deletion, line operations, ...) are recorded; press again to stop, and a dialog asks you to **name the macro** (the default is "錄製的巨集", numbered automatically if that name is taken; Enter or "Save" stores it in the `macros` folder with the name written as-is in the header's `@name` and the file name stripped of unusable characters; "Discard" does not save it, and Esc does not close the dialog, so a recording cannot be lost by accident). After saving, it becomes what F5 reruns. Mouse actions are not recorded. The result is editable JavaScript; consecutive typing is merged into one statement and repeated commands are written with a count:

```js
// 由錄製產生
editor.run("docEnd");
editor.paste(" world");
editor.run("selectRight", 2);
```

- A macro run is **one undo step**; if it fails, both the text and the selection are restored.

### Macro API (JavaScript)

| API | Description |
|---|---|
| `editor.text()` | Full text |
| `editor.length()` | Total number of characters |
| `editor.lineCount()` | Number of lines |
| `editor.line(i)` | Line `i` (including its newline); `undefined` if out of range |
| `editor.slice(start, end)` | Text of `[start, end)` |
| `editor.lineStart(i)` / `editor.lineOf(pos)` | Start of line `i` / which line position `pos` is on |
| `editor.insert(at, text)` | Insert at position `at` (the selection moves with the text) |
| `editor.remove(start, end)` | Delete `[start, end)` |
| `editor.replace(start, end, text)` | Replace `[start, end)` |
| `editor.selection()` | `[anchor, head]` |
| `editor.setSelection(anchor, head)` | Set the selection |
| `editor.selectedText()` | The selected text |
| `editor.path()` | Full path of the current file; `undefined` if never saved |
| `editor.run(name, count = 1)` | Run an editor command, same as the key; names below |
| `editor.type(text)` / `editor.paste(text)` | Type / paste like a user (replaces the selection, caret follows) |
| `editor.find(query, from = 0, opts)` | `[start, end]` if found, otherwise `undefined`; `opts`: `regex`, `caseSensitive`, `wholeWord`, `backward` |
| `editor.alert(msg)` / `editor.confirm(msg)` | Dialogs; `confirm` returns whether "Yes" was pressed |
| `console.log(...)` | Output |

Editor commands (`editor.run`): `left` `right` `up` `down` `wordLeft` `wordRight` `pageUp` `pageDown` `home` `end` `docStart` `docEnd`, their `select`-prefixed variants (`selectLeft` ... `selectDocEnd`, `selectAll`); `backspace` `delete` `deleteWordLeft` `deleteWordRight` `enter` `tab` `shiftTab` `undo` `redo`; `moveLineUp` `moveLineDown` `duplicateLine` `deleteLine` `toggleComment` `indent` `unindent`; `addCaretUp` `addCaretDown` `selectNextOccurrence` `clearCarets`. An unknown name throws an error.

- Positions are in **Unicode code points**, not JavaScript string UTF-16 units. `"😀".length` is 2, but `editor.length()` counts it as 1.
- Positions must be non-negative integers; otherwise a `RangeError` is thrown.
- Every run uses a fresh environment; macros do not share globals.
- Default limits are 5 seconds of run time and 64 MiB of memory; infinite loops and memory bombs are aborted.

## Known issues

- On the development machine, the wgpu **Vulkan backend crashes** (`0xC0000005`). On Windows the default is switched to DX12; override it with the `WGPU_BACKEND` environment variable.
- IME has only been verified on Windows + DX12; macOS and Linux are untested.
- The GitHub Actions configuration for cross-platform builds and releases is in `.github/workflows/` (tests and builds run on Windows, macOS, and Linux); the system packages Linux needs are listed in `ci.yml` and may need adjusting for other distributions.
- The macOS `FEditor.app` is not notarized by Apple and has not been tried on a real Mac yet; "Open With" on a file in Finder does not open it (only command-line arguments are supported).

## Download

Get the archive for your platform from [Releases](https://github.com/codemee/feditor/releases):

- **Windows**: extract and run `feditor.exe`.
- **macOS** (Apple Silicon): download `feditor-macos.dmg`, open it, and drag `FEditor.app` onto the Applications shortcut next to it. It is not notarized, so the first time, right-click it in Finder and choose "Open", or run `xattr -dr com.apple.quarantine FEditor.app`.
- **Linux**: extract and run `./feditor`.

See the [changelog](CHANGELOG.en.md) for what changed in each version.

## Development

```powershell
cargo test --workspace --exclude feditor       # unit tests (without the UI)
cargo run --release -p feditor                 # launch the editor
```
