//! 尋找與取代的狀態與流程：與 egui 無關，方便單元測試。
//!
//! 位置同時記錄「位元組」（給搜尋用）與「字元」（給編輯區用）兩種索引。

use feditor_core::Buffer;
use feditor_core::search::{SearchError, SearchOptions, Searcher};
use std::ops::Range;

/// 文字超過這個大小，目前文件的搜尋結果就不隨著每次輸入重做，避免每個字都掃描整份大檔案。
pub const STATS_MAX_BYTES: usize = 8 * 1024 * 1024;

#[derive(Default)]
pub struct FindState {
    pub show_replace: bool,
    pub query: String,
    pub replacement: String,
    pub opts: SearchOptions,
    /// 目前這一筆（位元組範圍）。
    pub current: Option<Range<usize>>,
    /// 目前這一筆（字元範圍），給編輯區選取與捲動用。
    pub current_chars: Option<Range<usize>>,
    /// 需要把目前這一筆捲動到可見處。
    pub reveal: bool,
    /// 這次搜尋是否繞過了檔案的開頭或結尾。
    pub wrapped: bool,
    error: Option<String>,
}

impl FindState {
    pub fn searcher(&self) -> Result<Searcher, SearchError> {
        Searcher::new(&self.query, self.opts)
    }

    /// 文字被改動：目前這一筆與統計都不再可信。
    pub fn invalidate(&mut self) {
        self.clear_current();
        self.wrapped = false;
    }

    fn clear_current(&mut self) {
        self.current = None;
        self.current_chars = None;
        self.reveal = false;
    }

    /// 尋找下一筆（或上一筆）。`from` 是位元組位置：向前找是「起點 >= from」，
    /// 向後找是「起點 < from」。找不到時會繞回另一端。回傳是否找到。
    pub fn find(&mut self, text: &str, from: usize, backward: bool) -> bool {
        self.wrapped = false;
        let sr = match self.searcher() {
            Ok(sr) => {
                self.error = None;
                sr
            }
            Err(SearchError::Empty) => {
                self.error = None;
                self.clear_current();
                return false;
            }
            Err(e) => {
                self.error = Some(e.to_string());
                self.clear_current();
                return false;
            }
        };

        let mut found = if backward {
            sr.find_prev(text, from)
        } else {
            sr.find_next(text, from)
        };
        if found.is_none() {
            found = if backward {
                sr.find_prev(text, usize::MAX)
            } else {
                sr.find_next(text, 0)
            };
            self.wrapped = found.is_some();
        }

        match found {
            Some(r) => {
                self.current_chars = Some(byte_to_char(text, r.start)..byte_to_char(text, r.end));
                self.current = Some(r);
                self.reveal = true;
                true
            }
            None => {
                self.clear_current();
                false
            }
        }
    }

    /// 取代 `selection`（位元組範圍）這一段，它必須恰好是一個有效的符合，然後找下一筆。
    /// 取代是緩衝區裡的一個復原步驟。回傳新文字佔的字元範圍；若 `selection` 不是符合，
    /// 就只是先找到下一筆而不取代，回傳 `None`。
    pub fn replace_current(
        &mut self,
        buf: &mut Buffer,
        selection: Range<usize>,
    ) -> Option<Range<usize>> {
        let text = buf.text();
        let Ok(sr) = self.searcher() else {
            // 搜尋字串是空的或有錯：讓 find() 去設定錯誤狀態
            self.find(&text, selection.start, false);
            return None;
        };
        let Some(new) = sr.expansion(&text, &selection, &self.replacement) else {
            self.find(&text, selection.start, false);
            return None;
        };
        let start = byte_to_char(&text, selection.start);
        let end = byte_to_char(&text, selection.end);
        buf.break_group();
        buf.replace(start..end, &new);
        let replaced = start..start + new.chars().count();

        let text = buf.text();
        let mut from = selection.start + new.len();
        if selection.is_empty() {
            // 空比對被取代後要往後跳一個字元，否則會原地踏步
            from = text[from.min(text.len())..]
                .chars()
                .next()
                .map_or(from, |c| from + c.len_utf8());
        }
        self.invalidate();
        self.find(&text, from.min(text.len()), false);
        Some(replaced)
    }

    /// 全部取代（整個動作是一個復原步驟），回傳取代的筆數。搜尋字串是空的或有錯時回傳錯誤。
    pub fn replace_all(&mut self, buf: &mut Buffer) -> Result<usize, SearchError> {
        let sr = self.searcher()?;
        let text = buf.text();
        let (out, n) = sr.replace_all(&text, &self.replacement);
        if n > 0 {
            // 只換掉「第一處到最後一處改動」之間的範圍：復原步驟小，復原後畫面也留在改動附近
            let (old_range, new_range) = changed_span(&text, &out);
            buf.break_group();
            buf.replace(
                byte_to_char(&text, old_range.start)..byte_to_char(&text, old_range.end),
                &out[new_range],
            );
            self.invalidate();
        }
        Ok(n)
    }
}

/// 兩段文字開頭與結尾相同的部分去掉之後，剩下不同的範圍（舊文字的、新文字的位元組範圍）。
fn changed_span(old: &str, new: &str) -> (Range<usize>, Range<usize>) {
    let mut prefix = old
        .bytes()
        .zip(new.bytes())
        .take_while(|(a, b)| a == b)
        .count();
    while !old.is_char_boundary(prefix) || !new.is_char_boundary(prefix) {
        prefix -= 1;
    }
    let max_suffix = old.len().min(new.len()) - prefix;
    let mut suffix = old
        .bytes()
        .rev()
        .zip(new.bytes().rev())
        .take(max_suffix)
        .take_while(|(a, b)| a == b)
        .count();
    while !old.is_char_boundary(old.len() - suffix) || !new.is_char_boundary(new.len() - suffix) {
        suffix -= 1;
    }
    (prefix..old.len() - suffix, prefix..new.len() - suffix)
}

/// 字元索引 → 位元組位置；超出範圍時回傳文字長度。
#[cfg(test)]
pub fn char_to_byte(text: &str, chars: usize) -> usize {
    text.char_indices()
        .nth(chars)
        .map_or(text.len(), |(b, _)| b)
}

/// 位元組位置 → 字元索引。`byte` 必須落在字元邊界上。
pub fn byte_to_char(text: &str, byte: usize) -> usize {
    text[..byte.min(text.len())].chars().count()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(query: &str) -> FindState {
        FindState {
            query: query.to_owned(),
            ..Default::default()
        }
    }

    #[test]
    fn find_next_wraps_and_reports_it() {
        let text = "a-a-a";
        let mut f = state("a");
        assert!(f.find(text, 0, false));
        assert_eq!(f.current, Some(0..1));
        assert!(!f.wrapped);
        assert!(f.find(text, 1, false));
        assert_eq!(f.current, Some(2..3));
        assert!(f.find(text, 5, false)); // 後面沒有了：繞回開頭
        assert_eq!(f.current, Some(0..1));
        assert!(f.wrapped);
    }

    #[test]
    fn find_prev_wraps_to_the_end() {
        let text = "a-a-a";
        let mut f = state("a");
        assert!(f.find(text, 0, true)); // 前面沒有了：繞到結尾
        assert_eq!(f.current, Some(4..5));
        assert!(f.wrapped);
        assert!(f.find(text, 4, true));
        assert_eq!(f.current, Some(2..3));
        assert!(!f.wrapped);
    }

    #[test]
    fn char_ranges_follow_byte_ranges_for_chinese() {
        let text = "你好，世界。你好";
        let mut f = state("世界");
        assert!(f.find(text, 0, false));
        assert_eq!(f.current_chars, Some(3..5));
        assert_eq!(&text[f.current.clone().unwrap()], "世界");
        assert_eq!(char_to_byte(text, 3), 9);
        assert_eq!(byte_to_char(text, 9), 3);
        assert_eq!(char_to_byte(text, 999), text.len());
    }

    #[test]
    fn not_found_and_empty_and_invalid() {
        let mut f = state("zzz");
        assert!(!f.find("abc", 0, false));
        assert!(f.current.is_none());

        let mut f = state("");
        assert!(!f.find("abc", 0, false));
        assert!(f.current.is_none());

        let mut f = state("(");
        f.opts.regex = true;
        assert!(!f.find("abc", 0, false));
        assert!(f.current.is_none());
    }

    fn buf(s: &str) -> Buffer {
        Buffer::from_text(s)
    }

    #[test]
    fn replace_current_replaces_then_selects_next() {
        let mut b = buf("cat dog cat");
        let mut f = state("cat");
        f.replacement = "bird".to_owned();
        f.find(&b.text(), 0, false);
        let sel = f.current.clone().unwrap();
        assert_eq!(f.replace_current(&mut b, sel), Some(0..4));
        assert_eq!(b.text(), "bird dog cat");
        assert_eq!(f.current, Some(9..12)); // 已經跳到下一個 cat
        let sel = f.current.clone().unwrap();
        assert!(f.replace_current(&mut b, sel).is_some());
        assert_eq!(b.text(), "bird dog bird");
    }

    #[test]
    fn each_replacement_is_one_undo_step() {
        let mut b = buf("cat cat");
        let mut f = state("cat");
        f.replacement = "bird".to_owned();
        f.find(&b.text(), 0, false);
        let sel = f.current.clone().unwrap();
        f.replace_current(&mut b, sel);
        let sel = f.current.clone().unwrap();
        f.replace_current(&mut b, sel);
        assert_eq!(b.text(), "bird bird");
        b.undo();
        assert_eq!(b.text(), "bird cat");
        b.undo();
        assert_eq!(b.text(), "cat cat");
    }

    #[test]
    fn replace_current_without_valid_selection_only_finds() {
        let mut b = buf("cat dog cat");
        let mut f = state("cat");
        f.replacement = "bird".to_owned();
        // 游標在 "dog" 上，不是一個符合：只會跳到下一個 cat，不改文字
        assert!(f.replace_current(&mut b, 4..7).is_none());
        assert_eq!(b.text(), "cat dog cat");
        assert_eq!(f.current, Some(8..11));
    }

    #[test]
    fn replace_with_longer_and_shorter_text_keeps_offsets_right() {
        let mut b = buf("你好 你好 你好");
        let mut f = state("你好");
        f.replacement = "Hello!".to_owned();
        f.find(&b.text(), 0, false);
        for _ in 0..3 {
            let sel = f.current.clone().unwrap();
            assert!(f.replace_current(&mut b, sel).is_some());
        }
        assert_eq!(b.text(), "Hello! Hello! Hello!");
    }

    #[test]
    fn replace_all_counts_invalidates_and_is_one_undo_step() {
        let mut b = buf("a1 b22 c333");
        let mut f = state(r"\d+");
        f.opts.regex = true;
        f.replacement = "#".to_owned();
        f.find(&b.text(), 0, false);
        assert_eq!(f.replace_all(&mut b).unwrap(), 3);
        assert_eq!(b.text(), "a# b# c#");
        assert!(f.current.is_none(), "文字改動後目前這一筆要作廢");
        assert_eq!(f.replace_all(&mut b).unwrap(), 0);
        b.undo();
        assert_eq!(b.text(), "a1 b22 c333");
    }

    #[test]
    fn replace_all_reports_empty_and_invalid_query() {
        let mut b = buf("abc");
        assert_eq!(state("").replace_all(&mut b), Err(SearchError::Empty));
        let mut f = state("(");
        f.opts.regex = true;
        assert!(matches!(
            f.replace_all(&mut b),
            Err(SearchError::InvalidRegex(_))
        ));
        assert_eq!(b.text(), "abc");
    }

    #[test]
    fn replacing_an_empty_match_does_not_stall() {
        let mut b = buf("a\nb");
        let mut f = state("^");
        f.opts.regex = true;
        f.replacement = "> ".to_owned();
        f.find(&b.text(), 0, false);
        for _ in 0..2 {
            let sel = f.current.clone().unwrap();
            assert!(f.replace_current(&mut b, sel).is_some());
        }
        assert_eq!(b.text(), "> a\n> b");
    }
    #[test]
    fn options_change_the_results() {
        let text = "Cat cat concat";
        let mut f = state("cat");
        let (all, _) = f.searcher().unwrap().replace_all(text, "X");
        assert_eq!(all, "X X conX");

        f.opts.case_sensitive = true;
        let (cs, _) = f.searcher().unwrap().replace_all(text, "X");
        assert_eq!(cs, "Cat X conX");

        f.opts.whole_word = true;
        let (ww, _) = f.searcher().unwrap().replace_all(text, "X");
        assert_eq!(ww, "Cat X concat");
    }

    #[test]
    fn changed_span_trims_common_prefix_and_suffix_on_char_boundaries() {
        assert_eq!(changed_span("abcXdef", "abcYYdef"), (3..4, 3..5));
        assert_eq!(changed_span("same", "same"), (4..4, 4..4));
        assert_eq!(changed_span("", "x"), (0..0, 0..1));
        // 「你」與「佢」的 UTF-8 前兩個位元組相同：不能把邊界切在字元中間
        let (old, new) = changed_span("a你b", "a佢b");
        assert_eq!(&"a你b"[old], "你");
        assert_eq!(&"a佢b"[new], "佢");
        // 重疊的前後綴不可算兩次
        assert_eq!(changed_span("aa", "aaa"), (2..2, 2..3));
    }

    #[test]
    fn replace_all_only_touches_the_changed_middle() {
        let mut b = buf("keep a keep a keep");
        let mut f = state("a");
        f.replacement = "b".to_owned();
        assert_eq!(f.replace_all(&mut b).unwrap(), 2);
        assert_eq!(b.text(), "keep b keep b keep");
        let span = b.undo().unwrap();
        assert_eq!(b.text(), "keep a keep a keep");
        assert!(span.start >= 5 && span.end <= 13, "{span:?}");
    }
}