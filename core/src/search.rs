//! 搜尋與取代：支援規則表達式、區分大小寫、完整單字。
//!
//! 所有位置都是 `&str` 的**位元組**偏移（落在字元邊界上），呼叫端自行轉成字元索引。
//! 底層使用 `regex` crate：保證線性時間，不會被惡意或寫壞的規則卡死。

use crate::i18n::t;
use regex::{Regex, RegexBuilder};
use std::borrow::Cow;
use std::fmt;
use std::ops::Range;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SearchOptions {
    /// 把搜尋字串當成規則表達式（取代字串可使用 `$1`、`${name}`、`\n`、`\t`）。
    pub regex: bool,
    pub case_sensitive: bool,
    /// 比對結果的前後不可緊鄰「單字字元」（字母、數字、底線）。
    pub whole_word: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchError {
    /// 搜尋字串是空的。
    Empty,
    /// 規則表達式有語法錯誤（已簡化成一行）。
    InvalidRegex(String),
}

impl fmt::Display for SearchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SearchError::Empty => f.write_str(t("搜尋字串是空的")),
            SearchError::InvalidRegex(msg) => f.write_str(&crate::tf!("規則表達式錯誤：{msg}", msg = msg)),
        }
    }
}

impl std::error::Error for SearchError {}

/// 統計結果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stats {
    /// 符合的筆數（達到上限時為上限值）。
    pub total: usize,
    /// 是否因達到上限而中止計數。
    pub capped: bool,
    /// 目前這一筆是第幾筆（從 0 起算），僅在有指定目前位置且找得到時有值。
    pub index: Option<usize>,
}

pub struct Searcher {
    re: Regex,
    opts: SearchOptions,
}

impl Searcher {
    pub fn new(pattern: &str, opts: SearchOptions) -> Result<Self, SearchError> {
        if pattern.is_empty() {
            return Err(SearchError::Empty);
        }
        let source: Cow<str> = if opts.regex {
            Cow::Borrowed(pattern)
        } else {
            Cow::Owned(regex::escape(pattern))
        };
        let re = RegexBuilder::new(&source)
            .case_insensitive(!opts.case_sensitive)
            // 編輯器裡 ^ 與 $ 一般是指「每一行」的開頭與結尾
            .multi_line(true)
            .build()
            .map_err(|e| SearchError::InvalidRegex(summarize(&e.to_string())))?;
        Ok(Self { re, opts })
    }

    /// 第一個起點 >= `from` 的符合範圍（不會繞回開頭，由呼叫端決定要不要繞）。
    pub fn find_next(&self, text: &str, from: usize) -> Option<Range<usize>> {
        self.next_match(text, from.min(text.len()))
    }

    /// 最後一個起點 < `before` 的符合範圍（不會繞回結尾）。
    pub fn find_prev(&self, text: &str, before: usize) -> Option<Range<usize>> {
        let mut last = None;
        let mut pos = 0;
        while let Some(m) = self.next_match(text, pos) {
            if m.start >= before {
                break;
            }
            pos = match advance(text, &m) {
                Some(p) => p,
                None => {
                    last = Some(m);
                    break;
                }
            };
            last = Some(m);
        }
        last
    }

    /// 計算總筆數；`current_start` 若有給，會一併算出目前是第幾筆。計數最多到 `limit` 筆。
    pub fn stats(&self, text: &str, limit: usize, current_start: Option<usize>) -> Stats {
        let mut total = 0;
        let mut index = None;
        let mut pos = 0;
        while let Some(m) = self.next_match(text, pos) {
            if current_start == Some(m.start) {
                index = Some(total);
            }
            total += 1;
            if total >= limit {
                return Stats {
                    total,
                    capped: true,
                    index,
                };
            }
            match advance(text, &m) {
                Some(p) => pos = p,
                None => break,
            }
        }
        Stats {
            total,
            capped: false,
            index,
        }
    }

    /// 某個符合範圍對應的取代文字；範圍不再是一個有效的符合時回傳 `None`
    /// （例如文字在這段期間被改過）。
    pub fn expansion(&self, text: &str, m: &Range<usize>, replacement: &str) -> Option<String> {
        if m.end > text.len() || !text.is_char_boundary(m.start) || !text.is_char_boundary(m.end) {
            return None;
        }
        let actual = self.next_match(text, m.start)?;
        if actual != *m {
            return None;
        }
        Some(self.expand_at(text, m, replacement))
    }

    /// 全部取代，回傳（新文字, 取代筆數）。
    pub fn replace_all(&self, text: &str, replacement: &str) -> (String, usize) {
        let mut out = String::with_capacity(text.len());
        let mut last = 0;
        let mut count = 0;
        let mut pos = 0;
        while let Some(m) = self.next_match(text, pos) {
            out.push_str(&text[last..m.start]);
            out.push_str(&self.expand_at(text, &m, replacement));
            last = m.end;
            count += 1;
            match advance(text, &m) {
                Some(p) => pos = p,
                None => break,
            }
        }
        out.push_str(&text[last..]);
        (out, count)
    }

    // ── 內部 ──────────────────────────────────────────────

    /// 從 `pos` 起找下一個通過「完整單字」檢查的符合範圍。
    fn next_match(&self, text: &str, mut pos: usize) -> Option<Range<usize>> {
        loop {
            // find_at 會保留 pos 之前的上下文，所以 ^ 與 \b 的判斷是正確的
            let m = self.re.find_at(text, pos)?;
            let range = m.range();
            if !self.opts.whole_word || is_whole_word(text, &range) {
                return Some(range);
            }
            // 不符合完整單字：從這個起點的下一個字元重試
            pos = next_boundary(text, range.start)?;
        }
    }

    fn expand_at(&self, text: &str, m: &Range<usize>, replacement: &str) -> String {
        if !self.opts.regex {
            return replacement.to_owned();
        }
        let template = unescape(replacement);
        match self.re.captures_at(text, m.start) {
            Some(caps) => {
                let mut dst = String::new();
                caps.expand(&template, &mut dst);
                dst
            }
            None => template,
        }
    }
}

/// 下一次搜尋的起點：非空比對接在它之後；空比對要多跳過一個字元，否則會原地踏步。
/// 已經沒有下一個位置時回傳 `None`。
fn advance(text: &str, m: &Range<usize>) -> Option<usize> {
    if m.is_empty() {
        next_boundary(text, m.end)
    } else {
        Some(m.end)
    }
}

fn next_boundary(text: &str, i: usize) -> Option<usize> {
    text.get(i..)?.chars().next().map(|c| i + c.len_utf8())
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// 比對結果兩端若是單字字元，則外側不可緊鄰單字字元。
/// 兩端若是標點（例如搜尋 `foo.`），該端不需要檢查。
fn is_whole_word(text: &str, r: &Range<usize>) -> bool {
    let first = text[r.start..].chars().next();
    let last = text[..r.end].chars().next_back();
    let before = text[..r.start].chars().next_back();
    let after = text[r.end..].chars().next();
    let start_ok = !matches!((first, before), (Some(f), Some(b)) if is_word_char(f) && is_word_char(b));
    let end_ok = !matches!((last, after), (Some(l), Some(a)) if is_word_char(l) && is_word_char(a));
    start_ok && end_ok
}

/// 規則表達式模式下，取代字串中的 `\n`、`\t`、`\r`、`\\` 轉成對應字元。
/// `$` 保留給 regex 的 `$1`、`${name}`；要輸出字面的 `$` 請寫 `$$`。
fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('\\') => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// regex 的錯誤訊息是多行的（含指示符號），只取最有用的那一行。
fn summarize(msg: &str) -> String {
    msg.lines()
        .rev()
        .find_map(|l| l.trim().strip_prefix("error:").map(|s| s.trim().to_owned()))
        .unwrap_or_else(|| msg.lines().next().unwrap_or(msg).trim().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(pattern: &str, regex: bool, case: bool, word: bool) -> Searcher {
        Searcher::new(
            pattern,
            SearchOptions {
                regex,
                case_sensitive: case,
                whole_word: word,
            },
        )
        .unwrap()
    }

    fn all(sr: &Searcher, text: &str) -> Vec<Range<usize>> {
        let mut out = vec![];
        let mut pos = 0;
        while let Some(m) = sr.find_next(text, pos) {
            let next = advance(text, &m);
            out.push(m);
            match next {
                Some(p) => pos = p,
                None => break,
            }
        }
        out
    }

    #[test]
    fn literal_is_case_insensitive_by_default() {
        let sr = s("Hello", false, false, false);
        assert_eq!(all(&sr, "hello HELLO Hello"), vec![0..5, 6..11, 12..17]);
    }

    #[test]
    fn case_sensitive_option() {
        let sr = s("Hello", false, true, false);
        assert_eq!(all(&sr, "hello HELLO Hello"), vec![12..17]);
    }

    #[test]
    fn literal_mode_does_not_interpret_regex_syntax() {
        let sr = s("a.b", false, true, false);
        assert_eq!(all(&sr, "a.b axb"), vec![0..3]);
        let sr = s("(", false, true, false); // 字面的左括號，不是語法錯誤
        assert_eq!(all(&sr, "f(x)"), vec![1..2]);
    }

    #[test]
    fn regex_mode() {
        let sr = s(r"\d+", true, true, false);
        assert_eq!(all(&sr, "a1 b22 c333"), vec![1..2, 4..6, 8..11]);
    }

    #[test]
    fn invalid_and_empty_patterns() {
        assert_eq!(
            Searcher::new("", SearchOptions::default()).err(),
            Some(SearchError::Empty)
        );
        let e = Searcher::new(
            "(",
            SearchOptions {
                regex: true,
                ..Default::default()
            },
        )
        .err();
        match e {
            Some(SearchError::InvalidRegex(m)) => assert!(!m.contains('\n') && !m.is_empty(), "{m}"),
            other => panic!("預期 InvalidRegex，得到 {other:?}"),
        }
    }

    #[test]
    fn whole_word_rejects_substrings() {
        let sr = s("cat", false, false, true);
        let text = "concatenate cat scat cat. (cat) cats";
        let found: Vec<&str> = all(&sr, text).into_iter().map(|r| &text[r]).collect();
        assert_eq!(found.len(), 3, "只有獨立的 cat：{found:?}");
        assert_eq!(all(&sr, text)[0], 12..15);
    }

    #[test]
    fn whole_word_with_punctuation_edges() {
        // 關鍵字本身以標點結尾時，該端不檢查
        let sr = s("foo.", false, true, true);
        assert_eq!(all(&sr, "foo. foo.bar xfoo."), vec![0..4, 5..9]);
    }

    #[test]
    fn whole_word_retries_after_rejected_candidate() {
        // 第一個 "ab" 在 "cab" 裡被拒絕，後面獨立的 "ab" 仍要找得到
        let sr = s("ab", false, true, true);
        assert_eq!(all(&sr, "cab ab"), vec![4..6]);
    }

    #[test]
    fn whole_word_with_cjk_is_strict() {
        // 中文沒有空格分詞，字母數字判定下相鄰漢字會互相阻擋；這是已知且一致的行為
        let sr = s("世界", false, true, true);
        assert_eq!(all(&sr, "你好世界 世界 "), vec![13..19]);
    }

    #[test]
    fn byte_offsets_with_multibyte_text() {
        let text = "你好世界你好";
        let sr = s("你好", false, true, false);
        assert_eq!(all(&sr, text), vec![0..6, 12..18]);
        assert_eq!(sr.find_next(text, 1), Some(12..18));
        assert_eq!(sr.find_next(text, 12), Some(12..18));
        assert_eq!(sr.find_next(text, 13), None);
    }

    #[test]
    fn find_prev_and_bounds() {
        let sr = s("a", false, true, false);
        let text = "a_a_a";
        assert_eq!(sr.find_prev(text, 5), Some(4..5));
        assert_eq!(sr.find_prev(text, 4), Some(2..3));
        assert_eq!(sr.find_prev(text, 1), Some(0..1));
        assert_eq!(sr.find_prev(text, 0), None);
        assert_eq!(sr.find_next(text, 99), None); // 超出範圍不會 panic
    }

    #[test]
    fn stats_with_current_index_and_cap() {
        let sr = s("a", false, true, false);
        let text = "a_a_a_a";
        let st = sr.stats(text, 100, Some(4));
        assert_eq!((st.total, st.capped, st.index), (4, false, Some(2)));
        let st = sr.stats(text, 3, None);
        assert_eq!((st.total, st.capped), (3, true));
        let st = sr.stats("zzz", 100, Some(0));
        assert_eq!((st.total, st.index), (0, None));
    }

    #[test]
    fn replace_all_literal_does_not_expand_dollar() {
        let sr = s("x", false, true, false);
        let (out, n) = sr.replace_all("axbxc", "$1\\n");
        assert_eq!(out, "a$1\\nb$1\\nc");
        assert_eq!(n, 2);
    }

    #[test]
    fn replace_all_regex_captures_and_escapes() {
        let sr = s(r"(\w+)@(\w+)", true, true, false);
        let (out, n) = sr.replace_all("bob@home, amy@work", "$2 at $1");
        assert_eq!(out, "home at bob, work at amy");
        assert_eq!(n, 2);

        let sr = s(",", true, true, false);
        let (out, _) = sr.replace_all("a,b,c", "\\n");
        assert_eq!(out, "a\nb\nc");
        let (out, _) = sr.replace_all("a,b", "$$");
        assert_eq!(out, "a$b");
    }

    #[test]
    fn empty_matches_terminate_and_work_per_line() {
        // `^` 會得到空比對：用來在每一行前面加字
        let sr = s("^", true, true, false);
        let (out, n) = sr.replace_all("a\nb\nc", "> ");
        assert_eq!(out, "> a\n> b\n> c");
        assert_eq!(n, 3);
        // `$` 同理，且包含文字最末端
        let sr = s("$", true, true, false);
        let (out, n) = sr.replace_all("a\nb", ";");
        assert_eq!(out, "a;\nb;");
        assert_eq!(n, 2);
        // 可能比對到空字串的規則也不能無限迴圈
        let sr = s("x*", true, true, false);
        let (_, n) = sr.replace_all("abc", "-");
        assert!(n > 0);
    }

    #[test]
    fn replace_all_handles_multibyte_and_overlap_free() {
        let sr = s("你好", false, true, false);
        let (out, n) = sr.replace_all("你好你好世界", "Hi");
        assert_eq!(out, "HiHi世界");
        assert_eq!(n, 2);
        // 重疊的候選不會重複計算
        let sr = s("aa", false, true, false);
        let (out, n) = sr.replace_all("aaaa", "b");
        assert_eq!((out.as_str(), n), ("bb", 2));
    }

    #[test]
    fn expansion_validates_range() {
        let sr = s("b", false, true, false);
        let text = "abc";
        assert_eq!(sr.expansion(text, &(1..2), "X").as_deref(), Some("X"));
        assert_eq!(sr.expansion(text, &(0..1), "X"), None); // 這裡不是符合
        assert_eq!(sr.expansion(text, &(1..99), "X"), None); // 超出範圍
        assert_eq!(sr.expansion("你b", &(1..2), "X"), None); // 不在字元邊界
    }

    #[test]
    fn multiline_anchors_and_crlf_free_text() {
        let sr = s("^b", true, true, false);
        assert_eq!(all(&sr, "ab\nb\nbb"), vec![3..4, 5..6]);
    }
}
