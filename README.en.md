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

- **Windows**: unzip and run `feditor.exe`.
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

The preview pane can be dragged anywhere in the layout, and clicking it lets Ctrl +/- adjust its own font size. To preview other file types, write a JavaScript **preview plugin** (see [Preview plugins](docs/preview-plugins.en.md)).

## Terminal

The `>_` toolbar button (Ctrl+Shift+`) adds a terminal running the system's default shell (`$SHELL` on Unix; on Windows PowerShell 7 first, then Windows PowerShell, then cmd).

- **A terminal is a kind of tab**: its label is "Terminal 1", "Terminal 2", ..., and you can **double-click the label to name it yourself** (leave it empty to go back to the default). Drag the tab like a text tab: drop it in the middle of another pane to join that pane's tab bar, or at a pane's side to split it into a new pane. While another tab is shown, the shell keeps running in the background.
- Ctrl+` cycles the focus through the terminals and returns to where you were after the last one. The tab closes when the shell exits (type `exit`) or you press its ×.
- Supports colors, full-screen programs, IME, copy and paste (Ctrl+C copies when text is selected and interrupts otherwise), mouse-drag selection and wheel scrolling. Colors follow the current color scheme, and text whose contrast with its background is too low is adjusted until it is readable.
- Not yet: mouse reporting to programs, clickable links, search inside the terminal.

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
- The macOS `FEditor.app` is not notarized and has not been tried on a real Mac; "Open With" on a file in Finder does not open it yet (only command-line arguments are supported).
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
