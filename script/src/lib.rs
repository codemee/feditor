//! feditor 巨集層：定義與語言無關的巨集 API。
//!
//! 具體的腳本語言（Python、JavaScript、Lua 等）之後各自實作 [`ScriptEngine`]，
//! 核心與 UI 不需要改動。

use feditor_core::Buffer;
use feditor_core::tf;
use std::ops::Range;

/// 尋找的選項（給 [`EditorApi::find`]）。
#[derive(Clone, Copy, Debug, Default)]
pub struct FindOptions {
    pub regex: bool,
    pub case_sensitive: bool,
    pub whole_word: bool,
    /// 往回找：找 `from` 之前最後一筆。
    pub backward: bool,
}

/// 巨集可以呼叫的編輯器能力。語言繫結（binding）只依賴這個 trait。
///
/// 位置一律是字元（Unicode 碼點）索引。第一組是純文字的讀寫，所有宿主都要實作；
/// 之後的方法（選取、指令、對話框…）有預設實作，沒有畫面的宿主（例如測試）可以不理它們。
pub trait EditorApi {
    fn text(&self) -> String;
    fn len_chars(&self) -> usize;
    fn line_count(&self) -> usize;
    fn line(&self, idx: usize) -> Option<String>;
    fn insert(&mut self, at: usize, text: &str);
    fn remove(&mut self, range: Range<usize>);
    fn replace(&mut self, range: Range<usize>, text: &str);

    /// `[start, end)` 的文字（超出範圍的部分會被忽略）。
    fn slice(&self, range: Range<usize>) -> String {
        self.text()
            .chars()
            .skip(range.start)
            .take(range.end.saturating_sub(range.start))
            .collect()
    }

    /// 第 `line` 行第一個字元的位置；超出範圍時為文字末端。
    fn line_start(&self, line: usize) -> usize {
        let text = self.text();
        let mut pos = 0;
        for (i, l) in text.split('\n').enumerate() {
            if i == line {
                return pos;
            }
            pos += l.chars().count() + 1;
        }
        self.len_chars()
    }

    /// 位置 `pos` 在第幾行（0 起算）。
    fn line_of(&self, pos: usize) -> usize {
        self.text().chars().take(pos).filter(|c| *c == '\n').count()
    }

    /// 目前的選取（錨點, 游標）。沒有選取時兩者相同。
    fn selection(&self) -> (usize, usize) {
        (0, 0)
    }

    fn set_selection(&mut self, _anchor: usize, _head: usize) {}

    /// 目前文件的完整路徑；還沒存過檔時為 `None`。
    fn path(&self) -> Option<String> {
        None
    }

    /// 執行編輯器指令（與按鍵相同，例如 `"selectAll"`、`"moveLineDown"`），重複 `count` 次。
    /// 不認得的指令名稱回傳錯誤。
    fn run_command(&mut self, name: &str, _count: usize) -> Result<(), String> {
        Err(tf!("不支援指令：{name}", name = name))
    }

    /// 像使用者打字一樣輸入文字：取代選取、游標移到文字之後。
    fn type_text(&mut self, text: &str) {
        let end = self.len_chars();
        self.insert(end, text);
    }

    /// 像使用者貼上一樣輸入文字。
    fn paste_text(&mut self, text: &str) {
        self.type_text(text);
    }

    /// 用 `prefix`、`suffix` 包住目前的選取（沒有選取就插入兩者，游標停在中間）；選取留在原本的文字上。
    fn wrap_text(&mut self, prefix: &str, suffix: &str) {
        let (a, b) = self.selection();
        let (start, end) = (a.min(b), a.max(b));
        self.insert(end, suffix);
        self.insert(start, prefix);
        let shift = prefix.chars().count();
        self.set_selection(a + shift, b + shift);
    }

    fn alert(&mut self, _message: &str) {}

    fn confirm(&mut self, _message: &str) -> bool {
        false
    }

    /// 從位置 `from` 開始找（`opts.backward` 時找 `from` 之前），回傳找到的字元範圍。
    fn find(
        &mut self,
        query: &str,
        from: usize,
        opts: FindOptions,
    ) -> Result<Option<Range<usize>>, String> {
        find_in_text(&self.text(), query, from, opts)
    }
}

/// 在整份文字裡尋找，位置都是字元索引。
pub fn find_in_text(
    text: &str,
    query: &str,
    from: usize,
    opts: FindOptions,
) -> Result<Option<Range<usize>>, String> {
    use feditor_core::search::{SearchOptions, Searcher};
    let searcher = Searcher::new(
        query,
        SearchOptions {
            regex: opts.regex,
            case_sensitive: opts.case_sensitive,
            whole_word: opts.whole_word,
        },
    )
    .map_err(|e| e.to_string())?;
    let byte = |chars: usize| text.char_indices().nth(chars).map_or(text.len(), |(b, _)| b);
    let found = if opts.backward {
        searcher.find_prev(text, byte(from))
    } else {
        searcher.find_next(text, byte(from))
    };
    Ok(found.map(|r| {
        let start = text[..r.start].chars().count();
        start..start + text[r.start..r.end].chars().count()
    }))
}

impl EditorApi for Buffer {
    fn text(&self) -> String {
        Buffer::text(self)
    }
    fn len_chars(&self) -> usize {
        Buffer::len_chars(self)
    }
    fn line_count(&self) -> usize {
        Buffer::line_count(self)
    }
    fn line(&self, idx: usize) -> Option<String> {
        Buffer::line(self, idx)
    }
    fn insert(&mut self, at: usize, text: &str) {
        Buffer::insert(self, at, text)
    }
    fn remove(&mut self, range: Range<usize>) {
        Buffer::remove(self, range)
    }
    fn replace(&mut self, range: Range<usize>, text: &str) {
        Buffer::replace(self, range, text)
    }
    fn slice(&self, range: Range<usize>) -> String {
        let n = Buffer::len_chars(self);
        Buffer::slice(self, range.start.min(n)..range.end.min(n).max(range.start.min(n)))
    }
    fn line_start(&self, line: usize) -> usize {
        if line >= Buffer::line_count(self) {
            Buffer::len_chars(self)
        } else {
            Buffer::line_start(self, line)
        }
    }
    fn line_of(&self, pos: usize) -> usize {
        Buffer::line_of_char(self, pos.min(Buffer::len_chars(self)))
    }
}
/// 巨集執行失敗的原因。
#[derive(Debug)]
pub struct ScriptError(pub String);

impl std::fmt::Display for ScriptError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ScriptError {}

/// 腳本引擎：拿到原始碼與編輯器，執行巨集。
pub trait ScriptEngine {
    /// 引擎名稱，例如 `"python"`、`"javascript"`。
    fn name(&self) -> &'static str;

    fn run(&mut self, source: &str, editor: &mut dyn EditorApi) -> Result<(), ScriptError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 測試用的假引擎：只支援 `append:<文字>` 這種一行指令。
    struct Fake;

    impl ScriptEngine for Fake {
        fn name(&self) -> &'static str {
            "fake"
        }

        fn run(&mut self, source: &str, editor: &mut dyn EditorApi) -> Result<(), ScriptError> {
            let text = source
                .strip_prefix("append:")
                .ok_or_else(|| ScriptError("未知指令".into()))?;
            let end = editor.len_chars();
            editor.insert(end, text);
            Ok(())
        }
    }

    #[test]
    fn engine_drives_buffer_through_api() {
        let mut buf = Buffer::from_text("你好");
        Fake.run("append:世界", &mut buf).unwrap();
        assert_eq!(buf.text(), "你好世界");
        assert!(Fake.run("oops", &mut buf).is_err());
    }

    #[test]
    fn buffer_helpers_clamp_and_locate() {
        let b = Buffer::from_text("ab\ncd\n");
        assert_eq!(EditorApi::slice(&b, 1..4), "b\nc");
        assert_eq!(EditorApi::slice(&b, 4..99), "d\n");
        assert_eq!(EditorApi::slice(&b, 50..60), "");
        assert_eq!(EditorApi::line_start(&b, 1), 3);
        assert_eq!(EditorApi::line_start(&b, 99), 6);
        assert_eq!(EditorApi::line_of(&b, 4), 1);
        assert_eq!(EditorApi::line_of(&b, 999), 2);
    }

    /// 只實作必要方法的宿主：預設實作要能正常工作。
    struct Minimal(String);

    impl EditorApi for Minimal {
        fn text(&self) -> String {
            self.0.clone()
        }
        fn len_chars(&self) -> usize {
            self.0.chars().count()
        }
        fn line_count(&self) -> usize {
            self.0.split('\n').count()
        }
        fn line(&self, _: usize) -> Option<String> {
            None
        }
        fn insert(&mut self, at: usize, text: &str) {
            let b = self.0.char_indices().nth(at).map_or(self.0.len(), |(b, _)| b);
            self.0.insert_str(b, text);
        }
        fn remove(&mut self, _: Range<usize>) {}
        fn replace(&mut self, _: Range<usize>, _: &str) {}
    }

    #[test]
    fn default_methods_work_for_a_minimal_host() {
        let mut m = Minimal("ab\n中文\n".to_owned());
        assert_eq!(m.slice(3..5), "中文");
        assert_eq!(m.line_start(1), 3);
        assert_eq!(m.line_start(9), 6);
        assert_eq!(m.line_of(4), 1);
        assert_eq!(m.selection(), (0, 0));
        assert!(m.run_command("nope", 1).is_err());
        assert!(!m.confirm("?"));
        m.type_text("!");
        assert!(m.text().ends_with('!'));
        // wrap_text 的預設實作：沒有選取（預設是 (0, 0)）就在游標處插入一對
        let mut m = Minimal("ab".to_owned());
        m.wrap_text("(", ")");
        assert_eq!(m.text(), "()ab");
    }

    #[test]
    fn find_uses_character_positions_and_options() {
        let mut b = Buffer::from_text("你好 foo 世界 Foo");
        let plain = FindOptions::default();
        assert_eq!(b.find("foo", 0, plain).unwrap(), Some(3..6));
        assert_eq!(b.find("foo", 4, plain).unwrap(), Some(10..13), "預設不分大小寫");
        let cs = FindOptions { case_sensitive: true, ..plain };
        assert_eq!(b.find("Foo", 0, cs).unwrap(), Some(10..13));
        let back = FindOptions { backward: true, ..plain };
        assert_eq!(b.find("foo", 10, back).unwrap(), Some(3..6));
        let re = FindOptions { regex: true, ..plain };
        assert_eq!(b.find("世.", 0, re).unwrap(), Some(7..9));
        assert!(b.find("(", 0, re).is_err(), "規則有誤要回報");
        assert_eq!(b.find("nope", 0, plain).unwrap(), None);
    }
}