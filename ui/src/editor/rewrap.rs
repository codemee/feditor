//! 排列格式時的括號內斷行：把多餘的斷行接回去，太長（超過安全界線）的地方在括號內斷開。
//!
//! 只處理圓括號與方括號（函式參數、陣列），不動 `{ }`（通常是程式區塊）；字串與註解不會被切開。
//! 斷行後下一行怎麼排由語言的 [`WrapStyle`] 決定：
//!
//! - `Align`：對齊開括號後的第一個參數，參數盡量排滿一行。
//! - `Hang`：開括號後換行，每個參數一行並縮排一層，閉括號獨立一行。
//!
//! 都是不碰緩衝區的純函式；套用到緩衝區在 `lineops.rs` 的 `reformat`。

use super::indent::{LineIndent, analyze, format_line, leading_ws, normalize_lead};
use crate::syntax::{Kind, Language, State, WrapStyle, highlight_line};
use feditor_core::motion;

/// 斷行的參數。
pub struct Opts<'a> {
    /// 一行最多幾欄（安全界線）。
    pub margin: usize,
    pub style: WrapStyle,
    /// `Hang` 斷行時最後一個參數後面也加逗號。
    pub trailing_comma: bool,
    /// 縮排一層的字串。
    pub unit: &'a str,
}

fn width(s: &str) -> usize {
    motion::display_col(s, s.chars().count())
}

/// 掃一行：累計圓／方括號深度；遇到註解、或括號裡有 `{ }` 就標成「不能接合」。
fn scan_line(chars: &[char], kinds: &[Kind], depth: &mut usize, bad: &mut bool) {
    for (j, &c) in chars.iter().enumerate() {
        match kinds[j] {
            Kind::Comment => *bad = true,
            Kind::Plain => match c {
                '(' | '[' => *depth += 1,
                ')' | ']' => *depth = depth.saturating_sub(1),
                '{' | '}' if *depth > 0 => *bad = true,
                _ => {}
            },
            _ => {}
        }
    }
}

/// 把 `a..=b` 這幾行分成處理單位（含頭尾的行號）：一般的行自成一個單位；開了括號、跨好幾行才關上的
/// 一組括號可以接回成一行（單位的頭尾是括號開始與結束的行），條件是整組都在 `a..=b` 裡、
/// 沒有註解、沒有空行、沒有多行字串，括號內也沒有 `{ }`。
pub fn plan(lang: &Language, lines: &[String], a: usize, b: usize) -> Vec<(usize, usize)> {
    let mut state = State::Normal;
    for l in &lines[..a] {
        state = highlight_line(lang, l, state).1;
    }
    let mut out = Vec::new();
    let mut i = a;
    while i <= b {
        let start_state = state;
        let (chars, kinds, end) = analyze(lang, &lines[i], state);
        state = end;
        let (mut depth, mut bad) = (0usize, false);
        if start_state == State::Normal {
            scan_line(&chars, &kinds, &mut depth, &mut bad);
        }
        if start_state != State::Normal || depth == 0 {
            out.push((i, i));
            i += 1;
            continue;
        }
        let first = i;
        let mut j = i;
        let mut closed = false;
        while j + 1 < lines.len() {
            j += 1;
            let st = state;
            let (c2, k2, e2) = analyze(lang, &lines[j], st);
            state = e2;
            if st != State::Normal || lines[j].trim().is_empty() {
                bad = true;
            }
            scan_line(&c2, &k2, &mut depth, &mut bad);
            if depth == 0 {
                closed = true;
                break;
            }
        }
        if state != State::Normal {
            bad = true;
        }
        if closed && !bad && j <= b {
            out.push((first, j));
        } else {
            for k in first..=j.min(b) {
                out.push((k, k));
            }
        }
        i = j + 1;
    }
    out
}

/// 把多行括號接成一行（去掉每行的縮排）。括號後、閉括號前不加空白；多餘的結尾逗號拿掉
///（只有一個元素的 `(a,)` 與 Python 保留，那個逗號有意義）。
pub fn join_group(lang: &Language, lines: &[String]) -> String {
    let mut s = lines[0].trim().to_owned();
    for l in &lines[1..] {
        let t = l.trim();
        if t.is_empty() {
            continue;
        }
        if !(s.ends_with(['(', '[']) || t.starts_with([')', ']'])) {
            s.push(' ');
        }
        s.push_str(t);
    }
    strip_trailing_commas(lang, &s)
}

/// 拿掉「閉括號前、而且那組括號裡還有別的逗號」的結尾逗號。
fn strip_trailing_commas(lang: &Language, s: &str) -> String {
    let (chars, kinds, _) = analyze(lang, s, State::Normal);
    let mut commas: Vec<usize> = Vec::new();
    let mut remove: Vec<usize> = Vec::new();
    let mut last_sig: Option<usize> = None;
    for (j, &c) in chars.iter().enumerate() {
        if kinds[j] == Kind::Plain {
            match c {
                '(' | '[' | '{' => commas.push(0),
                ',' => {
                    if let Some(n) = commas.last_mut() {
                        *n += 1;
                    }
                }
                ')' | ']' | '}' => {
                    if let Some(n) = commas.pop()
                        && n >= 2
                        && let Some(k) = last_sig
                        && chars[k] == ','
                        && kinds[k] == Kind::Plain
                    {
                        remove.push(k);
                    }
                }
                _ => {}
            }
        }
        if !c.is_whitespace() {
            last_sig = Some(j);
        }
    }
    chars.iter().enumerate().filter(|(j, _)| !remove.contains(j)).map(|(_, c)| *c).collect()
}

/// 把一行程式（`text`，沒有縮排）排成幾行：裝得下就是一行 `prefix + text`；
/// 超過安全界線就在越過界線的那組括號裡斷行。`prefix` 是第一行的縮排。
/// 斷不開的（沒有括號、括號只有一個參數且是 `Align`…）維持原樣。
pub fn layout(lang: &Language, text: &str, prefix: &str, o: &Opts) -> Vec<String> {
    let one = || vec![format!("{prefix}{text}")];
    if width(prefix) + width(text) <= o.margin {
        return one();
    }
    let (chars, kinds, _) = analyze(lang, text, State::Normal);
    if kinds.contains(&Kind::Comment) {
        return one(); // 行尾註解：不確定能不能動，維持原樣
    }
    // 第一個越過安全界線的字元
    let Some(k) = (0..chars.len()).find(|&k| {
        let upto: String = chars[..=k].iter().collect();
        width(prefix) + width(&upto) > o.margin
    }) else {
        return one();
    };
    // 最外層的圓／方括號群組
    let mut groups: Vec<(usize, usize)> = Vec::new();
    let (mut depth, mut open_at) = (0usize, 0usize);
    for (j, &c) in chars.iter().enumerate() {
        if kinds[j] != Kind::Plain {
            continue;
        }
        match c {
            '(' | '[' => {
                if depth == 0 {
                    open_at = j;
                }
                depth += 1;
            }
            ')' | ']' if depth > 0 => {
                depth -= 1;
                if depth == 0 {
                    groups.push((open_at, j));
                }
            }
            _ => {}
        }
    }
    let Some(&(open, close)) = groups.iter().find(|&&(op, cl)| op < k && cl >= k) else {
        return one();
    };
    // 參數：括號內最外層的逗號分隔
    let mut args: Vec<String> = Vec::new();
    let (mut d, mut cur) = (0i32, String::new());
    for j in open + 1..close {
        let ch = chars[j];
        if kinds[j] == Kind::Plain {
            match ch {
                '(' | '[' | '{' => d += 1,
                ')' | ']' | '}' => d -= 1,
                ',' if d == 0 => {
                    args.push(cur.trim().to_owned());
                    cur.clear();
                    continue;
                }
                _ => {}
            }
        }
        cur.push(ch);
    }
    args.push(cur.trim().to_owned());
    args.retain(|a| !a.is_empty());
    if args.is_empty() {
        return one();
    }
    let head: String = chars[..=open].iter().collect();
    let tail: String = chars[close..].iter().collect();
    let head_w = width(&format!("{prefix}{head}"));
    let align = o.style == WrapStyle::Align && args.len() >= 2 && head_w + 12 <= o.margin;
    if o.style == WrapStyle::Align && args.len() < 2 {
        return one(); // 對齊風格下，只有一個參數沒有地方可以斷
    }
    if align { layout_align(lang, &args, &head, &tail, prefix, head_w, o) } else { layout_hang(lang, &args, &head, &tail, prefix, o) }
}

/// 每個參數一行，縮排一層，閉括號獨立一行。
fn layout_hang(lang: &Language, args: &[String], head: &str, tail: &str, prefix: &str, o: &Opts) -> Vec<String> {
    let mut lines = vec![format!("{prefix}{head}")];
    let inner = format!("{prefix}{}", o.unit);
    for (n, arg) in args.iter().enumerate() {
        // 只有一個參數時不加逗號：`(a + b)` 加了逗號會變成元組
        let comma = n + 1 < args.len() || (o.trailing_comma && args.len() > 1);
        lines.extend(layout(lang, &format!("{arg}{}", if comma { "," } else { "" }), &inner, o));
    }
    lines.extend(layout(lang, tail, prefix, o));
    lines
}

/// 對齊第一個參數；參數盡量排滿一行，閉括號接在最後一個參數後面。
fn layout_align(lang: &Language, args: &[String], head: &str, tail: &str, prefix: &str, head_w: usize, o: &Opts) -> Vec<String> {
    let cont = format!("{prefix}{}", " ".repeat(head_w - width(prefix)));
    let mut lines = Vec::new();
    let mut cur = format!("{prefix}{head}");
    let mut fresh = true; // 這一行還沒放任何參數：不用空白隔開
    for (n, arg) in args.iter().enumerate() {
        let last = n + 1 == args.len();
        let piece = format!("{arg}{}", if last { "" } else { "," });
        let tail_w = if last { width(tail) } else { 0 };
        let sep = if fresh { "" } else { " " };
        if fresh || width(&cur) + sep.len() + width(&piece) + tail_w <= o.margin {
            cur.push_str(sep);
            cur.push_str(&piece);
            fresh = false;
        } else {
            lines.push(std::mem::take(&mut cur));
            if width(&cont) + width(&piece) + tail_w > o.margin {
                // 這個參數自己一行也放不下：它自己再斷
                let t = if last { format!("{piece}{tail}") } else { piece.clone() };
                lines.extend(layout(lang, &t, &cont, o));
                if last {
                    return lines;
                }
                cur = cont.clone();
                fresh = true;
            } else {
                cur = format!("{cont}{piece}");
                fresh = false;
            }
        }
    }
    cur.push_str(tail);
    lines.push(cur);
    lines
}

/// 排列 `a..=b` 這幾行（`all` 是整份文件，`how` 是 [`indent::scan`](super::indent::scan) 的結果）：
/// 縮排照 `how`；多行括號能接就接成一行，超過安全界線的再依風格斷開。
pub fn reformat_lines(lang: &Language, all: &[String], how: &[LineIndent], a: usize, b: usize, o: &Opts) -> Vec<String> {
    let mut out = Vec::new();
    for (first, last) in plan(lang, all, a, b) {
        let prefix = match how[first] {
            LineIndent::Depth(n) => o.unit.repeat(n),
            LineIndent::Same => normalize_lead(&leading_ws(&all[first])),
            LineIndent::Keep | LineIndent::Blank => {
                for k in first..=last {
                    out.push(format_line(&all[k], how[k], o.unit));
                }
                continue;
            }
        };
        let text = if first == last { all[first].trim().to_owned() } else { join_group(lang, &all[first..=last]) };
        out.extend(layout(lang, &text, &prefix, o));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::indent::scan;
    use crate::syntax::language_for_name;

    /// 測試用：Tab 設定固定成「4 欄、用空白」，結束還原。
    struct Guard(usize, bool);
    impl Drop for Guard {
        fn drop(&mut self) {
            motion::set_tab_width(self.0);
            motion::set_tab_uses_spaces(self.1);
        }
    }

    fn opts(margin: usize, style: WrapStyle, trailing_comma: bool) -> Opts<'static> {
        Opts { margin, style, trailing_comma, unit: "    " }
    }

    fn run(file: &str, text: &str, o: &Opts) -> String {
        let _g = Guard(motion::set_tab_width(4), motion::set_tab_uses_spaces(true));
        let lang = language_for_name(file).unwrap();
        let all: Vec<String> = text.split('\n').map(str::to_owned).collect();
        let how = scan(Some(lang), &all);
        reformat_lines(lang, &all, &how, 0, all.len() - 1, o).join("\n")
    }

    #[test]
    fn a_multi_line_group_that_fits_is_joined_back() {
        let text = "fn f() {\n    let x = foo(\n        a,\n        b,\n    );\n}";
        assert_eq!(run("a.rs", text, &opts(60, WrapStyle::Hang, true)), "fn f() {\n    let x = foo(a, b);\n}");
        // 只有一個元素的元組保留逗號
        assert_eq!(run("a.rs", "let t = (\n    a,\n);", &opts(60, WrapStyle::Hang, true)), "let t = (a,);");
        assert_eq!(run("a.py", "x = foo(\n    a,\n    b,\n)", &opts(60, WrapStyle::Align, true)), "x = foo(a, b)");
        assert_eq!(run("a.py", "x = (\n    a,\n)", &opts(60, WrapStyle::Align, true)), "x = (a,)", "Python 的單元素元組也保留逗號");
    }

    #[test]
    fn groups_with_comments_blocks_or_blank_lines_are_not_joined() {
        let o = opts(80, WrapStyle::Hang, true);
        let with_comment = "foo(\n    a, // 說明\n    b,\n);";
        assert_eq!(run("a.rs", with_comment, &o), with_comment);
        let with_block = "foo(a, |x| {\n    x\n});";
        assert_eq!(run("a.rs", with_block, &o), with_block);
        let with_blank = "foo(\n    a,\n\n    b,\n);";
        assert_eq!(run("a.rs", with_blank, &o), with_blank);
    }

    #[test]
    fn a_long_call_is_broken_by_the_wrap_style() {
        let text = "result = compute(first_argument, second_argument, third_argument)";
        let align = run("a.py", text, &opts(40, WrapStyle::Align, false));
        assert_eq!(align, "result = compute(first_argument,\n                 second_argument,\n                 third_argument)");
        let hang = run("a.rs", "let r = compute(first_argument, second_argument, third_argument);", &opts(40, WrapStyle::Hang, true));
        assert_eq!(hang, "let r = compute(\n    first_argument,\n    second_argument,\n    third_argument,\n);");
        let no_comma = run("a.rs", "let r = compute(first_argument, second_argument, third_argument);", &opts(40, WrapStyle::Hang, false));
        assert!(no_comma.contains("third_argument\n);"), "{no_comma}");
    }

    #[test]
    fn align_fills_each_line_and_hang_keeps_the_indent_of_the_statement() {
        let text = "    x = foo(aaaa, bbbb, cccc, dddd, eeee, ffff)";
        let got = run("a.py", text, &opts(30, WrapStyle::Align, false));
        assert_eq!(got, "    x = foo(aaaa, bbbb, cccc,\n            dddd, eeee, ffff)");
        let got = run("a.rs", "fn f() {\nlet v = foo(aaaa, bbbb, cccc, dddd);\n}", &opts(24, WrapStyle::Hang, true));
        assert_eq!(got, "fn f() {\n    let v = foo(\n        aaaa,\n        bbbb,\n        cccc,\n        dddd,\n    );\n}");
    }

    #[test]
    fn single_arguments_and_strings_are_not_broken_badly() {
        let o = opts(20, WrapStyle::Align, false);
        let s = "x = foo(\"a, b, c, d, e, f, g\")";
        assert_eq!(run("a.py", s, &o), s, "對齊風格下單一參數斷不開，字串也不會被切");
        let hang = run("a.rs", "let x = (aaaa + bbbb + cccc + dddd);", &opts(20, WrapStyle::Hang, true));
        assert!(!hang.contains("dddd,"), "單一參數不加逗號（不然 (a + b) 會變成元組）：{hang}");
        // 沒有括號的長行維持原樣
        let plain = "let x = aaaa + bbbb + cccc + dddd + eeee;";
        assert_eq!(run("a.rs", plain, &opts(20, WrapStyle::Hang, true)), plain);
        // 行尾有註解：維持原樣
        let c = "foo(aaaa, bbbb, cccc, dddd); // 很長的註解";
        assert_eq!(run("a.rs", c, &opts(20, WrapStyle::Hang, true)), c);
    }

    #[test]
    fn a_nested_long_argument_breaks_again() {
        let text = "x = outer(inner_function(aaaa, bbbb, cccc), second)";
        let got = run("a.rs", text, &opts(30, WrapStyle::Hang, false));
        assert_eq!(got, "x = outer(\n    inner_function(\n        aaaa,\n        bbbb,\n        cccc\n    ),\n    second\n)");
    }

    #[test]
    fn only_groups_inside_the_selected_lines_are_touched() {
        let lang = language_for_name("a.rs").unwrap();
        let all: Vec<String> = "foo(\n    a,\n);\nbar(\n    b,\n);".split('\n').map(str::to_owned).collect();
        assert_eq!(plan(lang, &all, 0, 5), vec![(0, 2), (3, 5)]);
        assert_eq!(plan(lang, &all, 0, 1), vec![(0, 0), (1, 1)], "選取切到一組括號中間：不接合");
        assert_eq!(plan(lang, &all, 3, 5), vec![(3, 5)]);
    }
}
