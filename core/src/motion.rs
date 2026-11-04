//! 游標移動：字素叢集（grapheme cluster）、詞、以及顯示用的欄位換算。
//!
//! 游標不能一次只動一個 `char`：`é`（`e` 加組合重音）、`👍🏽`（拇指加膚色）、
//! `👨‍👩‍👧`（用零寬連接符組成的家庭 emoji）、`🇹🇼`（兩個區域指示符）都是由多個 `char` 組成的
//! 「一個字」，游標切在中間就會把字切壞。所以左右移動與刪除都以字素叢集為單位。

use ropey::Rope;
use std::ops::Range;
use unicode_segmentation::{GraphemeCursor, GraphemeIncomplete};
use unicode_width::UnicodeWidthChar;

/// Tab 對齊的預設欄寬。
pub const DEFAULT_TAB_WIDTH: usize = 4;
/// Tab 欄寬的範圍。
pub const MIN_TAB_WIDTH: usize = 1;
pub const MAX_TAB_WIDTH: usize = 32;

// Tab 的設定（欄寬、輸入 Tab 時是否改用空白）。編輯器只在畫面執行緒上跑，所以放在執行緒本地的變數：
// 所有欄位換算函式（`display_col`、`expand_tabs`…）不用到處多傳一個參數。畫面在畫每一個編輯區、
// 處理每一次輸入之前，依那份文件的檔案類型設好；測試的每個執行緒各有一份，預設是 4 欄、不用空白。
thread_local! {
    static TAB_WIDTH_CELL: std::cell::Cell<usize> = const { std::cell::Cell::new(DEFAULT_TAB_WIDTH) };
    static TAB_SPACES_CELL: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static AUTO_INDENT_CELL: std::cell::Cell<bool> = const { std::cell::Cell::new(true) };
    static WRAP_MARGIN_CELL: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// 排列格式時一行不要超過的欄數（安全界線）；沒有啟用安全界線就是 `None`。
pub fn wrap_margin() -> Option<usize> {
    Some(WRAP_MARGIN_CELL.get()).filter(|m| *m > 0)
}

/// 設定排列格式的行寬上限（`None` 表示不限），回傳原本的值。
pub fn set_wrap_margin(margin: Option<usize>) -> Option<usize> {
    Some(WRAP_MARGIN_CELL.replace(margin.unwrap_or(0))).filter(|m| *m > 0)
}

/// 按 Enter 時是不是依語言自動加深／減淺縮排（關閉時只沿用上一行的縮排）。
pub fn auto_indent() -> bool {
    AUTO_INDENT_CELL.get()
}

/// 設定自動縮排的開關，回傳原本的值。
pub fn set_auto_indent(on: bool) -> bool {
    AUTO_INDENT_CELL.replace(on)
}

/// 目前的 Tab 欄寬。
pub fn tab_width() -> usize {
    TAB_WIDTH_CELL.get()
}

/// 設定 Tab 欄寬（拉回 [`MIN_TAB_WIDTH`]～[`MAX_TAB_WIDTH`]），回傳原本的值，方便用完還原。
pub fn set_tab_width(width: usize) -> usize {
    TAB_WIDTH_CELL.replace(width.clamp(MIN_TAB_WIDTH, MAX_TAB_WIDTH))
}

/// 輸入 Tab 時是不是改成輸入空白（縮排也用空白）。
pub fn tab_uses_spaces() -> bool {
    TAB_SPACES_CELL.get()
}

/// 設定輸入 Tab 時是否改用空白，回傳原本的值。
pub fn set_tab_uses_spaces(on: bool) -> bool {
    TAB_SPACES_CELL.replace(on)
}

fn chars_before(chunk: &str, byte_idx: usize) -> usize {
    chunk[..byte_idx].chars().count()
}

/// 下一個字素叢集的邊界（字元索引）。已在末端時回傳末端。
///
/// rope 是分塊儲存的，一個字素叢集可能橫跨兩個區塊，所以要用 `GraphemeCursor`
/// 一塊一塊餵給它。
pub fn next_grapheme(rope: &Rope, char_idx: usize) -> usize {
    let len_chars = rope.len_chars();
    if char_idx >= len_chars {
        return len_chars;
    }
    let byte_idx = rope.char_to_byte(char_idx);
    let (mut chunk, mut chunk_byte_idx, mut chunk_char_idx, _) = rope.chunk_at_byte(byte_idx);
    let mut cursor = GraphemeCursor::new(byte_idx, rope.len_bytes(), true);
    loop {
        match cursor.next_boundary(chunk, chunk_byte_idx) {
            Ok(None) => return len_chars,
            Ok(Some(n)) => return chunk_char_idx + chars_before(chunk, n - chunk_byte_idx),
            Err(GraphemeIncomplete::NextChunk) => {
                let (c, b, ci, _) = rope.chunk_at_byte(chunk_byte_idx + chunk.len());
                (chunk, chunk_byte_idx, chunk_char_idx) = (c, b, ci);
            }
            Err(GraphemeIncomplete::PreContext(n)) => {
                let (ctx, ctx_start, _, _) = rope.chunk_at_byte(n - 1);
                cursor.provide_context(ctx, ctx_start);
            }
            // 理論上不會發生；保底往前一個字元，確保游標一定會動
            Err(_) => return (char_idx + 1).min(len_chars),
        }
    }
}

/// 上一個字素叢集的邊界（字元索引）。已在開頭時回傳 0。
pub fn prev_grapheme(rope: &Rope, char_idx: usize) -> usize {
    let char_idx = char_idx.min(rope.len_chars());
    if char_idx == 0 {
        return 0;
    }
    let byte_idx = rope.char_to_byte(char_idx);
    let (mut chunk, mut chunk_byte_idx, mut chunk_char_idx, _) =
        rope.chunk_at_byte(byte_idx.saturating_sub(1));
    let mut cursor = GraphemeCursor::new(byte_idx, rope.len_bytes(), true);
    loop {
        match cursor.prev_boundary(chunk, chunk_byte_idx) {
            Ok(None) => return 0,
            Ok(Some(n)) => return chunk_char_idx + chars_before(chunk, n - chunk_byte_idx),
            Err(GraphemeIncomplete::PrevChunk) => {
                let (c, b, ci, _) = rope.chunk_at_byte(chunk_byte_idx - 1);
                (chunk, chunk_byte_idx, chunk_char_idx) = (c, b, ci);
            }
            Err(GraphemeIncomplete::PreContext(n)) => {
                let (ctx, ctx_start, _, _) = rope.chunk_at_byte(n - 1);
                cursor.provide_context(ctx, ctx_start);
            }
            Err(_) => return char_idx - 1,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Class {
    Space,
    Word,
    Other,
    Newline,
}

fn class(c: char) -> Class {
    if c == '\n' {
        Class::Newline
    } else if c.is_whitespace() {
        Class::Space
    } else if c.is_alphanumeric() || c == '_' {
        // 漢字、假名也算 alphanumeric：一串連續的中文會被當成同一個「詞」
        Class::Word
    } else {
        Class::Other
    }
}

/// Ctrl＋→：移到下一個詞的開頭。在行尾時移到下一行的開頭。
pub fn word_right(rope: &Rope, char_idx: usize) -> usize {
    let n = rope.len_chars();
    let mut i = char_idx.min(n);
    if i >= n {
        return n;
    }
    let first = class(rope.char(i));
    if first == Class::Newline {
        return i + 1;
    }
    while i < n && class(rope.char(i)) == first {
        i += 1;
    }
    if first != Class::Space {
        while i < n && class(rope.char(i)) == Class::Space {
            i += 1;
        }
    }
    i
}

/// Ctrl＋←：移到這個詞（或上一個詞）的開頭。在行首時移到上一行的行尾。
pub fn word_left(rope: &Rope, char_idx: usize) -> usize {
    let mut i = char_idx.min(rope.len_chars());
    if i == 0 {
        return 0;
    }
    while i > 0 && class(rope.char(i - 1)) == Class::Space {
        i -= 1;
    }
    if i == 0 {
        return 0;
    }
    let last = class(rope.char(i - 1));
    if last == Class::Newline {
        return i - 1;
    }
    while i > 0 && class(rope.char(i - 1)) == last {
        i -= 1;
    }
    i
}

/// 雙擊選取：游標所在位置的那一個「詞」（同一類字元連續的範圍）。
pub fn word_at(rope: &Rope, char_idx: usize) -> Range<usize> {
    let n = rope.len_chars();
    if n == 0 {
        return 0..0;
    }
    let idx = char_idx.min(n - 1);
    let cls = class(rope.char(idx));
    if cls == Class::Newline {
        return idx..idx + 1;
    }
    let (mut start, mut end) = (idx, idx + 1);
    while start > 0 && class(rope.char(start - 1)) == cls {
        start -= 1;
    }
    while end < n && class(rope.char(end)) == cls {
        end += 1;
    }
    start..end
}

// ── 顯示用的欄位換算 ──────────────────────────────────────

/// 一個字元佔幾欄：中文與全形字 2 欄，組合用的字元 0 欄，其餘 1 欄。
pub fn char_width(c: char) -> usize {
    c.width().unwrap_or(0)
}

/// 狀態列顯示的欄號（從 1 起算）：游標 `caret` 之前，這一行（從 `line_start` 起）有幾個字元，加 1。
/// 全形字（中日韓文字、全形標點等）算 `wide_cols` 欄（1 或 2），其餘字元算 1 欄；組合用的字元不佔欄；
/// Tab 算 1 欄（這是「字元位置」，不是畫面上對齊後的欄位）。
pub fn caret_column(rope: &Rope, line_start: usize, caret: usize, wide_cols: usize) -> usize {
    let end = caret.min(rope.len_chars());
    let start = line_start.min(end);
    let wide_cols = wide_cols.clamp(1, 2);
    1 + rope
        .slice(start..end)
        .chars()
        .map(|c| match char_width(c) {
            _ if c == '\t' => 1,
            0 => 0,
            2 => wide_cols,
            _ => 1,
        })
        .sum::<usize>()
}

fn advance(col: usize, c: char) -> usize {
    if c == '\t' {
        (col / tab_width() + 1) * tab_width()
    } else {
        col + char_width(c)
    }
}

/// 這一行第 `char_idx` 個字元之前佔了幾欄（Tab 會對齊到 [`tab_width`] 的倍數）。
/// 上下移動游標時要記住「想停在第幾欄」，就是用這個欄位。
pub fn display_col(line: &str, char_idx: usize) -> usize {
    line.chars().take(char_idx).fold(0, advance)
}

/// 顯示欄位對應到最接近的字元索引。欄位超過行尾時回傳行尾；
/// 落在全形字中間時，依靠近哪一側決定。
pub fn char_at_display_col(line: &str, col: usize) -> usize {
    let mut start = 0;
    for (idx, c) in line.chars().enumerate() {
        let end = advance(start, c);
        if col < end {
            let width = end - start;
            return if width > 1 && col - start >= width.div_ceil(2) {
                idx + 1
            } else {
                idx
            };
        }
        start = end;
    }
    line.chars().count()
}

/// 畫面上的位置（以欄為單位，可以是小數）對應到最近的字元邊界。
/// 每個字元佔它的欄寬（Tab 對齊到 [`tab_width`]），位置落在字元的前半就停在它前面、後半就停在後面。
/// 超過行尾回傳行尾；零寬度的組合字元跟著前一個字元。
pub fn char_index_at_col(line: &str, col: f32) -> usize {
    let mut start = 0usize;
    for (idx, c) in line.chars().enumerate() {
        let end = advance(start, c);
        let width = end - start;
        if width > 0 && col < start as f32 + width as f32 / 2.0 {
            return idx;
        }
        start = end;
    }
    line.chars().count()
}

/// 把 Tab 展開成空白，給畫面排版用（`egui` 不會自己把 Tab 對齊到固定欄寬）。
pub fn expand_tabs(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut col = 0;
    for c in line.chars() {
        if c == '\t' {
            let next = (col / tab_width() + 1) * tab_width();
            out.extend(std::iter::repeat_n(' ', next - col));
            col = next;
        } else {
            out.push(c);
            col += char_width(c);
        }
    }
    out
}

/// 原文第 `char_idx` 個字元，在 [`expand_tabs`] 展開後的文字裡是第幾個字元。
/// 排版出來的字形陣列用的是展開後的索引，要換算才能對到游標位置。
pub fn display_index(line: &str, char_idx: usize) -> usize {
    let mut index = 0;
    let mut col = 0;
    for c in line.chars().take(char_idx) {
        if c == '\t' {
            let next = (col / tab_width() + 1) * tab_width();
            index += next - col;
            col = next;
        } else {
            index += 1;
            col += char_width(c);
        }
    }
    index
}

/// [`display_index`] 的反函數：展開後的第 `d` 個字元，對應原文的第幾個字元。
/// 滑鼠點擊時，排版出來的位置是展開後的索引，要換回原文才能放游標。
/// 落在 Tab 展開出來的空白中間時，依靠近哪一側決定。
pub fn char_index_from_display_index(line: &str, d: usize) -> usize {
    let mut index = 0;
    let mut col = 0;
    for (i, c) in line.chars().enumerate() {
        let width = if c == '\t' {
            (col / tab_width() + 1) * tab_width() - col
        } else {
            1
        };
        if d < index + width {
            return if width > 1 && d - index >= width.div_ceil(2) {
                i + 1
            } else {
                i
            };
        }
        index += width;
        col = advance(col, c);
    }
    line.chars().count()
}

/// 不可以出現在行首的標點（避頭點）：折行時會把前一個字一起帶下來。
const NO_LINE_START: &str = "，。、；：！？）」』】》〉…‧,.;:!?)]}%";

/// 把一行文字依顯示欄數 `max_cols` 折成數列，回傳每一列的字元範圍（涵蓋整行、互不重疊）。
///
/// 盡量在空白之後、或中日韓字元前後折行，英文單字不會被切開（單字比一整列還長時才硬切）；
/// 避頭點不會落在行首。Tab 以「該列開頭」起算對齊。空行也有一列（`0..0`）。
pub fn wrap_rows(line: &str, max_cols: usize) -> Vec<Range<usize>> {
    let chars: Vec<char> = line.chars().collect();
    // 至少容得下一個全形字，不然會無窮折行
    let max_cols = max_cols.max(2);
    let mut rows = Vec::new();
    let mut start = 0;
    while start < chars.len() {
        let mut col = 0;
        let mut i = start;
        let mut brk: Option<usize> = None; // 最近一個可以折行的位置（下一列從這裡開始）
        while i < chars.len() {
            let c = chars[i];
            let w = if c == '\t' {
                tab_width() - col % tab_width()
            } else {
                char_width(c)
            };
            // 中日韓字元（寬度 2）之前、以及它之後都可以折，但不能讓避頭點落在行首
            if i > start && !NO_LINE_START.contains(c) && (w == 2 || char_width(chars[i - 1]) == 2) {
                brk = Some(i);
            }
            if w > 0 && col + w > max_cols && i > start {
                break;
            }
            col += w;
            i += 1;
            if c == ' ' || c == '\t' {
                brk = Some(i);
            }
        }
        if i >= chars.len() {
            rows.push(start..chars.len());
            break;
        }
        let mut at = brk.filter(|b| *b > start && *b <= i).unwrap_or(i);
        // 組合字元要跟著前一個字元
        while at < chars.len() && at > start && char_width(chars[at]) == 0 && chars[at] != '\t' {
            at -= 1;
        }
        if at <= start {
            at = i.max(start + 1);
        }
        // 避頭點：把前一個字一起帶到下一列
        if at > start + 1 && NO_LINE_START.contains(chars[at]) {
            at -= 1;
        }
        rows.push(start..at);
        start = at;
    }
    if rows.is_empty() {
        rows.push(0..0);
    }
    rows
}
#[cfg(test)]
mod tests {
    use super::*;
    use unicode_segmentation::UnicodeSegmentation;

    fn rope(s: &str) -> Rope {
        Rope::from_str(s)
    }

    #[test]
    fn ascii_and_cjk_move_one_char() {
        let r = rope("ab你好");
        assert_eq!(next_grapheme(&r, 0), 1);
        assert_eq!(next_grapheme(&r, 2), 3);
        assert_eq!(next_grapheme(&r, 3), 4);
        assert_eq!(next_grapheme(&r, 4), 4, "末端不再前進");
        assert_eq!(prev_grapheme(&r, 4), 3);
        assert_eq!(prev_grapheme(&r, 1), 0);
        assert_eq!(prev_grapheme(&r, 0), 0, "開頭不再後退");
        assert_eq!(next_grapheme(&r, 999), 4, "超出範圍視為末端");
        assert_eq!(next_grapheme(&rope(""), 0), 0);
        assert_eq!(prev_grapheme(&rope(""), 0), 0);
    }

    #[test]
    fn clusters_are_never_split() {
        // 每一個都是「一個字」，但由多個 char 組成
        for (name, text) in [
            ("組合重音 é", "e\u{301}"),
            ("膚色 👍🏽", "👍🏽"),
            ("家庭 👨‍👩‍👧", "👨‍👩‍👧"),
            ("旗幟 🇹🇼", "🇹🇼"),
            ("韓文組合", "\u{1112}\u{1161}\u{11ab}"),
        ] {
            let r = rope(&format!("x{text}y"));
            let inner = text.chars().count();
            assert_eq!(next_grapheme(&r, 1), 1 + inner, "{name}：往右要一次跳過整個字");
            assert_eq!(prev_grapheme(&r, 1 + inner), 1, "{name}：往左要一次跳過整個字");
            // 落在字中間時，也要退回字的開頭／前進到字的結尾
            if inner > 1 {
                assert_eq!(prev_grapheme(&r, 2), 1, "{name}：從字中間往左");
            }
        }
    }

    /// 和標準的 UAX #29 實作逐一比對，而且文字夠長、橫跨很多 rope 區塊，
    /// 這樣「字素叢集跨區塊」的情況也被測到。
    #[test]
    fn matches_unicode_segmentation_across_rope_chunks() {
        let pieces = ["a", "e\u{301}", "你", "😀", "👍🏽", "👨‍👩‍👧", "🇹🇼", "bc", "\n", "世界"];
        let mut text = String::new();
        for i in 0..2500 {
            text.push_str(pieces[(i * 7 + i / 3) % pieces.len()]);
        }
        let r = rope(&text);
        assert!(text.len() > 8000, "文字要夠長才會有多個區塊");

        // 預期的邊界：每個字素叢集的起點（字元索引），加上末端
        let mut boundaries = Vec::new();
        let mut char_pos = 0;
        for g in text.graphemes(true) {
            boundaries.push(char_pos);
            char_pos += g.chars().count();
        }
        boundaries.push(char_pos);
        let total = r.len_chars();
        assert_eq!(char_pos, total);

        for i in 0..=total {
            let expected_next = boundaries.iter().copied().find(|&b| b > i).unwrap_or(total);
            let expected_prev = boundaries.iter().copied().rev().find(|&b| b < i).unwrap_or(0);
            assert_eq!(next_grapheme(&r, i), expected_next, "next 在 {i}");
            assert_eq!(prev_grapheme(&r, i), expected_prev, "prev 在 {i}");
        }
    }

    #[test]
    fn word_movement_by_character_class() {
        let r = rope("foo_bar  baz.qux");
        // Ctrl+→：到下一個詞的開頭（跳過詞本身與後面的空白）
        assert_eq!(word_right(&r, 0), 9, "foo_bar 是一個詞，加上後面兩個空白");
        assert_eq!(word_right(&r, 9), 12, "baz 之後是標點 .");
        assert_eq!(word_right(&r, 12), 13, ". 自己是一類");
        assert_eq!(word_right(&r, 13), 16);
        assert_eq!(word_right(&r, 16), 16);
        // Ctrl+←：到這個詞（或上一個詞）的開頭
        assert_eq!(word_left(&r, 16), 13);
        assert_eq!(word_left(&r, 13), 12);
        assert_eq!(word_left(&r, 12), 9);
        assert_eq!(word_left(&r, 9), 0);
        assert_eq!(word_left(&r, 0), 0);
        assert_eq!(word_left(&r, 5), 0, "在詞中間：退到詞的開頭");
    }

    #[test]
    fn word_movement_treats_newline_as_a_stop() {
        let r = rope("ab\ncd");
        assert_eq!(word_right(&r, 0), 2, "到行尾");
        assert_eq!(word_right(&r, 2), 3, "換行本身算一步");
        assert_eq!(word_left(&r, 5), 3);
        assert_eq!(word_left(&r, 3), 2, "在行首往左：到上一行的行尾");
    }

    #[test]
    fn word_movement_with_chinese() {
        let r = rope("你好世界 hello 再見");
        assert_eq!(word_right(&r, 0), 5, "連續的中文是一個詞");
        assert_eq!(word_right(&r, 5), 11);
        assert_eq!(word_left(&r, 11), 5);
        assert_eq!(word_left(&r, 5), 0);
    }

    #[test]
    fn word_at_selects_the_run_under_the_cursor() {
        let r = rope("foo bar.baz");
        assert_eq!(word_at(&r, 1), 0..3);
        assert_eq!(word_at(&r, 3), 3..4, "落在空白上：選那串空白");
        assert_eq!(word_at(&r, 4), 4..7);
        assert_eq!(word_at(&r, 7), 7..8, "標點自己一類");
        assert_eq!(word_at(&r, 11), 8..11, "游標在文字末端：取最後一個詞");
        assert_eq!(word_at(&rope("a\nb"), 1), 1..2, "換行單獨一個");
        assert_eq!(word_at(&rope(""), 0), 0..0);
        assert_eq!(word_at(&rope("世界你好"), 2), 0..4);
    }

    #[test]
    fn caret_column_counts_wide_chars_by_setting() {
        let r = rope("ab\na你好b\tc");
        // 第 1 行行首與行中
        assert_eq!(caret_column(&r, 0, 0, 1), 1);
        assert_eq!(caret_column(&r, 0, 2, 2), 3);
        // 第 2 行（從字元 3 起）：a 你 好 b Tab c
        assert_eq!(caret_column(&r, 3, 3, 1), 1);
        assert_eq!(caret_column(&r, 3, 5, 1), 3, "全形字算 1 欄");
        assert_eq!(caret_column(&r, 3, 5, 2), 4, "全形字算 2 欄");
        assert_eq!(caret_column(&r, 3, 6, 2), 6, "a(1)+你好(4)，再加 1");
        assert_eq!(caret_column(&r, 3, 7, 2), 7, "a(1)+你好(4)+b(1)，再加 1");
        assert_eq!(caret_column(&r, 3, 8, 2), 8, "Tab 算 1 欄");
        // 組合字元不佔欄、參數超出範圍時不會當掉
        assert_eq!(caret_column(&rope("e\u{301}x"), 0, 2, 1), 2);
        assert_eq!(caret_column(&r, 3, 999, 2), 1 + 1 + 4 + 1 + 1 + 1);
        assert_eq!(caret_column(&r, 3, 5, 9), 4, "寬度只接受 1 或 2");
    }

    #[test]
    fn caret_column_handles_japanese_korean_and_symbols() {
        let col = |s: &str, wide: usize| caret_column(&rope(s), 0, s.chars().count(), wide);
        // 平假名、片假名、漢字、全形標點與全形英數：全形字
        assert_eq!(col("あア日。「Ａ", 1), 7);
        assert_eq!(col("あア日。「Ａ", 2), 13);
        // 半形片假名：寬度 1，兩種模式都一樣
        assert_eq!(col("ｱｲｳ", 1), 4);
        assert_eq!(col("ｱｲｳ", 2), 4);
        // 韓文
        assert_eq!(col("한글", 2), 5);
        // 分開存放的濁點（U+3099）不佔欄：「か」＋濁點只算一個字
        assert_eq!(col("か\u{3099}", 1), 2);
        assert_eq!(col("か\u{3099}", 2), 3);
        // 寬度不明確的符號當半形，兩種模式都算 1 欄
        assert_eq!(col("※○●★←", 1), 6);
        assert_eq!(col("※○●★←", 2), 6);
        // 日文與英數混排
        assert_eq!(col("abcこんにちは", 2), 14);
        assert_eq!(col("abcこんにちは", 1), 9);
    }

    #[test]
    fn display_width_of_cjk_and_tabs() {
        assert_eq!(char_width('a'), 1);
        assert_eq!(char_width('你'), 2);
        assert_eq!(char_width('😀'), 2);
        assert_eq!(char_width('\u{301}'), 0, "組合重音不佔欄");
        assert_eq!(display_col("a你b", 0), 0);
        assert_eq!(display_col("a你b", 1), 1);
        assert_eq!(display_col("a你b", 2), 3);
        assert_eq!(display_col("a你b", 3), 4);
        // Tab 對齊到 4 的倍數
        assert_eq!(display_col("\tx", 1), 4);
        assert_eq!(display_col("ab\tx", 3), 4);
        assert_eq!(display_col("abcd\tx", 5), 8);
        assert_eq!(display_col("abc", 99), 3, "超出範圍不會 panic");
    }

    #[test]
    fn char_at_display_col_picks_the_nearest_boundary() {
        let line = "a你b";
        assert_eq!(char_at_display_col(line, 0), 0);
        assert_eq!(char_at_display_col(line, 1), 1, "你 的左半邊：停在 你 之前");
        assert_eq!(char_at_display_col(line, 2), 2, "你 的右半邊：停在 你 之後");
        assert_eq!(char_at_display_col(line, 3), 2);
        assert_eq!(char_at_display_col(line, 4), 3);
        assert_eq!(char_at_display_col(line, 99), 3, "超過行尾：行尾");
        assert_eq!(char_at_display_col("", 5), 0);
        // 與 display_col 互為反函數（落在字元邊界上時）
        for text in ["abc", "你好世界", "a\tb你", "x😀y"] {
            for i in 0..=text.chars().count() {
                assert_eq!(char_at_display_col(text, display_col(text, i)), i, "{text:?} 的第 {i} 個");
            }
        }
    }

    #[test]
    fn display_index_roundtrip() {
        for text in ["abc", "a\tb", "\t\tx", "你\t好\t", "", "x\t"] {
            for i in 0..=text.chars().count() {
                let d = display_index(text, i);
                assert_eq!(char_index_from_display_index(text, d), i, "{text:?} 的第 {i} 個");
            }
        }
        // 落在 Tab 展開的空白中間：靠近哪邊就去哪邊
        assert_eq!(char_index_from_display_index("a\tb", 2), 1, "Tab 的前半：停在 Tab 之前");
        assert_eq!(char_index_from_display_index("a\tb", 3), 2, "Tab 的後半：停在 Tab 之後");
        assert_eq!(char_index_from_display_index("abc", 99), 3, "超過行尾：行尾");
    }

    #[test]
    fn the_tab_width_is_adjustable_clamped_and_restorable() {
        assert_eq!(tab_width(), DEFAULT_TAB_WIDTH);
        let before = set_tab_width(8);
        assert_eq!(before, 4);
        assert_eq!(tab_width(), 8);
        assert_eq!(display_col("\tx", 1), 8);
        assert_eq!(display_col("ab\tx", 3), 8);
        assert_eq!(expand_tabs("a\tb"), format!("a{}b", " ".repeat(7)));
        assert_eq!(display_index("a\tb", 2), 8, "Tab 展開成 7 個空白，b 在第 8 個");
        assert_eq!(char_index_at_col("a\tb", 5.0), 2, "Tab 的後半：停在 Tab 之後");
        set_tab_width(2);
        assert_eq!(display_col("a\tb", 2), 2);
        // 拉回範圍內
        set_tab_width(0);
        assert_eq!(tab_width(), MIN_TAB_WIDTH);
        set_tab_width(999);
        assert_eq!(tab_width(), MAX_TAB_WIDTH);
        set_tab_width(before);
        assert_eq!(tab_width(), 4);
        // 空白取代 Tab 的設定
        assert!(!tab_uses_spaces());
        assert!(!set_tab_uses_spaces(true));
        assert!(tab_uses_spaces());
        set_tab_uses_spaces(false);
    }

    #[test]
    fn expand_tabs_and_index_mapping() {
        assert_eq!(expand_tabs("a\tb"), "a   b");
        assert_eq!(expand_tabs("\t\t"), "        ");
        assert_eq!(expand_tabs("你\tb"), "你  b", "你 佔 2 欄，Tab 補到第 4 欄");
        assert_eq!(expand_tabs("plain"), "plain");
        // 展開後的索引
        assert_eq!(display_index("a\tb", 0), 0);
        assert_eq!(display_index("a\tb", 1), 1);
        assert_eq!(display_index("a\tb", 2), 4, "Tab 展開成 3 個空白，b 在第 4 個");
        assert_eq!(display_index("a\tb", 3), 5);
        // 沒有 Tab 時索引不變
        assert_eq!(display_index("你好", 2), 2);
        // 展開後的長度就是最後一個字元的展開索引
        for text in ["a\tb", "\t", "你\t好\t", "abc"] {
            assert_eq!(display_index(text, text.chars().count()), expand_tabs(text).chars().count());
        }
    }

    #[test]
    fn char_index_at_col_picks_the_nearest_boundary() {
        // 半形：前半在前面、後半在後面
        assert_eq!(char_index_at_col("abc", 0.2), 0);
        assert_eq!(char_index_at_col("abc", 0.6), 1);
        assert_eq!(char_index_at_col("abc", 99.0), 3);
        assert_eq!(char_index_at_col("", 3.0), 0);
        // 全形佔 2 欄：1.0 是它的正中央
        assert_eq!(char_index_at_col("中文", 0.9), 0);
        assert_eq!(char_index_at_col("中文", 1.1), 1);
        assert_eq!(char_index_at_col("中文", 2.9), 1);
        assert_eq!(char_index_at_col("中文", 3.1), 2);
        // Tab：4 欄
        assert_eq!(char_index_at_col("a\tb", 1.5), 1);
        assert_eq!(char_index_at_col("a\tb", 2.6), 2);
        assert_eq!(char_index_at_col("a\tb", 3.9), 2);
        // 組合字元零寬，跟著前一個字
        assert_eq!(char_index_at_col("e\u{301}x", 0.3), 0);
        assert_eq!(char_index_at_col("e\u{301}x", 0.7), 2);
    }
    fn rows_of(line: &str, cols: usize) -> Vec<String> {
        let chars: Vec<char> = line.chars().collect();
        wrap_rows(line, cols)
            .into_iter()
            .map(|r| chars[r].iter().collect())
            .collect()
    }

    #[test]
    fn wrap_rows_short_and_empty_lines_are_one_row() {
        assert_eq!(wrap_rows("", 10), vec![0..0]);
        assert_eq!(wrap_rows("abc", 10), vec![0..3]);
        assert_eq!(wrap_rows("0123456789", 10), vec![0..10], "剛好放得下");
    }

    #[test]
    fn wrap_rows_breaks_after_spaces_and_not_inside_words() {
        assert_eq!(rows_of("hello world foo", 8), vec!["hello ", "world ", "foo"]);
        assert_eq!(rows_of("abcdefghijkl", 5), vec!["abcde", "fghij", "kl"], "單字太長才硬切");
    }

    #[test]
    fn wrap_rows_cjk_breaks_between_characters() {
        // 每個全形字 2 欄：一列放 3 個字
        assert_eq!(rows_of("一二三四五六七", 6), vec!["一二三", "四五六", "七"]);
        // 全形字放不下最後一欄就整個移到下一列
        assert_eq!(rows_of("abc一二", 6), vec!["abc一", "二"]);
        assert_eq!(rows_of("abcd一二", 5), vec!["abcd", "一二"]);
    }

    #[test]
    fn wrap_rows_keeps_punctuation_off_the_line_start() {
        assert_eq!(rows_of("一二三，四五", 6), vec!["一二", "三，四", "五"]);
        let rows = rows_of("你好嗎。你好嗎。", 6);
        assert!(rows.iter().all(|r| !r.starts_with('。')), "{rows:?}");
    }

    #[test]
    fn wrap_rows_cover_the_whole_line_without_gaps() {
        let line = "The quick 棕色 fox\tjumps 過 lazy dog。 e\u{301}e\u{301}e\u{301} 😀😀😀";
        let n = line.chars().count();
        for cols in 2..30 {
            let rows = wrap_rows(line, cols);
            assert_eq!(rows.first().unwrap().start, 0);
            assert_eq!(rows.last().unwrap().end, n);
            for w in rows.windows(2) {
                assert_eq!(w[0].end, w[1].start, "cols={cols}");
            }
            assert!(rows.iter().all(|r| r.end > r.start), "cols={cols}");
        }
    }

    #[test]
    fn wrap_rows_never_exceed_the_width_unless_a_single_char_must() {
        let line = "abc def ghi jkl 一二三四五 mno\tpqr";
        for cols in 4..20 {
            let chars: Vec<char> = line.chars().collect();
            for r in wrap_rows(line, cols) {
                let text: String = chars[r.clone()].iter().collect();
                let width = display_col(&text, r.len());
                assert!(width <= cols + 1, "cols={cols} row={text:?} width={width}");
            }
        }
    }
}