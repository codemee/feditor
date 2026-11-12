//! 終端機畫面上的文字處理：找網址、搜尋。與畫面和終端機模擬無關，只處理「字元加上它在畫面的位置」，方便單元測試。
//!
//! 畫面上被折行的一行（行尾有 `WRAPLINE` 標記）在這裡算同一個「邏輯行」：網址或搜尋的字串可以跨越折行。

/// 畫面上的一個字元：寬字（中日韓全形字）佔兩欄，但只有一個 `Glyph`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Glyph {
    pub c: char,
    /// 終端機格線的列號（往上捲出去的歷史是負的）。
    pub line: i32,
    pub col: usize,
    pub wide: bool,
}

impl Glyph {
    /// 這個字元佔的欄數。
    pub fn width(&self) -> usize {
        if self.wide { 2 } else { 1 }
    }
}

/// 同一列上連續的一段欄位 `[start_col, end_col)`；跨越折行的符合項目會分成好幾段。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Segment {
    pub line: i32,
    pub start_col: usize,
    pub end_col: usize,
}

fn fold(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

/// 在 `glyphs` 裡找 `query`（不分大小寫，不重疊），回傳符合的字元範圍 `[開頭, 結尾)`（`glyphs` 的索引）。
pub fn find_matches(glyphs: &[Glyph], query: &str) -> Vec<(usize, usize)> {
    let q: Vec<char> = query.chars().map(fold).collect();
    let mut out = Vec::new();
    if q.is_empty() || glyphs.len() < q.len() {
        return out;
    }
    let mut i = 0;
    while i + q.len() <= glyphs.len() {
        if glyphs[i..i + q.len()].iter().zip(&q).all(|(g, c)| fold(g.c) == *c) {
            out.push((i, i + q.len()));
            i += q.len();
        } else {
            i += 1;
        }
    }
    out
}

/// 把 `glyphs[start..end]` 轉成畫面上的欄位範圍：同一列上連續的字元併成一段。
pub fn segments(glyphs: &[Glyph], start: usize, end: usize) -> Vec<Segment> {
    let mut out: Vec<Segment> = Vec::new();
    for g in &glyphs[start..end] {
        match out.last_mut() {
            Some(s) if s.line == g.line && s.end_col == g.col => s.end_col = g.col + g.width(),
            _ => out.push(Segment { line: g.line, start_col: g.col, end_col: g.col + g.width() }),
        }
    }
    out
}

const SCHEMES: [&str; 4] = ["http://", "https://", "ftp://", "file://"];

/// 程式用 OSC 8 標的連結能不能直接點開：只放行網頁和電子郵件。
/// 連結的文字和網址是程式各自決定的，`file:` 或 `ms-msdt:` 這類會交給系統去執行東西，不開。
pub fn openable_link(uri: &str) -> bool {
    let lower = uri.get(..8).unwrap_or(uri).to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("mailto:")
}

fn scheme_len(glyphs: &[Glyph]) -> Option<usize> {
    SCHEMES.iter().find_map(|s| {
        let n = s.chars().count();
        (glyphs.len() >= n && glyphs[..n].iter().zip(s.chars()).all(|(g, c)| g.c.to_ascii_lowercase() == c)).then_some(n)
    })
}

/// 網址裡可以出現的字元：可見的 ASCII，但不含常拿來包住網址的符號。中日韓文字不算，所以網址後面緊接中文也不會被吃進去。
fn url_char(c: char) -> bool {
    c.is_ascii_graphic() && !matches!(c, '<' | '>' | '"' | '\'' | '`' | '{' | '}' | '|' | '\\' | '^')
}

/// 找出包含 `index` 這個字元的網址（`http`、`https`、`ftp`、`file`）：回傳（開頭, 結尾, 網址文字）。
/// 網址尾巴的標點（`.,;:!?`）和沒有配對的右括號不算在內，所以句尾的網址和括號裡的網址都能正確點到。
pub fn url_at(glyphs: &[Glyph], index: usize) -> Option<(usize, usize, String)> {
    let mut i = 0;
    while i < glyphs.len() {
        let starts_url = scheme_len(&glyphs[i..]).filter(|_| i == 0 || !glyphs[i - 1].c.is_ascii_alphanumeric());
        let Some(prefix) = starts_url else {
            i += 1;
            continue;
        };
        let mut end = i + prefix;
        while end < glyphs.len() && url_char(glyphs[end].c) {
            end += 1;
        }
        // 去掉尾巴的標點與沒有配對的右括號
        while end > i + prefix {
            let last = glyphs[end - 1].c;
            let text: String = glyphs[i..end].iter().map(|g| g.c).collect();
            let count = |c: char| text.chars().filter(|x| *x == c).count();
            let unbalanced = |open: char, close: char| count(close) > count(open);
            if matches!(last, '.' | ',' | ';' | ':' | '!' | '?') || (last == ')' && unbalanced('(', ')')) || (last == ']' && unbalanced('[', ']')) {
                end -= 1;
            } else {
                break;
            }
        }
        if end > i + prefix && index >= i && index < end {
            return Some((i, end, glyphs[i..end].iter().map(|g| g.c).collect()));
        }
        i = end.max(i + 1);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 一列文字：字元依序排在第 `line` 列、從第 0 欄開始（全形字佔兩欄）。
    fn row(text: &str, line: i32) -> Vec<Glyph> {
        let mut col = 0;
        text.chars()
            .map(|c| {
                let wide = !c.is_ascii();
                let g = Glyph { c, line, col, wide };
                col += g.width();
                g
            })
            .collect()
    }

    fn text(glyphs: &[Glyph], range: (usize, usize)) -> String {
        glyphs[range.0..range.1].iter().map(|g| g.c).collect()
    }

    #[test]
    fn matches_ignore_case_do_not_overlap_and_handle_wide_characters() {
        let g = row("Error: error ERROR", 0);
        assert_eq!(find_matches(&g, "error").len(), 3);
        assert_eq!(find_matches(&g, "ERR"), vec![(0, 3), (7, 10), (13, 16)]);
        assert!(find_matches(&g, "").is_empty());
        assert!(find_matches(&g, "missing").is_empty());
        // 不重疊：aaaa 裡找 aa 是兩筆
        assert_eq!(find_matches(&row("aaaa", 0), "aa"), vec![(0, 2), (2, 4)]);
        // 中文：索引是字元，欄位範圍要算兩欄
        let cjk = row("a你好b", 0);
        let m = find_matches(&cjk, "你好");
        assert_eq!(m, vec![(1, 3)]);
        assert_eq!(segments(&cjk, 1, 3), vec![Segment { line: 0, start_col: 1, end_col: 5 }]);
    }

    #[test]
    fn a_match_that_crosses_a_wrapped_line_becomes_one_segment_per_row() {
        let mut g = row("see the err", 3);
        g.extend(row("or here", 4));
        let m = find_matches(&g, "error");
        assert_eq!(m.len(), 1);
        assert_eq!(
            segments(&g, m[0].0, m[0].1),
            vec![Segment { line: 3, start_col: 8, end_col: 11 }, Segment { line: 4, start_col: 0, end_col: 2 }]
        );
    }

    #[test]
    fn urls_are_found_with_trailing_punctuation_and_unbalanced_brackets_trimmed() {
        let url = |s: &str, at: usize| {
            let g = row(s, 0);
            url_at(&g, at).map(|(a, b, u)| (text(&g, (a, b)), u))
        };
        let plain = "see https://example.com/a?b=1#c now";
        assert_eq!(url(plain, 10).map(|x| x.1).as_deref(), Some("https://example.com/a?b=1#c"));
        assert_eq!(url(plain, 0), None, "網址前面的字不算");
        assert_eq!(url(plain, 31), None, "網址後面的字不算");
        // 句尾標點不算網址
        assert_eq!(url("go to http://x.org/path.", 10).map(|x| x.1).as_deref(), Some("http://x.org/path"));
        assert_eq!(url("(http://x.org/a)", 5).map(|x| x.1).as_deref(), Some("http://x.org/a"), "沒配對的右括號不算");
        assert_eq!(url("http://en.wikipedia.org/wiki/Rust_(language)", 5).map(|x| x.1).as_deref(), Some("http://en.wikipedia.org/wiki/Rust_(language)"), "有配對的括號要留著");
        assert_eq!(url("<https://x.org/a>", 5).map(|x| x.1).as_deref(), Some("https://x.org/a"));
        assert_eq!(url("\"ftp://host/f.txt\"", 5).map(|x| x.1).as_deref(), Some("ftp://host/f.txt"));
        assert_eq!(url("file:///C:/a/b.txt ok", 3).map(|x| x.1).as_deref(), Some("file:///C:/a/b.txt"));
        assert_eq!(url("HTTPS://X.ORG/A", 3).map(|x| x.1).as_deref(), Some("HTTPS://X.ORG/A"), "協定不分大小寫");
        // 協定前面不能緊接字母，只有協定沒有內容也不算
        assert_eq!(url("xhttp://x.org", 6), None);
        assert_eq!(url("http:// alone", 2), None);
        // 中文緊接在網址後面不會被吃進去
        assert_eq!(url("網址https://x.org/a很好", 6).map(|x| x.1).as_deref(), Some("https://x.org/a"));
        // 兩個網址：點到哪個就是哪個
        let two = "http://a.org http://b.org";
        assert_eq!(url(two, 2).map(|x| x.1).as_deref(), Some("http://a.org"));
        assert_eq!(url(two, 16).map(|x| x.1).as_deref(), Some("http://b.org"));
    }

    #[test]
    fn a_url_that_wraps_onto_the_next_row_is_one_url() {
        let mut g = row("open https://example.com/a/lo", 5);
        let first_row_len = g.len();
        g.extend(row("ng/path now", 6));
        let at_second_row = first_row_len + 4; // "ng/p" 的 p
        let (a, b, u) = url_at(&g, at_second_row).expect("跨列的網址");
        assert_eq!(u, "https://example.com/a/long/path");
        let segs = segments(&g, a, b);
        assert_eq!(segs.iter().map(|s| s.line).collect::<Vec<_>>(), vec![5, 6], "底線要畫在兩列上");
    }
}
