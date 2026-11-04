**English** | [繁體中文](CHANGELOG.md)

# Changelog

## Unreleased

### Added
- **Terminal: clickable URLs**: hold Ctrl and hover a URL to underline it, and click to open it in the default browser (including URLs that wrap over several rows).
- **Links to local files in the preview**: relative paths (relative to the document's folder), absolute paths and file:// URLs open in a new editor tab, and every other link (URLs, e-mail addresses, ...) goes to the default browser.
- **Search inside the terminal**: press Ctrl+F (as in the editor; a full-screen program still gets it while it runs) while a terminal has the focus to open a search bar; it searches live, ignoring case and including the history scrolled out of view, highlights the matches, moves between them with Enter / Shift+Enter, and closes with Esc.

### Fixed
- **The toolbar's terminal button no longer looks like a toggle**: it is an action button that adds a terminal, so it does not switch to the active style after being pressed and keeps the ordinary tool style.
- **Terminal: pressing Ctrl (or Shift, Alt) alone sent Ctrl+C**: a modifier key by itself is also a key event, and the first letter of its name was mistaken for a Ctrl+letter (ControlLeft → Ctrl+C). Now only real letter keys are turned into control characters.

## [0.2.1] - 2026-11-03

### Changed
- **The Windows release is now a single `feditor-windows-x64.exe`**: no zip any more, just download and run it. The C runtime is linked statically, so the Visual C++ redistributable does not need to be installed.
- The ends of the status bar and the output panel line up with the outer edges of the pane cards instead of sticking out past the rounded frames.

### Added
- **Mouse reporting in the terminal**: when a program (vim, htop and the like) asks for mouse reporting with sequences such as `ESC[?1000h`, presses, releases, drags and the wheel are sent to it; holding Shift still lets you select text.
- **Terminal settings** (a new "Terminal" group in the settings panel): the font size (Ctrl +/- adjusts it when a terminal is clicked), the minimum contrast between text and background (previously fixed at 3:1, now adjustable or off), and the shell new terminals run (arguments allowed).

## [0.2.0] - 2026-11-03

### Changed
- **Documentation reorganized**: the README now holds only what users need to know (download, operations, how each feature is used); what developers and advanced users need moved to `docs/`: [Macros](docs/macros.en.md), [Preview plugins](docs/preview-plugins.en.md), [Customization and settings file](docs/customization.en.md), [How the editor works](docs/internals.en.md), [Development guide](docs/development.en.md) (each with a Chinese version).
- **External change detection now uses a file-system watcher** (`notify`): instead of polling only the active file of the focused pane once a second, the folders of all open files are watched, covering every tab in every pane; a change is handled as soon as it settles (short debounce), with a re-check every 5 seconds and when the window regains focus as a fallback. A background tab without unsaved changes is reloaded automatically; otherwise its tab is marked "(changed on disk)" and you are asked when you switch to it.

### Added
- **Terminal** (experimental): the `>_` toolbar button or `` Ctrl+Shift+` `` adds a terminal running the system's default shell. A terminal is a kind of tab, labeled only with a name ("Terminal 1", "Terminal 2", ... by default; double-click the tab to name it yourself): you can open many, drag a tab into any pane's tab bar or to a pane's side to split it into a new pane, and the shell keeps running while it is not shown; `` Ctrl+` `` cycles focus through the terminals. Colors, full-screen programs, IME, copy/paste and wheel scrolling are supported. Terminal tabs are saved in the session: after a restart they come back in the same order with the same names (each is a fresh shell).
- **Minimum-contrast guard in the terminal**: text whose contrast with its background is below 3:1 (black on black, pale yellow on a light background, ...) is adjusted toward something readable while keeping its hue; text with enough contrast is untouched, and graphic glyphs (blocks, box drawing, Powerline symbols) are never adjusted.
- **Terminal colors belong to the color scheme**: schemes gain 16 terminal colors (black, red, green, yellow, blue, magenta, cyan, white and their bright variants; the config keys are `term_*`). Each built-in dark and light scheme has its own set (Dracula, Nord, Catppuccin, Gruvbox, etc. use the palettes commonly used with them), custom schemes can adjust them, and unspecified ones are inherited from the base. The default text and background of the terminal also follow the scheme.
- **Deleted or moved files**: the tab is marked "(deleted)" and a message is shown once in the output panel; the tab keeps its content, saving recreates the file, and it returns to normal if the file reappears.

## [0.1.5] - 2026-11-03

### Added
- **Preview**: the "eye" button on the toolbar (Ctrl+Shift+P) opens a preview pane (there is only one in the whole program, shared by all documents), following the focused document, its tab, and its text. Built in: Markdown, CSV/TSV (tables), JSON/TOML/YAML (collapsible trees, with the line and column of errors), and SVG.
- **Preview plugins**: `previews/*.js` in the config folder (the settings panel has a "Preview folder" button). Plugins run in the macro sandbox, declare `@extensions` and `@output` (`markdown`, `table`, `tree`, `svg`, `text`), and define `preview(text, info)`. Examples included: a log-file table, an INI tree, and a to-do list.
- **The pane layout is remembered**: after closing and reopening, every pane (its tabs, active tab, caret, divider ratios, focus, and preview panes) comes back; files that no longer exist are skipped, and a pane with no file left to open does not reappear.
- **Markdown preview understands the HTML `<kbd>`, `<picture>`, `<img>`, and `<br>`**: `<kbd>` is drawn as a key; `<picture>` picks the image for the dark or light theme (like GitHub). Other HTML is still not rendered.
- **The preview pane can take the input focus**: click it and the zoom shortcuts (Ctrl + / −, Ctrl + wheel, Ctrl + 0) adjust the preview font size, independent of the editor font size (the "Preview" group of the settings panel gains a "Preview font size"); click an editor pane or switch tabs and the focus returns to the editor.
- **The preview pane can be dragged anywhere in the layout like any other pane**: drag its title to the left, right, or bottom of another pane; you can also drag a tab onto the left, right, or bottom of the preview pane to split the preview pane. The preview pane is no longer tied to one pane, so it stays when other panes are closed.
- **A "Choose a tab" dropdown in the tab bar** lists all the tabs of the pane; pick one to switch to it.

### Fixed
- Tables in the Markdown preview had very narrow columns, with text squeezed to one character per line or cut off: column widths now follow the content, and only when the table does not fit are the wider columns shrunk proportionally, with the text wrapping inside the cells.
- A big blank gap appeared above tables in the Markdown preview: a `<picture>` / `<img>` inside a table cell was treated as a standalone block and moved above the table, and `width` / `height` were ignored. Images can now appear inside table cells, paragraphs, and headings (inline) and honor `width` / `height` (for example 24-pixel toolbar icons).
- In Markdown preview tables, columns did not line up between rows when the images in one column had different widths: each row was laid out on its own while the column width came from an estimate, so an image wider than the estimate pushed the rest of that row out of place. Column widths are now estimated from the real width in the image file header (counted up to 240 pixels), images in cells are always scaled down to fit the column, and every cell sits at a fixed position computed from the column widths, so content that overflows can never push other columns around.
- Code blocks, inline code, and `<kbd>` in the Markdown preview were much bigger than the body text: the preview drew monospaced text at the editor font size, which went out of balance as soon as the editor font size was raised. The preview's monospaced text is now derived from the body font size and no longer depends on the editor font size.

### Changed
- **Documentation reorganized**: the README now holds only what users need to know (download, operations, how each feature is used); what developers and advanced users need moved to `docs/`: [Macros](docs/macros.en.md), [Preview plugins](docs/preview-plugins.en.md), [Customization and settings file](docs/customization.en.md), [How the editor works](docs/internals.en.md), [Development guide](docs/development.en.md) (each with a Chinese version).
- In the find bar, Tab now only switches between the search and replace fields (and expands the replace row if it is collapsed); the other buttons (options, previous / next, replace, close, ...) are mouse-only and are never reached by Tab.
- The separator line under the toolbar is gone (the border at the top of the panes already separates them), and the panes now sit right under the toolbar instead of leaving a big gap.
- When tabs do not fit, the thick scrollbar is gone; thin-line `<` and `>` arrows on both sides scroll the tabs instead (the mouse wheel works too), and switching tabs scrolls the bar to the active one.
- Dropping a **folder** into the window now shows the Open File dialog starting in that folder so you can pick which files to open (it used to be skipped with a warning).

## [0.1.4] - 2026-11-02

### Changed
- **Documentation reorganized**: the README now holds only what users need to know (download, operations, how each feature is used); what developers and advanced users need moved to `docs/`: [Macros](docs/macros.en.md), [Preview plugins](docs/preview-plugins.en.md), [Customization and settings file](docs/customization.en.md), [How the editor works](docs/internals.en.md), [Development guide](docs/development.en.md) (each with a Chinese version).
- **Tabs now look like workbook tabs**: the active tab has the same color as the editor below it, rounded top corners, joins the editor at its lower edge, and has a thin accent line at its bottom edge (like a workbook sheet tab); the other tabs have no fill, only dimmer text, separated by thin lines, with a faint background appearing on hover. The leftmost tab lines up with the editor's text area (right of the line-number gutter).

### Fixed
- After switching, adding, opening, or closing a tab, the keyboard focus returns to the editor so you can type right away (clicking a tab used to leave the focus on the tab).

## [0.1.3] - 2026-11-02

### Added
- **Split panes**: drag a tab to the left, right, or bottom edge of the editor area to split off a new pane in that direction (the area is previewed while dragging). Panes nest, and each has its own independent tab bar and a close-pane button (the pane's tabs move to the neighbouring pane). Panes are rounded cards with a gap between them (the focused one has an accent border). Dividers can be dragged, and a pane never gets smaller than 240×140 points.
- **Interface language**: a three-state button in the settings panel cycles "Follow system" (globe icon), "中文" (Chinese, a "中" icon), and "English" (an "En" icon), and the change applies immediately. With "Follow system", a Chinese system gets Chinese and everything else gets English.
- The status bar shows the caret position as "行 X，欄 Y" (line X, column Y, starting at 1). In the column number, full-width characters (CJK ideographs, kana, Hangul, full-width punctuation) count as 1 or 2 columns, chosen under "欄號" (column number) in the settings panel (default 1).

### Changed
- **Documentation reorganized**: the README now holds only what users need to know (download, operations, how each feature is used); what developers and advanced users need moved to `docs/`: [Macros](docs/macros.en.md), [Preview plugins](docs/preview-plugins.en.md), [Customization and settings file](docs/customization.en.md), [How the editor works](docs/internals.en.md), [Development guide](docs/development.en.md) (each with a Chinese version).
- The settings panel is split into four groups, "Interface", "Color schemes", "Editor", and "Status bar", with separators between them; every row has the same height and spacing.

### Fixed
- Chinese text in the interface (dropdowns, labels, buttons) sat lower than the Latin text next to it: the baseline gap between the interface font and the Chinese font is now measured and the Chinese text is lifted to match.
- In the settings panel, the text labels of the "Appearance" and "Show" rows were not vertically aligned with the icon buttons next to them.

## [0.1.2] - 2026-11-02

### Added
- **Color schemes**: light and dark each get their own scheme. Built in: 11 dark (EVA, Dracula, Monokai, Solarized Dark, One Dark, Nord, Gruvbox Dark, Tokyo Night, Catppuccin Mocha / Macchiato / Frappé) and 7 light (Default Light, Solarized Light, GitHub Light, One Light, Gruvbox Light, VS Code Light+, Catppuccin Latte).
- **Custom color schemes**: "Edit current scheme…" in the settings panel adjusts 25 colors with a live preview and saves to `themes\*.conf`; you can also write a scheme file yourself, which is re-read whenever the settings panel opens.
- **Syntax coloring**:
  - Pick the language in the status bar; documents without a file name are detected from their content (shebang, `<?php`, HTML/XML, JSON).
  - Define custom languages with `syntax\*.conf`.
  - Embedded expressions in strings (JavaScript template strings, Python f-strings, Shell) are colored as code.
- **Drag and drop to open**: drag files from the file manager into the window; several files open one tab each.
- **Double-click a tab to rename the file** (for a tab that was never saved, it opens "Save As").
- **Recording a macro now asks for a name** first; the macro panel can rename a macro (it rewrites the header's `@name`).
- **Collapsible output panel**, collapsed by default and showing only the latest message; it pops open temporarily for errors or macro output.
- BOM is now part of the encoding list as separate entries (UTF-8 BOM, UTF-16LE BOM, …); the separate BOM button is gone.
- Dialogs (save, reload, delete, rename) are now flat in-app dialogs instead of system ones.

### Fixed
- The Windows build no longer opens a black console window behind the editor.
- Running a macro with its shortcut (for example Alt+M) no longer types that letter.
- The "find in files" icon now draws the bottom edge of the folder.
- The tab's × is part of the tab and only appears on hover; tabs, the status bar, and dropdowns no longer draw an outline on hover, and the list scrollbar no longer overlaps the item backgrounds.

## [0.1.1] - 2026-11-01

Only the macOS packaging changed; the program itself is identical to 0.1.0.

- macOS now ships as a `.dmg`: open `feditor-macos.dmg` and drag `FEditor.app` onto the Applications shortcut (0.1.0 was a bare executable).
- `FEditor.app` has its own icon and `Info.plist` and is ad-hoc signed (not notarized by Apple; Apple Silicon only).
- CI checks on macOS that the `.app` and `.dmg` are packaged correctly.
- The README has a new "Download" section.

## [0.1.0] - 2026-11-01

The first public release.

- Editing: custom-drawn editor area (tested with 346 MB / 4 million lines), undo/redo, word wrap, line numbers, current-line highlight, whitespace display, line operations, multiple carets, rectangular selection.
- Tabs with session restore; detects and asks when another program changes a file.
- File formats: encoding auto-detection; save as UTF-8 (with or without BOM), Big5, GBK, Shift_JIS, EUC-KR, or UTF-16; LF/CRLF/CR line endings.
- Search: find/replace (regular expressions run in linear time), background "find in files".
- Syntax coloring: 14 language families, embedded expressions in strings, language picker in the status bar, content-based detection when there is no file name, custom languages via `syntax/*.conf`.
- JavaScript macros: QuickJS engine (with time and memory limits), recording, macro panel, editor API.
- Interface: icon toolbar, light/dark themes (EVA palette), monospace-only font list, input method support (verified on Windows).

[0.2.1]: https://github.com/codemee/feditor/releases/tag/v0.2.1
[0.2.0]: https://github.com/codemee/feditor/releases/tag/v0.2.0
[0.1.5]: https://github.com/codemee/feditor/releases/tag/v0.1.5
[0.1.4]: https://github.com/codemee/feditor/releases/tag/v0.1.4
[0.1.3]: https://github.com/codemee/feditor/releases/tag/v0.1.3
[0.1.2]: https://github.com/codemee/feditor/releases/tag/v0.1.2
[0.1.1]: https://github.com/codemee/feditor/releases/tag/v0.1.1
[0.1.0]: https://github.com/codemee/feditor/releases/tag/v0.1.0
