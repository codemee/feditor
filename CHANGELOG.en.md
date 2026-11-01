**English** | [繁體中文](CHANGELOG.md)

# Changelog

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

[0.1.1]: https://github.com/codemee/feditor/releases/tag/v0.1.1
[0.1.0]: https://github.com/codemee/feditor/releases/tag/v0.1.0
