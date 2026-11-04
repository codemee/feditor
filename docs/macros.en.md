**English** | [繁體中文](macros.md)

[← Back to the README](../README.en.md)

# Macros

## Library, recording, and shortcuts

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

## Macro API (JavaScript)

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

Editor commands (`editor.run`): `left` `right` `up` `down` `wordLeft` `wordRight` `pageUp` `pageDown` `home` `end` `docStart` `docEnd`, their `select`-prefixed variants (`selectLeft` ... `selectDocEnd`, `selectAll`); `backspace` `delete` `deleteWordLeft` `deleteWordRight` `enter` `tab` `shiftTab` `undo` `redo`; `moveLineUp` `moveLineDown` `duplicateLine` `deleteLine` `toggleComment` `indent` `unindent` `tabsToSpaces` `spacesToTabs`; `addCaretUp` `addCaretDown` `selectNextOccurrence` `clearCarets`. An unknown name throws an error.

- Positions are in **Unicode code points**, not JavaScript string UTF-16 units. `"😀".length` is 2, but `editor.length()` counts it as 1.
- Positions must be non-negative integers; otherwise a `RangeError` is thrown.
- Every run uses a fresh environment; macros do not share globals.
- Default limits are 5 seconds of run time and 64 MiB of memory; infinite loops and memory bombs are aborted.
