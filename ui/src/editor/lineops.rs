//! 以「行」為單位的編輯：上下移動、複製、刪除、縮排、註解切換。
//!
//! 每個操作都是一個獨立的復原步驟；選取範圍會跟著文字一起移動。

use super::model::{EditorState, Selection};
use feditor_core::{Buffer, motion};

impl EditorState {
    /// 選取涵蓋的行（含頭尾）。選取結束在行首（而且不是空選取）時，那一行不算。
    pub fn selected_lines(&self, buf: &Buffer) -> (usize, usize) {
        let r = self.sel.range();
        let first = buf.line_of_char(r.start.min(buf.len_chars()));
        let mut last = buf.line_of_char(r.end.min(buf.len_chars()));
        if !self.sel.is_empty() && last > first && r.end == buf.line_start(last) {
            last -= 1;
        }
        (first, last)
    }

    fn line_end(buf: &Buffer, line: usize) -> usize {
        buf.line_start(line) + buf.line_len_chars(line)
    }

    /// 把 `first..=last` 這幾行（不含最後的換行）換成 `new_lines`，並用 `map` 轉換選取位置。
    fn rewrite_lines(
        &mut self,
        buf: &mut Buffer,
        first: usize,
        last: usize,
        new_lines: &[String],
        map: impl Fn(usize) -> usize,
    ) {
        let start = buf.line_start(first);
        let end = Self::line_end(buf, last);
        buf.break_group();
        buf.replace(start..end, &new_lines.join("\n"));
        buf.break_group();
        self.sel = Selection {
            anchor: map(self.sel.anchor),
            head: map(self.sel.head),
        };
        self.after_line_edit(buf);
    }

    fn after_line_edit(&mut self, buf: &Buffer) {
        self.clamp(buf);
        self.goal_col = None;
        self.scroll_to_caret = true;
    }

    fn lines_text(buf: &Buffer, first: usize, last: usize) -> Vec<String> {
        (first..=last).map(|l| buf.line_text(l)).collect()
    }

    /// 把選到的行往上（`up`）或往下搬一行。到頭了回傳 `false`。
    pub fn move_lines(&mut self, buf: &mut Buffer, up: bool) -> bool {
        self.interrupt_composition();
        let (a, b) = self.selected_lines(buf);
        if (up && a == 0) || (!up && b + 1 >= buf.line_count()) {
            return false;
        }
        let block = Self::lines_text(buf, a, b);
        if up {
            let other = buf.line_text(a - 1);
            let shift = other.chars().count() + 1;
            let mut new = block;
            new.push(other);
            self.rewrite_lines(buf, a - 1, b, &new, |p| p - shift);
        } else {
            let other = buf.line_text(b + 1);
            let shift = other.chars().count() + 1;
            let mut new = vec![other];
            new.extend(block);
            self.rewrite_lines(buf, a, b + 1, &new, |p| p + shift);
        }
        true
    }

    /// 複製選到的行，插在它們的下面，選取移到複製出來的那份。
    pub fn duplicate_lines(&mut self, buf: &mut Buffer) -> bool {
        self.interrupt_composition();
        let (a, b) = self.selected_lines(buf);
        let block = Self::lines_text(buf, a, b);
        let shift = block.iter().map(|l| l.chars().count() + 1).sum::<usize>();
        let at = Self::line_end(buf, b);
        buf.break_group();
        buf.insert(at, &format!("\n{}", block.join("\n")));
        buf.break_group();
        self.sel = Selection {
            anchor: self.sel.anchor + shift,
            head: self.sel.head + shift,
        };
        self.after_line_edit(buf);
        true
    }

    /// 刪除選到的整行（含換行）。
    pub fn delete_lines(&mut self, buf: &mut Buffer) -> bool {
        self.interrupt_composition();
        let (a, b) = self.selected_lines(buf);
        let last_line = buf.line_count() - 1;
        let (from, to) = if b < last_line {
            (buf.line_start(a), buf.line_start(b + 1))
        } else if a > 0 {
            // 刪到最後一行：連同前一行的換行一起刪，不留下空行
            (Self::line_end(buf, a - 1), Self::line_end(buf, b))
        } else {
            (0, Self::line_end(buf, b))
        };
        if from == to {
            return false;
        }
        buf.break_group();
        buf.remove(from..to);
        buf.break_group();
        let line = a.min(buf.line_count() - 1);
        self.sel = Selection::collapsed(buf.line_start(line));
        self.after_line_edit(buf);
        true
    }

    /// 多行縮排／反縮排的共同部分：`edit` 回傳新的行文字與「行首增加了幾個字元」（負數為刪除）。
    fn reindent(&mut self, buf: &mut Buffer, edit: impl Fn(&str) -> (String, isize)) -> bool {
        self.interrupt_composition();
        let (a, b) = self.selected_lines(buf);
        let old = Self::lines_text(buf, a, b);
        let mut new = Vec::with_capacity(old.len());
        // 每一行的（舊行首, 新行首, 行首增減）
        let mut marks = Vec::with_capacity(old.len());
        let mut new_start = buf.line_start(a);
        for (i, line) in old.iter().enumerate() {
            let (text, delta) = edit(line);
            marks.push((buf.line_start(a + i), new_start, delta));
            new_start += text.chars().count() + 1;
            new.push(text);
        }
        if new == old {
            return false;
        }
        let map = move |p: usize| {
            // 位置落在哪一行就套用那一行的增減；退縮時不能退到行首之前
            let Some(&(os, ns, d)) = marks.iter().rev().find(|m| m.0 <= p) else {
                return p;
            };
            let col = p - os;
            ns + if d >= 0 {
                col + d as usize
            } else {
                col.saturating_sub((-d) as usize)
            }
        };
        self.rewrite_lines(buf, a, b, &new, map);
        true
    }

    /// 縮排一層（每個非空行行首加一個 Tab）。
    pub fn indent_lines(&mut self, buf: &mut Buffer) -> bool {
        self.reindent(buf, |l| {
            if l.is_empty() {
                (String::new(), 0)
            } else {
                (format!("\t{l}"), 1)
            }
        })
    }

    /// 反縮排一層（行首一個 Tab，或最多 [`motion::TAB_WIDTH`] 個空白）。
    pub fn unindent_lines(&mut self, buf: &mut Buffer) -> bool {
        self.reindent(buf, |l| {
            let n = if l.starts_with('\t') {
                1
            } else {
                l.chars().take(motion::TAB_WIDTH).take_while(|c| *c == ' ').count()
            };
            (l.chars().skip(n).collect(), -(n as isize))
        })
    }

    /// 註解切換：選到的非空行都已經有註解符號就全部拿掉，否則全部加上（對齊最小縮排）。
    pub fn toggle_comment(&mut self, buf: &mut Buffer) -> bool {
        let prefix = self.comment_prefix;
        if prefix.is_empty() {
            return false;
        }
        let (a, b) = self.selected_lines(buf);
        let lines = Self::lines_text(buf, a, b);
        let indent_of = |l: &str| l.chars().take_while(|c| *c == ' ' || *c == '\t').count();
        let non_blank: Vec<&String> = lines.iter().filter(|l| !l.trim().is_empty()).collect();
        if non_blank.is_empty() {
            return false;
        }
        let all_commented = non_blank
            .iter()
            .all(|l| l.chars().skip(indent_of(l)).collect::<String>().starts_with(prefix));
        let min_indent = non_blank.iter().map(|l| indent_of(l)).min().unwrap_or(0);
        let plen = prefix.chars().count() as isize;
        let spaced = format!("{prefix} ");
        self.reindent_at(buf, move |l| {
            if l.trim().is_empty() {
                return (l.to_owned(), 0, 0);
            }
            if all_commented {
                let ind = indent_of(l);
                let rest: String = l.chars().skip(ind).collect();
                let cut = if rest.starts_with(&spaced) { plen + 1 } else { plen };
                let head: String = l.chars().take(ind).collect();
                (format!("{head}{}", rest.chars().skip(cut as usize).collect::<String>()), ind, -cut)
            } else {
                let head: String = l.chars().take(min_indent).collect();
                let tail: String = l.chars().skip(min_indent).collect();
                (format!("{head}{spaced}{tail}"), min_indent, plen + 1)
            }
        })
    }

    /// 和 [`reindent`](Self::reindent) 相同，但增減發生在行內第 `col` 個字元（註解符號在縮排之後）。
    fn reindent_at(
        &mut self,
        buf: &mut Buffer,
        edit: impl Fn(&str) -> (String, usize, isize),
    ) -> bool {
        self.interrupt_composition();
        let (a, b) = self.selected_lines(buf);
        let old = Self::lines_text(buf, a, b);
        let mut new = Vec::with_capacity(old.len());
        let mut marks = Vec::with_capacity(old.len());
        let mut new_start = buf.line_start(a);
        for (i, line) in old.iter().enumerate() {
            let (text, col, delta) = edit(line);
            marks.push((buf.line_start(a + i), new_start, col, delta));
            new_start += text.chars().count() + 1;
            new.push(text);
        }
        if new == old {
            return false;
        }
        let map = move |p: usize| {
            let Some(&(os, ns, at, d)) = marks.iter().rev().find(|m| m.0 <= p) else {
                return p;
            };
            let col = p - os;
            ns + if d >= 0 {
                if col >= at { col + d as usize } else { col }
            } else if col >= at {
                (col - at).saturating_sub((-d) as usize) + at
            } else {
                col
            }
        };
        self.rewrite_lines(buf, a, b, &new, map);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup(text: &str, anchor: usize, head: usize) -> (Buffer, EditorState) {
        let mut s = EditorState::default();
        s.sel = Selection { anchor, head };
        (Buffer::from_text(text), s)
    }

    #[test]
    fn move_lines_down_and_up_keeps_the_caret_on_the_moved_line() {
        let (mut b, mut s) = setup("a\nbb\nccc", 2, 2); // 游標在 bb 行首
        assert!(s.move_lines(&mut b, false));
        assert_eq!(b.text(), "a\nccc\nbb");
        assert_eq!(s.sel, Selection::collapsed(6));
        assert!(!s.move_lines(&mut b, false), "已經在最後一行");
        assert!(s.move_lines(&mut b, true));
        assert_eq!(b.text(), "a\nbb\nccc");
        assert_eq!(s.sel, Selection::collapsed(2));
        b.undo();
        assert_eq!(b.text(), "a\nccc\nbb", "每次移動是一個復原步驟");
    }

    #[test]
    fn move_lines_moves_a_multi_line_selection() {
        let (mut b, mut s) = setup("a\nb\nc\nd", 2, 5); // 選 b、c
        assert!(s.move_lines(&mut b, true));
        assert_eq!(b.text(), "b\nc\na\nd");
        assert_eq!(s.sel, Selection { anchor: 0, head: 3 });
        let (mut b, mut s) = setup("a\nb\nc", 0, 0);
        assert!(!s.move_lines(&mut b, true));
    }

    #[test]
    fn selection_ending_at_line_start_does_not_include_that_line() {
        let (b, s) = setup("a\nb\nc", 0, 4); // 到 c 的行首
        assert_eq!(s.selected_lines(&b), (0, 1));
    }

    #[test]
    fn duplicate_lines_selects_the_copy() {
        let (mut b, mut s) = setup("a\nb", 0, 0);
        assert!(s.duplicate_lines(&mut b));
        assert_eq!(b.text(), "a\na\nb");
        assert_eq!(s.sel, Selection::collapsed(2));
        let (mut b, mut s) = setup("a\nb", 2, 3); // 最後一行沒有換行
        s.duplicate_lines(&mut b);
        assert_eq!(b.text(), "a\nb\nb");
    }

    #[test]
    fn delete_lines_handles_first_middle_and_last() {
        let (mut b, mut s) = setup("a\nb\nc", 2, 2);
        assert!(s.delete_lines(&mut b));
        assert_eq!(b.text(), "a\nc");
        assert_eq!(s.sel, Selection::collapsed(2));
        let (mut b, mut s) = setup("a\nb\nc", 4, 4);
        s.delete_lines(&mut b);
        assert_eq!(b.text(), "a\nb", "最後一行：不留空行");
        assert_eq!(s.sel, Selection::collapsed(2));
        let (mut b, mut s) = setup("only", 2, 2);
        s.delete_lines(&mut b);
        assert_eq!(b.text(), "");
        assert!(!s.delete_lines(&mut b), "空文件沒東西可刪");
        let (mut b, mut s) = setup("a\nb\nc\nd", 0, 4); // 選 a、b（結束在 c 行首）
        s.delete_lines(&mut b);
        assert_eq!(b.text(), "c\nd");
    }

    #[test]
    fn indent_and_unindent_multiple_lines() {
        let (mut b, mut s) = setup("a\n\nb", 0, 4);
        assert!(s.indent_lines(&mut b));
        assert_eq!(b.text(), "\ta\n\n\tb");
        assert_eq!(s.sel, Selection { anchor: 1, head: 6 });
        assert!(s.unindent_lines(&mut b));
        assert_eq!(b.text(), "a\n\nb");
        assert_eq!(s.sel, Selection { anchor: 0, head: 4 });
        assert!(!s.unindent_lines(&mut b), "沒有縮排可拿掉");
        let (mut b, mut s) = setup("      x", 7, 7);
        s.unindent_lines(&mut b);
        assert_eq!(b.text(), "  x", "空白最多拿掉 TAB_WIDTH 個");
        assert_eq!(s.sel, Selection::collapsed(3));
    }

    #[test]
    fn toggle_comment_adds_aligned_and_removes() {
        let (mut b, mut s) = setup("fn a() {\n    x;\n\n    y;\n}", 0, 25);
        s.comment_prefix = "//";
        assert!(s.toggle_comment(&mut b));
        assert_eq!(b.text(), "// fn a() {\n//     x;\n\n//     y;\n// }");
        assert!(s.toggle_comment(&mut b));
        assert_eq!(b.text(), "fn a() {\n    x;\n\n    y;\n}");
        b.undo();
        assert!(b.text().starts_with("// fn"), "取消註解也是一個復原步驟");
    }

    #[test]
    fn toggle_comment_keeps_indent_and_works_with_other_prefixes() {
        let (mut b, mut s) = setup("    a\n  b", 0, 0);
        s.comment_prefix = "#";
        s.toggle_comment(&mut b);
        assert_eq!(b.text(), "    # a\n  b", "只動游標那一行，縮排後加註解");
        s.comment_prefix = "";
        assert!(!s.toggle_comment(&mut b), "沒有註解符號的語言不動作");
        let (mut b, mut s) = setup("\n  \n", 0, 0);
        s.comment_prefix = "//";
        assert!(!s.toggle_comment(&mut b), "全是空行不動作");
    }
}
