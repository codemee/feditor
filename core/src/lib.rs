//! feditor 核心：與 UI、巨集引擎無關的文字緩衝區。
//!
//! 所有位置一律以「字元（char）索引」表示，不是位元組；需要位元組時用 [`Buffer::char_to_byte`]。
//! 緩衝區內的換行一律是 `\n`（讀檔時已統一，見 [`fileio`]）。

pub mod fileio;
pub mod grep;
pub mod i18n;
pub mod motion;
pub mod search;

use ropey::Rope;
use std::ops::Range;

/// 一次編輯操作，用於 undo／redo。
#[derive(Debug, Clone)]
enum Edit {
    Insert { at: usize, text: String },
    Remove { at: usize, text: String },
}

impl Edit {
    fn at(&self) -> usize {
        match self {
            Edit::Insert { at, .. } | Edit::Remove { at, .. } => *at,
        }
    }

    fn len_chars(&self) -> usize {
        match self {
            Edit::Insert { text, .. } | Edit::Remove { text, .. } => text.chars().count(),
        }
    }
}

/// 哪一種連續操作可以併進同一個復原步驟。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Coalesce {
    /// 連續打字。
    Typing,
    /// 連續刪除（Backspace 或 Delete）。
    Deleting,
}

/// 文字緩衝區，底層為 rope，並附帶 undo／redo 歷史。
///
/// 復原以「組」為單位：一般的 [`insert`](Self::insert)／[`remove`](Self::remove)／
/// [`replace`](Self::replace) 各自成一組；[`typed`](Self::typed) 與 [`deleted`](Self::deleted)
/// 會把連續的打字、連續的刪除併成一組，這樣按一次復原會退回一段話，而不是一個字元。
#[derive(Debug, Default)]
pub struct Buffer {
    rope: Rope,
    undo: Vec<Vec<Edit>>,
    redo: Vec<Vec<Edit>>,
    coalesce: Option<Coalesce>,
    /// 每次文字內容被改動就加一。畫面用它判斷緩衝區有沒有變（包含巨集等外部改動）。
    revision: u64,
}

impl Buffer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_text(text: &str) -> Self {
        Self {
            rope: Rope::from_str(text),
            ..Self::default()
        }
    }

    /// 修訂編號：文字內容每被改動一次（含復原、重做）就加一，純讀取與沒有實際改動的呼叫不會變。
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// 底層的 rope（唯讀），給游標移動等需要直接走訪文字的函式使用。
    pub fn rope(&self) -> &Rope {
        &self.rope
    }

    /// 總字元數。
    pub fn len_chars(&self) -> usize {
        self.rope.len_chars()
    }

    /// 總位元組數。
    pub fn len_bytes(&self) -> usize {
        self.rope.len_bytes()
    }

    pub fn is_empty(&self) -> bool {
        self.rope.len_chars() == 0
    }

    /// 行數（空緩衝區為 1 行；文字以換行結尾時，最後多一個空行）。
    pub fn line_count(&self) -> usize {
        self.rope.len_lines()
    }

    /// 取得第 `idx` 行（含換行字元）；超出範圍回傳 `None`。
    pub fn line(&self, idx: usize) -> Option<String> {
        (idx < self.rope.len_lines()).then(|| self.rope.line(idx).to_string())
    }

    /// 第 `idx` 行的文字，不含結尾的換行；超出範圍時回傳空字串。
    pub fn line_text(&self, idx: usize) -> String {
        let Some(line) = self.rope.get_line(idx) else {
            return String::new();
        };
        let mut text = line.to_string();
        if text.ends_with('\n') {
            text.pop();
        }
        text
    }

    /// 第 `idx` 行的字元數，不含結尾的換行。
    pub fn line_len_chars(&self, idx: usize) -> usize {
        let Some(line) = self.rope.get_line(idx) else {
            return 0;
        };
        let n = line.len_chars();
        if n > 0 && line.char(n - 1) == '\n' {
            n - 1
        } else {
            n
        }
    }

    /// 第 `idx` 行第一個字元的索引；`idx` 超出範圍時回傳文字末端。
    pub fn line_start(&self, idx: usize) -> usize {
        if idx >= self.rope.len_lines() {
            self.rope.len_chars()
        } else {
            self.rope.line_to_char(idx)
        }
    }

    /// 字元所在的行（超出末端時視為最後一行）。
    pub fn line_of_char(&self, char_idx: usize) -> usize {
        self.rope.char_to_line(char_idx.min(self.rope.len_chars()))
    }

    pub fn char_to_byte(&self, char_idx: usize) -> usize {
        self.rope.char_to_byte(char_idx.min(self.rope.len_chars()))
    }

    pub fn byte_to_char(&self, byte_idx: usize) -> usize {
        self.rope.byte_to_char(byte_idx.min(self.rope.len_bytes()))
    }

    /// 取得字元範圍內的文字；範圍超出緩衝區會被截斷。
    pub fn slice(&self, range: Range<usize>) -> String {
        let end = range.end.min(self.len_chars());
        let start = range.start.min(end);
        self.rope.slice(start..end).to_string()
    }

    /// 完整文字。
    pub fn text(&self) -> String {
        self.rope.to_string()
    }

    // ── 編輯 ────────────────────────────────────────────────

    /// 在字元位置 `at` 插入文字；`at` 超出末端時插在最後。自成一個復原步驟。
    pub fn insert(&mut self, at: usize, text: &str) {
        if text.is_empty() {
            return;
        }
        let at = at.min(self.len_chars());
        self.rope.insert(at, text);
        self.revision += 1;
        self.push_group(vec![Edit::Insert {
            at,
            text: text.to_owned(),
        }]);
    }

    /// 刪除字元範圍；範圍超出緩衝區會被截斷。自成一個復原步驟。
    pub fn remove(&mut self, range: Range<usize>) {
        if let Some(edit) = self.remove_raw(range) {
            self.push_group(vec![edit]);
        }
    }

    /// 取代字元範圍。先刪後插，兩者算同一個復原步驟。
    pub fn replace(&mut self, range: Range<usize>, text: &str) {
        let start = range.start.min(self.len_chars());
        let mut group = Vec::new();
        group.extend(self.remove_raw(range));
        if !text.is_empty() {
            self.rope.insert(start, text);
            self.revision += 1;
            group.push(Edit::Insert {
                at: start,
                text: text.to_owned(),
            });
        }
        if !group.is_empty() {
            self.push_group(group);
        }
    }

    /// 打字：與緊接在前面的連續打字併成同一個復原步驟。
    /// 打字位置不連續、或這次輸入含換行時，另起一個步驟。
    pub fn typed(&mut self, at: usize, text: &str) {
        if text.is_empty() {
            return;
        }
        let at = at.min(self.len_chars());
        let joins_previous = self.coalesce == Some(Coalesce::Typing)
            && !text.contains('\n')
            && matches!(
                self.undo.last().and_then(|g| g.last()),
                Some(edit @ Edit::Insert { at: prev, .. }) if prev + edit.len_chars() == at
            );
        self.rope.insert(at, text);
        self.revision += 1;
        let edit = Edit::Insert {
            at,
            text: text.to_owned(),
        };
        if joins_previous {
            self.redo.clear();
            if let Some(group) = self.undo.last_mut() {
                group.push(edit);
            }
            self.coalesce = Some(Coalesce::Typing);
        } else {
            self.push_group(vec![edit]);
            // 含換行的輸入單獨成一步，之後的打字不要接在它後面
            self.coalesce = (!text.contains('\n')).then_some(Coalesce::Typing);
        }
    }

    /// 刪除：與緊接在前面的連續刪除（連按 Backspace 或 Delete）併成同一個復原步驟。
    pub fn deleted(&mut self, range: Range<usize>) {
        let Some(edit) = self.remove_raw(range) else {
            return;
        };
        let Edit::Remove { at, .. } = &edit else {
            unreachable!()
        };
        let at = *at;
        let joins_previous = self.coalesce == Some(Coalesce::Deleting)
            && matches!(
                self.undo.last().and_then(|g| g.last()),
                // Backspace：這次刪的範圍緊貼在上次的前面；Delete：這次從上次的起點開始
                Some(Edit::Remove { at: prev, .. }) if at + edit.len_chars() == *prev || at == *prev
            );
        if joins_previous {
            self.redo.clear();
            if let Some(group) = self.undo.last_mut() {
                group.push(edit);
            }
        } else {
            self.push_group(vec![edit]);
        }
        self.coalesce = Some(Coalesce::Deleting);
    }

    /// 結束目前的連續操作：之後的打字或刪除會另起一個復原步驟。
    /// 游標移動、貼上、切換視窗焦點等「打斷輸入」的時機要呼叫。
    pub fn break_group(&mut self) {
        self.coalesce = None;
    }

    fn remove_raw(&mut self, range: Range<usize>) -> Option<Edit> {
        let end = range.end.min(self.len_chars());
        let start = range.start.min(end);
        if start == end {
            return None;
        }
        let removed = self.rope.slice(start..end).to_string();
        self.rope.remove(start..end);
        self.revision += 1;
        Some(Edit::Remove {
            at: start,
            text: removed,
        })
    }

    fn push_group(&mut self, group: Vec<Edit>) {
        self.undo.push(group);
        self.redo.clear();
        self.coalesce = None;
    }

    // ── 復原與重做 ──────────────────────────────────────────

    /// 目前有幾個復原步驟。跟 [`merge_undo_since`](Self::merge_undo_since)、
    /// [`rollback_to`](Self::rollback_to) 搭配，用來把一連串編輯當成一個整體處理（例如執行巨集）。
    pub fn undo_depth(&self) -> usize {
        self.undo.len()
    }

    /// 把 `depth` 之後累積的所有復原步驟併成一個，這樣一次復原就能退回整段操作。
    pub fn merge_undo_since(&mut self, depth: usize) {
        if self.undo.len() <= depth.saturating_add(1) {
            return;
        }
        let merged: Vec<Edit> = self.undo.drain(depth..).flatten().collect();
        self.undo.push(merged);
        self.coalesce = None;
    }

    /// 退回到只有 `depth` 個復原步驟的狀態，並且不留下重做紀錄（像是這些編輯從未發生）。
    pub fn rollback_to(&mut self, depth: usize) {
        while self.undo.len() > depth {
            self.undo();
        }
        self.redo.clear();
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// 復原上一個步驟。回傳這一步影響到的範圍，呼叫端把游標放在範圍的**末端**。
    /// 退回一段插入時，末端是插入的起點；退回一段刪除時，末端是被還原文字的後面。
    /// 沒有可復原的內容時回傳 `None`。
    pub fn undo(&mut self) -> Option<Range<usize>> {
        let group = self.undo.pop()?;
        self.coalesce = None;
        self.revision += 1;
        for edit in group.iter().rev() {
            match edit {
                Edit::Insert { at, text } => {
                    self.rope.remove(*at..*at + text.chars().count());
                }
                Edit::Remove { at, text } => self.rope.insert(*at, text),
            }
        }
        // 受影響範圍：起點是這一組所有編輯位置的最小值；末端由「最早」的那個編輯決定
        // （退回插入：停在插入的起點；退回刪除：停在被還原文字的後面）
        let start = group.iter().map(Edit::at).min()?;
        let end = match group.first()? {
            Edit::Insert { at, .. } => *at,
            edit @ Edit::Remove { at, .. } => *at + edit.len_chars(),
        };
        self.redo.push(group);
        Some(start..end)
    }

    /// 重做上一個被復原的步驟。回傳這一步影響到的範圍，呼叫端把游標放在範圍的**末端**。
    pub fn redo(&mut self) -> Option<Range<usize>> {
        let group = self.redo.pop()?;
        self.coalesce = None;
        self.revision += 1;
        for edit in &group {
            match edit {
                Edit::Insert { at, text } => self.rope.insert(*at, text),
                Edit::Remove { at, text } => {
                    self.rope.remove(*at..*at + text.chars().count());
                }
            }
        }
        // 受影響範圍：起點是這一組所有編輯位置的最小值；末端由「最後」的那個編輯決定
        // （重做插入：停在插入文字的後面；重做刪除：停在刪除處）
        let start = group.iter().map(Edit::at).min()?;
        let end = match group.last()? {
            edit @ Edit::Insert { at, .. } => *at + edit.len_chars(),
            Edit::Remove { at, .. } => *at,
        };
        self.undo.push(group);
        Some(start..end)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_and_remove_use_char_indices() {
        let mut b = Buffer::from_text("你好世界");
        b.insert(2, "，");
        assert_eq!(b.text(), "你好，世界");
        b.remove(2..3);
        assert_eq!(b.text(), "你好世界");
    }

    #[test]
    fn out_of_range_is_clamped() {
        let mut b = Buffer::from_text("abc");
        b.insert(100, "d");
        assert_eq!(b.text(), "abcd");
        b.remove(2..100);
        assert_eq!(b.text(), "ab");
        assert_eq!(b.slice(1..100), "b");
    }

    #[test]
    fn undo_redo_roundtrip() {
        let mut b = Buffer::from_text("abc");
        b.insert(3, "def");
        b.remove(0..2);
        assert_eq!(b.text(), "cdef");
        assert!(b.undo().is_some());
        assert_eq!(b.text(), "abcdef");
        assert!(b.undo().is_some());
        assert_eq!(b.text(), "abc");
        assert!(b.undo().is_none());
        assert!(b.redo().is_some());
        assert!(b.redo().is_some());
        assert_eq!(b.text(), "cdef");
        assert!(b.redo().is_none());
    }

    #[test]
    fn new_edit_clears_redo() {
        let mut b = Buffer::from_text("a");
        b.insert(1, "b");
        b.undo();
        b.insert(1, "c");
        assert!(b.redo().is_none());
        assert_eq!(b.text(), "ac");
    }

    #[test]
    fn replace_is_a_single_undo_step() {
        let mut b = Buffer::from_text("hello world");
        b.replace(6..11, "你好");
        assert_eq!(b.text(), "hello 你好");
        b.undo();
        assert_eq!(b.text(), "hello world", "先刪後插算同一步，一次復原要整個退回");
        b.redo();
        assert_eq!(b.text(), "hello 你好");
        assert!(!b.can_redo());
    }

    #[test]
    fn replace_with_empty_or_into_empty_range() {
        let mut b = Buffer::from_text("abc");
        b.replace(1..2, ""); // 純刪除
        assert_eq!(b.text(), "ac");
        b.replace(1..1, "XY"); // 純插入
        assert_eq!(b.text(), "aXYc");
        b.replace(1..1, ""); // 什麼都不做，不該留下空的復原步驟
        b.undo();
        assert_eq!(b.text(), "ac");
        b.undo();
        assert_eq!(b.text(), "abc");
        assert!(!b.can_undo());
    }

    #[test]
    fn lines() {
        let b = Buffer::from_text("a\nb\n");
        assert_eq!(b.line_count(), 3);
        assert_eq!(b.line(0).as_deref(), Some("a\n"));
        assert_eq!(b.line(3), None);
    }

    #[test]
    fn line_helpers_exclude_the_newline() {
        let b = Buffer::from_text("你好\n\nabc");
        assert_eq!(b.line_count(), 3);
        assert_eq!(b.line_text(0), "你好");
        assert_eq!(b.line_text(1), "");
        assert_eq!(b.line_text(2), "abc");
        assert_eq!(b.line_text(99), "");
        assert_eq!(b.line_len_chars(0), 2);
        assert_eq!(b.line_len_chars(1), 0);
        assert_eq!(b.line_len_chars(2), 3);
        assert_eq!(b.line_len_chars(99), 0);
        assert_eq!((b.line_start(0), b.line_start(1), b.line_start(2)), (0, 3, 4));
        assert_eq!(b.line_start(99), b.len_chars());
        assert_eq!(b.line_of_char(0), 0);
        assert_eq!(b.line_of_char(2), 0); // 第 0 行的換行字元
        assert_eq!(b.line_of_char(3), 1);
        assert_eq!(b.line_of_char(4), 2);
        assert_eq!(b.line_of_char(999), 2);
    }

    #[test]
    fn byte_and_char_conversions() {
        let b = Buffer::from_text("a你😀b");
        assert_eq!(b.len_chars(), 4);
        assert_eq!(b.len_bytes(), 1 + 3 + 4 + 1);
        assert_eq!(b.char_to_byte(2), 4);
        assert_eq!(b.byte_to_char(4), 2);
        assert_eq!(b.char_to_byte(999), b.len_bytes());
    }

    #[test]
    fn typing_is_coalesced_into_one_undo_step() {
        let mut b = Buffer::new();
        for (i, ch) in "hello".chars().enumerate() {
            b.typed(i, &ch.to_string());
        }
        assert_eq!(b.text(), "hello");
        assert_eq!(b.undo(), Some(0..0), "游標退回打字的起點");
        assert_eq!(b.text(), "");
        assert!(!b.can_undo(), "五個字只該是一步");
        b.redo();
        assert_eq!(b.text(), "hello");
    }

    #[test]
    fn typing_at_a_different_place_starts_a_new_step() {
        let mut b = Buffer::from_text("....");
        b.typed(0, "a");
        b.typed(1, "b"); // 接在後面：併入同一步
        b.typed(5, "c"); // 跳到別處：另起一步
        assert_eq!(b.text(), "ab...c.");
        b.undo();
        assert_eq!(b.text(), "ab....");
        b.undo();
        assert_eq!(b.text(), "....");
        assert!(!b.can_undo());
    }
    #[test]
    fn newline_ends_a_typing_group() {
        let mut b = Buffer::new();
        b.typed(0, "a");
        b.typed(1, "b");
        b.typed(2, "\n");
        b.typed(3, "c");
        assert_eq!(b.text(), "ab\nc");
        b.undo();
        assert_eq!(b.text(), "ab\n", "換行後的打字是另一步");
        b.undo();
        assert_eq!(b.text(), "ab", "含換行的輸入自己是一步");
        b.undo();
        assert_eq!(b.text(), "");
    }

    #[test]
    fn break_group_separates_typing() {
        let mut b = Buffer::new();
        b.typed(0, "a");
        b.break_group(); // 例如使用者移動了游標
        b.typed(1, "b");
        b.undo();
        assert_eq!(b.text(), "a");
        b.undo();
        assert_eq!(b.text(), "");
    }

    #[test]
    fn consecutive_backspaces_are_one_step() {
        let mut b = Buffer::from_text("abcdef");
        // 游標在末端，連按三次 Backspace
        b.deleted(5..6);
        b.deleted(4..5);
        b.deleted(3..4);
        assert_eq!(b.text(), "abc");
        assert_eq!(b.undo(), Some(3..6), "游標回到還原文字的後面");
        assert_eq!(b.text(), "abcdef");
        assert!(!b.can_undo());
    }

    #[test]
    fn consecutive_forward_deletes_are_one_step() {
        let mut b = Buffer::from_text("abcdef");
        b.deleted(1..2);
        b.deleted(1..2);
        b.deleted(1..2);
        assert_eq!(b.text(), "aef");
        b.undo();
        assert_eq!(b.text(), "abcdef");
        assert!(!b.can_undo());
    }

    #[test]
    fn typing_and_deleting_do_not_merge_with_each_other() {
        let mut b = Buffer::new();
        b.typed(0, "abc");
        b.deleted(2..3);
        b.typed(2, "X");
        assert_eq!(b.text(), "abX");
        b.undo();
        assert_eq!(b.text(), "ab");
        b.undo();
        assert_eq!(b.text(), "abc");
        b.undo();
        assert_eq!(b.text(), "");
    }

    #[test]
    fn undo_and_redo_report_where_the_caret_goes() {
        let mut b = Buffer::from_text("hello world");
        b.insert(5, ",");
        assert_eq!(b.undo(), Some(5..5), "退回插入：游標在插入的起點");
        assert_eq!(b.redo(), Some(5..6), "重做插入：游標在插入文字的後面");
        b.remove(0..5);
        assert_eq!(b.undo(), Some(0..5), "退回刪除：游標在還原文字的後面");
        assert_eq!(b.redo(), Some(0..0), "重做刪除：游標在刪除處");
    }

    #[test]
    fn revision_changes_exactly_when_the_text_changes() {
        let mut b = Buffer::from_text("abc");
        let r0 = b.revision();
        // 純讀取、沒有實際改動的呼叫：不變
        let _ = (b.text(), b.slice(0..2), b.line_text(0), b.len_chars());
        b.insert(0, "");
        b.remove(1..1);
        b.replace(1..1, "");
        b.deleted(2..2);
        b.typed(0, "");
        b.break_group();
        assert_eq!(b.revision(), r0);
        // 每一種改動都要增加
        let mut last = r0;
        let mut bumped = |b: &Buffer| {
            assert!(b.revision() > last, "revision 應該增加");
            last = b.revision();
        };
        b.insert(0, "x");
        bumped(&b);
        b.remove(0..1);
        bumped(&b);
        b.replace(0..1, "yy");
        bumped(&b);
        b.typed(0, "t");
        bumped(&b);
        b.deleted(0..1);
        bumped(&b);
        b.undo();
        bumped(&b);
        b.redo();
        bumped(&b);
        assert!(b.undo().is_some());
        bumped(&b);
    }

    #[test]
    fn multibyte_undo_keeps_char_positions_right() {
        let mut b = Buffer::from_text("你好😀世界");
        b.deleted(2..3); // 刪 😀
        b.typed(2, "中文");
        assert_eq!(b.text(), "你好中文世界");
        b.undo();
        b.undo();
        assert_eq!(b.text(), "你好😀世界");
        b.redo();
        b.redo();
        assert_eq!(b.text(), "你好中文世界");
    }

    #[test]
    fn merge_undo_since_makes_one_step() {
        let mut b = Buffer::from_text("abc");
        b.insert(0, "x"); // 先有的一步，不能被併進去
        let depth = b.undo_depth();
        b.insert(0, "1");
        b.insert(b.len_chars(), "2");
        b.remove(0..1);
        b.merge_undo_since(depth);
        assert_eq!(b.undo_depth(), depth + 1);
        assert_eq!(b.text(), "xabc2");
        b.undo();
        assert_eq!(b.text(), "xabc");
        b.undo();
        assert_eq!(b.text(), "abc");
        b.redo();
        b.redo();
        assert_eq!(b.text(), "xabc2");
    }

    #[test]
    fn rollback_restores_text_and_leaves_no_redo() {
        let mut b = Buffer::from_text("abc");
        b.insert(0, "x");
        let depth = b.undo_depth();
        b.insert(0, "1");
        b.replace(0..2, "ZZ");
        b.rollback_to(depth);
        assert_eq!(b.text(), "xabc");
        assert!(!b.can_redo());
        assert_eq!(b.undo_depth(), depth);
        b.undo();
        assert_eq!(b.text(), "abc");
    }}
