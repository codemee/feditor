**English** | [繁體中文](CHANGELOG.md)

# Changelog

## [0.13.9] - 2026-11-14

### Added
- **Syntax highlighting for `.env` files**: `.env`, `.env.local`, `.env.production`, `.env.example` (any name starting with `.env.`), `*.env` and `.flaskenv` use the "config file" highlighting (`#` comments, `KEY=value`, strings, numbers), and Ctrl+/ toggles a `#` comment. Files with the same syntax (`.editorconfig`, `.gitconfig`, `.gitattributes`, `.gitmodules`, `.dockerignore`, `.npmrc`) join the same group; direnv's `.envrc` is a shell script and uses the Shell highlighting.
- **More extensions in the "config files" category of Open by default**: `.env`, `.flaskenv`, `.editorconfig`, `.gitconfig`, `.gitattributes`, `.gitmodules`, `.dockerignore`, `.npmrc`. The extension of `.env.local` and `.env.example` is `local` / `example` on Windows, which cannot be registered, so a double-click still opens them with the previous program (highlighting is not affected).

### Fixed
- **While running Claude Code in the terminal, the IME composition text jumped to the bottom-left and flickered**: on every redraw (a turning spinner, for example) Claude Code hides the cursor, jumps to the bottom of the screen to draw, and jumps back to the input box before showing it again; the composition text and the candidate window followed the cursor's position at that moment, so they jumped to the bottom in the middle of a redraw. While the cursor is hidden, the IME now stays at the position where the cursor was last shown; only when the cursor stays hidden for more than 1 second (the program draws its own cursor) does it use the cursor's current position.

## [0.13.8] - 2026-11-13

### Added
- **Drag tabs to reorder them**: drag a tab along its own tab bar and drop it to insert it there; a line marks the insertion point while dragging (Esc cancels), and the active tab stays the same. Dragging anywhere outside the tab bar still moves the tab to another pane or splits off a new pane as before.
- **The tab list can sort by name**: the bottom of the "Choose a tab" dropdown now has "Sort by name (ascending A→Z)" and "(descending Z→A)", case-insensitive with numbers compared by value (`a2` before `a10`).
- **Search only inside a selection**: a new "in selection" button on the second row of the find panel (current-document scope only): select text first, then press it, and find, replace, replace all and the hit list work only inside that text; searching wraps around inside the range, and the editor tints it. The range follows typing and replacing around it, and is cancelled when you switch tabs, reload, or switch to folder scope.

### Changed
- **The hit list now shows every match**: previously only the first match of a line was listed, so the count did not agree with the editor; now a line with three matches lists three, and empty matches count too. The 5000-result limit of the folder scope counts matches as well.

### Fixed
- **A pattern containing `\n` showed "not found" in the hit list**: the hit list matched line by line, and a line contains no newline character; it now matches against the whole text, so every match the editor finds is listed (a multi-line match only marks its first line).
## [0.13.7] - 2026-11-12

### Fixed
- **Ctrl+Tab and Ctrl+Shift+Tab did not switch tabs while the caret was in a terminal**: the terminal took the key and sent it to the shell as a plain Tab. These two combinations are now left for switching tabs; a plain Tab (the shell's completion) and all other keys are unchanged.

## [0.13.6] - 2026-11-12

### Fixed
- **"Next tab" and "Previous tab" on the "Shortcuts" page of the settings panel were missing Ctrl**: the real shortcuts are Ctrl+Tab and Ctrl+Shift+Tab, and they are now shown correctly.

## [0.13.5] - 2026-11-12

### Added
- **The preview and the editor scroll together**: scroll one side and the other follows to the matching passage (both ways; when either side reaches the bottom the other does too; it lines up with word wrap on as well). For now this covers the preview of Markdown files. "Synced scrolling" on the "Preview" page of the settings turns it off; settings key `preview_sync_scroll`, default `true`.
- **The macro panel has a "Macro docs" button** (to the right of "Show shortcuts"): it opens the macro documentation in the browser, the English version when the interface is in English.
- **Dragging a selection past the top or bottom edge of a terminal scrolls it automatically**: the farther from the edge the faster, it keeps going while the mouse is still, stops on release, and the selection follows.

### Changed
- **The macro documentation is rewritten as a user manual**: chapters in the order you use them (record your first macro, run, the panel, shortcuts, limiting to file types, managing files, writing your own, API reference, limits, troubleshooting), with tables and steps, and the buttons and icons it mentions are shown as the real icons.
- **A terminal tab no longer carries the default "Terminal N"**: a title set by the program is shown alone ("✳ Claude Code"), a folder reported by the shell is shown alone; "Terminal N" appears only when neither is known.
- **The "Shortcuts" page of the settings panel gets a scroll bar when the list is long**, and the groups are separated by lines.

### Fixed
- **The preview did not follow a file changed by another program**: after the editor reloaded it the buffer's revision number started over and the preview thought nothing had changed. Every load now has its own number and the preview redraws at once.
- **The settings panel stayed as tall as the tallest page seen**, leaving a large blank area under shorter pages; it now sizes itself to each page.
- **Git Bash in the terminal had no logo**: Git's `bash.exe` has no icon of its own, so the icon of `git-bash.exe` in the Git folder is used.

## [0.13.4] - 2026-11-12

### Fixed
- **A selection in the terminal jumped to the wrong place when the window or pane was made narrower**: after a resize the text reflows (and programs such as Claude Code redraw), so the selected cells no longer held that text. A resize now clears the selection; copy before resizing.

## [0.13.3] - 2026-11-12

### Changed
- **The Open and Save As dialogs now start in the current tab's folder**: the folder of the file for ordinary tabs, the working folder for terminal tabs; for an unsaved document or a folder that no longer exists the system decides. They used to start in the program's own folder or the last place used.

## [0.13.2] - 2026-11-12

### Changed
- **The unsaved mark on tabs is now a small red dot at the far right of the tab** (where the × is, which replaces it on hover) instead of a ● after the file name, and the tooltip says "Unsaved". The terminal's new-notification bell sits in the same spot. The window title and the tab list still use ●.

### Added
- **The output pane can be turned off in the settings** (Editor page, "Output pane: Show"): when off, even the collapsed title bar is hidden, errors and macro output no longer pop the panel open, and output is still recorded. Settings key `output_pane`, default `true`.
- **The terminal supports OSC 8 hyperlinks**: hold Ctrl over a link a program marked and the whole link is underlined, the tooltip shows the real address, and a click opens it. Only `http`, `https` and `mailto` open; anything the system would execute, such as `file:`, does not.
- **The terminal answers programs asking for its colors** (`?` in OSC 4 / 10 / 11 / 12): vim, bat, Claude Code and others use it to tell a dark theme from a light one; they now get the current colors.
- **The terminal receives notifications and folder reports**: notifications sent with OSC 9, OSC 777;notify or OSC 99 show a bell icon on the tab when it has no focus, like the bell does (not a ●, which would be confused with the unsaved mark of a file); on Windows, OSC 7 and OSC 9;9 from shells such as Git Bash and Nushell update the current folder (only folders that exist on this computer are accepted).

### Fixed
- **Text selected with the mouse in Claude Code inside the terminal could not be copied**: Claude Code handles mouse selection itself and asks the terminal to write the clipboard with an OSC 52 sequence, which FEditor ignored. It now writes the system clipboard (write only; requests to read the clipboard are not answered).

## [0.13.1] - 2026-11-11

### Changed
- **Word wrap now follows the file type**: the word wrap button in the settings panel's Display row now changes only the file type of the current document (Markdown can wrap while Python does not, each remembered separately); the File type page has a new Word wrap row to choose default, on or off for each type. Plain text files have their own setting too. The settings file gains `word_wrap.<language>`; types without their own line use the global default (the new "Word wrap: on by default" checkbox on the Editor page, which is the old `word_wrap`), so existing settings files keep working.

### Added
- **The terminal cursor blinks**: about every half second while the terminal has focus; it stays lit at the moment you type or send a key.

### Fixed
- **A flood of terminal output left the view away from the bottom** (a problem since the scroll bar of 0.12.1): while output kept growing, the scroll bar position lagged one screen behind and pulled the view up every frame. The scroll bar's position is now followed only while you actually drag it.
- **Selecting text in the terminal with the mouse often missed the first character under the pointer**: egui only starts a drag after the pointer has moved a little, but the selection started from that later position; it now starts where the button was pressed.
- **After typing in an opened longer file and pressing Ctrl+Z, the unsaved mark stayed**: the fingerprint compared against the loaded content depended on how the text happened to be split in memory, so identical content was judged different. It no longer depends on that. (New untitled documents were unaffected because their content is empty.)

## [0.13.0] - 2026-11-10

### Added
- **Command-line options**: the `feditor` command has a few more options.
  - `file:line[:col]`: open the file and jump to that position, e.g. `feditor src/main.rs:120:5`; files that do not exist yet work too (they open as new files to be created). Nothing is split when the whole text is itself an existing file.
  - `-w` / `--wait`: when a FEditor is already running, wait until the tabs of those files are closed before exiting (closing the window also ends the wait). This lets it serve as the editor for programs such as `git commit`: `GIT_EDITOR="feditor --wait"`.
  - `-n` / `--new-window`: do not hand the files to an open window; open a new window instead.
  - `-h` / `--help`, `-v` / `--version`, and `--` (everything after it is a file name).
  - The terminal command (`~/.local/bin/feditor`) stays in the foreground for `--wait`, `--help` and `--version`, and still runs in the background otherwise.
- **The title bar shows the version**: `name - FEditor 0.13.0`.

### Fixed
- **Shell rc files without an extension, such as `.zshrc`, `.bashrc` and `.profile`, were not highlighted as shell**: `zshrc`, `zshenv`, `zprofile`, `bashrc`, `bash_profile`, `bash_aliases`, `profile`, `kshrc` and so on (with or without the leading dot) are now recognized as shell.

### Changed
- The internal protocol a newly started program uses to hand files to a running FEditor has a new version; two versions running at once cannot hand files to each other (the newly started one opens its own window).

## [0.12.1] - 2026-11-10

### Added
- **The terminal has a scroll bar**: when output has scrolled off the screen, a scroll bar appears at the right edge (same style as the editor's: hidden until the pointer comes near), so you can drag back through the output and see how much there is. It is not shown for blank-only history or while a full-screen program such as vim runs.

### Fixed
- **Fast English typing suddenly produced capital letters when Caps Lock is used to switch the IME between Chinese and English**: in the IME's English mode Caps Lock is on, and while typing fast some letters bypassed the IME and were turned into capitals by the system (Windows Terminal and VSCode are unaffected because all their keys go through the IME). Now, when the IME has just sent English letters, a capital typed without Shift is turned back into lowercase; people who really use Caps Lock for capitals are not affected.
- **Shells installed with Scoop showed a generic blue-window icon on the terminal tab**: the icon of the program the shim points to is used instead.
## [0.12.0] - 2026-11-10

### Added
- **Recently opened files**: the last 10 files you opened are remembered; a new dropdown arrow next to "Open" on the toolbar lists them (the file name plus the folder in pale text), one click opens one, and the list can be cleared at the bottom. A file that cannot be opened (deleted, moved) is removed from the list with an explanation. Only files you open yourself count (the Open dialog, dropping on the window, hand-over from Explorer / Finder, the command line, Save as); restoring the previous tabs and jumping from a search result do not.
- **The Windows taskbar jump list**: right-clicking FEditor's icon shows a "Recent" category; one click opens the file in FEditor (handed to the running window when there is one). Files that no longer exist are not listed, and an item the user removed from the jump list is not added back.
- **The macOS Dock menu**: newly opened files are reported to the system's recent documents, so the Dock icon's menu lists them; clearing the list clears the system's too. (Not yet verified on a real Mac.)

## [0.11.0] - 2026-11-08

### Added
- **Macros can apply to certain file types only**: a `// @lang Rust, Python` header (names as in the status bar language list) limits a macro to those file types; a macro without `@lang` applies to all files, so old macros need no change. The macro panel lists only the macros that apply to the current file, those limited to other types are collapsed under "Macros for other file types"; shortcuts and F5 do not fire on a file the macro does not apply to. **Macros whose scopes do not overlap can share a shortcut.** The naming dialog at the end of a recording has one more choice (all files, or only files of the current language), and the dropdown on each row of the panel changes it later. See `docs/macros.en.md`.
- **Every row of the macro list has a "file types it applies to" dropdown**: it shows the current choice ("All files" when not limited); open it and tick the supported file types (several allowed), and the change takes effect at once. A shortcut that would clash with a macro whose scope overlaps is refused with an explanation.
- **A new "Shortcuts" page in the settings panel** lists the built-in shortcuts (file, tabs and terminals, editing, search, view, macros) and the shortcuts given to macros (pale when they do not apply to the current file); "Show shortcuts" at the bottom of the macro panel opens that page directly.

- **Wrap the selection in text**: a new `[ - ]` button next to "Reformat" on the toolbar opens the common choices (double quotes, single quotes, backticks, parentheses, square brackets, curly braces, angle brackets, bold, a Markdown code block), and you can type your own text for before and after (`\n` is a newline). The selection stays on the original text so it can be wrapped again; with no selection a pair is inserted and the caret stays between them; multiple carets each wrap their own; one undo step; the focus returns to the editor afterwards. Macros get `editor.wrap(before, after)`, which is recorded too.

### Fixed
- **After starting or stopping a macro recording, or running a macro, the keyboard focus stayed on the button**, so you had to click the editor to type, and clicking cleared the existing selection. The focus now returns to the editor. The same problem is fixed after: Save, Save as, F5 to rerun a macro, running a macro from the macro panel, reloading with an encoding, toggling the preview, changing the language / encoding / runtime in the status bar, and closing the macro naming, renaming, scope and delete dialogs.

## [0.10.0] - 2026-11-08

### Added
- **Project commands (Rust and Python projects)**: when a `.rs` or `.py` file is open, FEditor looks upwards from its folder for `Cargo.toml` and `pyproject.toml` and puts the commands they can run (`cargo run` / `--example` / `--bin`, `cargo test`, `cargo build`, uv's `uv run file` and the `[project.scripts]` scripts, poetry's `poetry run`, scripts already installed in the `.venv`) in the "Project commands" section of the status bar runtime list, and the Run button uses it (the default is still the interpreter; Rust has none, so the first project command is used). **When the tool a command needs (`cargo`, `uv`, `poetry`) is not on the system or cannot run, no item that could not run is offered**; only one line of text with an install hint remains. Discovery only reads configuration files and checks that the tool works; it never runs a command from the project. See `docs/project-commands.en.md`.

### Fixed
- The code comment describing `runtime_env` still said "only Python for now"; corrected.

## [0.9.1] - 2026-11-07

### Fixed
- **Boxes (missing glyphs) in the interface on Linux**: the fullwidth "＋", "－" and "／" in some help texts exist in Windows' Microsoft JhengHei but the fallback CJK fonts on Linux often lack them (for example the font-size hint under "Editor" in the settings panel). Interface texts now use the ASCII `+`, `-` and `/`, which every font has, and a test keeps fullwidth ＋ － ／ ＝ out of them.
- **A new blank document could not be added while a blank untitled tab was active**: pressing "New" did nothing when the active tab was a blank, unmodified untitled one. It now adds another one ("Untitled 2", ...); opening a file still reuses the active blank tab so no empty tab is left behind.
- **The font-size hint in the settings panel was too small to read**: it used a reduced size together with a pale gray; it is now as large as the other settings (still pale gray).
- **The "Terminal command" row in the settings panel overlapped the "Open by default" checkboxes** (macOS, Linux): the "Open by default" row holds a column of checkboxes taller than a normal row, the grid did not count that height, so the next row was drawn over it. The cell now uses a vertical layout, and a test measures the layout.

### Documentation
- Known issues now explains the Chinese boxes in the WSL (WSLg) title bar and the fix: it happens with a `C.UTF-8` system locale and goes away with a Chinese locale.

## [0.9.0] - 2026-11-07

### Added
- **Terminal tabs show the current folder**: the tab label becomes "Terminal 2 · feditor" and follows `cd`; a terminal you named yourself keeps its name. How the folder is obtained depends on the system and the shell:
  - **PowerShell (7 and Windows PowerShell, on every system)**: it reports the folder through a title sequence (OSC 2) every time it shows its prompt. At startup the program wraps the original `prompt` with `-NoExit -EncodedCommand` (after the profile has loaded, so the user's own prompt and `$?` are unaffected); if the configured shell arguments already contain `-Command` / `-File` nothing is added.
  - **cmd (Windows)**: a title sequence is put in front of the `PROMPT` environment variable; the original prompt is unchanged.
  - **Shells on macOS and Linux (zsh, bash, fish, ...)**: no cooperation from the shell is needed; the operating system is asked for the shell process's working directory (`proc_pidinfo` on macOS, `/proc/<pid>/cwd` on Linux), and the user's config files are left alone.
  - Git Bash, WSL and Nushell on Windows do not report yet, and their labels stay "Terminal N".
- **A new terminal reuses the current folder**: when the input focus is in a terminal (the focused pane is showing one), a new terminal starts in that terminal's current folder; (when the shell does not report its folder, the folder it was opened in); with the focus in the editor it works as before and uses the folder of the current file.
- **Terminals return to their folders when the program is reopened**: the `terminal =` line of the session file now also records the folder and the shell the terminal uses (records from older versions are still read). After a restart every terminal is still a fresh shell, but it starts in the folder it was in when the program was closed and runs the shell chosen originally; if the folder no longer exists the default is used. For shells that do not report a folder (Git Bash, WSL on Windows) the folder they were opened in is recorded.

- **Terminal tabs show the title a CLI tool reports**: programs such as Claude Code report their state with a title sequence (OSC 0 / 2), and it is now shown on the tab label ("Terminal 2 · ✳ Claude Code"), taking priority over the folder name; when the program exits and the shell is back at its prompt the folder is shown again. The shell's own default title (the executable's path) is not shown; a leading braille spinner animation (`⠂`) is removed so the label does not keep jumping; titles longer than 32 characters are shortened. A terminal you named yourself keeps its name.

- **Notifications from terminal programs (Codex CLI and others)**:
  - **Focus reporting**: the terminal sends `ESC[I` / `ESC[O` whenever it gains or loses the keyboard focus (including the window going to the background, the tab being in the background, or the focus being in another pane). Codex CLI only notifies while the terminal is unfocused and assumes it is focused at the start; FEditor never reported anything, so it never notified. On macOS / Linux and for Git Bash, WSL and the like on Windows it is sent when the program asks with `ESC[?1004h`; for PowerShell and cmd on Windows it is sent unconditionally, because ConPTY turns these two sequences into console focus events without the program asking first (that is what Codex reads on Windows, where it even turns `?1004` off itself), and the program's input does not get any extra character from them.
  - **The bell is the notification**: in FEditor (where `WT_SESSION`, `TERM_PROGRAM` and the other variables that identify a known terminal are not set) Codex CLI's "auto" mode notifies with the bell (BEL). When an unfocused terminal receives a bell, a ● appears after its tab label and goes away when you return to that terminal; when the window is not in the foreground the taskbar button also flashes.
  - OSC 9 notifications are not handled yet (`alacritty_terminal` does not hand out that sequence); if Codex is set to `notification_method = "osc9"` nothing arrives, so use `"auto"` (the default) or `"bel"`.

- **Untitled documents are numbered**: tabs without a file read "Untitled 1", "Untitled 2", ... so they can be told apart; each one takes the smallest free number among the open untitled documents, moving it to another pane does not renumber it, and the number is given up when it is saved to a file or closed. The path field of the status bar shows the same name.

- **Shift / Ctrl / Alt+Enter is a newline, not submit, in the terminal**: PowerShell (PSReadLine), Claude Code and similar programs can break the line in their input; Ctrl+Enter sends LF so Node programs can tell it is a newline.

### Changed
- **Closing a pane now closes all the tabs in it**: it used to move the tabs to the neighbouring pane; now the close button at the right end of a pane closes the whole pane, including its terminals (the shells end) and the documents being edited. If the pane holds unsaved documents they are asked about one by one (Save / Don't save / Cancel, plus "Don't save all" when more than one is unsaved): "Save" and "Don't save" close that tab right away once it is dealt with, so if you press Cancel halfway the tabs already closed do not come back and the remaining tabs (and the pane) stay; "Don't save all" closes the whole pane at once.

### Fixed
- **IME composition in the terminal**: the caret inside the composition is drawn in the right place, the arrow keys move it within the composition, and the composed text wraps at the pane edge.
- **Clicking on a pane under a confirmation dialog stole the focus**: when the position pressed on a dialog button had another pane underneath, that pane took the focus and "Save" / "Don't save" acted on the wrong tab. One symptom: when closing a pane, "Don't save" made the dialog appear again and the pane could not be closed, only "Don't save all" worked. The pane focus no longer changes while a dialog is open.
- **Seams in the terminal's block characters**: block elements (U+2580–259F: halves, eighths, quadrants, shades) are now drawn as pixel-aligned rectangles and cell edges are aligned to physical pixels, so the Claude Code mascot no longer has a dark vertical line in the middle.
- **Missing glyphs in the terminal (⏵ ⏸ and similar symbols)**: Segoe UI Symbol was added to the font fallback, so Claude Code's "⏵⏵ auto mode" is no longer two boxes.

## [0.8.0] - 2026-11-05

### Added
- **Find, replace and find in a folder are now one panel**: Ctrl+F opens it and searches the current document, Ctrl+Shift+F opens it with the scope set to a folder, and Ctrl+H also expands the replace field; the search text, options and replacement exist only once, the old find bar at the top is gone, and the two search buttons on the toolbar are merged into one. The panel is docked to one edge of the window and takes that whole edge (on the right by default); the remaining space is what the other panes get. Drag the panel's title to move it to the top, bottom, left or right edge, and drag the divider between it and the other panes to resize it.
- **The scope can be switched**: the "folder" and "include subfolders" state buttons on the second row. With the folder off, the current document (including unsaved content) is searched as you type; with it on, a folder is searched, by default the folder of the active tab's file (it follows tab switches until you edit or pick a folder yourself), and "include subfolders" is off by default.
- **Results are shown in two tables**: files (file name, hit count) and the hits of the selected file (line number and the text around the match, the match colored), each with a caption and a frame; a wide, short panel puts them side by side and a narrow, tall one stacks them, the divider between them can be dragged to change the ratio (double-click to reset), and when stacked the file table is only as tall as its files need. Folder results appear while the search is still running; click a file or a hit to open it with the caret on the match, scrolled into view; when the search finishes the table first scrolls to the hit on screen, or the first hit is opened if the current file has none. When a table has focus the arrow keys, PageUp / PageDown and Home / End move through the hits, and F3 / Shift+F3 move to the next / previous result.
- **Enter in folder scope**: after the conditions change, Enter searches; once searched, Enter steps to the next hit (Shift+Enter the previous one) and the input focus stays in the field; Esc closes the panel from anywhere inside it.
- **Folder scope can replace too**: "replace this match" opens the file, replaces in its tab and moves on to the next result; "replace all" asks first, files open in tabs are changed in their tabs (undoable) and the others are written to disk directly (keeping encoding and line endings).
- **The search panel layout is remembered**: the docked edge, the width (left/right edge) or height (top/bottom edge), and the split between the two tables are saved in the settings file (`search_dock`, `search_dock_width`, `search_dock_height`, `search_split`) and restored on the next start; whether the panel is open is not remembered.
- **Search hits in the editor are marked with a rounded box**, the mark goes away when you click elsewhere in the editor, and the current highlight is cleared before searching again or switching the scope.

### Fixed
- The search button of the folder search panel did not match the other search options in size and position; it is now the same size and sits right after the options.
- When the panel gets narrow the search box shrinks first (only when the option buttons have no room left) and the other fields (folder, file filter, file name column) shrink too, so buttons and the hit counts are no longer pushed out of the panel.
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

[0.13.8]: https://github.com/codemee/feditor/releases/tag/v0.13.8
[0.13.7]: https://github.com/codemee/feditor/releases/tag/v0.13.7
[0.13.6]: https://github.com/codemee/feditor/releases/tag/v0.13.6
[0.13.5]: https://github.com/codemee/feditor/releases/tag/v0.13.5
[0.13.4]: https://github.com/codemee/feditor/releases/tag/v0.13.4
[0.13.3]: https://github.com/codemee/feditor/releases/tag/v0.13.3
[0.13.2]: https://github.com/codemee/feditor/releases/tag/v0.13.2
[0.13.1]: https://github.com/codemee/feditor/releases/tag/v0.13.1
[0.13.0]: https://github.com/codemee/feditor/releases/tag/v0.13.0
[0.12.1]: https://github.com/codemee/feditor/releases/tag/v0.12.1
[0.12.0]: https://github.com/codemee/feditor/releases/tag/v0.12.0
[0.11.0]: https://github.com/codemee/feditor/releases/tag/v0.11.0
[0.10.0]: https://github.com/codemee/feditor/releases/tag/v0.10.0
[0.9.1]: https://github.com/codemee/feditor/releases/tag/v0.9.1
[0.9.0]: https://github.com/codemee/feditor/releases/tag/v0.9.0
[0.8.0]: https://github.com/codemee/feditor/releases/tag/v0.8.0
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
