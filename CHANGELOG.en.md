**English** | [繁體中文](CHANGELOG.md)

# Changelog

## Unreleased

### Changed
- **Find, replace and find in a folder are now one panel** (docked to one edge of the window and taking that whole edge, on the right by default; drag the title to move it to the top, bottom, left or right edge; the remaining space is what the other panes get): Ctrl+F opens it and searches the current document, Ctrl+Shift+F opens it with the scope set to a folder, and Ctrl+H also expands the replace field; the old find bar at the top is gone, and the search text, options and replacement exist only once. The two search buttons on the toolbar are merged into one.
- **The scope can be switched**: the second row has new "folder" and "include subfolders" state buttons. With the folder off, the current document (including unsaved content) is searched as you type and the result table follows the document; with it on, a folder is searched, by default the folder of the active tab's file (it follows tab switches until you edit or pick a folder yourself), and "include subfolders" is off by default.
- **Results are shown in two stacked tables**: files on top (file name, hit count) and the hits of the selected file below (line number and the text around the match, highlighted), and rows appear one by one while the search is still running; clicking a row opens the file and puts the caret on the match, scrolled into view; F3 / Shift+F3 move to the next / previous result.
- **Folder scope can replace too**: "replace this match" opens the file, replaces in its tab and moves on to the next result; "replace all" asks first, files open in tabs are changed in their tabs (undoable) and the others are written to disk directly (keeping encoding and line endings).

- **The search panel position is remembered**: the edge it is docked to, its width (left/right edge) or height (top/bottom edge), and the split between the two result tables are saved in the settings file (`search_dock`, `search_dock_width`, `search_dock_height`, `search_split`) and restored on the next start; whether the panel is open is not remembered, it always starts closed.

### Fixed
- The search button of the folder search panel did not match the other search options in size and position; it is now the same size and sits right after the options.

## [0.7.0] - 2026-11-05

### Added
- **Open by default (double-click opens in FEditor)**: the settings panel's Interface page gains an "Open by default" switch with category ticks (source code, config files, markup, plain text; only the first two are ticked by default; files that are run when double-clicked, such as `.bat`/`.cmd`/`.exe`, are not included). On Windows it is registered in the current user's registry: FEditor appears under "Open with" and "Default apps", extensions with no default program yet are set to FEditor, and the ones the user already chose a program for (which the system does not let programs change) are listed with the system's "Default apps" page opened for the user to confirm; on macOS it sets the Launch Services default handler (the original is backed up and restored when unticked); on Linux it writes `feditor.desktop` and `mimeapps.list`. The settings file gains `file_assoc` and `file_assoc_groups`.
- **Shell rc files without an extension are recognized**: `.zshrc`, `.bashrc`, `.zprofile`, `.bash_profile`, `.profile`, `.zshenv` and so on (with or without the leading dot) are detected as the Shell language, so they get syntax coloring and can be configured on the "File types" page. They are not treated as runnable scripts (no shebang and no clear extension).
- **Shell scripts can be run too, and the status bar shows their interpreter**: the interpreter comes from the shebang (`#!/bin/bash`, `#!/usr/bin/env zsh`...), then from the extension when there is no shebang (`.sh`/`.bash` bash, `.zsh` zsh, `.ps1`/`.psm1` PowerShell, `.bat`/`.cmd` cmd); it is looked up on `PATH`, checked in the background with its version shown, and can be changed by hand. On Windows, bash skips WSL's `bash.exe` in `system32` and `WindowsApps` and uses the real `bash.exe` inside the Git folder that `git-bash` (a scoop shim also works) belongs to. A file with an unclear extension and no shebang is not treated as a runnable script. Files without an extension but with a shebang (Python, shell) can be run too.
- **A non-existent file given on the command line**: `feditor new.py` with a file that does not exist yet opens a blank document whose tab is named after it and is marked unsaved; the file is created when you save (like vim). A running FEditor that receives the forwarded path does the same. Until the file is created it is never reported as deleted, and emptying the text again still counts as unsaved; an invalid file name (e.g. a Windows reserved name) is not opened, and a missing folder gets a warning that saving will fail.

### Fixed
- **cmd's version could not be read**: without a console window `cmd /c ver` prints a localized "[版本 10.0…]" (Big5), and the code only knew the English `[Version ` and read the output as UTF-8, so `.bat` files found no cmd. The output is now read as bytes and, without the English prefix, the first dotted number is used.
- **After opening a file the keyboard focus moves to the editor right away**: when the current document was a blank new one, the opened file reused it without a new tab, and in that case the focus stayed on the "Open" button so you had to click the editor before typing.

## [0.6.0] - 2026-11-04

### Added
- **Run the current file in the terminal**: a new "Run" toolbar button (a terminal prompt with a small play triangle; the dropdown beside it sets command-line arguments, remembered per file, settings key `run_args.<file path>`) runs the current file in a terminal tab with the detected (or manually chosen) runtime environment (Python for now). Unsaved changes are saved first; the same terminal is reused, and before running again the previous run is interrupted and the shell changes to the file's folder; the command is quoted for the shell (bash/zsh, PowerShell, cmd).
- **Automatic runtime environment (Python)**: opening a `.py` file shows the Python it found and its version in the status bar (virtual environments from the file's folder upwards, then `VIRTUAL_ENV`/`CONDA_PREFIX`, then the global interpreters on `PATH`; checked in the background and cached). Click it to see every candidate, pick one by hand (remembered per project) or search again. A new terminal automatically gets the project's virtual environment first on `PATH` and `VIRTUAL_ENV` set. The search rules are data so other languages can be added later. The settings file gains `runtime.<project folder>`.
- **Terminal command (macOS, Linux)**: a new "Terminal command" switch in the settings panel's Interface page adds or removes a `feditor` command in `~/.local/bin` (a short script that starts the app in the background). A dialog reminds you to put `~/.local/bin` in your `PATH` when it is added; only a script FEditor created is touched, a different file with the same name is never overwritten or deleted, and the path is refreshed on the next start if the app moved. The settings file gains `cli_command`.
- **"Don't save any" when closing the window**: when several documents are unsaved, the save prompt gains a "Don't save any" button that closes right away instead of asking one by one; it is not shown for the last remaining document.
- **A "File types" page in the settings panel**: pick a file type from a dropdown (types that already have their own settings are marked with ●) and set that one type's Tab width and the "Wrap style" for how Reformat breaks inside brackets (align with the first argument / break and indent); the type's indent rule is shown too. The per-file-type width list that used to be on the editing page moved here, and the editing page keeps only the default Tab width.
- **Auto indent**: Enter indents by file type: a line ending in `{ ( [` (or `:` in Python) indents the next line one more level, Enter between a pair of brackets opens three lines, and typing `} ) ]` on a blank indented line removes one level; brackets in strings and comments are ignored. The indent characters follow "Spaces instead of Tab" and the Tab width. A new "Auto indent" switch in the settings panel's editing page (on by default); the settings file gains `auto_indent`. Custom language files gain `indent = none|brace|python`.
- **Reformat**: a new "Reformat" toolbar button (`Alt+Shift+F` in the editor, `editor.run("reformat")` in macros). With a selection only the selected lines are processed, otherwise the whole document. It recomputes indentation from the bracket depth and trims trailing whitespace (lines inside multi-line strings and block comments are left alone); JSON is pretty-printed completely (key order kept); Python and other types with no depth rule only get trailing whitespace trimmed and indentation converted to the current setting.
- **Wrapping inside brackets in Reformat**: when the safe margin is enabled, Reformat joins needless line breaks inside round and square brackets back into one line and breaks what goes past the safe margin inside the brackets; how the next line lines up follows the language's wrap style (`align` lines up with the first argument, `hang` breaks and indents one level). Language files gain `wrap` and `trailing_comma`, and the settings file gains `wrap_style.<language name>` to override. Indentation also now adds only one level for a line that opens several brackets (the body of `foo(a, |x| {` is indented once).

## [0.5.1] - 2026-11-04

### Fixed
- **macOS "Open With" missed `.toml`, `.json`, `.rs` and similar files**: only `public.data` and a few other types were declared, but the system ignores `public.data` and does not know types for extensions like `.toml`. The app now declares the system text types (text, source code, JSON, XML, YAML, script) and adds an explicit extension list for files without a defined type. Images, archives and the like no longer list FEditor.

## [0.5.0] - 2026-11-04

### Added
- **macOS: Finder "Open With" list**: `FEditor.app` declares that it can open text-like files, so Finder's "Open With" menu lists it; it also handles the open-document events from "Open With" and from dropping files on the Dock icon (both when launching and when already running).

### Changed
- **Starting with paths reuses a running FEditor**: when started with paths from the File Explorer / Finder context menu (or the command line) while a FEditor is already running, the paths are handed to it and its window is brought to the front instead of opening a new window; otherwise it starts itself. Selecting several files in File Explorer also yields a single window. Starting without paths still opens a new window.

## [0.4.1] - 2026-11-04

### Fixed
- **Windows taskbar icon**: the taskbar button showed the generic blank-window icon. Once the window appears, the icon embedded in the exe is now explicitly set as both the large and the small window icon (sized for the screen scaling).

## [0.4.0] - 2026-11-04

### Added
- **File Explorer / Finder context menu integration**: a new "Add to the context menu" switch on the "Interface" page of the settings panel. Files gain "Open with FEditor" and folders (including the empty space inside) gain "Pick files to open with FEditor..." (the open dialog starting in that folder). On Windows it is written to the current user's registry without administrator rights; on macOS two Finder Quick Actions are generated. The entries are refreshed at startup when the program moves or the language changes. The command line now accepts several paths (files or folders) at once.

## [0.3.0] - 2026-11-04

### Added
- **Settings panel with tabs on the left**: Interface, Editor, Preview, Terminal and Status bar each get a page instead of one long scrolling list; the "Per file type" list no longer overlaps the items below it.
- **Indentation conversion tools**: two new toolbar buttons, "Tabs to spaces" and "Indentation spaces to Tabs", convert by the Tab width; with a selection only the selected lines are converted, otherwise the whole document, and spaces to Tabs only touches leading indentation. Macros gain the editor commands `tabsToSpaces` and `spacesToTabs`.
- **Tab width and spaces instead of Tabs**: the "Editor" group of the settings panel gains "Tab width" (4 by default), "Per file type" (give one file type its own width; types without one use the default) and "Use spaces" under "Typing Tab" (shared by every file type). With spaces on, the Tab key pads up to the next tab stop and multi-line indent adds a Tab width of spaces; Tabs already in a file are left alone. Each editor pane lays out by its own document's file type, and word wrap and the horizontal scroll range are recomputed when the width changes. The settings file gains `tab_width`, `tab_width.<language>` and `tab_spaces`.
- **Margin guide**: a new "Margin guide" in the "Editor" group of the settings panel: tick it to enable (on by default) and set a column (80 by default); from the right edge of that column on, the whole area to its right is tinted faintly, so on a line that runs past the guide the extra characters land in the tinted area. A full-width character counts as two columns, as on screen; the column is remembered while the guide is unticked.
- **A list of shells and a default terminal**: the shells available on this machine are found automatically (Windows: PowerShell 7, Windows PowerShell, cmd, Git Bash, WSL, Nushell; macOS / Linux: `$SHELL` and `/etc/shells`). A dropdown next to the toolbar's terminal button adds a terminal with the shell you pick (affecting only that terminal), each entry showing that shell's icon (on Windows taken from the executable itself; terminal tabs show it too); the arrow is as tall as the other toolbar buttons; the button itself uses the "Default terminal" in the "Terminal" group of the settings panel (choose from the list or write your own command).
- **Terminal: clickable URLs**: hold Ctrl and hover a URL to underline it, and click to open it in the default browser (including URLs that wrap over several rows).
- **Links to local files in the preview**: relative paths (relative to the document's folder), absolute paths and file:// URLs open in a new editor tab, and every other link (URLs, e-mail addresses, ...) goes to the default browser.
- **Search inside the terminal**: press Ctrl+F (as in the editor; a full-screen program still gets it while it runs) while a terminal has the focus to open a search bar; it searches live, ignoring case and including the history scrolled out of view, highlights the matches, moves between them with Enter / Shift+Enter, and closes with Esc.

### Changed
- **Number fields in the settings panel no longer carry their unit inside the input field**: the unit (pt, cols, :1) is written next to the field and the field holds only the number, so nobody wonders whether to type the unit too.

### Fixed
- **The toolbar's terminal button no longer looks like a toggle**: it is an action button that adds a terminal, so it does not switch to the active style after being pressed and keeps the ordinary tool style.
- **Terminal: pressing Ctrl (or Shift, Alt) alone sent Ctrl+C**: a modifier key by itself is also a key event, and the first letter of its name was mistaken for a Ctrl+letter (ControlLeft → Ctrl+C). Now only real letter keys are turned into control characters.

## [0.2.1] - 2026-11-03

### Changed
- **The Windows release is now a single `feditor-windows-x64.exe`**: no zip any more, just download and run it. The C runtime is linked statically, so the Visual C++ redistributable does not need to be installed.
- The ends of the status bar and the output panel line up with the outer edges of the pane cards instead of sticking out past the rounded frames.

### Added
- **Margin guide**: a new "Margin guide" in the "Editor" group of the settings panel: tick it to enable (on by default) and set a column (80 by default); from the right edge of that column on, the whole area to its right is tinted faintly, so on a line that runs past the guide the extra characters land in the tinted area. A full-width character counts as two columns, as on screen; the column is remembered while the guide is unticked.
- **Mouse reporting in the terminal**: when a program (vim, htop and the like) asks for mouse reporting with sequences such as `ESC[?1000h`, presses, releases, drags and the wheel are sent to it; holding Shift still lets you select text.
- **Terminal settings** (a new "Terminal" group in the settings panel): the font size (Ctrl +/- adjusts it when a terminal is clicked), the minimum contrast between text and background (previously fixed at 3:1, now adjustable or off), and the shell new terminals run (arguments allowed).

## [0.2.0] - 2026-11-03

### Changed
- **Documentation reorganized**: the README now holds only what users need to know (download, operations, how each feature is used); what developers and advanced users need moved to `docs/`: [Macros](docs/macros.en.md), [Preview plugins](docs/preview-plugins.en.md), [Customization and settings file](docs/customization.en.md), [How the editor works](docs/internals.en.md), [Development guide](docs/development.en.md) (each with a Chinese version).
- **External change detection now uses a file-system watcher** (`notify`): instead of polling only the active file of the focused pane once a second, the folders of all open files are watched, covering every tab in every pane; a change is handled as soon as it settles (short debounce), with a re-check every 5 seconds and when the window regains focus as a fallback. A background tab without unsaved changes is reloaded automatically; otherwise its tab is marked "(changed on disk)" and you are asked when you switch to it.

### Added
- **Margin guide**: a new "Margin guide" in the "Editor" group of the settings panel: tick it to enable (on by default) and set a column (80 by default); from the right edge of that column on, the whole area to its right is tinted faintly, so on a line that runs past the guide the extra characters land in the tinted area. A full-width character counts as two columns, as on screen; the column is remembered while the guide is unticked.
- **Terminal** (experimental): the `>_` toolbar button or `` Ctrl+Shift+` `` adds a terminal running the system's default shell. A terminal is a kind of tab, labeled only with a name ("Terminal 1", "Terminal 2", ... by default; double-click the tab to name it yourself): you can open many, drag a tab into any pane's tab bar or to a pane's side to split it into a new pane, and the shell keeps running while it is not shown; `` Ctrl+` `` cycles focus through the terminals. Colors, full-screen programs, IME, copy/paste and wheel scrolling are supported. Terminal tabs are saved in the session: after a restart they come back in the same order with the same names (each is a fresh shell).
- **Minimum-contrast guard in the terminal**: text whose contrast with its background is below 3:1 (black on black, pale yellow on a light background, ...) is adjusted toward something readable while keeping its hue; text with enough contrast is untouched, and graphic glyphs (blocks, box drawing, Powerline symbols) are never adjusted.
- **Terminal colors belong to the color scheme**: schemes gain 16 terminal colors (black, red, green, yellow, blue, magenta, cyan, white and their bright variants; the config keys are `term_*`). Each built-in dark and light scheme has its own set (Dracula, Nord, Catppuccin, Gruvbox, etc. use the palettes commonly used with them), custom schemes can adjust them, and unspecified ones are inherited from the base. The default text and background of the terminal also follow the scheme.
- **Deleted or moved files**: the tab is marked "(deleted)" and a message is shown once in the output panel; the tab keeps its content, saving recreates the file, and it returns to normal if the file reappears.

## [0.1.5] - 2026-11-03

### Added
- **Margin guide**: a new "Margin guide" in the "Editor" group of the settings panel: tick it to enable (on by default) and set a column (80 by default); from the right edge of that column on, the whole area to its right is tinted faintly, so on a line that runs past the guide the extra characters land in the tinted area. A full-width character counts as two columns, as on screen; the column is remembered while the guide is unticked.
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
- **Margin guide**: a new "Margin guide" in the "Editor" group of the settings panel: tick it to enable (on by default) and set a column (80 by default); from the right edge of that column on, the whole area to its right is tinted faintly, so on a line that runs past the guide the extra characters land in the tinted area. A full-width character counts as two columns, as on screen; the column is remembered while the guide is unticked.
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
- **Margin guide**: a new "Margin guide" in the "Editor" group of the settings panel: tick it to enable (on by default) and set a column (80 by default); from the right edge of that column on, the whole area to its right is tinted faintly, so on a line that runs past the guide the extra characters land in the tinted area. A full-width character counts as two columns, as on screen; the column is remembered while the guide is unticked.
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

[0.7.0]: https://github.com/codemee/feditor/releases/tag/v0.7.0
[0.6.0]: https://github.com/codemee/feditor/releases/tag/v0.6.0
[0.5.1]: https://github.com/codemee/feditor/releases/tag/v0.5.1
[0.5.0]: https://github.com/codemee/feditor/releases/tag/v0.5.0
[0.4.1]: https://github.com/codemee/feditor/releases/tag/v0.4.1
[0.4.0]: https://github.com/codemee/feditor/releases/tag/v0.4.0
[0.3.0]: https://github.com/codemee/feditor/releases/tag/v0.3.0
[0.2.1]: https://github.com/codemee/feditor/releases/tag/v0.2.1
[0.2.0]: https://github.com/codemee/feditor/releases/tag/v0.2.0
[0.1.5]: https://github.com/codemee/feditor/releases/tag/v0.1.5
[0.1.4]: https://github.com/codemee/feditor/releases/tag/v0.1.4
[0.1.3]: https://github.com/codemee/feditor/releases/tag/v0.1.3
[0.1.2]: https://github.com/codemee/feditor/releases/tag/v0.1.2
[0.1.1]: https://github.com/codemee/feditor/releases/tag/v0.1.1
[0.1.0]: https://github.com/codemee/feditor/releases/tag/v0.1.0
