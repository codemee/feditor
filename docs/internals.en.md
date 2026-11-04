**English** | [繁體中文](internals.md)

[← Back to the README](../README.en.md)

# How the editor works

This document records the design and implementation details of the editor, for people who want to understand or change the code. For how to use it, see the [README](../README.en.md).

## Editor area

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

## Find & Replace in detail

### Find & Replace

- **The keyboard only serves the two input fields**: pressing Tab (or Shift+Tab) in the search or replace field only switches between those two fields (and expands the replace row if it is collapsed); all the other buttons (options, previous / next, replace, close, ...) are mouse-only by default and are never reached by Tab.
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


## Preview

Some text files can be shown in a different form. The **eye** button on the toolbar (Ctrl+Shift+P) opens a **preview pane** to the right of the current pane, showing a preview of the **focused document**; it follows pane switches, tab switches, and edits (it re-renders 0.3 s after you stop typing, in the background, so the UI never freezes). Built in:

| File | What you see |
|---|---|
| Markdown (`.md`, `.markdown`) | Headings, bold / italic / strikethrough, inline code, links (URLs are clickable), lists (nested, with task checkboxes), quotes, code blocks, tables, rules, and local images (relative paths are resolved against the file's folder; nothing is fetched from the network). Of the raw HTML, only `<kbd>` (drawn as a key), `<picture>` / `<img>` (a `<picture>` picks the `<source media="(prefers-color-scheme: dark)">` or `light` image to match the current dark or light theme, like GitHub), and `<br>` are understood; other tags, `<script>`, `<style>`, and comments are skipped |
| CSV, TSV | A table, the first row as the header, resizable columns; only the first 5000 rows are shown |
| JSON, TOML, YAML | A collapsible tree with colors for strings, numbers, and booleans; on a syntax error, the line and column of the error |
| SVG | The drawing itself, updating as you edit |

- There is only **one** preview pane in the whole program, shared by all documents: it always shows the focused document, is not tied to any pane, and stays when other panes are closed. It is a rounded card like the others, with its own title bar and close button; it can be resized and **dragged anywhere in the layout**: drag its title to the left, right, or bottom of another pane; conversely, **drag a tab onto the preview pane**'s left, right, or bottom edge to split the preview pane (the tab becomes a new pane next to it), while dropping into the middle does nothing because the preview pane takes no tabs. A preview shows where it will land, and Esc cancels. **The preview pane can take the input focus too**: click it (its border turns to the accent color) and Ctrl + / −, Ctrl + wheel, and Ctrl + 0 adjust the preview font size (independent of the editor font size); click an editor pane or switch tabs and the focus returns to the editor. The file needs an extension (unsaved documents have no preview), files over 5 MB are not previewed, and a preview cannot be opened when the pane is too narrow to hold two minimum-size panes.

## Tabs, panes, sessions, and external changes

- New and Open create a new tab; if the current document is blank and unmodified it is reused, and a file that is already open is switched to instead of being opened twice. Each tab has its own caret, selection, and undo history. When closing the window, every unsaved tab is asked about in turn.
- **Double-click a tab to rename the file**: a dialog opens with only the base name selected (the extension is not overwritten); Enter or "Rename" actually renames the file on disk (it stays in the same folder). If the name is invalid (empty, contains `\ / : * ? " < > |`, or is a reserved Windows name), the folder already has a file with that name, or another tab already has that file open, the dialog explains why and nothing is overwritten. A tab with unsaved changes can be renamed too; its content and unsaved state are unchanged. For a tab that is not saved to a file yet, a double-click opens "Save As".
- **Split panes**: drag a tab to the **left, right, or bottom** edge of the editor area (a preview shows the area the new pane will take) and a new pane is split off in that direction with the tab moved into it. You can keep splitting, in any mix of side-by-side and stacked. Every pane has its own independent tab bar; drag a tab onto another pane's tab bar or the middle of its editor to move it into that pane. Panes are rounded cards with a gap between them; the gap is the divider and can be dragged to resize.
  - **Minimum pane size**: a pane never gets smaller than 240×140 points (a divider cannot be dragged past that); on a side where splitting would leave two panes below the minimum, no split zone is offered.
  - A tab is *moved*, not copied: the same document never appears in two panes; opening a file that is already open switches to the pane that holds it.
  - The pane with an accent line on top has the **focus** (click in a pane to change it); find, save, macros, and the status bar all act on its current tab.
  - Closing the last tab of a pane closes the pane too; you can also press the button at the right end of a pane's tab bar to close the pane, and its tabs (including unsaved changes) move to the neighbouring pane, so no document is closed.
  - Press Esc while dragging to cancel. There is no "top" split for now, and tabs cannot be reordered by dragging within the same pane.
- **Session**: when started without a file argument, the files that were open last time (and their caret positions) are reopened. It is stored in `session.conf` in the settings folder; blank never-saved documents are not recorded. **The pane layout is remembered too**: after closing and reopening, every pane (with its own tabs, active tab, caret positions, divider ratios, the focused pane, and preview panes) comes back; files that no longer exist are skipped, and a pane with no file left to open does not reappear.
- **External change detection**: the folders of all open files are watched with the system file watcher (`notify`), covering every tab in every pane (also re-checked when the window regains focus and every 5 seconds as a fallback). When another program modifies a file, a document without unsaved changes is reloaded automatically with the caret kept; otherwise the active tab asks whether to reload (choosing to keep your version means you are not asked again), and a background tab is marked "(changed on disk)" and asks when you switch to it. When a file is deleted or moved, its tab is marked "(deleted)" and a message appears in the output panel; the content is kept, and saving recreates the file.

## Terminal

- The `>_` toolbar button (`` Ctrl+Shift+` ``) adds a terminal running the system's default shell (`$SHELL` on Unix; on Windows PowerShell 7 first, then Windows PowerShell, then cmd). It starts in the folder of the current document. **A terminal is a kind of tab**, labeled only with a name, "Terminal 1", "Terminal 2", ... by default (closed numbers are reused), and double-clicking the tab lets you name it yourself (leave it empty to go back to the default): a new one is split below the focused pane in its own pane, and its tab can be dragged like any text tab: drop it in the middle of another pane to join that pane's tab bar, or at a pane's side to split it into a new pane; while another tab is shown, the shell keeps running in the background. You can open as many as you like. `` Ctrl+` `` cycles focus through the terminals and returns to where you were after the last one (and adds one if there are none yet). The tab closes when its shell exits (type `exit`) or you press its ×; the others are not affected.
- The pseudo-terminal and escape sequences (colors, cursor movement, full-screen programs) are handled by `alacritty_terminal`. **Terminal tabs are saved in the session**: after a restart each pane gets its files and terminal tabs back in their original order, the terminals keep their names, and the active tab is the same; but every terminal is a fresh shell (earlier output and running programs cannot be restored), starting in the folder of the pane's first file.
- Input: ordinary keys, arrows and function keys, Ctrl+letter (Ctrl+C copies when text is selected, otherwise interrupts), paste (bracketed paste mode), and IME; mouse drag selects and the wheel scrolls (in full-screen programs the wheel sends arrow keys).
- Colors come from the current color scheme (the 16 colors, default text and background). When text and its background have a contrast below 3:1 (for example dark gray text on a dark background, or pale yellow on a light one), the text is adjusted toward something readable; text that already has enough contrast is left alone, and graphic glyphs (blocks, box drawing, Powerline symbols) are never adjusted. `examples\terminal-colors.ps1` checks the colors and this guard (section 10).
- Not yet: mouse reporting to programs, clickable links, search, several terminals as tabs, an independent font size (it uses the editor's monospace font).

## Session file format

The session is stored in `session.conf` in the config folder, in a simple one-`key = value`-per-line format:

```text
focus = 2
layout = H0.5[P0,V0.4[P2,W3]]
preview = 3,2
pane = 0
active = 1
file = C:\project\a.txt<Tab>120
terminal = 1<Tab>2<Tab>build
pane = 2
active = 0
file = C:\project\b.md<Tab>0
```

- `layout` is the pane layout: `P` is an ordinary pane, `W` a preview pane, and `H` / `V` a left-right / top-bottom split (the number before the bracket is the share of the first child). `preview = preview pane,the pane it previews`.
- The `active`, `file` and `terminal` lines after each `pane = N` belong to that pane. `file` is `path<Tab>caret`; `terminal` is `tab position<Tab>number<Tab>name` (the name may be empty, meaning the default "Terminal N"); `active` is the position of the active tab, counting files and terminals together.
- Blank documents that were never saved are not recorded; an older file with only a flat list of `active` / `file` lines (no `pane` or `layout`) is treated as a single pane. On restore, files that no longer exist are skipped and panes with no tabs left are removed from the layout.

## How external changes are detected

- `notify` watches the folders (non-recursively) of all opened files. An event is treated only as a signal that "something happened" and is not analyzed to find which file, because paths in events are written differently on each platform (case, symbolic links, macOS's `/private`) and comparing them directly is unreliable. The signal is debounced for 250 ms, after which each document's modification time is compared (just a few `stat` calls).
- The files are also re-checked when the window regains focus and every 5 seconds, as a fallback for network drives or a failing watcher.
- A tab with no unsaved changes is reloaded directly; one with changes is marked `Stale` in `Document::disk_state` and asked about when you switch to it. A vanished file is marked `Missing` (reported once) and returns to normal when it reappears. The code is in `ui/src/watcher.rs` and `ui/src/document.rs`.

## How the terminal is built

- The pseudo-terminal (ConPTY on Windows, a pty on Unix), the background reader / writer thread and the parsing of escape sequences are all left to `alacritty_terminal`; we only choose the shell, resize it, translate keys into bytes (`ui/src/terminal.rs`) and draw the cells with egui (`ui/src/terminal_pane.rs`).
- The terminal lives in `Document::terminal`: it moves along with the `Document` between tabs and panes, so dragging, splitting and closing reuse the tab machinery directly; when the active tab is a terminal, the pane draws the terminal instead of the editor. While a tab is not in front, its shell keeps running in the background.
- Keyboard focus uses egui's focus mechanism: while the terminal's widget has the focus, the key, text, copy / paste and IME events of the frame are taken out of the input and sent to the shell, so neither the editor nor the global shortcuts see them (except Ctrl+, which toggles terminals).
- URLs and search: the screen rows that were wrapped (a `WRAPLINE` flag at the end of the row) form one "logical line", so both URLs and search strings can span a wrap. The URL under the mouse is only looked for while Ctrl is held (scanning just that logical line); search scans the whole terminal (history included) again whenever the query changes and every 0.4 s, because the output keeps changing. The pure-text part of both (URL recognition: schemes, trailing punctuation and unbalanced brackets; case-insensitive matching) is in `ui/src/terminal_text.rs`, and the drawing and scrolling in `ui/src/terminal_pane.rs`.
- Mouse: when a program asks for reports with `ESC[?1000h` / `1002h` / `1003h` (press and release / plus drags / plus all motion), and for the SGR format with `1006h`, mouse events are converted to cell coordinates and sent to it (`terminal::encode_mouse`); the legacy format cannot reach cells beyond 223. In reporting mode the wheel is sent as buttons 64 / 65. While Shift is held nothing is reported and text is selected as in an ordinary terminal. When no reporting is requested, dragging selects and the wheel scrolls the history (arrow keys in full-screen programs).
- The list of shells: on Windows it checks `pwsh.exe`, `powershell.exe`, `cmd.exe`, `wsl.exe` and `nu.exe` on the PATH and a few common Git Bash locations; elsewhere it uses `$SHELL` plus the shells that actually exist in `/etc/shells` (only the first of the same name is kept). Each terminal remembers its own shell when it is added (`TerminalPane::shell`), so changing the setting later does not affect the ones already open. The code is `terminal::shells_for` (a pure function with tests). The icon in front of each entry: on Windows the small icon of the executable is requested from the operating system with the Shell API (`SHGetFileInfoW`), converted to an RGBA texture and cached (`ui/src/shell_icons.rs`); when it cannot be read, or off Windows, the program's own terminal icon is drawn. The same icon is drawn on the tab label of a terminal (according to that terminal's own shell, the system default when it is empty), dimmed a little on tabs that are not active.
- Colors: the 16 colors come from the color scheme (`term_*`), the rest of the 256 colors are the standard ones, and true colors are drawn as given; text whose contrast with its background is below 3:1 is adjusted toward readable (keeping the hue), except blocks, box drawing, braille and Powerline symbols. `examples/terminal-colors.ps1` checks them.

## How interface languages are implemented

- The English table is in `core/src/i18n/en.rs`, keyed by the Chinese original: the code gets strings through `t("中文")` and `tf!("…{}…", args)`. A test scans the source to make sure every string has an English entry, the table has no unused entries, and the argument names and order match. So any new interface text must get its English at the same time, or the test fails.
- What is translated is the interface text, tooltips and messages; documents such as the README and the comments inside recorded macro files are not affected. "Follow system" depends on the operating system's language (Chinese on a Chinese system, English otherwise).
