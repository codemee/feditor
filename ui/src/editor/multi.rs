//! 多重游標與矩形選取。
//!
//! 主要的選取是 [`EditorState::sel`]；其他的存在 [`EditorState::extra`]。輸入、刪除、貼上、
//! 移動都會套用到每一個選取，且整個動作是一個復原步驟。
//!
//! `extra` 只在「我們自己改的那一版文字」上有效：記下當時的緩衝區修訂編號
//! （[`EditorState::multi_rev`]），文字被別的方式改動（復原、取代、巨集…）就整組作廢。

use super::model::{EditorState, Selection, normalize_newlines};
use feditor_core::{Buffer, motion};
use std::ops::Range;

/// 一個編輯：把 `range` 換成 `text`。
type Edit = (Range<usize>, String);

impl EditorState {
    /// 有沒有多個選取。
    pub fn has_multi(&self) -> bool {
        !self.extra.is_empty()
    }

    /// 所有選取（含主要的），由前往後排序。第二個值是主要選取在其中的位置。
    pub fn all_selections(&self) -> (Vec<Selection>, usize) {
        let mut all: Vec<(Selection, bool)> = vec![(self.sel, true)];
        all.extend(self.extra.iter().map(|s| (*s, false)));
        normalize(all)
    }

    /// 把一組選取設成目前的狀態（第 `primary` 個是主要的）。
    fn set_selections(&mut self, buf: &Buffer, mut all: Vec<Selection>, primary: usize) {
        let primary = primary.min(all.len().saturating_sub(1));
        self.sel = all.remove(primary);
        self.extra = all;
        self.multi_rev = buf.revision();
    }

    /// 回到單一選取。
    pub fn clear_extra(&mut self) {
        self.extra.clear();
    }

    /// 文字在我們不知情的情況下變了：多重選取作廢。
    pub(super) fn drop_stale_extra(&mut self, buf: &Buffer) {
        if !self.extra.is_empty() && self.multi_rev != buf.revision() {
            self.extra.clear();
        }
        let n = buf.len_chars();
        for s in &mut self.extra {
            s.anchor = s.anchor.min(n);
            s.head = s.head.min(n);
        }
    }

    /// 在 `idx` 加上一個游標；那裡已經有選取時改成移除它（至少會留一個）。
    pub fn toggle_caret(&mut self, buf: &mut Buffer, idx: usize) {
        self.interrupt_composition();
        let idx = idx.min(buf.len_chars());
        let (mut all, primary) = self.all_selections();
        if let Some(i) = all
            .iter()
            .position(|s| s.range().start <= idx && idx <= s.range().end && (!s.is_empty() || s.head == idx))
        {
            if all.len() > 1 {
                all.remove(i);
                let p = if i < primary { primary - 1 } else if i == primary { all.len() - 1 } else { primary };
                self.set_selections(buf, all, p);
            }
        } else {
            all.push(Selection::collapsed(idx));
            let p = all.len() - 1;
            let mut tagged: Vec<(Selection, bool)> = all.into_iter().map(|s| (s, false)).collect();
            tagged[p].1 = true;
            let (all, primary) = normalize(tagged);
            self.set_selections(buf, all, primary);
        }
        self.goal_col = None;
        self.scroll_to_caret = true;
        buf.break_group();
    }

    /// 在最上面（`up`）或最下面一個選取的上一行／下一行，同一個顯示欄位加上游標。
    pub fn add_caret_vertical(&mut self, buf: &mut Buffer, up: bool) -> bool {
        self.interrupt_composition();
        let (all, _) = self.all_selections();
        let edge = if up { all.first() } else { all.last() }.copied().unwrap_or(self.sel);
        let line = buf.line_of_char(edge.head.min(buf.len_chars()));
        let target = if up {
            if line == 0 {
                return false;
            }
            line - 1
        } else {
            if line + 1 >= buf.line_count() {
                return false;
            }
            line + 1
        };
        let col = edge.head - buf.line_start(line);
        let dcol = motion::display_col(&buf.line_text(line), col);
        let idx = buf.line_start(target) + motion::char_at_display_col(&buf.line_text(target), dcol);
        let mut tagged: Vec<(Selection, bool)> = all.into_iter().map(|s| (s, false)).collect();
        tagged.push((Selection::collapsed(idx), true));
        let (all, primary) = normalize(tagged);
        self.set_selections(buf, all, primary);
        self.scroll_to_caret = true;
        buf.break_group();
        true
    }

    /// Ctrl＋D：沒有選取就先選游標下的詞；已經有選取就再加上「下一個相同的文字」。
    pub fn add_next_occurrence(&mut self, buf: &mut Buffer) -> bool {
        self.interrupt_composition();
        if self.sel.is_empty() {
            self.select_word_at(buf, self.sel.head);
            return !self.sel.is_empty();
        }
        let needle = buf.slice(self.sel.range());
        let (all, _) = self.all_selections();
        let from = all.last().map_or(0, |s| s.range().end);
        let total = buf.len_chars();
        let find = |range: Range<usize>| -> Option<usize> {
            let hay = buf.slice(range.clone());
            hay.find(&needle).map(|b| range.start + hay[..b].chars().count())
        };
        let found = find(from..total).or_else(|| {
            let first = all.first().map_or(0, |s| s.range().start);
            find(0..(first + needle.chars().count()).min(total))
        });
        let Some(start) = found else { return false };
        let new = Selection {
            anchor: start,
            head: start + needle.chars().count(),
        };
        if all.iter().any(|s| s.range() == new.range()) {
            return false; // 全部都已經選了
        }
        let mut tagged: Vec<(Selection, bool)> = all.into_iter().map(|s| (s, false)).collect();
        tagged.push((new, true));
        let (all, primary) = normalize(tagged);
        self.set_selections(buf, all, primary);
        self.scroll_to_caret = true;
        buf.break_group();
        true
    }

    /// 矩形選取：從（行, 顯示欄）`a` 拉到 `b`，每一行各一個選取；短的行只有游標。
    /// 主要選取是 `b` 所在的那一行。
    pub fn rect_select(&mut self, buf: &mut Buffer, a: (usize, usize), b: (usize, usize)) {
        self.interrupt_composition();
        let last = buf.line_count().saturating_sub(1);
        let (la, lb) = (a.0.min(last), b.0.min(last));
        let (lo, hi) = (la.min(lb), la.max(lb));
        let mut all = Vec::with_capacity(hi - lo + 1);
        let mut primary = 0;
        for line in lo..=hi {
            let text = buf.line_text(line);
            let anchor = buf.line_start(line) + motion::char_at_display_col(&text, a.1);
            let head = buf.line_start(line) + motion::char_at_display_col(&text, b.1);
            if line == lb {
                primary = all.len();
            }
            all.push(Selection { anchor, head });
        }
        self.set_selections(buf, all, primary);
        self.goal_col = None;
        self.scroll_to_caret = true;
        buf.break_group();
    }

    // ── 把單一選取的操作套用到每一個選取 ─────────────────────

    /// 對每一個選取各做一次 `f`（`f` 會看到只有那一個選取的狀態），再把結果合併回來。
    /// 用在移動游標：左右、上下、Home、End 都是各自獨立的。
    pub(super) fn for_each_selection(
        &mut self,
        buf: &mut Buffer,
        f: impl Fn(&mut EditorState, &mut Buffer),
    ) {
        let (all, primary) = self.all_selections();
        let mut tagged = Vec::with_capacity(all.len());
        self.multi_step = true;
        for (i, s) in all.into_iter().enumerate() {
            self.sel = s;
            self.extra.clear();
            self.goal_col = None;
            f(self, buf);
            tagged.push((self.sel, i == primary));
        }
        self.multi_step = false;
        let (all, primary) = normalize(tagged);
        self.set_selections(buf, all, primary);
        self.goal_col = None;
    }

    /// 每個選取各自算出一個編輯，整批套用（一個復原步驟）。`f` 收到（第幾個, 選取）。
    /// 回傳是否真的改了文字。
    pub(super) fn edit_each(
        &mut self,
        buf: &mut Buffer,
        f: impl Fn(usize, Selection, &Buffer) -> Edit,
    ) -> bool {
        self.interrupt_composition();
        let (all, primary) = self.all_selections();
        let mut edits: Vec<Edit> = all.iter().enumerate().map(|(i, s)| f(i, *s, buf)).collect();
        // 往前擴張的編輯（Backspace）可能壓到前一個的範圍：讓出重疊的部分
        let mut prev_end = 0;
        for (range, _) in &mut edits {
            if range.start < prev_end {
                range.start = prev_end.min(range.end);
            }
            prev_end = range.end.max(prev_end);
        }
        if edits.iter().all(|(r, t)| r.is_empty() && t.is_empty()) {
            return false;
        }
        buf.break_group();
        let depth = buf.undo_depth();
        for (range, text) in edits.iter().rev() {
            if !(range.is_empty() && text.is_empty()) {
                buf.replace(range.clone(), text);
            }
        }
        // 每個 replace 各是一個復原步驟：併成一步，復原時一次全部還原
        buf.merge_undo_since(depth);
        buf.break_group();
        // 每個編輯之後的游標位置：本身插入的文字之後，加上前面所有編輯造成的位移
        let mut shift: isize = 0;
        let mut carets = Vec::with_capacity(edits.len());
        for (range, text) in &edits {
            let len = text.chars().count();
            let at = (range.start as isize + shift) as usize + len;
            carets.push(Selection::collapsed(at));
            shift += len as isize - range.len() as isize;
        }
        self.set_selections(buf, carets, primary);
        self.goal_col = None;
        self.scroll_to_caret = true;
        true
    }

    pub(super) fn multi_insert(&mut self, buf: &mut Buffer, text: &str) -> bool {
        let text = normalize_newlines(text);
        self.edit_each(buf, |_, s, _| (s.range(), text.clone()))
    }

    /// 貼上：剪貼簿的行數剛好等於選取數時，一個選取一行；否則每個選取都貼同一份。
    pub(super) fn multi_paste(&mut self, buf: &mut Buffer, text: &str) -> bool {
        let text = normalize_newlines(text);
        let n = self.extra.len() + 1;
        let lines: Vec<&str> = text.trim_end_matches('\n').split('\n').collect();
        let distribute = lines.len() == n;
        self.edit_each(buf, |i, s, _| {
            let t = if distribute { lines[i].to_owned() } else { text.clone() };
            (s.range(), t)
        })
    }

    pub(super) fn multi_enter(&mut self, buf: &mut Buffer) -> bool {
        self.edit_each(buf, |_, s, b| {
            let r = s.range();
            let line = b.line_of_char(r.start);
            let col = r.start - b.line_start(line);
            let indent: String = b
                .line_text(line)
                .chars()
                .take(col)
                .take_while(|c| *c == ' ' || *c == '\t')
                .collect();
            (r, format!("\n{indent}"))
        })
    }

    pub(super) fn multi_backspace(&mut self, buf: &mut Buffer, by_word: bool) -> bool {
        self.edit_each(buf, |_, s, b| {
            if !s.is_empty() {
                return (s.range(), String::new());
            }
            let from = if by_word {
                motion::word_left(b.rope(), s.head)
            } else {
                motion::prev_grapheme(b.rope(), s.head)
            };
            (from..s.head, String::new())
        })
    }

    pub(super) fn multi_delete(&mut self, buf: &mut Buffer, by_word: bool) -> bool {
        self.edit_each(buf, |_, s, b| {
            if !s.is_empty() {
                return (s.range(), String::new());
            }
            let to = if by_word {
                motion::word_right(b.rope(), s.head)
            } else {
                motion::next_grapheme(b.rope(), s.head)
            };
            (s.head..to, String::new())
        })
    }

    /// 每個有選取的部分用換行接起來（複製、剪下用）。都沒有選取時回傳 `None`。
    pub(super) fn multi_selected_text(&self, buf: &Buffer) -> Option<String> {
        let (all, _) = self.all_selections();
        let parts: Vec<String> = all
            .iter()
            .filter(|s| !s.is_empty())
            .map(|s| buf.slice(s.range()))
            .collect();
        (!parts.is_empty()).then(|| parts.join("\n"))
    }

    pub(super) fn multi_cut(&mut self, buf: &mut Buffer) -> Option<String> {
        let text = self.multi_selected_text(buf)?;
        self.edit_each(buf, |_, s, _| (s.range(), String::new()));
        Some(text)
    }
}

/// 排序並合併重疊的選取。兩個選取只是「碰到」時：有一個是空的游標就合併，兩段都有內容就保留。
/// 合併後只要包含主要選取，結果就是主要的。回傳（選取們, 主要選取的位置）。
fn normalize(mut all: Vec<(Selection, bool)>) -> (Vec<Selection>, usize) {
    all.sort_by_key(|(s, _)| (s.range().start, s.range().end));
    let mut out: Vec<(Selection, bool)> = Vec::with_capacity(all.len());
    for (s, primary) in all {
        if let Some((last, last_primary)) = out.last_mut() {
            let (lr, r) = (last.range(), s.range());
            let touching_with_content = lr.end == r.start && !lr.is_empty() && !r.is_empty();
            if r.start <= lr.end && !touching_with_content {
                let merged = lr.start..lr.end.max(r.end);
                // 方向沿用主要的那一個
                let src = if primary { s } else { *last };
                *last = if src.anchor <= src.head {
                    Selection { anchor: merged.start, head: merged.end }
                } else {
                    Selection { anchor: merged.end, head: merged.start }
                };
                *last_primary |= primary;
                continue;
            }
        }
        out.push((s, primary));
    }
    let primary = out.iter().position(|(_, p)| *p).unwrap_or(0);
    (out.into_iter().map(|(s, _)| s).collect(), primary)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup(text: &str, sels: &[(usize, usize)]) -> (Buffer, EditorState) {
        let mut s = EditorState::default();
        s.sel = Selection { anchor: sels[0].0, head: sels[0].1 };
        s.extra = sels[1..]
            .iter()
            .map(|&(anchor, head)| Selection { anchor, head })
            .collect();
        let b = Buffer::from_text(text);
        s.multi_rev = b.revision();
        (b, s)
    }

    fn carets(s: &EditorState) -> Vec<usize> {
        s.all_selections().0.iter().map(|x| x.head).collect()
    }

    #[test]
    fn typing_goes_to_every_caret_in_one_undo_step() {
        let (mut b, mut s) = setup("a\nb\nc", &[(1, 1), (3, 3), (5, 5)]);
        assert!(s.insert_text(&mut b, "X"));
        assert_eq!(b.text(), "aX\nbX\ncX");
        assert_eq!(carets(&s), vec![2, 5, 8]);
        s.insert_text(&mut b, "Y");
        assert_eq!(b.text(), "aXY\nbXY\ncXY");
        b.undo();
        assert_eq!(b.text(), "aX\nbX\ncX", "每次輸入是一個復原步驟");
    }

    #[test]
    fn typing_replaces_each_selection() {
        let (mut b, mut s) = setup("one two three", &[(0, 3), (4, 7), (8, 13)]);
        s.insert_text(&mut b, "#");
        assert_eq!(b.text(), "# # #");
        assert_eq!(carets(&s), vec![1, 3, 5]);
    }

    #[test]
    fn backspace_and_delete_work_on_every_caret() {
        let (mut b, mut s) = setup("ab\ncd\nef", &[(2, 2), (5, 5), (8, 8)]);
        assert!(s.backspace(&mut b, false));
        assert_eq!(b.text(), "a\nc\ne");
        assert_eq!(carets(&s), vec![1, 3, 5]);
        let (mut b, mut s) = setup("ab\ncd\nef", &[(0, 0), (3, 3), (6, 6)]);
        assert!(s.delete_forward(&mut b, false));
        assert_eq!(b.text(), "b\nd\nf");
        assert_eq!(carets(&s), vec![0, 2, 4]);
    }

    #[test]
    fn adjacent_backspaces_do_not_double_delete() {
        let (mut b, mut s) = setup("abcd", &[(2, 2), (3, 3)]);
        s.backspace(&mut b, false);
        assert_eq!(b.text(), "ad", "各刪一個字，不會多刪");
        assert_eq!(carets(&s), vec![1], "兩個游標都落在同一點，合併成一個");
    }

    #[test]
    fn enter_keeps_each_lines_indent() {
        let (mut b, mut s) = setup("  a\n\tb", &[(3, 3), (6, 6)]);
        s.enter(&mut b);
        assert_eq!(b.text(), "  a\n  \n\tb\n\t");
    }

    #[test]
    fn movement_applies_to_every_caret_and_merges_collisions() {
        let (mut b, mut s) = setup("abc\ndef", &[(1, 1), (5, 5)]);
        s.move_right(&mut b, false, false);
        assert_eq!(carets(&s), vec![2, 6]);
        s.end(&mut b, false);
        assert_eq!(carets(&s), vec![3, 7]);
        s.move_vertical(&mut b, -1, false);
        // 第二個游標上到第一行的 3，和第一個游標相遇：合併成一個
        let (mut b, mut s) = setup("ab", &[(0, 0), (1, 1)]);
        s.move_left(&mut b, false, false);
        assert_eq!(carets(&s), vec![0], "擠在同一點的游標合併");
        assert!(!s.has_multi());
    }

    #[test]
    fn shift_movement_extends_every_selection() {
        let (mut b, mut s) = setup("ab\ncd", &[(0, 0), (3, 3)]);
        s.move_right(&mut b, true, false);
        let (all, _) = s.all_selections();
        assert_eq!(all, vec![Selection { anchor: 0, head: 1 }, Selection { anchor: 3, head: 4 }]);
    }

    #[test]
    fn paste_distributes_lines_when_counts_match() {
        let (mut b, mut s) = setup("\n\n", &[(0, 0), (1, 1), (2, 2)]);
        s.paste(&mut b, "x\ny\nz");
        assert_eq!(b.text(), "x\ny\nz");
        let (mut b, mut s) = setup("\n", &[(0, 0), (1, 1)]);
        s.paste(&mut b, "ab");
        assert_eq!(b.text(), "ab\nab");
        let (mut b, mut s) = setup("\n", &[(0, 0), (1, 1)]);
        s.paste(&mut b, "a\nb\nc");
        assert_eq!(b.text(), "a\nb\nc\na\nb\nc", "數量不符就每個都貼整份");
    }

    #[test]
    fn copy_and_cut_join_the_selections() {
        let (mut b, mut s) = setup("one two three", &[(0, 3), (4, 7)]);
        assert_eq!(s.selected_text(&b).as_deref(), Some("one\ntwo"));
        assert_eq!(s.cut(&mut b).as_deref(), Some("one\ntwo"));
        assert_eq!(b.text(), "  three");
        let (b, s) = setup("abc", &[(1, 1), (2, 2)]);
        assert_eq!(s.selected_text(&b), None);
    }

    #[test]
    fn toggle_caret_adds_and_removes() {
        let (mut b, mut s) = setup("abcdef", &[(1, 1)]);
        s.toggle_caret(&mut b, 4);
        assert_eq!(carets(&s), vec![1, 4]);
        assert_eq!(s.sel.head, 4, "新加的是主要的");
        s.toggle_caret(&mut b, 4);
        assert_eq!(carets(&s), vec![1]);
        s.toggle_caret(&mut b, 1);
        assert_eq!(carets(&s), vec![1], "最後一個不能移除");
    }

    #[test]
    fn add_caret_above_and_below_keeps_the_display_column() {
        let (mut b, mut s) = setup("abcd\nx\nabcd", &[(3, 3)]);
        assert!(s.add_caret_vertical(&mut b, false));
        assert_eq!(carets(&s), vec![3, 6], "短的行夾到行尾");
        assert!(s.add_caret_vertical(&mut b, false));
        assert_eq!(carets(&s), vec![3, 6, 8]);
        assert!(!s.add_caret_vertical(&mut b, false), "已經是最後一行");
        let (mut b, mut s) = setup("a\nb", &[(0, 0)]);
        assert!(!s.add_caret_vertical(&mut b, true));
    }

    #[test]
    fn add_next_occurrence_selects_the_word_then_each_match() {
        let (mut b, mut s) = setup("foo bar foo baz foo", &[(1, 1)]);
        assert!(s.add_next_occurrence(&mut b));
        assert_eq!(s.sel.range(), 0..3);
        assert!(s.add_next_occurrence(&mut b));
        assert!(s.add_next_occurrence(&mut b));
        let (all, _) = s.all_selections();
        assert_eq!(all.iter().map(|x| x.range()).collect::<Vec<_>>(), vec![0..3, 8..11, 16..19]);
        assert!(!s.add_next_occurrence(&mut b), "全部都選了");
        s.insert_text(&mut b, "x");
        assert_eq!(b.text(), "x bar x baz x");
    }

    #[test]
    fn rect_select_makes_one_selection_per_line() {
        let (mut b, mut s) = setup("abcdef\nab\nabcdef", &[(0, 0)]);
        s.rect_select(&mut b, (0, 1), (2, 4));
        let (all, primary) = s.all_selections();
        assert_eq!(
            all,
            vec![
                Selection { anchor: 1, head: 4 },
                Selection { anchor: 8, head: 9 },
                Selection { anchor: 11, head: 14 },
            ]
        );
        assert_eq!(primary, 2, "主要選取是拖曳終點那一行");
        s.insert_text(&mut b, "-");
        assert_eq!(b.text(), "a-ef\na-\na-ef");
    }

    #[test]
    fn rect_select_with_zero_width_makes_carets() {
        let (mut b, mut s) = setup("abc\nabc\nabc", &[(0, 0)]);
        s.rect_select(&mut b, (2, 2), (0, 2));
        assert_eq!(carets(&s), vec![2, 6, 10]);
        s.insert_text(&mut b, "|");
        assert_eq!(b.text(), "ab|c\nab|c\nab|c");
    }

    #[test]
    fn rect_select_uses_display_columns_for_wide_characters() {
        let (mut b, mut s) = setup("中文ab\nabcdef", &[(0, 0)]);
        s.rect_select(&mut b, (0, 0), (1, 4));
        let (all, _) = s.all_selections();
        assert_eq!(all[0].range(), 0..2, "兩個全形字佔 4 欄");
        assert_eq!(all[1].range(), 5..9);
    }

    #[test]
    fn external_edits_drop_the_extra_selections() {
        let (mut b, mut s) = setup("abc", &[(0, 0), (2, 2)]);
        b.insert(0, "x"); // 不是我們做的
        s.clamp(&b);
        assert!(!s.has_multi());
        let (mut b, mut s) = setup("abc", &[(0, 0), (2, 2)]);
        s.insert_text(&mut b, "x");
        s.clamp(&b);
        assert!(s.has_multi(), "自己改的不會作廢");
    }

    #[test]
    fn undo_clears_the_extra_selections() {
        let (mut b, mut s) = setup("a\nb", &[(1, 1), (3, 3)]);
        s.insert_text(&mut b, "X");
        s.undo(&mut b);
        s.clamp(&b);
        assert!(!s.has_multi());
        assert_eq!(b.text(), "a\nb");
    }
}
