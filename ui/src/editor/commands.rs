//! 以名稱執行的編輯指令。
//!
//! 按鍵、巨集（`editor.run("name")`）與巨集錄製共用這一層：按鍵先翻成指令名稱再執行，
//! 錄製時就只要記下名稱，重播時結果和手動操作完全一樣。

use super::model::EditorState;
use feditor_core::Buffer;

/// 所有指令名稱（給測試用）。
#[cfg(test)]
pub const COMMANDS: &[&str] = &[
    "left", "right", "up", "down", "wordLeft", "wordRight", "pageUp", "pageDown", "home", "end",
    "docStart", "docEnd", "selectLeft", "selectRight", "selectUp", "selectDown", "selectWordLeft",
    "selectWordRight", "selectPageUp", "selectPageDown", "selectHome", "selectEnd",
    "selectDocStart", "selectDocEnd", "selectAll", "backspace", "delete", "deleteWordLeft",
    "deleteWordRight", "enter", "tab", "shiftTab", "undo", "redo", "moveLineUp", "moveLineDown",
    "duplicateLine", "deleteLine", "toggleComment", "indent", "unindent", "addCaretUp",
    "addCaretDown", "selectNextOccurrence", "clearCarets",
];

impl EditorState {
    /// 執行指令 `name`。回傳 `None` 表示不認得；否則回傳這次操作「算不算有動作」
    /// （移動永遠算；編輯類要真的改了文字才算）。有在錄製時會把指令記下來。
    pub fn command(&mut self, buf: &mut Buffer, name: &str) -> Option<bool> {
        let page = self.view_rows.max(2) as isize - 1;
        let did = match name {
            "left" => self.move_left(buf, false, false).into_true(),
            "right" => self.move_right(buf, false, false).into_true(),
            "wordLeft" => self.move_left(buf, false, true).into_true(),
            "wordRight" => self.move_right(buf, false, true).into_true(),
            "selectLeft" => self.move_left(buf, true, false).into_true(),
            "selectRight" => self.move_right(buf, true, false).into_true(),
            "selectWordLeft" => self.move_left(buf, true, true).into_true(),
            "selectWordRight" => self.move_right(buf, true, true).into_true(),
            "up" => self.move_vertical(buf, -1, false).into_true(),
            "down" => self.move_vertical(buf, 1, false).into_true(),
            "selectUp" => self.move_vertical(buf, -1, true).into_true(),
            "selectDown" => self.move_vertical(buf, 1, true).into_true(),
            "pageUp" => self.move_vertical(buf, -page, false).into_true(),
            "pageDown" => self.move_vertical(buf, page, false).into_true(),
            "selectPageUp" => self.move_vertical(buf, -page, true).into_true(),
            "selectPageDown" => self.move_vertical(buf, page, true).into_true(),
            "home" => self.home(buf, false).into_true(),
            "end" => self.end(buf, false).into_true(),
            "selectHome" => self.home(buf, true).into_true(),
            "selectEnd" => self.end(buf, true).into_true(),
            "docStart" => self.doc_start(buf, false).into_true(),
            "docEnd" => self.doc_end(buf, false).into_true(),
            "selectDocStart" => self.doc_start(buf, true).into_true(),
            "selectDocEnd" => self.doc_end(buf, true).into_true(),
            "selectAll" => self.select_all(buf).into_true(),
            "backspace" => self.backspace(buf, false),
            "delete" => self.delete_forward(buf, false),
            "deleteWordLeft" => self.backspace(buf, true),
            "deleteWordRight" => self.delete_forward(buf, true),
            "enter" => self.enter(buf),
            "tab" => self.tab(buf),
            "shiftTab" => self.shift_tab(buf),
            "undo" => self.undo(buf),
            "redo" => self.redo(buf),
            "moveLineUp" => self.move_lines(buf, true),
            "moveLineDown" => self.move_lines(buf, false),
            "duplicateLine" => self.duplicate_lines(buf),
            "deleteLine" => self.delete_lines(buf),
            "toggleComment" => self.toggle_comment(buf),
            "indent" => self.indent_lines(buf),
            "unindent" => self.unindent_lines(buf),
            "addCaretUp" => self.add_caret_vertical(buf, true),
            "addCaretDown" => self.add_caret_vertical(buf, false),
            "selectNextOccurrence" => self.add_next_occurrence(buf),
            "clearCarets" => self.clear_extra().into_true(),
            _ => return None,
        };
        // 沒有真的做事的編輯（例如在開頭按 Backspace）也照樣記，重播時才會和當時一樣
        if let Some(rec) = &mut self.recording {
            rec.push(Recorded::Command(name.to_owned()));
        }
        Some(did)
    }

    /// 使用者打字（含輸入法確定的文字）：錄製時記下。
    pub fn record_typed(&mut self, text: &str) {
        if let Some(rec) = &mut self.recording {
            rec.push(Recorded::Type(text.to_owned()));
        }
    }

    pub fn record_pasted(&mut self, text: &str) {
        if let Some(rec) = &mut self.recording {
            rec.push(Recorded::Paste(text.to_owned()));
        }
    }

    pub fn is_recording(&self) -> bool {
        self.recording.is_some()
    }

    pub fn start_recording(&mut self) {
        self.recording = Some(Vec::new());
    }

    /// 結束錄製，回傳錄下來的 JavaScript 巨集原始碼（沒有任何操作時為 `None`）。
    pub fn stop_recording(&mut self) -> Option<String> {
        let rec = self.recording.take()?;
        (!rec.is_empty()).then(|| render_macro(&rec))
    }
}

/// 錄下來的一個動作。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Recorded {
    Command(String),
    Type(String),
    Paste(String),
}

trait IntoTrue {
    fn into_true(self) -> bool;
}

impl IntoTrue for () {
    fn into_true(self) -> bool {
        true
    }
}

/// 轉成 JavaScript 字串常值（雙引號）。
pub fn js_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            // U+2028／U+2029 在舊的 JavaScript 規範裡是換行；其他控制字元一律跳脫
            c if (c as u32) < 0x20 || c == '\u{2028}' || c == '\u{2029}' || c == '\u{7f}' => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// 把錄下來的動作轉成巨集原始碼：連續的打字併成一句，連續重複的指令用次數表示。
fn render_macro(rec: &[Recorded]) -> String {
    let mut lines: Vec<String> = Vec::new();
    let mut i = 0;
    while i < rec.len() {
        match &rec[i] {
            Recorded::Type(_) => {
                let mut text = String::new();
                while let Some(Recorded::Type(t)) = rec.get(i) {
                    text.push_str(t);
                    i += 1;
                }
                lines.push(format!("editor.type({});", js_string(&text)));
            }
            Recorded::Paste(t) => {
                lines.push(format!("editor.paste({});", js_string(t)));
                i += 1;
            }
            Recorded::Command(name) => {
                let mut n = 0;
                while rec.get(i) == Some(&Recorded::Command(name.clone())) {
                    n += 1;
                    i += 1;
                }
                if n == 1 {
                    lines.push(format!("editor.run({});", js_string(name)));
                } else {
                    lines.push(format!("editor.run({}, {n});", js_string(name)));
                }
            }
        }
    }
    format!("// 由錄製產生\n{}\n", lines.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::model::Selection;

    fn state(text: &str, caret: usize) -> (Buffer, EditorState) {
        let mut s = EditorState::default();
        s.sel = Selection::collapsed(caret);
        (Buffer::from_text(text), s)
    }

    #[test]
    fn every_listed_command_is_known_and_unknown_names_are_rejected() {
        for name in COMMANDS {
            let (mut b, mut s) = state("a b\nc d\n", 2);
            assert!(s.command(&mut b, name).is_some(), "{name}");
        }
        let (mut b, mut s) = state("x", 0);
        assert_eq!(s.command(&mut b, "nope"), None);
    }

    #[test]
    fn commands_behave_like_the_keys() {
        let (mut b, mut s) = state("hello world", 0);
        s.command(&mut b, "wordRight");
        assert_eq!(s.sel.head, 6, "詞右移停在下一個詞的開頭");
        s.command(&mut b, "selectEnd");
        assert_eq!(s.sel.range(), 6..11);
        s.command(&mut b, "backspace");
        assert_eq!(b.text(), "hello ");
        assert_eq!(s.command(&mut b, "undo"), Some(true));
        assert_eq!(b.text(), "hello world");
        s.command(&mut b, "selectAll");
        assert_eq!(s.sel.range(), 0..11);
    }

    #[test]
    fn edit_commands_report_whether_they_did_anything() {
        let (mut b, mut s) = state("a", 0);
        assert_eq!(s.command(&mut b, "backspace"), Some(false), "在開頭沒東西可刪");
        assert_eq!(s.command(&mut b, "delete"), Some(true));
        assert_eq!(s.command(&mut b, "right"), Some(true), "移動永遠算有動作");
    }

    #[test]
    fn recording_collects_commands_and_typing() {
        let (mut b, mut s) = state("abc", 0);
        assert!(!s.is_recording());
        s.start_recording();
        s.command(&mut b, "end");
        s.insert_text(&mut b, "x");
        s.record_typed("x");
        s.record_typed("y");
        for _ in 0..3 {
            s.command(&mut b, "left");
        }
        s.record_pasted("p\"q");
        let src = s.stop_recording().unwrap();
        assert!(!s.is_recording());
        assert_eq!(
            src,
            "// 由錄製產生\neditor.run(\"end\");\neditor.type(\"xy\");\neditor.run(\"left\", 3);\neditor.paste(\"p\\\"q\");\n"
        );
    }

    #[test]
    fn nothing_recorded_gives_none_and_commands_are_not_recorded_when_idle() {
        let (mut b, mut s) = state("abc", 0);
        s.command(&mut b, "end");
        assert_eq!(s.stop_recording(), None);
        s.start_recording();
        assert_eq!(s.stop_recording(), None, "空的錄製不產生巨集");
    }

    #[test]
    fn js_string_escapes_everything_that_could_break_a_literal() {
        assert_eq!(js_string("a\"b\\c\nd\te"), "\"a\\\"b\\\\c\\nd\\te\"");
        assert_eq!(js_string("\u{0}\u{2028}"), "\"\\u0000\\u2028\"");
        assert_eq!(js_string("中文😀"), "\"中文😀\"");
    }
}
