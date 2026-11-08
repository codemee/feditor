//! 巨集執行：與 egui 無關，方便單元測試。

use crate::editor::model::{EditorState, Selection};
use feditor_core::Buffer;
use feditor_core::i18n::t;
use feditor_core::tf;
use feditor_script::{EditorApi, ScriptEngine};
use feditor_script_js::JsEngine;
use std::ops::Range;

/// 一次巨集執行的結果。
#[derive(Debug)]
pub struct MacroReport {
    /// `console.log` 的輸出。
    pub output: Vec<String>,
    /// 失敗時的錯誤訊息。
    pub error: Option<String>,
    /// 失敗時是否有還原已做的修改。
    pub rolled_back: bool,
}

/// 巨集看到的編輯器：文字在緩衝區，選取與指令在編輯狀態。
/// 巨集直接改文字（`insert`／`remove`／`replace`）時，選取會跟著文字移動。
struct Host<'a> {
    buffer: &'a mut Buffer,
    state: &'a mut EditorState,
    path: Option<String>,
    /// 可以跳出對話框（測試時不行）。
    interactive: bool,
    /// 沒有對話框可用時，`alert` 的內容改記在這裡，會併進巨集的輸出。
    notes: Vec<String>,
}

/// 文字的 `edit` 範圍被換成 `new_len` 個字元之後，原本的位置 `p` 變成哪裡。
fn map_pos(p: usize, edit: &Range<usize>, new_len: usize) -> usize {
    if p <= edit.start {
        p
    } else if p >= edit.end {
        p - edit.len() + new_len
    } else {
        edit.start
    }
}

impl Host<'_> {
    fn remap_selection(&mut self, edit: Range<usize>, new_len: usize) {
        let s = self.state.sel;
        self.state.sel = Selection {
            anchor: map_pos(s.anchor, &edit, new_len),
            head: map_pos(s.head, &edit, new_len),
        };
        self.state.extra.clear();
    }
}

impl EditorApi for Host<'_> {
    fn text(&self) -> String {
        self.buffer.text()
    }
    fn len_chars(&self) -> usize {
        self.buffer.len_chars()
    }
    fn line_count(&self) -> usize {
        self.buffer.line_count()
    }
    fn line(&self, idx: usize) -> Option<String> {
        self.buffer.line(idx)
    }
    fn insert(&mut self, at: usize, text: &str) {
        EditorApi::insert(self.buffer, at, text);
        self.remap_selection(at..at, text.chars().count());
    }
    fn remove(&mut self, range: Range<usize>) {
        EditorApi::remove(self.buffer, range.clone());
        self.remap_selection(range, 0);
    }
    fn replace(&mut self, range: Range<usize>, text: &str) {
        EditorApi::replace(self.buffer, range.clone(), text);
        self.remap_selection(range, text.chars().count());
    }
    fn slice(&self, range: Range<usize>) -> String {
        EditorApi::slice(self.buffer, range)
    }
    fn line_start(&self, line: usize) -> usize {
        EditorApi::line_start(self.buffer, line)
    }
    fn line_of(&self, pos: usize) -> usize {
        EditorApi::line_of(self.buffer, pos)
    }
    fn selection(&self) -> (usize, usize) {
        (self.state.sel.anchor, self.state.sel.head)
    }
    fn set_selection(&mut self, anchor: usize, head: usize) {
        let n = self.buffer.len_chars();
        self.state.sel = Selection {
            anchor: anchor.min(n),
            head: head.min(n),
        };
        self.state.extra.clear();
        self.state.scroll_to_caret = true;
    }
    fn path(&self) -> Option<String> {
        self.path.clone()
    }
    fn run_command(&mut self, name: &str, count: usize) -> Result<(), String> {
        for _ in 0..count {
            self.state.clamp(self.buffer);
            self.state
                .command(self.buffer, name)
                .ok_or_else(|| tf!("不認得的編輯指令：{name}", name = name))?;
        }
        Ok(())
    }
    fn type_text(&mut self, text: &str) {
        self.state.clamp(self.buffer);
        self.state.insert_text(self.buffer, text);
    }
    fn paste_text(&mut self, text: &str) {
        self.state.clamp(self.buffer);
        self.state.paste(self.buffer, text);
    }
    fn wrap_text(&mut self, prefix: &str, suffix: &str) {
        self.state.clamp(self.buffer);
        self.state.wrap_selections(self.buffer, prefix, suffix);
    }
    fn alert(&mut self, message: &str) {
        if self.interactive {
            rfd::MessageDialog::new()
                .set_level(rfd::MessageLevel::Info)
                .set_title(t("巨集"))
                .set_description(message)
                .set_buttons(rfd::MessageButtons::Ok)
                .show();
        } else {
            self.notes.push(format!("[alert] {message}"));
        }
    }
    fn confirm(&mut self, message: &str) -> bool {
        if !self.interactive {
            return false;
        }
        rfd::MessageDialog::new()
            .set_level(rfd::MessageLevel::Info)
            .set_title(t("巨集"))
            .set_description(message)
            .set_buttons(rfd::MessageButtons::YesNo)
            .show()
            == rfd::MessageDialogResult::Yes
    }
}

/// 以 JavaScript 執行巨集（沒有編輯狀態、沒有對話框，給測試與簡單用途）。
#[allow(dead_code)]
pub fn run_js_macro(source: &str, buffer: &mut Buffer) -> MacroReport {
    run_js_macro_in(source, buffer, &mut EditorState::default(), None, false)
}

/// 以 JavaScript 執行巨集。失敗時把緩衝區與選取還原成執行前的樣子，不留下做到一半的修改；
/// 成功時整支巨集算一個復原步驟。巨集執行期間不會被錄進正在進行的錄製。
pub fn run_js_macro_in(
    source: &str,
    buffer: &mut Buffer,
    state: &mut EditorState,
    path: Option<String>,
    interactive: bool,
) -> MacroReport {
    let mut engine = match JsEngine::new() {
        Ok(engine) => engine,
        Err(e) => {
            return MacroReport {
                output: Vec::new(),
                error: Some(e.to_string()),
                rolled_back: false,
            };
        }
    };

    // 整支巨集當成一個整體：成功時併成一個復原步驟，失敗時退回執行前，兩者都不污染復原歷史
    buffer.break_group();
    let depth = buffer.undo_depth();
    state.interrupt_ime = true;
    let saved_sel = state.sel;
    let recording = state.recording.take();
    let mut host = Host {
        buffer,
        state,
        path,
        interactive,
        notes: Vec::new(),
    };
    let result = engine.run(source, &mut host);
    let notes = std::mem::take(&mut host.notes);
    let (buffer, state) = (host.buffer, host.state);
    state.recording = recording;
    let mut output = engine.take_output();
    output.extend(notes);

    match result {
        Ok(()) => {
            buffer.merge_undo_since(depth);
            buffer.break_group();
            state.clamp(buffer);
            state.scroll_to_caret = true;
            MacroReport {
                output,
                error: None,
                rolled_back: false,
            }
        }
        Err(e) => {
            let rolled_back = buffer.undo_depth() > depth;
            buffer.rollback_to(depth);
            state.sel = saved_sel;
            state.extra.clear();
            state.clamp(buffer);
            MacroReport {
                output,
                error: Some(e.to_string()),
                rolled_back,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn success_keeps_changes_and_collects_output() {
        let mut buf = Buffer::from_text("abc");
        let r = run_js_macro("editor.insert(3, 'd'); console.log('done');", &mut buf);
        assert!(r.error.is_none());
        assert!(!r.rolled_back);
        assert_eq!(r.output, vec!["done"]);
        assert_eq!(buf.text(), "abcd");
    }

    #[test]
    fn failure_rolls_back_partial_edits() {
        let mut buf = Buffer::from_text("abc");
        let r = run_js_macro(
            "editor.insert(0, 'X'); console.log('before'); throw new Error('boom');",
            &mut buf,
        );
        assert!(r.error.as_deref().is_some_and(|e| e.contains("boom")));
        assert!(r.rolled_back);
        assert_eq!(r.output, vec!["before"]);
        assert_eq!(buf.text(), "abc");
    }

    #[test]
    fn example_uppercase() {
        let mut buf = Buffer::from_text("hello 你好\nworld\n");
        let r = run_js_macro(include_str!("../../examples/macros/uppercase.js"), &mut buf);
        assert!(r.error.is_none(), "{:?}", r.error);
        assert_eq!(buf.text(), "HELLO 你好\nWORLD\n");
    }

    #[test]
    fn example_line_numbers() {
        let mut buf = Buffer::from_text("你好\n😀 emoji\nlast\n");
        let r = run_js_macro(
            include_str!("../../examples/macros/line-numbers.js"),
            &mut buf,
        );
        assert!(r.error.is_none(), "{:?}", r.error);
        assert_eq!(buf.text(), "1: 你好\n2: 😀 emoji\n3: last\n");
    }

    #[test]
    fn example_fail_demo_rolls_back() {
        let mut buf = Buffer::from_text("原文\n");
        let r = run_js_macro(include_str!("../../examples/macros/fail-demo.js"), &mut buf);
        assert!(r.error.is_some());
        assert!(r.rolled_back);
        assert_eq!(buf.text(), "原文\n");
    }

    #[test]
    fn failure_without_edits_is_not_a_rollback() {
        let mut buf = Buffer::from_text("abc");
        let r = run_js_macro("let = ;", &mut buf);
        assert!(r.error.is_some());
        assert!(!r.rolled_back);
        assert_eq!(buf.text(), "abc");
    }

    #[test]
    fn successful_macro_is_a_single_undo_step() {
        let mut buf = Buffer::from_text("abc");
        let before = buf.undo_depth();
        let r = run_js_macro("editor.insert(0, 'X'); editor.insert(0, 'Y');", &mut buf);
        assert!(r.error.is_none());
        assert_eq!(buf.text(), "YXabc");
        assert_eq!(buf.undo_depth(), before + 1);
        buf.undo();
        assert_eq!(buf.text(), "abc");
    }

    #[test]
    fn failed_macro_leaves_no_trace_in_history() {
        let mut buf = Buffer::from_text("abc");
        buf.insert(3, "d");
        let depth = buf.undo_depth();
        run_js_macro("editor.insert(0, 'X'); throw new Error('x');", &mut buf);
        assert_eq!(buf.undo_depth(), depth);
        assert!(!buf.can_redo());
        assert_eq!(buf.text(), "abcd");
    }

    fn run_in(src: &str, text: &str, caret: (usize, usize)) -> (Buffer, EditorState, MacroReport) {
        let mut buf = Buffer::from_text(text);
        let mut state = EditorState::default();
        state.sel = Selection { anchor: caret.0, head: caret.1 };
        let r = run_js_macro_in(src, &mut buf, &mut state, Some("C:\\x\\a.txt".to_owned()), false);
        (buf, state, r)
    }

    #[test]
    fn selection_api_reads_and_writes_the_editor_selection() {
        let (_, state, r) = run_in(
            "const [a, h] = editor.selection(); console.log(a, h, editor.selectedText(), editor.path()); editor.setSelection(0, 2);",
            "hello",
            (1, 4),
        );
        assert_eq!(r.error, None);
        assert_eq!(r.output, vec!["1 4 ell C:\\x\\a.txt"]);
        assert_eq!(state.sel, Selection { anchor: 0, head: 2 });
    }

    #[test]
    fn run_executes_editor_commands_like_keys() {
        let (buf, state, r) = run_in(
            "editor.run('selectAll'); editor.type('X'); editor.run('left', 1); editor.type('Y');",
            "abc",
            (0, 0),
        );
        assert_eq!(r.error, None, "{:?}", r.error);
        assert_eq!(buf.text(), "YX");
        assert_eq!(state.sel.head, 1);
    }

    #[test]
    fn wrap_surrounds_the_selection_and_keeps_it_on_the_original_text() {
        let (buf, state, r) = run_in("editor.wrap('<', '>'); editor.wrap('\"', '\"');", "abc def ghi", (4, 7));
        assert_eq!(r.error, None, "{:?}", r.error);
        assert_eq!(buf.text(), "abc <\"def\"> ghi", "第二次包的是還選著的 def");
        assert_eq!(state.sel.range(), 6..9, "選取還在 def 上");
    }

    #[test]
    fn unknown_command_fails_and_rolls_back_text_and_selection() {
        let (buf, state, r) = run_in("editor.type('Z'); editor.run('nope');", "abc", (1, 2));
        assert!(r.error.as_deref().is_some_and(|e| e.contains("nope")));
        assert!(r.rolled_back);
        assert_eq!(buf.text(), "abc");
        assert_eq!(state.sel, Selection { anchor: 1, head: 2 }, "選取也要還原");
    }

    #[test]
    fn direct_edits_move_the_selection_along_with_the_text() {
        // 在選取前面插入：選取往後移；在後面插入：不動；刪掉包住選取的範圍：收到起點
        let (_, state, _) = run_in("editor.insert(0, '12');", "abcdef", (2, 4));
        assert_eq!(state.sel, Selection { anchor: 4, head: 6 });
        let (_, state, _) = run_in("editor.insert(5, '12');", "abcdef", (2, 4));
        assert_eq!(state.sel, Selection { anchor: 2, head: 4 });
        let (_, state, _) = run_in("editor.remove(1, 5);", "abcdef", (2, 4));
        assert_eq!(state.sel, Selection { anchor: 1, head: 1 });
        let (_, state, _) = run_in("editor.replace(0, 1, 'XYZ');", "abcdef", (2, 4));
        assert_eq!(state.sel, Selection { anchor: 4, head: 6 });
    }

    #[test]
    fn selection_based_macro_uppercases_only_the_selection() {
        let (buf, _, r) = run_in(
            "const [a, h] = editor.selection(); const s = Math.min(a, h), e = Math.max(a, h); editor.replace(s, e, editor.slice(s, e).toUpperCase());",
            "abc def ghi",
            (4, 7),
        );
        assert_eq!(r.error, None);
        assert_eq!(buf.text(), "abc DEF ghi");
    }

    #[test]
    fn macro_is_one_undo_step_even_with_commands() {
        let mut buf = Buffer::from_text("abc");
        let mut state = EditorState::default();
        let before = buf.undo_depth();
        run_js_macro_in("editor.type('1'); editor.type('2'); editor.run('enter');", &mut buf, &mut state, None, false);
        assert_eq!(buf.undo_depth(), before + 1);
        buf.undo();
        assert_eq!(buf.text(), "abc");
    }

    #[test]
    fn find_and_alert_work_without_dialogs() {
        let (_, _, r) = run_in(
            "const m = editor.find('b'); editor.alert('found ' + m[0]);",
            "abc",
            (0, 0),
        );
        assert_eq!(r.error, None);
        assert_eq!(r.output, vec!["[alert] found 1"]);
    }

    #[test]
    fn running_a_macro_does_not_record_into_a_recording_in_progress() {
        let mut buf = Buffer::from_text("abc");
        let mut state = EditorState::default();
        state.start_recording();
        state.record_typed("a");
        run_js_macro_in("editor.run('end'); editor.type('Z');", &mut buf, &mut state, None, false);
        let src = state.stop_recording().unwrap();
        assert_eq!(src, "// 由錄製產生\neditor.type(\"a\");\n", "巨集內的動作不重複錄下");
    }

    #[test]
    fn recorded_macro_replays_to_the_same_result() {
        // 手動操作並錄下來，再對相同的文字重播，結果要一樣
        let text = "one two\nthree four\n";
        let mut buf = Buffer::from_text(text);
        let mut s = EditorState::default();
        s.start_recording();
        for name in ["wordRight", "selectWordRight", "backspace", "down", "home"] {
            s.command(&mut buf, name);
        }
        s.insert_text(&mut buf, "★");
        s.record_typed("★");
        s.command(&mut buf, "docEnd");
        let manual = buf.text();
        let src = s.stop_recording().unwrap();

        let (replayed, _, r) = run_in(&src, text, (0, 0));
        assert_eq!(r.error, None, "{:?}\n{src}", r.error);
        assert_eq!(replayed.text(), manual, "{src}");
    }
}