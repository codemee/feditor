**English** | [繁體中文](CHANGELOG.md)

# Changelog

## Unreleased

### Added
- **Interface language**: the settings panel offers "Follow system", "中文" (Chinese), and "English", and the change applies immediately. With "Follow system", a Chinese system gets Chinese and everything else gets English.
- The status bar shows the caret position as "行 X，欄 Y" (line X, column Y, starting at 1). In the column number, full-width characters (CJK ideographs, kana, Hangul, full-width punctuation) count as 1 or 2 columns, chosen under "欄號" (column number) in the settings panel (default 1).

### Changed
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

[0.1.2]: https://github.com/codemee/feditor/releases/tag/v0.1.2
[0.1.1]: https://github.com/codemee/feditor/releases/tag/v0.1.1
[0.1.0]: https://github.com/codemee/feditor/releases/tag/v0.1.0
