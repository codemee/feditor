**English** | [繁體中文](macros.md)

[← Back to README](../README.en.md)

# Macro User Manual

A macro turns a repetitive editing job into one button or one shortcut. You can **record** your own actions, or **write** one yourself in JavaScript.

**Contents**

1. [Quick start: record your first macro](#1-quick-start-record-your-first-macro)
2. [Running a macro](#2-running-a-macro)
3. [The macro panel](#3-the-macro-panel)
4. [Setting a shortcut](#4-setting-a-shortcut)
5. [Limiting a macro to some file types](#5-limiting-a-macro-to-some-file-types)
6. [Managing macro files](#6-managing-macro-files)
7. [Writing your own macro](#7-writing-your-own-macro)
8. [Macro API reference](#8-macro-api-reference)
9. [Limits and things to know](#9-limits-and-things-to-know)
10. [Troubleshooting](#10-troubleshooting)

---

## 1. Quick start: record your first macro

1. Press the **Record** button <img src="images/icons/record.svg" width="18" height="18" align="absmiddle" alt="Record"> on the toolbar to start recording.
2. Work as usual: type, paste, use the arrow keys, delete, run line operations...
3. Press it again to stop (while recording the button becomes <img src="images/icons/stop.svg" width="18" height="18" align="absmiddle" alt="Stop recording">).
4. In the dialog that appears, name the macro (the default is "錄製的巨集", the Chinese for "Recorded macro") and press **Enter** or "Save".
5. Press **F5** to replay the macro you just recorded.

> **What is recorded**: keys and typing, including typing, text committed by an IME, paste, arrow keys, deletion and line operations.
> **What is not recorded**: mouse actions (clicks, drag selections). To select text, use the keyboard instead (for example Shift+arrow).

In the dialog:

| Button / key | What it does |
|---|---|
| Enter, "Save" | Stores it in the macro folder |
| "Discard" | Does not save it |
| Esc | Does **not** close the dialog (so a recording cannot be lost by accident) |

If the name is taken, a number is added automatically. When recording starts or stops, the keyboard focus returns to the editor and the selection is kept, so you can keep typing.

## 2. Running a macro

| Way | Description |
|---|---|
| **F5** | Replays the macro that was run last (or just recorded); so does <img src="images/icons/rerun.svg" width="18" height="18" align="absmiddle" alt="Run the last macro again"> on the toolbar |
| **<img src="images/icons/run.svg" width="18" height="18" align="absmiddle" alt="Run"> in the macro panel** | Runs that macro (see the next section) |
| **Shortcut** | A macro with a shortcut runs when you press the combination (see section 4) |
| **Run from file…** | A button at the bottom of the panel; runs any `.js` file, which need not be in the macro folder |

**A run is one undo step**: a single Ctrl+Z reverts everything the macro did. **A failure rolls back**: if the macro fails halfway, the text and the selection return to how they were before the run.

## 3. The macro panel

Press the **Macro** button <img src="images/icons/run.svg" width="18" height="18" align="absmiddle" alt="Macro"> on the toolbar to open the panel. Each row is a macro; the icons on a row:

| Icon | What it does |
|---|---|
| <img src="images/icons/run.svg" width="18" height="18" align="absmiddle" alt="Run"> | Run |
| <img src="images/icons/keyboard.svg" width="18" height="18" align="absmiddle" alt="Keyboard"> | Set the shortcut |
| <img src="images/icons/edit.svg" width="18" height="18" align="absmiddle" alt="Pencil"> | Edit the source in a new tab |
| <img src="images/icons/dropdown.svg" width="18" height="18" align="absmiddle" alt="Dropdown"> | The dropdown on each row: set the file types it applies to (see section 5) |
| <img src="images/icons/rename.svg" width="18" height="18" align="absmiddle" alt="Rename"> | Rename (double-clicking the name works too) |
| <img src="images/icons/trash.svg" width="18" height="18" align="absmiddle" alt="Delete"> | Delete |

The buttons in the bottom row:

| Button | What it does |
|---|---|
| Run from file… | Pick a `.js` file and run it |
| Open macro folder | Opens the folder that holds the macros in the file manager |
| Show shortcuts | Opens the "Shortcuts" page of the settings panel, listing the built-in shortcuts and those given to macros |
| Macro docs | Opens this manual in the browser |

The panel lists only the macros that **apply to the current file**; those limited to other file types are collapsed under "Macros for other file types" (closed by default).

## 4. Setting a shortcut

1. Press the keyboard icon <img src="images/icons/keyboard.svg" width="18" height="18" align="absmiddle" alt="Keyboard"> on the macro's row.
2. Press the combination you want.
3. The program writes it into the `@shortcut` at the top of the macro file.

| Key | What it does |
|---|---|
| A combination | Sets it |
| **Esc** | Cancels |
| **Delete** | Clears this macro's shortcut |

**Rules**

- It must include **Ctrl** or **Alt** (function keys F1 to F12 are the exception).
- It cannot duplicate a **built-in shortcut**; it is rejected with the reason.
- It cannot duplicate another macro whose **scope overlaps** (see the next section).
- Running a macro with its shortcut (for example Alt+M) does not type that letter.
- To see which shortcuts exist: "Show shortcuts" in the panel.

## 5. Limiting a macro to some file types

By default a macro applies to **all files**. You can limit it to certain file types, for example only Rust or Python.

**How to set it**

- In the dropdown on each row of the panel, open it and tick the file types; several are allowed and the change **takes effect at once**.
- The dropdown shows the current choice; it reads "All files" when not limited.
- The naming dialog at the end of a recording has one more choice when the current file has a language: "All files" (the default) or "Only Rust files".
- Or write `// @lang Rust, Python` in the header (names as in the status bar language list, comma-separated, case-insensitive).

**What happens once limited**

| Situation | Result |
|---|---|
| The current file's language matches | Listed in the panel, can run, shortcut and F5 work |
| The current file's language does not match | Collapsed under "Macros for other file types"; can still be renamed, rescoped and deleted, but **not run**; the shortcut and F5 do not fire (F5 says why) |
| Plain text, unrecognized files | Only the universal (not limited) macros apply |

The comparison uses the language the document **actually has**, so changing the language by hand in the status bar also changes which macros apply.

**Sharing a shortcut**: macros whose scopes do not overlap can use the same shortcut, for example a Rust-only and a Python-only macro both on Alt+R. A universal macro overlaps every scope, so it cannot share one with another macro.

## 6. Managing macro files

A macro is an ordinary `.js` file, one per macro.

| System | Macro folder |
|---|---|
| Windows | `%APPDATA%\feditor\macros` |
| macOS | `~/Library/Application Support/feditor/macros` |
| Linux | `$XDG_CONFIG_HOME/feditor/macros` (`~/.config/feditor/macros` when unset) |

The quickest way to open it: "**Open macro folder**" in the macro panel.

**Header tags**: comments at the top of the file can carry these tags. The program maintains them when you use the panel, and you can also write them by hand.

```js
// @name Display name
// @shortcut Ctrl+Alt+U
// @lang Rust, Python
```

| Tag | What it is for |
|---|---|
| `@name` | The name shown in the panel |
| `@shortcut` | The shortcut |
| `@lang` | The file types it applies to |

**Renaming** rewrites `@name`; the file name stays and the other tags, such as the shortcut, are kept. When a recording is saved, unusable characters are stripped from the file name, while the name itself is written as-is in `@name`.

## 7. Writing your own macro

### Examples

`examples/macros/` has three examples you can copy into the macro folder and try:

| File | What it does |
|---|---|
| `uppercase.js` | Converts the whole text to upper case |
| `line-numbers.js` | Adds a line number at the start of every line |
| `fail-demo.js` | Edits, then fails on purpose, to show the rollback on failure |

### A recording is JavaScript too

You can open a recorded macro with the pencil icon <img src="images/icons/edit.svg" width="18" height="18" align="absmiddle" alt="Pencil"> and edit it. Consecutive typing is merged into one statement and repeated commands are written with a count:

```js
// Generated by recording
editor.run("docEnd");
editor.paste(" world");
editor.run("selectRight", 2);
```

### A simple example

Put parentheses around the selected text:

```js
// @name Add parentheses
// @shortcut Alt+9
editor.wrap("(", ")");
```

### Ideas to keep in mind

- A macro reads and changes the current document through the global object `editor`; the output of `console.log(...)` appears in the output pane at the bottom of the window.
- **Positions** count characters, starting at 0 (see section 9).
- When inserting text in several places, insert **from the end backwards**, so earlier positions do not shift because of the insertions (see `line-numbers.js`).

## 8. Macro API reference

### Reading the document

| API | Description |
|---|---|
| `editor.text()` | The whole text |
| `editor.length()` | Total number of characters |
| `editor.lineCount()` | Number of lines |
| `editor.line(i)` | Line `i` (with its line break); `undefined` when out of range |
| `editor.slice(start, end)` | The text in `[start, end)` |
| `editor.lineStart(i)` | The start position of line `i` |
| `editor.lineOf(pos)` | Which line position `pos` is on |
| `editor.path()` | The full path of the current file; `undefined` if it was never saved |

### Changing the document

| API | Description |
|---|---|
| `editor.insert(at, text)` | Insert at position `at` (selections move with the text) |
| `editor.remove(start, end)` | Delete `[start, end)` |
| `editor.replace(start, end, text)` | Replace `[start, end)` |
| `editor.type(text)` | Like the user typing (replaces the selection, the caret moves) |
| `editor.paste(text)` | Like the user pasting (replaces the selection, the caret moves) |
| `editor.wrap(prefix, suffix)` | Wraps each selection in the two pieces of text; with no selection it inserts both and leaves the caret between them; one undo step |

After `wrap` the selection stays on the original text, so you can wrap another layer around it.

### Selection

| API | Description |
|---|---|
| `editor.selection()` | `[anchor, caret]` |
| `editor.setSelection(anchor, head)` | Set the selection |
| `editor.selectedText()` | The selected text |

### Search

| API | Description |
|---|---|
| `editor.find(query, from = 0, opts)` | Returns `[start, end]` when found, otherwise `undefined` |

Options for `opts`:

| Option | What it does |
|---|---|
| `regex` | Treat `query` as a regular expression |
| `caseSensitive` | Match case |
| `wholeWord` | Whole words only |
| `backward` | Search backwards |

### Dialogs and output

| API | Description |
|---|---|
| `editor.alert(msg)` | Show a message |
| `editor.confirm(msg)` | Ask; returns whether "Yes" was pressed |
| `console.log(...)` | Write to the output pane |

### Editing commands: `editor.run(name, count = 1)`

Same effect as the key; `count` is the number of repeats. An unknown name throws an error.

| Category | Commands |
|---|---|
| Movement | `left` `right` `up` `down` `wordLeft` `wordRight` `pageUp` `pageDown` `home` `end` `docStart` `docEnd` |
| Selection | A movement command with the `select` prefix: `selectLeft`, `selectRight`...`selectDocEnd`; plus `selectAll` |
| Deleting and input | `backspace` `delete` `deleteWordLeft` `deleteWordRight` `enter` `tab` `shiftTab` |
| History | `undo` `redo` |
| Line operations | `moveLineUp` `moveLineDown` `duplicateLine` `deleteLine` |
| Code | `toggleComment` `indent` `unindent` `tabsToSpaces` `spacesToTabs` `reformat` (format the layout) |
| Multiple carets | `addCaretUp` `addCaretDown` `selectNextOccurrence` `clearCarets` |

## 9. Limits and things to know

- **Positions are in Unicode code points**, not JavaScript string UTF-16 units. For example `"😀".length` is 2, but `editor.length()` counts it as 1.
- A position must be a **non-negative integer**, otherwise a `RangeError` is thrown.
- **Every run starts in a fresh environment**; macros do not share global variables.
- The time limit is **5 seconds** and the memory limit **64 MiB**; infinite loops and memory bombs are stopped and handled like any failure: every change is rolled back.
- Mouse actions are not recorded.

## 10. Troubleshooting

| Symptom | Likely cause and what to do |
|---|---|
| My macro is not in the panel | It is limited to another file type; look at the collapsed "Macros for other file types" below, or change it to "All files" in the dropdown |
| The shortcut does nothing | The macro does not apply to the current file's language, or the shortcut is taken by a built-in one (that is rejected when you set it). Check "Show shortcuts" |
| F5 does not run and shows a message | The last macro does not apply to the current file; the message gives the reason |
| Setting a shortcut is rejected | There is no Ctrl or Alt, it duplicates a built-in shortcut, or it duplicates another macro with an overlapping scope |
| The macro failed halfway | The text was restored automatically; the error is in the output pane at the bottom |
| The macro was stopped | It exceeded 5 seconds or 64 MiB; look for an infinite loop |
| A recorded macro replays differently | Mouse actions are not recorded; record again using the keyboard |
