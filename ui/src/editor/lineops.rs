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

    /// 轉換空白字元：有選取就只轉選到的行，沒有就轉整份文件。`edit` 把一行轉成新的一行；
    /// 游標與選取位置依「那個位置之前的文字轉換後有多長」搬到對應的位置。
    fn convert_whitespace(&mut self, buf: &mut Buffer, edit: impl Fn(&str) -> String) -> bool {
        self.interrupt_composition();
        let (a, b) = if self.sel.is_empty() { (0, buf.line_count() - 1) } else { self.selected_lines(buf) };
        let old = Self::lines_text(buf, a, b);
        let new: Vec<String> = old.iter().map(|l| edit(l)).collect();
        if new == old {
            return false;
        }
        // 每一行的（舊行首, 新行首）
        let mut marks = Vec::with_capacity(old.len());
        let mut new_start = buf.line_start(a);
        for (i, line) in new.iter().enumerate() {
            marks.push((buf.line_start(a + i), new_start));
            new_start += line.chars().count() + 1;
        }
        let map = |p: usize| {
            let Some(i) = marks.iter().rposition(|m| m.0 <= p) else {
                return p;
            };
            let prefix: String = old[i].chars().take(p - marks[i].0).collect();
            marks[i].1 + edit(&prefix).chars().count()
        };
        self.rewrite_lines(buf, a, b, &new, map);
        true
    }

    /// 把 Tab 換成空白（依目前的 Tab 欄寬補到下一個 Tab 位置）：有選取只轉選到的行，否則整份文件。
    pub fn tabs_to_spaces(&mut self, buf: &mut Buffer) -> bool {
        self.convert_whitespace(buf, motion::expand_tabs)
    }

    /// 把行首的縮排換成 Tab（每個 Tab 欄寬一個 Tab，不足一個的餘數留成空白）：有選取只轉選到的行，否則整份文件。
    /// 只動行首的縮排；文字中間的空白是對齊用的，不會被換掉。
    pub fn spaces_to_tabs(&mut self, buf: &mut Buffer) -> bool {
        self.convert_whitespace(buf, |line| {
            let lead = line.chars().take_while(|c| *c == ' ' || *c == '\t').count();
            let cols = motion::display_col(line, lead);
            let width = motion::tab_width();
            let rest: String = line.chars().skip(lead).collect();
            format!("{}{}{rest}", "\t".repeat(cols / width), " ".repeat(cols % width))
        })
    }

    /// 排列格式：依語言重新排好縮排並去掉行尾空白。有選取只處理選到的行，沒有就處理整份文件。
    ///
    /// - JSON：整個（或選到的行）能解析成 JSON 時完整重新排版（鍵順序保留）；解析不了就改用下一項。
    /// - 用括號決定縮排的語言：依 `{ ( [` 的深度重算每一行的縮排（字串、註解裡的括號不算；
    ///   多行字串與區塊註解裡面的行原樣不動）。深度永遠從文件開頭累計，所以只選一段也對得上。
    /// - 其他語言（Python…）算不出層數：只去行尾空白，並把行首縮排換成目前設定的字元（欄位寬度不變）。
    ///
    /// 縮排字元跟隨「改用空白」與 Tab 欄寬。整件事是一個復原步驟；沒有任何變化回傳 `false`。
    pub fn reformat(&mut self, buf: &mut Buffer) -> bool {
        self.interrupt_composition();
        let last_line = buf.line_count() - 1;
        let (a, b) = if self.sel.is_empty() { (0, last_line) } else { self.selected_lines(buf) };
        let all = Self::lines_text(buf, 0, last_line);
        let old: Vec<String> = all[a..=b].to_vec();
        let unit = super::indent::indent_unit();
        let json = self.language.is_some_and(|l| l.name == "JSON");
        let pretty = json.then(|| super::indent::json_pretty(&old.join("\n"), &unit)).flatten();
        let new: Vec<String> = match pretty {
            Some(text) => {
                let base = super::indent::leading_ws(&old[0]);
                let mut lines: Vec<String> = text.split('\n').map(|l| format!("{base}{l}")).collect();
                if old.last().is_some_and(|l| l.is_empty()) && b == last_line && old.len() > 1 {
                    lines.push(String::new()); // 檔尾原本的換行保留
                }
                lines
            }
            None => {
                let how = super::indent::scan(self.language, &all);
                (a..=b).map(|i| super::indent::format_line(&all[i], how[i], &unit)).collect()
            }
        };
        if new == old {
            return false;
        }
        let lead = |l: &str| l.chars().take_while(|c| *c == ' ' || *c == '\t').count();
        // 每一行的（舊行首, 新行首, 舊縮排長度, 新縮排長度, 新的行長）；行數不一樣（JSON）時用「第幾行」對應
        let mut marks = Vec::with_capacity(old.len());
        let mut new_start = buf.line_start(a);
        for (i, line) in new.iter().enumerate() {
            let (os, ol) = match old.get(i) {
                Some(o) => (buf.line_start(a + i), lead(o)),
                None => (usize::MAX, 0),
            };
            marks.push((os, new_start, ol, lead(line), line.chars().count()));
            new_start += line.chars().count() + 1;
        }
        let old_end = Self::line_end(buf, b);
        let new_end = buf.line_start(a) + new.join("\n").chars().count();
        let map = move |p: usize| {
            if p > old_end {
                return (p as isize + new_end as isize - old_end as isize) as usize;
            }
            match marks.iter().rposition(|m| m.0 <= p) {
                None => p,
                Some(i) => {
                    let (os, ns, ol, nl, len) = marks[i];
                    let col = p - os;
                    let col = if col < ol { col.min(nl) } else { col - ol + nl };
                    (ns + col.min(len)).min(new_end)
                }
            }
        };
        self.rewrite_lines(buf, a, b, &new, map);
        true
    }

    /// 縮排一層（每個非空行行首加一個 Tab；設定成用空白取代 Tab 時，加一個 Tab 欄寬的空白）。
    pub fn indent_lines(&mut self, buf: &mut Buffer) -> bool {
        let indent = if motion::tab_uses_spaces() { " ".repeat(motion::tab_width()) } else { "\t".to_owned() };
        let added = indent.chars().count() as isize;
        self.reindent(buf, move |l| {
            if l.is_empty() {
                (String::new(), 0)
            } else {
                (format!("{indent}{l}"), added)
            }
        })
    }

    /// 反縮排一層（行首一個 Tab，或最多 [`motion::tab_width`] 個空白）。
    pub fn unindent_lines(&mut self, buf: &mut Buffer) -> bool {
        self.reindent(buf, |l| {
            let n = if l.starts_with('\t') {
                1
            } else {
                l.chars().take(motion::tab_width()).take_while(|c| *c == ' ').count()
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
    fn indenting_with_spaces_adds_a_tab_width_of_spaces_and_unindent_takes_them_back() {
        let spaces = motion::set_tab_uses_spaces(true);
        let width = motion::set_tab_width(2);
        let (mut b, mut s) = setup("a\n\nb", 0, 4);
        assert!(s.indent_lines(&mut b));
        assert_eq!(b.text(), "  a\n\n  b", "一個 Tab 欄寬（2）的空白");
        assert_eq!(s.sel, Selection { anchor: 2, head: 8 });
        assert!(s.unindent_lines(&mut b));
        assert_eq!(b.text(), "a\n\nb");
        assert_eq!(s.sel, Selection { anchor: 0, head: 4 });
        // 用空白縮排的檔案裡原有的 Tab 也一樣退得掉
        let (mut b, mut s) = setup("\tx", 2, 2);
        assert!(s.unindent_lines(&mut b));
        assert_eq!(b.text(), "x");
        motion::set_tab_width(width);
        motion::set_tab_uses_spaces(spaces);
    }

    #[test]
    fn tabs_and_leading_spaces_convert_by_the_tab_width() {
        let width = motion::set_tab_width(4);
        // 沒有選取：整份文件。Tab 補到下一個 Tab 位置（全形字佔兩欄）
        let (mut b, mut s) = setup("\ta\tb\n  \t你\tc\nplain", 0, 0);
        assert!(s.tabs_to_spaces(&mut b));
        assert_eq!(b.text(), "    a   b\n    你  c\nplain");
        assert!(!s.tabs_to_spaces(&mut b), "已經沒有 Tab：沒有動作");
        // 空白換回 Tab：只動行首，餘數留成空白，行中間的空白不動
        let (mut b, mut s) = setup("        x\n      y  z\n  w\n", 0, 0);
        assert!(s.spaces_to_tabs(&mut b));
        assert_eq!(b.text(), "\t\tx\n\t  y  z\n  w\n");
        assert!(!s.spaces_to_tabs(&mut b), "已經是 Tab：沒有動作");
        // 欄寬 2
        motion::set_tab_width(2);
        let (mut b, mut s) = setup("    a\n \tb", 0, 0);
        assert!(s.spaces_to_tabs(&mut b));
        assert_eq!(b.text(), "\t\ta\n\tb", "一個空白加一個 Tab 只到第 2 欄（Tab 補到下一個 Tab 位置），所以是一個 Tab");
        // 有選取：只轉選到的行，游標跟著文字走
        motion::set_tab_width(4);
        let (mut b, mut s) = setup("\ta\n\tb\n\tc", 4, 5);
        assert!(s.tabs_to_spaces(&mut b));
        assert_eq!(b.text(), "\ta\n    b\n\tc");
        assert_eq!(s.sel, Selection { anchor: 7, head: 8 }, "選取原本在第二行的 b 前後，現在還是 b 的前後");
        motion::set_tab_width(width);
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

    /// 測試用：固定 Tab 設定（4 欄、用空白、自動縮排開），結束還原。
    struct Tabs(usize, bool, bool);
    impl Tabs {
        fn set(width: usize, spaces: bool) -> Self {
            Tabs(motion::set_tab_width(width), motion::set_tab_uses_spaces(spaces), motion::auto_indent())
        }
    }
    impl Drop for Tabs {
        fn drop(&mut self) {
            motion::set_tab_width(self.0);
            motion::set_tab_uses_spaces(self.1);
            motion::set_auto_indent(self.2);
        }
    }

    #[test]
    fn reformat_fixes_the_whole_document_without_a_selection_in_one_undo_step() {
        let _t = Tabs::set(4, true);
        let (mut b, mut s) = setup("fn a() {\n  x();   \n      }\n", 12, 12);
        s.language = crate::syntax::language_for_name("a.rs");
        assert!(s.reformat(&mut b));
        assert_eq!(b.text(), "fn a() {\n    x();\n}\n");
        assert_eq!(s.sel, Selection::collapsed(14), "游標還在 x 的後面（縮排多了兩格）");
        assert!(!s.reformat(&mut b), "已經排好了：沒有變化");
        b.undo();
        assert_eq!(b.text(), "fn a() {\n  x();   \n      }\n", "一個復原步驟");
    }

    #[test]
    fn reformat_with_a_selection_only_touches_those_lines_but_counts_depth_from_the_top() {
        let _t = Tabs::set(4, true);
        let text = "fn a() {\n   if x {\n   y;\n   }\n   z;\n}";
        // 選第 3、4 行（y; 與 }）：深度從文件開頭算，第 2 行（沒選）維持原樣
        let start = text.find("   y;").unwrap();
        let end = text.find("   z;").unwrap();
        let (mut b, mut s) = setup(text, start, end);
        s.language = crate::syntax::language_for_name("a.rs");
        assert!(s.reformat(&mut b));
        assert_eq!(b.text(), "fn a() {\n   if x {\n        y;\n    }\n   z;\n}");
        assert_eq!(s.sel, Selection { anchor: start, head: end + 6 }, "選取跟著文字走（結束仍在下一行行首）");
    }

    #[test]
    fn reformat_pretty_prints_json_and_falls_back_when_it_is_not_valid() {
        let _t = Tabs::set(2, true);
        let (mut b, mut s) = setup("{\"a\":[1,2],\"b\":{}}\n", 0, 0);
        s.language = crate::syntax::language_for_name("a.json");
        assert!(s.reformat(&mut b));
        assert_eq!(b.text(), "{\n  \"a\": [\n    1,\n    2\n  ],\n  \"b\": {}\n}\n");
        // 有註解的 JSONC 解析不了：改用括號深度重排
        let (mut b, mut s) = setup("{\n// c\n\"a\": 1\n}", 0, 0);
        s.language = crate::syntax::language_for_name("a.json");
        assert!(s.reformat(&mut b));
        assert_eq!(b.text(), "{\n  // c\n  \"a\": 1\n}");
    }

    #[test]
    fn reformat_uses_tabs_when_spaces_are_off() {
        let _t = Tabs::set(4, false);
        let (mut b, mut s) = setup("a {\nb;\n}", 0, 0);
        s.language = crate::syntax::language_for_name("a.css");
        assert!(s.reformat(&mut b));
        assert_eq!(b.text(), "a {\n\tb;\n}");
    }
}
