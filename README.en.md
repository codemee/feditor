**English** | [繁體中文](README.md)

# FEditor

A cross-platform text editor with scripting macros, modeled after EmEditor and written in Rust.

- **Smooth with big files**: files of hundreds of MB and millions of lines open and edit directly.
- **Tabs and freely splittable panes**: tabs can be dragged around, and the built-in **preview** (Markdown, CSV, JSON, ...) and **terminal** can be placed in any pane just like tabs.
- **Friendly to CJK text**: Traditional Chinese IME support, aligned full-width characters, and automatic detection of Big5, GBK, Shift_JIS and other encodings.
- **Extensible**: JavaScript macros (recordable), preview plugins, custom color schemes and syntax highlighting.
- **Interface**: separate color schemes for light and dark (18 built in, including Dracula, Nord and Catppuccin), and the interface language can be Chinese or English.

## Download and install

Download the archive for your platform from [Releases](https://github.com/codemee/feditor/releases):

- **Windows**: download `feditor-windows-x64.exe` and run it (a single file: no installation and nothing to unzip).
- **macOS** (Apple Silicon): download `feditor-macos.dmg`, open it, and drag `FEditor.app` onto the "Applications" shortcut next to it. It is not notarized, so the first time you open it, right-click it in Finder and choose "Open", or run `xattr -dr com.apple.quarantine FEditor.app`.
- **Linux**: unpack and run `./feditor`.

You can also open files from the command line (`feditor path/to/file`) or drag files from the file manager into the window (several at once, one tab each). Dropping a **folder** shows the "Open file" dialog starting in that folder. See the [changelog](CHANGELOG.en.md) for what changed in each version.

## Basics

Operations are driven by **toolbar buttons**; there are no menus. Buttons show only icons, with the text description (and shortcut) in the tooltip.

| Button | Shortcut | Description |
|---|---|---|
| New / Open / Save / Save As | Ctrl+N / O / S / Shift+S | Basic file operations; unsaved changes are asked about before closing or switching documents |
| Find & Replace | Ctrl+F (replace Ctrl+H), F3 / Shift+F3 | Opens the find bar (see below) |
| Find in Files | Ctrl+Shift+F | Searches all text files under a folder |
| Terminal (`>_`) | Ctrl+Shift+` (new), Ctrl+` (cycle focus) | Adds a terminal tab (see below) |
| Preview (eye) | Ctrl+Shift+P | Toggles the preview pane (see below) |
| Macro panel / rerun | F5 (rerun) | Run, set a shortcut for, edit and delete macros; a failed macro rolls back all changes |
| Record macro | | Starts / stops recording keys and input and saves them as a macro |
| Settings (⋯, far right) | | Interface language, appearance, color scheme, font, font size, display options |
| Font size | Ctrl +/-, Ctrl+wheel, Ctrl+0 | Adjusts the font size; when the preview pane has the focus it adjusts the preview's font size |
| Tabs | Ctrl+W close, Ctrl+Tab / Ctrl+Shift+Tab switch | See below |

The left of the status bar shows the file path; the right shows this file's format settings: **language** (syntax highlighting), **encoding** (with BOM), **line ending**, and **reload** (re-read the file with a chosen encoding when auto-detection got it wrong), plus the caret position "Line X, Col Y" (whether a full-width character counts as 1 or 2 columns is a setting). Above the status bar is the **output panel** for messages about opened files, warnings, errors and macro output; it is collapsed to one line by default and pops open temporarily when an error appears.

## Tabs and panes

- Each opened file is a tab with its own caret, selection and undo history. Opening a file that is already open switches to it instead of duplicating it. Unsaved tabs have a "●" after their name.
- **Double-click a tab to rename the file**: only the base name is selected at first, and pressing Enter really renames the file on disk. An invalid name, an existing file with that name, or another tab already showing that file is explained in the dialog and nothing is overwritten. For a tab that was never saved, double-clicking opens "Save As".
- **Split panes**: drag a tab to the left, right or bottom of the editor area to split off a new pane in that direction (the area is previewed while dragging); drop it in the middle of another pane to add it to that pane's tab bar. Panes can be split again and again in any combination, the gaps can be dragged to resize, and a pane never gets smaller than 240×140 points. Press Esc while dragging to cancel.
- The pane with the accent-colored border has the **focus** (click to change it); find, save, macros and the status bar act on its current tab. Closing the last tab of a pane closes the pane; you can also press the button at the right end of the tab bar to close a pane, and its tabs (including unsaved changes) move to a neighboring pane.
- When tabs do not fit there is no scroll bar: thin `<` `>` arrows on both sides scroll (the mouse wheel works too), and the `v` "Choose a tab" dropdown on the right picks one directly.
- **Remembers where you left off**: after closing and reopening, all panes (their tabs, the active tab, carets, divider ratios, the preview pane and terminal tabs) are restored; files that no longer exist are skipped. Terminals come back as fresh shells.
- **External change detection**: when another program changes a file, it is reloaded automatically if you have no unsaved changes (the caret is kept); otherwise you are asked, and a background tab is marked "(changed on disk)" and asks when you switch to it. When a file is deleted or moved, its tab is marked "(deleted)", the content is kept, and saving recreates the file.

## Preview

The **eye** button on the toolbar opens a preview pane to the right of the current pane, showing a preview of the **document that currently has the focus**, updated as you type. There is only one preview pane in the whole program, shared by all documents.

| File | What the preview looks like |
|---|---|
| Markdown (`.md`) | Headings, bold / italic, links, lists, quotes, code blocks, tables, local images; of the HTML only `<kbd>`, `<picture>` / `<img>` (picking the image for the light or dark theme) and `<br>` are understood |
| CSV, TSV | A table with draggable column widths |
| JSON, TOML, YAML | A collapsible tree; on a syntax error, the line and column of the error |
| SVG | The drawing itself, updated as you edit |

Links in the preview: URLs, e-mail addresses and the like go to the default browser; a link to a **local file** (a relative path is relative to the document's folder) opens in a new editor tab, or switches to it if it is already open. The preview pane can be dragged anywhere in the layout, and clicking it lets Ctrl +/- adjust its own font size. To preview other file types, write a JavaScript **preview plugin** (see [Preview plugins](docs/preview-plugins.en.md)).

## Terminal

The `>_` toolbar button (Ctrl+Shift+`) adds a terminal running the system's default shell (`$SHELL` on Unix; on Windows PowerShell 7 first, then Windows PowerShell, then cmd).

- **A terminal is a kind of tab**: its label is "Terminal 1", "Terminal 2", ..., with the icon of the shell it runs in front (see below), and you can **double-click the label to name it yourself** (leave it empty to go back to the default). Drag the tab like a text tab: drop it in the middle of another pane to join that pane's tab bar, or at a pane's side to split it into a new pane. While another tab is shown, the shell keeps running in the background.
- **Default terminal and the list of shells**: next to the toolbar's terminal button is a dropdown arrow listing the shells found on this machine (Windows: PowerShell 7, Windows PowerShell, cmd, Git Bash, WSL, Nushell; macOS / Linux: the ones that exist among `$SHELL` and `/etc/shells`); each entry has that shell's icon in front (on Windows taken from the executable itself, the same as in File Explorer; elsewhere a generic terminal icon) to tell them apart. Pick one to add a terminal with it, affecting only that terminal. The button itself uses the **default terminal** (the "Terminal" group of the settings panel; choose from the same list or write your own command; unset means the system default). Terminals that are already open are not affected by later changes to the setting.
- Ctrl+` cycles the focus through the terminals and returns to where you were after the last one. The tab closes when the shell exits (type `exit`) or you press its ×.
- Supports colors, full-screen programs, IME, copy and paste (Ctrl+C copies when text is selected and interrupts otherwise), mouse-drag selection and wheel scrolling. Colors follow the current color scheme, and text whose contrast with its background is too low is adjusted until it is readable.
- When a program asks for mouse reporting (vim, htop and the like), mouse events are sent to it; holding Shift still lets you select text.
- **URLs**: hold Ctrl and move the mouse over a URL (`http`, `https`, `ftp`, `file`) and the whole URL is underlined and the pointer becomes a hand; click to open it in the default browser. URLs that wrap over several rows work too.
- **Search**: press Ctrl+F while a terminal has the focus (the same as in the editor) and a search bar appears in its top-right corner. While a full-screen program such as vim or less is running, Ctrl+F still goes to the program. It searches as you type, ignoring case and including the history scrolled out of view; matches are highlighted, Enter finds the earlier match, Shift+Enter the later one, and Esc closes it.

## Editing

- **A custom-drawn editor area** that draws only the visible lines, so it stays smooth however large the file is. Chinese and other full-width characters always take two columns so that letters and digits line up from row to row; the caret moves by "character" (combining marks and emoji count as one).
- **Common operations**: arrow keys, Home / End, Ctrl+arrows (by word), Shift to select, Ctrl+Z / Y undo and redo, Ctrl+X / C / V. Enter keeps the previous line's indentation; double-click selects a word and triple-click a line.
- **Line operations**: Alt+Up / Down move a line, Shift+Alt+Up / Down duplicate it, Ctrl+Shift+K deletes it, Ctrl+/ toggles a comment, Ctrl+] / Ctrl+[ indent and unindent.
- **Multiple carets and rectangular selection**: Ctrl+click adds a caret, Ctrl+D selects the next occurrence of the same text, Alt+drag selects a rectangle, Esc returns to a single caret.
- **Word wrap**, **syntax highlighting** (Rust, C/C++, Java/C#, JavaScript/TypeScript, Python, Go, Shell, SQL, CSS, Lua, JSON, TOML/INI/YAML, HTML/XML, Markdown and more; pick the language by hand in the status bar, or define your own), **line numbers** and **showing whitespace** can all be toggled in the settings panel.
- **Undo** works in units of "one operation": continuous typing and continuous deletion count as one step each; a single replace, replace all, and a whole macro are one step each.
- **IME**: composition text is drawn at the caret with an underline and does not pollute the undo history; the candidate window sits at the composition.
- **File format**: the encoding is detected automatically (BOM → UTF-8 → Big5, etc.), the original line ending (CRLF / LF / CR) is remembered and restored on save; saving writes a temporary file and renames it, a character the target encoding cannot represent makes the save **fail** instead of being silently replaced, and overwriting is refused when the file had bytes that could not be decoded.

## Find & Replace

- Tab in the search or replace box only moves between those two boxes; the other buttons are meant to be clicked with the mouse.
- Three options: regular expression, match case (off by default), whole word; it searches as you type and shows "n / total", and wraps around when nothing is found. Hovering over the "regular expression" button shows a syntax cheat sheet.
- Regular expressions use Rust `regex` syntax (guaranteed linear time, so a badly written pattern cannot hang the program); in the replacement `$1` and `${name}` insert groups, `\n` and `\t` stand for newline and tab, and a literal `$` is written `$$`.
- Changes made by replacing can be undone with Ctrl+Z: a single replace and replace all are one step each.
- **Find in Files** (Ctrl+Shift+F) searches all text files under a folder (with a file name filter such as `*.rs;*.toml`), runs in the background and can be stopped at any time; click a result to open it and select the match. It detects encodings automatically and skips binary files, files over 20 MiB, and `.git` / `target` / `node_modules`; at most 5000 results are listed.

## Appearance and settings

The "⋯" at the far right of the toolbar is the **settings** panel:

- **Interface language**: a three-state button (follow system / Chinese / English), effective immediately.
- **Appearance**: a three-state button (follow system / light / dark).
- **Color scheme**: choose one for light and one for dark **independently**. 11 dark schemes are built in (EVA is the default, plus Dracula, Monokai, Solarized Dark, One Dark, Nord, Gruvbox Dark, Tokyo Night and three Catppuccin flavors) and 7 light ones (Default Light, Solarized Light, GitHub Light, One Light, Gruvbox Light, VS Code Light+, Catppuccin Latte). "Edit current scheme..." lets you adjust all 41 colors (including the terminal's 16) and save the result as a custom scheme.
- **Font**: only monospace fonts are listed; the default is the operating system's default monospace font. Adjust the **font size** in the panel or with the shortcuts; the two stay in sync.
- **Display options**: line numbers, highlighting the current line, showing whitespace, syntax highlighting, word wrap.
- **File Explorer / Finder context menu**: tick "Add to the context menu" on the "Interface" page of the settings panel (labelled "File Explorer" on Windows and "Finder" on macOS) and the context menu of a file gains "Open with FEditor", while a folder (and the empty space inside a folder) gains "Pick files to open with FEditor...", which shows the "Open file" dialog starting in that folder. On Windows it is written to the current user's registry (no administrator rights needed), and the new Windows 11 context menu shows it under "Show more options"; on macOS two Finder services are generated in `~/Library/Services`, found in the "Services" submenu (to have them in the "Quick Actions" submenu too, tick them in System Settings → Privacy & Security → Extensions → Finder). Untick to remove. If the program moves or the interface language changes, the entries are refreshed on the next start. There is no such feature on Linux yet. You can also give several paths on the command line: `feditor a.txt b.txt folder`. When a FEditor is already running, starting with paths (including from the context menu) hands the paths to it and brings its window to the front instead of opening a new window; otherwise it starts itself. Starting without paths (for example from the Start menu) always opens a new window. The FEditor instances coordinate over port 49731 on the local machine (127.0.0.1) and only accept the same user's FEditor; if another program holds that port, every start simply opens a new window again.

  On macOS there are two ways in, and they differ:

  | | "Open With" | "Services" (the switch above) |
  |---|---|---|
  | Setup | none — it is there once `FEditor.app` is installed | tick "Add to the context menu" in the settings panel |
  | Files | FEditor is listed for text-like files | every file gets "Open with FEditor" |
  | Folders | not available | "Pick files to open with FEditor…" |
  | Where | right-click → "Open With" | right-click → "Services" |
  | Also | "Get Info" can make FEditor the default app for a file type; files can be dropped on the Dock icon | |

  If FEditor is missing from "Open With" after an upgrade, make sure `FEditor.app` is the new version (an old copy keeps its old registration); if needed, re-register it with `lsregister -f`.
- **Tab width and spaces instead of Tabs**: "Tab width" in the "Editor" group of the settings panel is the default column count (4 by default); "Per file type" gives one file type (a syntax highlighting language, such as Python or Go) its own width, every type without one uses the default, and the close button on its right removes the setting. Ticking "Use spaces" under "Typing Tab" makes the Tab key pad with spaces up to the next tab stop and multi-line indent add a Tab width of spaces (shared by every file type); Tab characters already in a file are left alone, and unindent removes either kind.
- **Convert indentation**: two toolbar buttons, "Tabs to spaces" and "Indentation spaces to Tabs", convert by the Tab width of the document's file type; with a selection only the selected lines are converted, otherwise the whole document. Tabs to spaces pads each Tab up to the next tab stop; spaces to Tabs only touches the leading indentation (one Tab per Tab width, a remainder smaller than that stays as spaces), so spaces used for alignment in the middle of a line are left alone. Macros can use `editor.run("tabsToSpaces")` and `editor.run("spacesToTabs")`.
- **Auto indent**: when you press Enter, a line ending in an opening bracket `{ ( [` (or a colon `:` in Python) indents the next line one more level, and pressing Enter between a pair of brackets (`{|}`) opens three lines with the caret on the indented middle one; typing `}`, `)` or `]` at the start of an otherwise blank indented line removes one level. Brackets inside strings and comments are ignored, and inside block comments or multi-line strings Enter only copies the indent. The rules come from the file type (Rust, C/C++, Java/C#, JavaScript/TypeScript, Go, CSS and JSON use brackets; Python uses the colon; other types just copy the previous line's indent; custom languages set `indent = brace|python|none`). The indent characters follow "Spaces instead of Tab" and the Tab width. The switch is "Auto indent" in the settings panel's editing page (on by default); turning it off goes back to just copying the previous line's indent.
- **Reformat**: the toolbar's "Reformat" button, `Alt+Shift+F` in the editor, or `editor.run("reformat")` in macros. With a selection only the selected lines are processed, otherwise the whole document; it is one undo step. Languages that use brackets get their indentation recomputed from the bracket depth (the depth is always counted from the top of the file, so selecting just a part still lines up) and trailing whitespace removed; lines inside block comments and multi-line strings are left alone. JSON is pretty-printed completely (key order kept; JSON with comments falls back to the bracket rules). Python and other types with no depth rule only get trailing whitespace trimmed and the leading indentation converted to the current setting.
  - **Wrapping inside brackets**: when the safe margin is enabled (the "Safe margin" checkbox in the settings panel), Reformat also handles round and square brackets (call arguments, arrays): needless line breaks are joined back into one line, and what does not fit (goes past the safe margin) is broken inside the brackets. `{ }` (code blocks) are left alone, strings and trailing comments are never split, and a bracket group that contains a comment, a blank line or `{ }` is not joined. How the next line lines up depends on the language's "wrap style": `align` lines up with the first argument after the opening bracket (arguments fill each line, the closing bracket stays after the last argument); `hang` breaks right after the opening bracket, one argument per line indented one level, with the closing bracket on its own line. By default Python, C/C++ and Java/C# use `align`; Rust, JavaScript/TypeScript and Go use `hang` (with a trailing comma after the last argument). To change it: a custom language file can set `wrap = align|hang` and `trailing_comma = true|false`, or the settings file can override per language with `wrap_style.<language name> = align|hang` (e.g. `wrap_style.Rust = align`). Without the safe margin nothing is joined or broken.
- **Margin guide**: tick it to enable (on by default) and set a column (80 by default); from the right edge of that column on, the whole area to its right is tinted faintly, so on a line that runs past the guide the extra characters land in the tinted area. Columns match the display: a full-width character counts as two; the column is remembered while the guide is unticked. In the settings panel the unit of a number field (pt, cols) is written outside the field, so you only type the number.
- **Terminal**: the font size (Ctrl +/- adjusts it when a terminal is clicked, independent of the editor and the preview), the minimum contrast between text and background (can be turned off), and the default terminal (chosen from the detected list of shells, or your own command with arguments).
- Settings live in the config folder (Windows `%APPDATA%\feditor\`, macOS `~/Library/Application Support/feditor/`, Linux `~/.config/feditor/`); the settings panel has buttons that open the scheme and preview plugin folders directly.

The formats of custom color scheme files, custom syntax definitions and the settings file are in [Customization and settings file](docs/customization.en.md).

## Macros

Macros are JavaScript files in the `macros` folder of the config directory, one `.js` file each.

- The macro button on the toolbar opens the macro panel: run, set a shortcut (Alt/Ctrl combinations), edit the source, rename, delete. F5 reruns the last one.
- **Recording**: after pressing the record button, the keys and input that follow are recorded; press it again to stop, name the macro, and it is saved as editable JavaScript (continuous typing is merged into one statement). Mouse actions are not recorded.
- Running a macro is **one undo step**; when it fails, the text and selection are restored to how they were before. Macros run in a sandbox with time (5 s) and memory (64 MiB) limits and cannot read or write files or use the network.

The full API and the header markers are in [Macros](docs/macros.en.md); examples are in `examples/macros/`.

## Known issues

- The IME has only been verified on Windows; macOS and Linux are untested.
- The macOS `FEditor.app` is not notarized and has not been fully tested. Right-click a file in Finder → "Open With" lists FEditor (declared as an Alternate editor for text-like files, so it does not take over the default app) and opens the file; dropping files on the Dock icon works too.
- A single very long line (hundreds of thousands of characters) makes that line slow; above about 1,000,000 lines the vertical scroll position has limited precision (a 4-million-line, 346 MB file opens and can be edited, but the line spacing visibly jitters).
- While the IME has the focus on Windows, some Ctrl+Alt+letter combinations are swallowed by the IME.
- The terminal is experimental and is best tested on Windows so far.

## Documentation

- [Macros](docs/macros.en.md): the macro library, recording, and the full JavaScript API.
- [Preview plugins](docs/preview-plugins.en.md): write plugins to preview more file types.
- [Customization and settings file](docs/customization.en.md): custom color schemes, custom syntax, the settings file format, font details.
- [How the editor works](docs/internals.en.md): design and implementation details of the editor area, IME, file watching, sessions, the terminal and more.
- [Development guide](docs/development.en.md): project layout, design decisions, building, testing, releasing.
- [Changelog](CHANGELOG.en.md).
