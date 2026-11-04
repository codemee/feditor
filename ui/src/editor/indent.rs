//! 依語言的縮排規則：Enter 自動縮排、輸入閉括號減一層、排列格式（reformat）。
//!
//! 這裡都是不碰緩衝區的純函式，輸入是行的文字與語言，容易單元測試；套用到緩衝區的部分
//! 在 `model.rs`（Enter、輸入）與 `lineops.rs`（排列格式）。
//!
//! 縮排用的字元一律跟隨全域設定：「改用空白」時一層是 Tab 欄寬個空白，否則是一個 Tab。
//! 括號只計「一般程式碼」的：字串與註解裡的 `{ ( [` 不算（用語法上色的分詞結果判斷）。

use crate::syntax::{IndentStyle, Kind, Language, State, highlight_line};
use feditor_core::motion;

/// 縮排一層的字串（依「改用空白」與 Tab 欄寬）。
pub fn indent_unit() -> String {
    if motion::tab_uses_spaces() { " ".repeat(motion::tab_width()) } else { "\t".to_owned() }
}

/// 行首的空白與 Tab。
pub fn leading_ws(line: &str) -> String {
    line.chars().take_while(|c| *c == ' ' || *c == '\t').collect()
}

/// 把行首的縮排換成目前設定的字元，欄位寬度不變（Tab 依欄寬換算）。
pub fn normalize_lead(line: &str) -> String {
    let lead = line.chars().take_while(|c| *c == ' ' || *c == '\t').count();
    let cols = motion::display_col(line, lead);
    let rest: String = line.chars().skip(lead).collect();
    let width = motion::tab_width();
    if motion::tab_uses_spaces() {
        format!("{}{rest}", " ".repeat(cols))
    } else {
        format!("{}{}{rest}", "\t".repeat(cols / width), " ".repeat(cols % width))
    }
}

/// 這個語言有沒有縮排規則、自動縮排有沒有開。
fn active(lang: Option<&Language>) -> Option<&Language> {
    lang.filter(|l| l.indent != IndentStyle::None && motion::auto_indent())
}

/// 一行的字元、每個字元的種類、行尾的跨行狀態。
fn analyze(lang: &Language, line: &str, state: State) -> (Vec<char>, Vec<Kind>, State) {
    let chars: Vec<char> = line.chars().collect();
    let (spans, end) = highlight_line(lang, line, state);
    let mut kinds = vec![Kind::Plain; chars.len()];
    for (range, kind) in spans {
        for k in kinds.iter_mut().take(range.end.min(chars.len())).skip(range.start) {
            *k = kind;
        }
    }
    (chars, kinds, end)
}

fn is_closer(c: char) -> bool {
    matches!(c, '}' | ')' | ']')
}

/// Enter 要插入的文字，以及插入後游標在這段文字的第幾個字元。
#[derive(Debug, PartialEq, Eq)]
pub struct EnterPlan {
    pub text: String,
    pub caret: usize,
}

/// 在 `before`（這一行游標之前的文字）與 `after`（游標之後的文字）之間按 Enter。
/// `state` 是這一行開頭的跨行狀態：在區塊註解或多行字串裡就只沿用縮排。
///
/// - 前一段（去掉行尾的空白與註解）以 `{ ( [`（Python 還有 `:`）結尾 → 下一行多縮排一層；
///   游標後面緊接著對應的閉括號時，展開成三行：開、縮排的空行（游標在這）、閉。
/// - 其他情況沿用這一行的縮排。
pub fn enter_plan(lang: Option<&Language>, state: State, before: &str, after: &str) -> EnterPlan {
    let base = leading_ws(before);
    let plain = |base: &str| {
        let text = format!("\n{base}");
        let caret = text.chars().count();
        EnterPlan { text, caret }
    };
    let Some(lang) = active(lang).filter(|_| state == State::Normal) else {
        return plain(&base);
    };
    let (chars, kinds, _) = analyze(lang, before, state);
    let last = (0..chars.len()).rev().find(|&i| !chars[i].is_whitespace() && kinds[i] != Kind::Comment);
    let Some(i) = last.filter(|&i| kinds[i] == Kind::Plain) else {
        return plain(&base);
    };
    let c = chars[i];
    let opens = matches!(c, '{' | '(' | '[') || (lang.indent == IndentStyle::Python && c == ':');
    if !opens {
        return plain(&base);
    }
    let inner = format!("\n{base}{}", indent_unit());
    let closer = match c {
        '{' => Some('}'),
        '(' => Some(')'),
        '[' => Some(']'),
        _ => None,
    };
    if closer.is_some() && after.chars().find(|c| !c.is_whitespace()) == closer {
        let caret = inner.chars().count();
        return EnterPlan { text: format!("{inner}\n{base}"), caret };
    }
    let caret = inner.chars().count();
    EnterPlan { text: inner, caret }
}

/// 在一行只有空白的開頭輸入閉括號 `typed` 時，要先拿掉游標前幾個空白字元（減一層縮排）。
/// `before` 是這一行游標之前的文字。不適用時回傳 0。
pub fn closer_dedent(lang: Option<&Language>, state: State, before: &str, typed: char) -> usize {
    if !is_closer(typed) || before.is_empty() || !before.chars().all(|c| c == ' ' || c == '\t') {
        return 0;
    }
    if active(lang).is_none() || state != State::Normal {
        return 0;
    }
    if before.ends_with('\t') {
        1
    } else {
        before.chars().rev().take_while(|c| *c == ' ').count().min(motion::tab_width())
    }
}

/// 排列格式時一行的處理方式。
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum LineIndent {
    /// 這一行開頭在多行字串或區塊註解裡：原樣不動。
    Keep,
    /// 空白行：清成空字串。
    Blank,
    /// 縮排到這一層（括號深度，只有用括號決定縮排的語言才有）。
    Depth(usize),
    /// 沒辦法算出層數（Python、沒有規則的語言）：保留原本的縮排欄位，只換成目前設定的縮排字元。
    Same,
}

/// 逐行算出每一行該怎麼處理。`lines` 是整份文件（深度要從頭累計）。
pub fn scan(lang: Option<&Language>, lines: &[String]) -> Vec<LineIndent> {
    let Some(lang) = lang else {
        return lines
            .iter()
            .map(|l| if l.trim().is_empty() { LineIndent::Blank } else { LineIndent::Same })
            .collect();
    };
    let brace = lang.indent == IndentStyle::Brace;
    let mut out = Vec::with_capacity(lines.len());
    let (mut depth, mut state) = (0usize, State::Normal);
    for line in lines {
        let (chars, kinds, end) = analyze(lang, line, state);
        let plain_bracket = |j: usize| kinds[j] == Kind::Plain;
        let mut leading_closers = 0;
        if let Some(first) = chars.iter().position(|c| !c.is_whitespace()) {
            leading_closers =
                chars[first..].iter().enumerate().take_while(|(k, c)| is_closer(**c) && plain_bracket(first + k)).count();
        }
        let item = if state != State::Normal {
            LineIndent::Keep
        } else if line.trim().is_empty() {
            LineIndent::Blank
        } else if brace {
            LineIndent::Depth(depth.saturating_sub(leading_closers))
        } else {
            LineIndent::Same
        };
        out.push(item);
        // 括號只在 Python 與用括號決定縮排的語言裡計算；Python 不重算層數，所以只有 brace 要累計
        if brace {
            for (j, c) in chars.iter().enumerate() {
                if !plain_bracket(j) {
                    continue;
                }
                match c {
                    '{' | '(' | '[' => depth += 1,
                    '}' | ')' | ']' => depth = depth.saturating_sub(1),
                    _ => {}
                }
            }
        }
        state = end;
    }
    out
}

/// 排列後的一行。`Keep` 原樣；其他都去掉行尾空白。
pub fn format_line(line: &str, how: LineIndent, unit: &str) -> String {
    match how {
        LineIndent::Keep => line.to_owned(),
        LineIndent::Blank => String::new(),
        LineIndent::Depth(n) => format!("{}{}", unit.repeat(n), line.trim()),
        LineIndent::Same => normalize_lead(line.trim_end()),
    }
}

/// JSON 重新排版（鍵的順序保留）：縮排一層用 `unit`。不是有效的 JSON 回傳 `None`。
/// 結果沒有尾端換行，由呼叫端決定。
pub fn json_pretty(text: &str, unit: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    let two = serde_json::to_string_pretty(&value).ok()?;
    // serde_json 固定用兩個空白縮排；JSON 字串裡的換行都已跳脫，所以行首的空白一定是縮排
    let lines: Vec<String> = two
        .lines()
        .map(|l| {
            let spaces = l.chars().take_while(|c| *c == ' ').count();
            format!("{}{}", unit.repeat(spaces / 2), &l[spaces..])
        })
        .collect();
    Some(lines.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syntax::language_for_name;

    fn rust() -> Option<&'static Language> {
        language_for_name("x.rs")
    }
    fn python() -> Option<&'static Language> {
        language_for_name("x.py")
    }

    /// 測試用：把 Tab 設定固定下來，結束還原。
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

    fn plan(lang: Option<&Language>, before: &str, after: &str) -> (String, usize) {
        let p = enter_plan(lang, State::Normal, before, after);
        (p.text, p.caret)
    }

    #[test]
    fn enter_after_an_opening_bracket_indents_one_more_level() {
        let _t = Tabs::set(4, true);
        assert_eq!(plan(rust(), "fn main() {", ""), ("\n    ".into(), 5));
        assert_eq!(plan(rust(), "    if x {", ""), ("\n        ".into(), 9));
        assert_eq!(plan(rust(), "    foo(", ""), ("\n        ".into(), 9));
        assert_eq!(plan(rust(), "let v = [   ", ""), ("\n    ".into(), 5), "行尾空白不影響");
        assert_eq!(plan(rust(), "x = 1; // {", ""), ("\n".into(), 1), "註解裡的括號不算");
        assert_eq!(plan(rust(), "let s = \"{\";", ""), ("\n".into(), 1), "字串裡的括號不算");
        assert_eq!(plan(rust(), "foo(); // 說明 {", ""), ("\n".into(), 1));
        assert_eq!(plan(rust(), "  foo { // 開始", ""), ("\n      ".into(), 7), "行尾註解略過");
    }

    #[test]
    fn enter_uses_tabs_unless_spaces_are_chosen() {
        let _t = Tabs::set(4, false);
        assert_eq!(plan(rust(), "\tif x {", ""), ("\n\t\t".into(), 3));
        let _t2 = Tabs::set(2, true);
        assert_eq!(plan(rust(), "if x {", ""), ("\n  ".into(), 3));
    }

    #[test]
    fn enter_between_matching_brackets_opens_three_lines() {
        let _t = Tabs::set(4, true);
        assert_eq!(plan(rust(), "    fn f() {", "}"), ("\n        \n    ".into(), 9));
        assert_eq!(plan(rust(), "foo(", ")"), ("\n    \n".into(), 5));
        assert_eq!(plan(rust(), "foo(", "x)"), ("\n    ".into(), 5), "後面不是閉括號就不展開");
        assert_eq!(plan(python(), "def f():", ""), ("\n    ".into(), 5));
        assert_eq!(plan(python(), "xs = [", "]"), ("\n    \n".into(), 5));
    }

    #[test]
    fn python_colon_opens_a_block_but_other_languages_only_copy_the_indent() {
        let _t = Tabs::set(4, true);
        assert_eq!(plan(python(), "  if x:  # 註解", ""), ("\n      ".into(), 7));
        assert_eq!(plan(python(), "x = 'a:'", ""), ("\n".into(), 1), "字串裡的冒號不算");
        assert_eq!(plan(python(), "# note:", ""), ("\n".into(), 1), "註解裡的冒號不算");
        assert_eq!(plan(rust(), "match x {", ""), ("\n    ".into(), 5));
        assert_eq!(plan(rust(), "let x: u8", ""), ("\n".into(), 1), "Rust 的冒號不開區塊");
        assert_eq!(plan(None, "foo {", ""), ("\n".into(), 1), "沒有語言只沿用縮排");
        assert_eq!(plan(None, "  foo {", ""), ("\n  ".into(), 3));
    }

    #[test]
    fn switching_auto_indent_off_goes_back_to_copying() {
        let _t = Tabs::set(4, true);
        motion::set_auto_indent(false);
        assert_eq!(plan(rust(), "  if x {", "}"), ("\n  ".into(), 3));
    }

    #[test]
    fn inside_a_block_comment_only_the_indent_is_copied() {
        let _t = Tabs::set(4, true);
        let p = enter_plan(rust(), State::Block, "  * foo {", "");
        assert_eq!((p.text.as_str(), p.caret), ("\n  ", 3));
    }

    #[test]
    fn typing_a_closer_on_a_blank_line_removes_one_level() {
        let _t = Tabs::set(4, true);
        let d = |before: &str, c| closer_dedent(rust(), State::Normal, before, c);
        assert_eq!(d("        ", '}'), 4);
        assert_eq!(d("  ", ')'), 2, "不足一層就拿掉現有的");
        assert_eq!(d("    ", 'x'), 0);
        assert_eq!(d("    x", '}'), 0, "前面有文字不動");
        assert_eq!(d("", '}'), 0);
        assert_eq!(closer_dedent(None, State::Normal, "    ", '}'), 0);
        assert_eq!(closer_dedent(rust(), State::Block, "    ", '}'), 0);
        let _t2 = Tabs::set(4, false);
        assert_eq!(d("\t\t", '}'), 1);
    }

    fn lines(text: &str) -> Vec<String> {
        text.split('\n').map(str::to_owned).collect()
    }

    fn reformat(lang: Option<&Language>, text: &str) -> String {
        let ls = lines(text);
        let unit = indent_unit();
        scan(lang, &ls).into_iter().zip(&ls).map(|(how, l)| format_line(l, how, &unit)).collect::<Vec<_>>().join("\n")
    }

    #[test]
    fn reformat_rebuilds_indentation_from_bracket_depth() {
        let _t = Tabs::set(4, true);
        let messy = "fn main() {\nlet x = foo(1,\n2);\n        if x {\n   bar();\n  }\n\n}   \n";
        assert_eq!(
            reformat(rust(), messy),
            "fn main() {\n    let x = foo(1,\n        2);\n    if x {\n        bar();\n    }\n\n}\n"
        );
    }

    #[test]
    fn reformat_ignores_brackets_in_strings_and_comments_and_keeps_multiline_content() {
        let _t = Tabs::set(4, true);
        let text = "fn f() {\nlet s = \"{\";\n// }\n/* {\n   keep   \n*/\nx();\n}";
        assert_eq!(
            reformat(rust(), text),
            "fn f() {\n    let s = \"{\";\n    // }\n    /* {\n   keep   \n*/\n    x();\n}"
        );
    }

    #[test]
    fn reformat_without_a_depth_rule_only_trims_and_normalizes_the_lead() {
        let _t = Tabs::set(4, true);
        assert_eq!(reformat(python(), "if x:\n\tfoo()  \n  \nbar()"), "if x:\n    foo()\n\nbar()");
        assert_eq!(reformat(None, "a  \n\tb"), "a\n    b");
        let _t2 = Tabs::set(4, false);
        assert_eq!(reformat(python(), "if x:\n        foo()"), "if x:\n\t\tfoo()");
        let triple = "s = \"\"\"\n   keep  \n\"\"\"\nx = 1  ";
        assert_eq!(reformat(python(), triple), "s = \"\"\"\n   keep  \n\"\"\"\nx = 1");
    }

    #[test]
    fn json_is_pretty_printed_with_the_current_indent_and_key_order() {
        assert_eq!(
            json_pretty("{\"b\":[1,2],\"a\":{\"x\":\"  y\"}}", "    ").as_deref(),
            Some("{\n    \"b\": [\n        1,\n        2\n    ],\n    \"a\": {\n        \"x\": \"  y\"\n    }\n}")
        );
        assert_eq!(json_pretty("[1]", "\t").as_deref(), Some("[\n\t1\n]"));
        assert_eq!(json_pretty("{ not json", "  "), None);
        assert_eq!(json_pretty("// c\n{}", "  "), None);
    }
}
