**English** | [繁體中文](IME-TEST.md)

# IME Verification Checklist

Goal: before choosing a UI framework, verify how Traditional Chinese input methods (Bopomofo, Cangjie, etc.) behave on each candidate framework.

## How to run

```powershell
cargo run --release -p ime-egui
cargo run --release -p ime-iced
cargo run --release -p ime-slint
```

## Test items

Test each prototype in order and record the result (○ OK / △ minor issues / ✕ fail).

| # | Item | Action | Expected result |
|---|---|---|---|
| 1 | Preedit display | Type Bopomofo `ㄋㄧˇ` without selecting a candidate | Underlined preedit text appears inside the text box |
| 2 | Candidate window position | Type at different positions (top-left, bottom-right, after a line break) | Candidate window stays next to the caret, not in a screen corner |
| 3 | Commit | Press Space to pick, Enter to commit | Text is committed correctly; Enter does not insert an extra newline |
| 4 | Cancel during composition | Press Esc while composing | Preedit text disappears with no leftovers |
| 5 | Caret movement during composition | Press arrow keys and Backspace while composing | Behaves like Notepad |
| 6 | Continuous typing | Type a whole sentence quickly (20+ characters) | No dropped, duplicated, or lagging characters |
| 7 | Caret position | Insert Chinese in the middle of a sentence | Inserted at the right place; following text shifts |
| 8 | Typing over a selection | Select some text, then type Chinese | Selection is replaced |
| 9 | Mixed Chinese/English | `Hello 你好 world` | Spaces and half/full-width spacing look normal |
| 10 | Cangjie, Pinyin | Switch IMEs and repeat 1–3 | Same as Bopomofo |
| 11 | Punctuation and full-width | Type `，。「」？！` | Committed correctly |
| 12 | Text rendering | Inspect the font | No missing-glyph boxes, no broken glyphs |
| 13 | High DPI | Set display scaling to 125% and 150% | Candidate window position is not offset |
| 14 | Window movement | Drag the window while composing | Candidate window follows |

## Result table

| # | egui | iced | Slint |
|---|---|---|---|
| 1 | | | |
| 2 | | | |
| 3 | | | |
| 4 | | | |
| 5 | | | |
| 6 | | | |
| 7 | | | |
| 8 | | | |
| 9 | | | |
| 10 | | | |
| 11 | | | |
| 12 | | | |
| 13 | | | |
| 14 | | | |

## Pass/fail rule

Items 1, 2, 3, and 4 are mandatory. A framework that fails any of them is eliminated.
