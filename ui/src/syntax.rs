//! 輕量的語法高亮：逐行分詞，只標出註解、字串、數字、關鍵字、型別與函式名稱。
//!
//! 不是完整的語法分析，目標是「看得出結構、永遠不會拖慢編輯」：
//!
//! - 每一行獨立分詞，只帶一個跨行狀態（區塊註解、三引號字串、反引號字串）；
//! - 編輯區只排版看得到的行，所以只替它們分詞；
//! - 跨行狀態用 [`StateCache`] 逐行累積，修改文字後作廢重算；超過 [`MAX_FULL_LINES`] 行的文件
//!   改成「往回看 [`LOOKBACK`] 行」估計，不會因為檔案很大而卡住。
//!
//! 與畫面無關，方便單元測試。

use feditor_core::Buffer;
use std::ops::Range;
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Kind {
    #[default]
    Plain,
    Comment,
    String,
    Number,
    Keyword,
    Type,
    Function,
}

/// 跨行的狀態：一行結束時還沒結束的東西。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum State {
    #[default]
    Normal,
    /// 在區塊註解裡。
    Block,
    /// 在三引號字串裡（記錄是哪種引號）。
    Triple(char),
    /// 在反引號字串（JavaScript 的樣板字串）裡。
    Backtick,
    /// 在支援內嵌運算式的字串裡（記錄結尾引號）。目前只有反引號字串會跨行。
    InterpStr(char),
    /// 在字串的內嵌運算式 `${ … }` 裡，運算式還沒結束（記錄所屬字串的結尾引號）。
    InterpExpr(char),
}

/// 字串裡的內嵌運算式：`${x}`、`{x}`（f-string）、`#{x}`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Interp {
    pub open: &'static str,
    pub close: &'static str,
    /// 反引號字串裡有效。
    pub backtick: bool,
    /// 雙引號字串裡有效（Shell、Ruby、PHP）。
    pub double: bool,
    /// 帶 `f`／`$` 前綴的字串裡有效（Python 的 f"…"）。`{{` 是跳脫的大括號。
    pub fstring: bool,
}

impl Interp {
    /// 在 `i` 這個引號字元開始的字串，是不是會有內嵌運算式。
    fn applies(&self, chars: &[char], i: usize) -> bool {
        match chars[i] {
            '`' => self.backtick,
            c @ ('"' | '\'') => {
                (c == '"' && self.double) || (self.fstring && has_f_prefix(chars, i))
            }
            _ => false,
        }
    }
}

/// 引號前面緊接著 `f`／`F`／`$`（可再夾 `r`、`b`、`u`、`@`），而且前面不是識別字的一部分。
fn has_f_prefix(chars: &[char], quote: usize) -> bool {
    let (mut k, mut has_f) = (quote, false);
    while k > 0 && quote - k < 2 && "fFrRbBuU$@".contains(chars[k - 1]) {
        has_f |= matches!(chars[k - 1], 'f' | 'F' | '$');
        k -= 1;
    }
    has_f && (k == 0 || !is_ident(chars[k - 1]))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Code,
    /// HTML／XML。
    Markup,
    Markdown,
}

pub struct Language {
    pub name: &'static str,
    mode: Mode,
    line_comments: &'static [&'static str],
    block: Option<(&'static str, &'static str)>,
    /// 字串的引號字元。
    quotes: &'static [char],
    triple_quotes: bool,
    backtick_strings: bool,
    /// Rust：單引號可能是字元常值，也可能是生命週期。
    rust_char: bool,
    keywords: &'static [&'static str],
    /// 大寫開頭的識別字當成型別。
    capital_types: bool,
    /// 行首的 `[區段]` 與「鍵 =」（ini、toml、yaml）。
    config: bool,
    case_insensitive: bool,
    /// 字串裡的內嵌運算式；`None` 表示字串整個當成字串色。
    interp: Option<Interp>,
}

const fn lang(name: &'static str) -> Language {
    Language {
        name,
        mode: Mode::Code,
        line_comments: &[],
        block: None,
        quotes: &['"', '\''],
        triple_quotes: false,
        backtick_strings: false,
        rust_char: false,
        keywords: &[],
        capital_types: false,
        config: false,
        case_insensitive: false,
        interp: None,
    }
}

const SLASH_BLOCK: Option<(&str, &str)> = Some(("/*", "*/"));

static RUST: Language = Language {
    line_comments: &["//"],
    block: SLASH_BLOCK,
    quotes: &['"'],
    rust_char: true,
    capital_types: true,
    keywords: &[
        "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum",
        "extern", "false", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move",
        "mut", "pub", "ref", "return", "self", "Self", "static", "struct", "super", "trait",
        "true", "type", "unsafe", "use", "where", "while",
    ],
    ..lang("Rust")
};

static C_LIKE: Language = Language {
    line_comments: &["//"],
    block: SLASH_BLOCK,
    capital_types: true,
    keywords: &[
        "auto", "bool", "break", "case", "catch", "char", "class", "const", "continue", "default",
        "delete", "do", "double", "else", "enum", "explicit", "extern", "false", "float", "for",
        "friend", "goto", "if", "include", "inline", "int", "long", "namespace", "new", "nullptr",
        "operator", "private", "protected", "public", "register", "return", "short", "signed",
        "sizeof", "static", "struct", "switch", "template", "this", "throw", "true", "try",
        "typedef", "typename", "union", "unsigned", "using", "virtual", "void", "volatile", "while",
    ],
    ..lang("C/C++")
};

static JAVA_LIKE: Language = Language {
    line_comments: &["//"],
    block: SLASH_BLOCK,
    capital_types: true,
    keywords: &[
        "abstract", "as", "async", "await", "base", "boolean", "break", "byte", "case", "catch",
        "char", "class", "const", "continue", "default", "do", "double", "else", "enum", "extends",
        "false", "final", "finally", "float", "for", "foreach", "if", "implements", "import", "in",
        "instanceof", "int", "interface", "internal", "is", "long", "namespace", "new", "null",
        "override", "package", "private", "protected", "public", "return", "sealed", "short",
        "static", "string", "super", "switch", "this", "throw", "throws", "true", "try", "using",
        "var", "virtual", "void", "while", "yield",
    ],
    ..lang("Java/C#")
};

static JAVASCRIPT: Language = Language {
    line_comments: &["//"],
    block: SLASH_BLOCK,
    backtick_strings: true,
    capital_types: true,
    keywords: &[
        "async", "await", "break", "case", "catch", "class", "const", "continue", "debugger",
        "default", "delete", "do", "else", "enum", "export", "extends", "false", "finally", "for",
        "from", "function", "if", "implements", "import", "in", "instanceof", "interface", "let",
        "new", "null", "of", "private", "protected", "public", "readonly", "return", "static",
        "super", "switch", "this", "throw", "true", "try", "type", "typeof", "undefined", "var",
        "void", "while", "with", "yield",
    ],
    interp: Some(Interp { open: "${", close: "}", backtick: true, double: false, fstring: false }),
    ..lang("JavaScript/TypeScript")
};

static PYTHON: Language = Language {
    line_comments: &["#"],
    triple_quotes: true,
    capital_types: true,
    keywords: &[
        "and", "as", "assert", "async", "await", "break", "class", "continue", "def", "del",
        "elif", "else", "except", "False", "finally", "for", "from", "global", "if", "import",
        "in", "is", "lambda", "None", "nonlocal", "not", "or", "pass", "raise", "return", "True",
        "try", "while", "with", "yield",
    ],
    interp: Some(Interp { open: "{", close: "}", backtick: false, double: false, fstring: true }),
    ..lang("Python")
};

static GO: Language = Language {
    line_comments: &["//"],
    block: SLASH_BLOCK,
    quotes: &['"', '\''],
    backtick_strings: true,
    keywords: &[
        "break", "case", "chan", "const", "continue", "default", "defer", "else", "fallthrough",
        "false", "for", "func", "go", "goto", "if", "import", "interface", "map", "nil",
        "package", "range", "return", "select", "struct", "switch", "true", "type", "var",
    ],
    capital_types: true,
    ..lang("Go")
};

static SHELL: Language = Language {
    line_comments: &["#"],
    keywords: &[
        "case", "do", "done", "elif", "else", "esac", "export", "fi", "for", "function", "if",
        "in", "local", "return", "then", "until", "while", "param", "foreach", "switch",
    ],
    interp: Some(Interp { open: "${", close: "}", backtick: false, double: true, fstring: false }),
    ..lang("Shell")
};

static SQL: Language = Language {
    line_comments: &["--"],
    block: SLASH_BLOCK,
    case_insensitive: true,
    keywords: &[
        "add", "all", "alter", "and", "as", "asc", "between", "by", "case", "create", "delete",
        "desc", "distinct", "drop", "else", "end", "exists", "from", "group", "having", "in",
        "index", "inner", "insert", "into", "is", "join", "left", "like", "limit", "not", "null",
        "on", "or", "order", "outer", "primary", "right", "select", "set", "table", "then",
        "union", "unique", "update", "values", "view", "when", "where",
    ],
    ..lang("SQL")
};

static CSS: Language = Language {
    block: SLASH_BLOCK,
    keywords: &["important"],
    ..lang("CSS")
};

static LUA: Language = Language {
    line_comments: &["--"],
    block: Some(("--[[", "]]")),
    keywords: &[
        "and", "break", "do", "else", "elseif", "end", "false", "for", "function", "if", "in",
        "local", "nil", "not", "or", "repeat", "return", "then", "true", "until", "while",
    ],
    ..lang("Lua")
};

static JSON: Language = Language {
    quotes: &['"'],
    keywords: &["true", "false", "null"],
    ..lang("JSON")
};

static CONFIG: Language = Language {
    line_comments: &["#", ";"],
    config: true,
    keywords: &["true", "false", "null", "yes", "no", "on", "off"],
    ..lang("設定檔")
};

static MARKUP: Language = Language {
    mode: Mode::Markup,
    block: Some(("<!--", "-->")),
    ..lang("HTML/XML")
};

static MARKDOWN: Language = Language {
    mode: Mode::Markdown,
    ..lang("Markdown")
};

// ── 使用者自訂語言 ───────────────────────────────────────────

/// 使用者語言：（副檔名或檔名（小寫）, 語言）。啟動時載入一次，優先於內建語言。
static USER: std::sync::OnceLock<Vec<(Vec<String>, &'static Language)>> = std::sync::OnceLock::new();

/// 自訂語言檔的資料夾：設定資料夾下的 `syntax`。
pub fn syntax_dir() -> Option<std::path::PathBuf> {
    crate::settings::Settings::default_path().and_then(|p| p.parent().map(|d| d.join("syntax")))
}

fn leak_str(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

fn leak_strs(v: Vec<String>) -> &'static [&'static str] {
    Box::leak(v.into_iter().map(leak_str).collect::<Vec<_>>().into_boxed_slice())
}

/// 以逗號或空白分隔的清單。
fn split_list(value: &str) -> Vec<String> {
    value
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}

fn parse_flag(key: &str, value: &str) -> Result<bool, String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" | "yes" | "on" | "1" => Ok(true),
        "false" | "no" | "off" | "0" => Ok(false),
        _ => Err(format!("{key} 要寫 true 或 false，不是「{value}」")),
    }
}

/// 解析一個自訂語言檔。回傳（副檔名／檔名清單（小寫）, 語言）。格式是簡單的 `key = value`：
///
/// ```text
/// name = PHP
/// extensions = php, phtml          # 也可以寫完整檔名，例如 makefile
/// line_comment = //, #
/// block_comment = /*, */           # 開頭與結尾，以逗號分開
/// quotes = ", '                    # 字串的引號字元
/// keywords = if else while return  # 逗號或空白分隔，可寫多行
/// capital_types = true             # 大寫開頭的識別字當成型別
/// case_insensitive = false         # 關鍵字不分大小寫（SQL）
/// triple_quotes = false            # 三引號字串（Python）
/// backtick_strings = false         # 反引號字串（JavaScript）
/// mode = code                      # code／markup（HTML）／markdown／config（ini）
/// ```
///
/// `#` 開頭的整行是註解；`keywords` 可以重複出現，內容會合併。
pub fn parse_language(text: &str) -> Result<(Vec<String>, Language), String> {
    let mut name = None;
    let mut exts: Vec<String> = Vec::new();
    let mut lang = lang("");
    let mut line_comments: Vec<String> = Vec::new();
    let mut keywords: Vec<String> = Vec::new();
    let mut quotes: Option<Vec<char>> = None;
    let mut interp_marks: Option<(&'static str, &'static str)> = None;
    let mut interp_in: Option<Vec<String>> = None;
    for (n, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err(format!("第 {} 行：需要「鍵 = 值」", n + 1));
        };
        let (key, value) = (key.trim(), value.trim());
        match key {
            "name" => name = Some(value.to_owned()),
            "extensions" => exts.extend(split_list(value).into_iter().map(|e| {
                e.trim_start_matches('.').to_ascii_lowercase()
            })),
            // 行註解符號本身可能含 # ，所以這個值不做「# 開頭是註解」的處理，用逗號／空白分隔
            "line_comment" => line_comments.extend(split_list(value)),
            "block_comment" => {
                let parts = split_list(value);
                match parts.as_slice() {
                    [a, b] => lang.block = Some((leak_str(a.clone()), leak_str(b.clone()))),
                    _ => return Err(format!("第 {} 行：block_comment 要寫「開頭, 結尾」兩項", n + 1)),
                }
            }
            "quotes" => {
                quotes = Some(split_list(value).iter().flat_map(|s| s.chars()).collect());
            }
            "keywords" => keywords.extend(split_list(value)),
            "interpolation" => match split_list(value).as_slice() {
                [a, b] => interp_marks = Some((leak_str(a.clone()), leak_str(b.clone()))),
                _ => return Err(format!("第 {} 行：interpolation 要寫「開頭, 結尾」兩項，例如 ${{, }}", n + 1)),
            },
            "interpolation_in" => {
                interp_in = Some(split_list(value));
            }
            "capital_types" => lang.capital_types = parse_flag(key, value)?,
            "case_insensitive" => lang.case_insensitive = parse_flag(key, value)?,
            "triple_quotes" => lang.triple_quotes = parse_flag(key, value)?,
            "backtick_strings" => lang.backtick_strings = parse_flag(key, value)?,
            "mode" => match value.to_ascii_lowercase().as_str() {
                "code" => {}
                "markup" => lang.mode = Mode::Markup,
                "markdown" => lang.mode = Mode::Markdown,
                "config" => lang.config = true,
                _ => return Err(format!("第 {} 行：mode 只能是 code、markup、markdown、config", n + 1)),
            },
            _ => return Err(format!("第 {} 行：不認得的設定「{key}」", n + 1)),
        }
    }
    let name = name.filter(|n| !n.is_empty()).ok_or("缺少 name")?;
    if exts.is_empty() {
        return Err("缺少 extensions".to_owned());
    }
    if let Some((open, close)) = interp_marks {
        let places = interp_in.unwrap_or_else(|| vec!["backtick".into(), "double".into()]);
        let mut ip = Interp { open, close, backtick: false, double: false, fstring: false };
        for p in &places {
            match p.to_ascii_lowercase().as_str() {
                "backtick" => ip.backtick = true,
                "double" => ip.double = true,
                "fstring" => ip.fstring = true,
                other => return Err(format!("interpolation_in 只能是 backtick、double、fstring，不是「{other}」")),
            }
        }
        lang.interp = Some(ip);
    } else if interp_in.is_some() {
        return Err("有 interpolation_in 但缺少 interpolation".to_owned());
    }
    lang.name = leak_str(name);
    lang.line_comments = leak_strs(line_comments);
    lang.keywords = leak_strs(keywords);
    if let Some(q) = quotes {
        lang.quotes = Box::leak(q.into_boxed_slice());
    }
    Ok((exts, lang))
}

/// 讀取設定資料夾下 `syntax/*.conf` 的自訂語言。每個檔案一種語言；壞掉的檔案略過，
/// 錯誤訊息收在回傳值裡讓呼叫端顯示。只會真正載入一次（之後再呼叫不會重讀）。
pub fn load_user_languages(dir: Option<&Path>) -> Vec<String> {
    let mut messages = Vec::new();
    let mut found: Vec<(Vec<String>, &'static Language)> = Vec::new();
    if let Some(dir) = dir {
        let mut files: Vec<_> = std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("conf")))
            .collect();
        files.sort();
        for path in files {
            let label = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let Ok(bytes) = std::fs::read(&path) else {
                messages.push(format!("無法讀取語言檔 {label}"));
                continue;
            };
            match parse_language(&String::from_utf8_lossy(&bytes)) {
                Ok((exts, lang)) => found.push((exts, Box::leak(Box::new(lang)))),
                Err(e) => messages.push(format!("語言檔 {label} 有誤，已略過：{e}")),
            }
        }
    }
    let _ = USER.set(found);
    messages
}

/// 依副檔名選語言；不認得的回傳 `None`（不上色）。使用者自訂的語言優先。
pub fn language_for(path: Option<&Path>) -> Option<&'static Language> {
    let name = path?.file_name()?.to_string_lossy().to_ascii_lowercase();
    let ext = name.rsplit_once('.').map_or(name.as_str(), |(_, e)| e);
    if let Some(users) = USER.get() {
        // 副檔名或完整檔名（例如 makefile）都可以
        if let Some((_, lang)) = users.iter().find(|(exts, _)| exts.iter().any(|e| e == ext || *e == name)) {
            return Some(lang);
        }
    }
    Some(match ext {
        "rs" => &RUST,
        "c" | "h" | "cc" | "cpp" | "cxx" | "hpp" | "hh" | "ino" => &C_LIKE,
        "java" | "cs" | "kt" | "kts" | "scala" | "dart" | "swift" => &JAVA_LIKE,
        "js" | "mjs" | "cjs" | "jsx" | "ts" | "tsx" | "slint" => &JAVASCRIPT,
        "py" | "pyw" => &PYTHON,
        "go" => &GO,
        "sh" | "bash" | "zsh" | "ps1" | "psm1" | "bat" | "cmd" => &SHELL,
        "sql" => &SQL,
        "css" | "scss" | "less" => &CSS,
        "lua" => &LUA,
        "json" | "jsonc" => &JSON,
        "toml" | "ini" | "cfg" | "conf" | "yml" | "yaml" | "properties" | "gitignore" => &CONFIG,
        "html" | "htm" | "xml" | "svg" | "xaml" | "vue" => &MARKUP,
        "md" | "markdown" => &MARKDOWN,
        _ => return None,
    })
}

fn is_ident_start(c: char) -> bool {
    c.is_alphabetic() || c == '_'
}

fn is_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn matches_at(chars: &[char], i: usize, pat: &str) -> bool {
    pat.chars().enumerate().all(|(k, p)| chars.get(i + k) == Some(&p))
}

fn find_from(chars: &[char], from: usize, pat: &str) -> Option<usize> {
    (from..chars.len()).find(|&i| matches_at(chars, i, pat))
}

/// 一行的分詞結果：字元範圍（相對於這一行）與種類，Plain 不列出；以及行尾的狀態。
pub fn highlight_line(lang: &Language, line: &str, state: State) -> (Vec<(Range<usize>, Kind)>, State) {
    let chars: Vec<char> = line.chars().collect();
    highlight_chars(lang, &chars, state)
}

/// 字串裡的內嵌運算式：把 `chars[from..to]` 當成程式碼分詞，結果放進 `out`。
/// （運算式裡面還有字串、甚至還有內嵌時，會遞迴處理。）
fn push_code(lang: &Language, chars: &[char], from: usize, to: usize, out: &mut Vec<(Range<usize>, Kind)>) {
    if to > from {
        for (r, k) in highlight_chars(lang, &chars[from..to], State::Normal).0 {
            out.push((r.start + from..r.end + from, k));
        }
    }
}

/// 找內嵌運算式的結尾：從 `from` 開始，略過運算式裡的字串，數括號的層數。
fn find_close(lang: &Language, ip: &Interp, chars: &[char], from: usize) -> Option<usize> {
    let open_ch = ip.open.chars().last()?;
    let close_len = ip.close.chars().count();
    let (mut depth, mut i) = (0usize, from);
    while i < chars.len() {
        let c = chars[i];
        if (c == '`' && ip.backtick) || lang.quotes.contains(&c) {
            i = scan_string(chars, i + 1, c).0;
            continue;
        }
        if matches_at(chars, i, ip.close) {
            if depth == 0 {
                return Some(i);
            }
            depth -= 1;
            i += close_len;
            continue;
        }
        if c == open_ch {
            depth += 1;
        }
        i += 1;
    }
    None
}

/// 掃一個有內嵌運算式的字串。`span` 是目前這段字串色的起點，`from` 是掃描起點；
/// `in_expr` 為 true 表示一開始就在運算式裡（接續上一行）。
/// 回傳（結束位置, 行尾還沒結束時的狀態）：字串收尾了就是 `None`。
fn scan_interp(
    lang: &Language,
    ip: &Interp,
    chars: &[char],
    (from, span): (usize, usize),
    delim: char,
    in_expr: bool,
    out: &mut Vec<(Range<usize>, Kind)>,
) -> (usize, Option<State>) {
    let (open_len, close_len) = (ip.open.chars().count(), ip.close.chars().count());
    let (mut i, mut span, mut expr) = (from, span, in_expr);
    loop {
        if expr {
            match find_close(lang, ip, chars, i) {
                Some(close) => {
                    push_code(lang, chars, i, close, out);
                    // 結尾的括號算在字串色裡，跟開頭一致
                    span = close;
                    i = close + close_len;
                    expr = false;
                }
                None => {
                    push_code(lang, chars, i, chars.len(), out);
                    return (chars.len(), Some(State::InterpExpr(delim)));
                }
            }
            continue;
        }
        if i >= chars.len() {
            break;
        }
        let c = chars[i];
        if c == '\\' {
            i += 2;
        } else if c == delim {
            out.push((span..i + 1, Kind::String));
            return (i + 1, None);
        } else if matches_at(chars, i, ip.open) {
            if ip.fstring && open_len == 1 && chars.get(i + 1) == Some(&c) {
                i += 2; // {{ 是跳脫的大括號
            } else {
                out.push((span..i + open_len, Kind::String));
                i += open_len;
                expr = true;
            }
        } else {
            i += 1;
        }
    }
    let end = chars.len();
    if end > span {
        out.push((span..end, Kind::String));
    }
    // 只有反引號字串會跨行，其他引號的字串到行尾就結束
    (end, (delim == '`').then_some(State::InterpStr('`')))
}

fn highlight_chars(lang: &Language, chars: &[char], state: State) -> (Vec<(Range<usize>, Kind)>, State) {
    let mut out: Vec<(Range<usize>, Kind)> = Vec::new();
    let mut i = 0;
    let mut state = state;

    // 先把上一行沒結束的東西接完
    match state {
        State::Normal => {}
        State::InterpStr(d) | State::InterpExpr(d) => match &lang.interp {
            Some(ip) => {
                let in_expr = matches!(state, State::InterpExpr(_));
                match scan_interp(lang, ip, chars, (0, 0), d, in_expr, &mut out) {
                    (_, Some(next)) => return (out, next),
                    (end, None) => {
                        i = end;
                        state = State::Normal;
                    }
                }
            }
            // 換了語言：沒有內嵌可言，當作從頭開始
            None => state = State::Normal,
        },
        State::Block => {
            let (_, end) = lang.block.unwrap_or(("", "*/"));
            match find_from(&chars, 0, end) {
                Some(p) => {
                    i = p + end.chars().count();
                    out.push((0..i, Kind::Comment));
                    state = State::Normal;
                }
                None => {
                    if !chars.is_empty() {
                        out.push((0..chars.len(), Kind::Comment));
                    }
                    return (out, State::Block);
                }
            }
        }
        State::Triple(q) => {
            let pat: String = std::iter::repeat_n(q, 3).collect();
            match find_from(&chars, 0, &pat) {
                Some(p) => {
                    i = p + 3;
                    out.push((0..i, Kind::String));
                    state = State::Normal;
                }
                None => {
                    if !chars.is_empty() {
                        out.push((0..chars.len(), Kind::String));
                    }
                    return (out, State::Triple(q));
                }
            }
        }
        State::Backtick => match scan_string(&chars, 0, '`') {
            (end, true) => {
                out.push((0..end, Kind::String));
                i = end;
                state = State::Normal;
            }
            (end, false) => {
                if end > 0 {
                    out.push((0..end, Kind::String));
                }
                return (out, State::Backtick);
            }
        },
    }

    match lang.mode {
        Mode::Markup => return highlight_markup(lang, &chars, i, out),
        Mode::Markdown => return (highlight_markdown(&chars), State::Normal),
        Mode::Code => {}
    }

    // 設定檔：行首的 [區段]，與「鍵 =」「鍵:」
    if lang.config && i == 0 {
        let indent = chars.iter().take_while(|c| c.is_whitespace()).count();
        if chars.get(indent) == Some(&'[') {
            if let Some(close) = chars.iter().rposition(|c| *c == ']') {
                if close > indent {
                    out.push((indent..close + 1, Kind::Type));
                    i = close + 1;
                }
            }
        } else if let Some(sep) = chars.iter().position(|c| *c == '=' || *c == ':') {
            let key_end = chars[..sep].iter().rposition(|c| !c.is_whitespace()).map_or(0, |p| p + 1);
            let key_start = indent + usize::from(chars.get(indent) == Some(&'-')) * 2;
            let is_comment = lang.line_comments.iter().any(|c| matches_at(&chars, indent, c));
            if !is_comment && key_end > key_start && chars[key_start..key_end].iter().all(|c| is_ident(*c) || *c == '.' || *c == '-' || *c == ' ') {
                out.push((key_start..key_end, Kind::Function));
            }
        }
    }

    while i < chars.len() {
        let c = chars[i];
        if let Some(pre) = lang.line_comments.iter().find(|p| matches_at(&chars, i, p)) {
            // 設定檔裡 `#` 要在行首或前面是空白才算註解（網址裡的 # 不是）
            let ok = !lang.config || i == 0 || chars[i - 1].is_whitespace();
            if ok || pre.is_empty() {
                out.push((i..chars.len(), Kind::Comment));
                return (out, State::Normal);
            }
        }
        if let Some((open, close)) = lang.block {
            if matches_at(&chars, i, open) {
                match find_from(&chars, i + open.chars().count(), close) {
                    Some(p) => {
                        let end = p + close.chars().count();
                        out.push((i..end, Kind::Comment));
                        i = end;
                        continue;
                    }
                    None => {
                        out.push((i..chars.len(), Kind::Comment));
                        return (out, State::Block);
                    }
                }
            }
        }
        if lang.triple_quotes && (c == '"' || c == '\'') && matches_at(&chars, i, &format!("{c}{c}{c}")) {
            let pat = format!("{c}{c}{c}");
            match find_from(&chars, i + 3, &pat) {
                Some(p) => {
                    out.push((i..p + 3, Kind::String));
                    i = p + 3;
                    continue;
                }
                None => {
                    out.push((i..chars.len(), Kind::String));
                    return (out, State::Triple(c));
                }
            }
        }
        if let Some(ip) = &lang.interp {
            if ip.applies(chars, i) {
                match scan_interp(lang, ip, chars, (i + 1, i), c, false, &mut out) {
                    (_, Some(next)) => return (out, next),
                    (end, None) => {
                        i = end;
                        continue;
                    }
                }
            }
        }
        if lang.rust_char && c == '\'' {
            // 字元常值 'a'、'\n'；其他（'a、'static）是生命週期
            let end = if chars.get(i + 1) == Some(&'\\') {
                (i + 2..(i + 8).min(chars.len())).find(|&k| chars[k] == '\'').map(|k| k + 1)
            } else if chars.get(i + 2) == Some(&'\'') {
                Some(i + 3)
            } else {
                None
            };
            if let Some(end) = end {
                out.push((i..end, Kind::String));
                i = end;
            } else {
                i += 1;
            }
            continue;
        }
        if lang.backtick_strings && c == '`' {
            match scan_string(&chars, i + 1, '`') {
                (end, true) => {
                    out.push((i..end, Kind::String));
                    i = end;
                }
                (end, false) => {
                    out.push((i..end, Kind::String));
                    return (out, State::Backtick);
                }
            }
            continue;
        }
        if !lang.rust_char && lang.quotes.contains(&c) || lang.rust_char && c == '"' {
            let (end, _) = scan_string(&chars, i + 1, c);
            out.push((i..end, Kind::String));
            i = end;
            continue;
        }
        if c.is_ascii_digit() && (i == 0 || !is_ident(chars[i - 1])) {
            let mut j = i + 1;
            while j < chars.len() && (chars[j].is_ascii_alphanumeric() || chars[j] == '_' || (chars[j] == '.' && chars.get(j + 1).is_some_and(char::is_ascii_digit))) {
                j += 1;
            }
            out.push((i..j, Kind::Number));
            i = j;
            continue;
        }
        if is_ident_start(c) {
            let mut j = i + 1;
            while j < chars.len() && is_ident(chars[j]) {
                j += 1;
            }
            let word: String = chars[i..j].iter().collect();
            let is_kw = if lang.case_insensitive {
                lang.keywords.iter().any(|k| k.eq_ignore_ascii_case(&word))
            } else {
                lang.keywords.contains(&word.as_str())
            };
            let next = chars[j..].iter().find(|c| !matches!(c, ' ' | '\t'));
            let kind = if is_kw {
                Kind::Keyword
            } else if lang.capital_types && c.is_uppercase() && word.chars().any(|ch| ch.is_lowercase()) {
                Kind::Type
            } else if next == Some(&'(') {
                Kind::Function
            } else {
                Kind::Plain
            };
            if kind != Kind::Plain {
                out.push((i..j, kind));
            }
            i = j;
            continue;
        }
        i += 1;
    }
    (out, state)
}

/// 從 `from` 開始掃一個字串，直到遇到結尾的 `quote`（略過反斜線跳脫）。
/// 回傳（字串結束後的位置, 有沒有找到結尾）。沒找到結尾時位置是行尾。
fn scan_string(chars: &[char], from: usize, quote: char) -> (usize, bool) {
    let mut i = from;
    while i < chars.len() {
        match chars[i] {
            '\\' => i += 2,
            c if c == quote => return (i + 1, true),
            _ => i += 1,
        }
    }
    (chars.len(), false)
}

fn highlight_markup(
    lang: &Language,
    chars: &[char],
    mut i: usize,
    mut out: Vec<(Range<usize>, Kind)>,
) -> (Vec<(Range<usize>, Kind)>, State) {
    let (open, close) = lang.block.unwrap_or(("<!--", "-->"));
    while i < chars.len() {
        if matches_at(chars, i, open) {
            match find_from(chars, i + open.chars().count(), close) {
                Some(p) => {
                    let end = p + close.chars().count();
                    out.push((i..end, Kind::Comment));
                    i = end;
                    continue;
                }
                None => {
                    out.push((i..chars.len(), Kind::Comment));
                    return (out, State::Block);
                }
            }
        }
        if chars[i] == '<' && chars.get(i + 1).is_some_and(|c| is_ident_start(*c) || matches!(c, '/' | '!' | '?')) {
            // 標籤名稱（含 < 與 /）
            let mut j = i + 1;
            if matches!(chars.get(j), Some('/' | '!' | '?')) {
                j += 1;
            }
            while j < chars.len() && (is_ident(chars[j]) || matches!(chars[j], '-' | ':' | '.')) {
                j += 1;
            }
            out.push((i..j, Kind::Keyword));
            // 標籤內：屬性值（字串）、屬性名稱（函式色），直到 >
            while j < chars.len() && chars[j] != '>' {
                if chars[j] == '"' || chars[j] == '\'' {
                    let (end, _) = scan_string(chars, j + 1, chars[j]);
                    out.push((j..end, Kind::String));
                    j = end;
                } else if is_ident_start(chars[j]) {
                    let s = j;
                    while j < chars.len() && (is_ident(chars[j]) || matches!(chars[j], '-' | ':')) {
                        j += 1;
                    }
                    out.push((s..j, Kind::Function));
                } else {
                    j += 1;
                }
            }
            if j < chars.len() {
                out.push((j..j + 1, Kind::Keyword));
                j += 1;
            }
            i = j;
            continue;
        }
        i += 1;
    }
    (out, State::Normal)
}

fn highlight_markdown(chars: &[char]) -> Vec<(Range<usize>, Kind)> {
    let mut out = Vec::new();
    let indent = chars.iter().take_while(|c| **c == ' ').count();
    let rest = &chars[indent..];
    if rest.first() == Some(&'#') {
        let hashes = rest.iter().take_while(|c| **c == '#').count();
        if hashes <= 6 && rest.get(hashes).is_none_or(|c| *c == ' ') {
            out.push((indent..chars.len(), Kind::Keyword));
            return out;
        }
    }
    if matches_at(chars, indent, "```") || matches_at(chars, indent, "~~~") {
        out.push((indent..chars.len(), Kind::Comment));
        return out;
    }
    if matches!(rest.first(), Some('-' | '*' | '+')) && rest.get(1) == Some(&' ') {
        out.push((indent..indent + 1, Kind::Keyword));
    }
    // 行內程式碼 `...`
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '`' {
            if let Some(end) = chars[i + 1..].iter().position(|c| *c == '`') {
                out.push((i..i + 1 + end + 1, Kind::String));
                i += end + 2;
                continue;
            }
        }
        i += 1;
    }
    out
}

/// 一行裡每個字元的種類（長度等於字元數）。
pub fn kinds_of(lang: &Language, line: &str, state: State) -> Vec<Kind> {
    let n = line.chars().count();
    let mut kinds = vec![Kind::Plain; n];
    for (range, kind) in highlight_line(lang, line, state).0 {
        for k in kinds.iter_mut().take(range.end.min(n)).skip(range.start) {
            *k = kind;
        }
    }
    kinds
}

/// 文件超過這個行數就不累積整份文件的跨行狀態，改用「往回看」估計。
pub const MAX_FULL_LINES: usize = 20_000;
/// 估計時往回看幾行。
pub const LOOKBACK: usize = 1000;

/// 每一行開頭的跨行狀態，逐行累積。
#[derive(Debug, Default)]
pub struct StateCache {
    rev: u64,
    chars: usize,
    lang: Option<&'static str>,
    /// `starts[i]` 是第 `i` 行開頭的狀態。
    starts: Vec<State>,
}

impl StateCache {
    /// 第 `line` 行開頭的狀態。
    pub fn state_at(&mut self, lang: &'static Language, buf: &Buffer, line: usize) -> State {
        if buf.line_count() > MAX_FULL_LINES {
            let from = line.saturating_sub(LOOKBACK);
            let mut state = State::Normal;
            for l in from..line {
                state = highlight_line(lang, &buf.line_text(l), state).1;
            }
            return state;
        }
        if self.rev != buf.revision()
            || self.chars != buf.len_chars()
            || self.lang != Some(lang.name)
            || self.starts.is_empty()
        {
            self.rev = buf.revision();
            self.chars = buf.len_chars();
            self.lang = Some(lang.name);
            self.starts.clear();
            self.starts.push(State::Normal);
        }
        while self.starts.len() <= line {
            let l = self.starts.len() - 1;
            let next = highlight_line(lang, &buf.line_text(l), self.starts[l]).1;
            self.starts.push(next);
        }
        self.starts[line]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(lang: &Language, line: &str) -> Vec<(String, Kind)> {
        let chars: Vec<char> = line.chars().collect();
        highlight_line(lang, line, State::Normal)
            .0
            .into_iter()
            .map(|(r, k)| (chars[r].iter().collect(), k))
            .collect()
    }

    #[test]
    fn language_is_picked_by_extension() {
        let p = |s: &str| language_for(Some(Path::new(s))).map(|l| l.name);
        assert_eq!(p("a/main.rs"), Some("Rust"));
        assert_eq!(p("X.PY"), Some("Python"));
        assert_eq!(p("index.html"), Some("HTML/XML"));
        assert_eq!(p("notes.txt"), None);
        assert_eq!(p("Makefile"), None);
        assert!(language_for(None).is_none());
    }

    #[test]
    fn rust_tokens() {
        let r = kinds(&RUST, r#"let x: Vec<u8> = foo("hi", 42); // note"#);
        assert!(r.contains(&("let".into(), Kind::Keyword)));
        assert!(r.contains(&("Vec".into(), Kind::Type)));
        assert!(r.contains(&("foo".into(), Kind::Function)));
        assert!(r.contains(&("\"hi\"".into(), Kind::String)));
        assert!(r.contains(&("42".into(), Kind::Number)));
        assert!(r.contains(&("// note".into(), Kind::Comment)));
    }

    #[test]
    fn rust_lifetimes_are_not_strings_but_char_literals_are() {
        let r = kinds(&RUST, "fn f<'a>(x: &'a str) -> char { 'x' }");
        assert!(r.contains(&("'x'".into(), Kind::String)));
        assert!(!r.iter().any(|(t, k)| *k == Kind::String && t.contains("'a")), "{r:?}");
        let r = kinds(&RUST, r"let n = '\n'; let s = '\'';");
        assert!(r.contains(&(r"'\n'".into(), Kind::String)));
    }

    #[test]
    fn keywords_inside_strings_and_comments_stay_plain() {
        let r = kinds(&RUST, r#"let s = "fn if"; // for while"#);
        assert_eq!(r.iter().filter(|(_, k)| *k == Kind::Keyword).count(), 1, "{r:?}");
    }

    #[test]
    fn string_escapes_do_not_end_the_string() {
        let r = kinds(&RUST, r#"let s = "a\"b"; let t = 1;"#);
        assert!(r.contains(&(r#""a\"b""#.into(), Kind::String)));
        assert!(r.contains(&("1".into(), Kind::Number)));
    }

    #[test]
    fn block_comments_carry_across_lines() {
        let (spans, st) = highlight_line(&RUST, "x /* start", State::Normal);
        assert_eq!(st, State::Block);
        assert_eq!(spans, vec![(2..10, Kind::Comment)]);
        let (spans, st) = highlight_line(&RUST, "still inside", State::Block);
        assert_eq!((spans, st), (vec![(0..12, Kind::Comment)], State::Block));
        let (spans, st) = highlight_line(&RUST, "end */ let", State::Block);
        assert_eq!(st, State::Normal);
        assert_eq!(spans[0], (0..6, Kind::Comment));
        assert!(spans.iter().any(|(r, k)| *r == (7..10) && *k == Kind::Keyword));
    }

    #[test]
    fn python_triple_quotes_and_hash_comments() {
        let (spans, st) = highlight_line(&PYTHON, "x = \"\"\"doc", State::Normal);
        assert_eq!(st, State::Triple('"'));
        assert!(spans.contains(&(4..10, Kind::String)));
        let (_, st) = highlight_line(&PYTHON, "end\"\"\" + 1", State::Triple('"'));
        assert_eq!(st, State::Normal);
        let r = kinds(&PYTHON, "def f(x): # hi");
        assert!(r.contains(&("def".into(), Kind::Keyword)));
        assert!(r.contains(&("# hi".into(), Kind::Comment)));
    }

    #[test]
    fn javascript_template_strings_span_lines() {
        let (_, st) = highlight_line(&JAVASCRIPT, "const a = `one", State::Normal);
        assert_eq!(st, State::InterpStr('`'));
        let (spans, st) = highlight_line(&JAVASCRIPT, "two` + 3", State::InterpStr('`'));
        assert_eq!(st, State::Normal);
        assert_eq!(spans[0], (0..4, Kind::String));
    }

    #[test]
    fn template_string_expressions_are_code() {
        let r = kinds(&JAVASCRIPT, "`a ${x + 1} b`");
        assert_eq!(
            r,
            vec![
                ("`a ${".to_string(), Kind::String),
                ("1".to_string(), Kind::Number),
                ("} b`".to_string(), Kind::String),
            ]
        );
        let r = kinds(&JAVASCRIPT, "`${ if (f(a)) { 1 } }` + 2");
        assert!(r.contains(&("if".into(), Kind::Keyword)));
        assert!(r.contains(&("f".into(), Kind::Function)), "{r:?}");
        assert!(r.contains(&("2".into(), Kind::Number)), "運算式結束後回到程式碼：{r:?}");
    }

    #[test]
    fn expressions_can_hold_strings_and_nested_templates() {
        // 字串裡的 } 不算運算式結尾；巢狀樣板字串也要配對
        let r = kinds(&JAVASCRIPT, "`${ a ? \"}\" : `y${z}` } tail` + 7");
        assert!(r.contains(&("\"}\"".into(), Kind::String)), "{r:?}");
        assert!(r.contains(&("7".into(), Kind::Number)), "{r:?}");
        let tail = r.iter().find(|(t, _)| t.ends_with("tail`")).expect("結尾的字串色");
        assert_eq!(tail.1, Kind::String);
    }

    #[test]
    fn expressions_continue_across_lines() {
        let (spans, st) = highlight_line(&JAVASCRIPT, "const s = `v ${", State::Normal);
        assert_eq!(st, State::InterpExpr('`'));
        assert_eq!(spans.last().unwrap().1, Kind::String);
        let (spans, st) = highlight_line(&JAVASCRIPT, "  foo(1)} end`; let", State::InterpExpr('`'));
        assert_eq!(st, State::Normal);
        let r = kinds(&JAVASCRIPT, "foo(1)}"); // 只是確認運算式的 foo 會是函式
        assert!(r.iter().any(|(t, k)| t == "foo" && *k == Kind::Function));
        assert!(spans.iter().any(|(_, k)| *k == Kind::Function));
        assert!(spans.iter().any(|(_, k)| *k == Kind::Keyword), "後面的 let 回到程式碼");
        // 運算式整行都沒結束：維持在運算式裡
        let (_, st) = highlight_line(&JAVASCRIPT, "  still going", State::InterpExpr('`'));
        assert_eq!(st, State::InterpExpr('`'));
    }

    #[test]
    fn python_fstrings_color_expressions_but_plain_strings_do_not() {
        let r = kinds(&PYTHON, r#"print(f"n={len(x)} {{lit}}", "{no}")"#);
        assert!(r.contains(&("len".into(), Kind::Function)), "{r:?}");
        // 沒有 f 前綴的字串、與跳脫的 {{ }}，整段都是字串
        assert!(r.contains(&("\"{no}\"".into(), Kind::String)), "{r:?}");
        assert!(r.iter().any(|(t, k)| t.contains("{{lit}}") && *k == Kind::String), "{r:?}");
        let r = kinds(&PYTHON, "x = rf'{a + 1}'");
        assert!(r.contains(&("1".into(), Kind::Number)), "rf 前綴也算：{r:?}");
        let r = kinds(&PYTHON, "x = self'{a + 1}'"); // 前面是識別字：不是前綴
        assert!(!r.contains(&("1".into(), Kind::Number)), "{r:?}");
        // f-string 到行尾沒結束也不會卡在字串狀態
        let (_, st) = highlight_line(&PYTHON, "x = f\"abc {y", State::Normal);
        assert_eq!(st, State::InterpExpr('"'));
        let (_, st) = highlight_line(&PYTHON, "z}\"", State::InterpExpr('"'));
        assert_eq!(st, State::Normal);
    }

    #[test]
    fn shell_double_quotes_interpolate_single_quotes_do_not() {
        // 雙引號裡 ${…} 的內容是程式碼（if 是關鍵字），單引號整段都是字串
        let r = kinds(&SHELL, "echo \"${if} x\" '${if}'");
        assert_eq!(r.iter().filter(|(_, k)| *k == Kind::Keyword).count(), 1, "{r:?}");
        assert!(r.contains(&("'${if}'".into(), Kind::String)), "{r:?}");
    }

    #[test]
    fn user_language_can_declare_interpolation() {
        let base = "name = Rb\nextensions = rb\nquotes = \", '\nkeywords = if end\n";
        let (_, lang) = parse_language(&format!("{base}interpolation = #{{, }}\ninterpolation_in = double\n")).unwrap();
        let r = kinds(&lang, "\"a #{if} b\" '#{if}'");
        assert!(r.contains(&("if".into(), Kind::Keyword)), "{r:?}");
        assert!(r.contains(&("'#{if}'".into(), Kind::String)), "{r:?}");
        let err = |t: &str| parse_language(&format!("{base}{t}")).err().unwrap();
        assert!(err("interpolation = ${").contains("interpolation"));
        assert!(err("interpolation = ${, }\ninterpolation_in = weird").contains("weird"));
        assert!(err("interpolation_in = double").contains("interpolation"));
    }

    #[test]
    fn interpolation_keeps_cache_and_full_scan_in_agreement() {
        let text = "const a = `x ${\n  f(1)\n} y\nz`;\nlet b = 2;\n";
        let buf = Buffer::from_text(text);
        let mut cache = StateCache::default();
        let lang: &'static Language = &JAVASCRIPT;
        let mut state = State::Normal;
        for l in 0..buf.line_count() {
            assert_eq!(cache.state_at(lang, &buf, l), state, "第 {l} 行");
            state = highlight_line(lang, &buf.line_text(l), state).1;
        }
    }

    #[test]
    fn numbers_need_a_word_boundary() {
        let r = kinds(&RUST, "let a1 = 0x1F + 3.5 + x2;");
        assert!(r.contains(&("0x1F".into(), Kind::Number)));
        assert!(r.contains(&("3.5".into(), Kind::Number)));
        assert!(!r.iter().any(|(t, _)| t == "1" || t == "2"), "{r:?}");
    }

    #[test]
    fn cjk_text_is_indexed_by_characters_not_bytes() {
        let r = kinds(&RUST, r#"let 名稱 = "你好"; // 註解"#);
        assert!(r.contains(&(r#""你好""#.into(), Kind::String)));
        assert!(r.contains(&("// 註解".into(), Kind::Comment)));
        let ks = kinds_of(&RUST, r#"a "你好" b"#, State::Normal);
        assert_eq!(ks.len(), 8);
        assert_eq!(ks[2], Kind::String);
        assert_eq!(ks[5], Kind::String);
        assert_eq!(ks[7], Kind::Plain);
    }

    #[test]
    fn sql_is_case_insensitive() {
        let r = kinds(&SQL, "SeLeCt * From t WHERE x = 1 -- c");
        assert_eq!(r.iter().filter(|(_, k)| *k == Kind::Keyword).count(), 3, "{r:?}");
        assert!(r.contains(&("-- c".into(), Kind::Comment)));
    }

    #[test]
    fn markup_tags_attributes_and_comments() {
        let r = kinds(&MARKUP, r#"<div class="a">text</div> <!-- c -->"#);
        assert!(r.contains(&("<div".into(), Kind::Keyword)));
        assert!(r.contains(&("class".into(), Kind::Function)));
        assert!(r.contains(&("\"a\"".into(), Kind::String)));
        assert!(r.contains(&("</div".into(), Kind::Keyword)));
        assert!(r.contains(&("<!-- c -->".into(), Kind::Comment)));
        let (_, st) = highlight_line(&MARKUP, "<!-- open", State::Normal);
        assert_eq!(st, State::Block);
    }

    #[test]
    fn markdown_headings_fences_and_inline_code() {
        assert_eq!(kinds(&MARKDOWN, "## Title"), vec![("## Title".into(), Kind::Keyword)]);
        assert_eq!(kinds(&MARKDOWN, "```rust"), vec![("```rust".into(), Kind::Comment)]);
        let r = kinds(&MARKDOWN, "- use `cargo` now");
        assert!(r.contains(&("-".into(), Kind::Keyword)));
        assert!(r.contains(&("`cargo`".into(), Kind::String)));
        assert!(kinds(&MARKDOWN, "#hashtag").is_empty(), "# 後面沒有空白不是標題");
    }

    #[test]
    fn config_files_sections_keys_and_url_hashes() {
        let r = kinds(&CONFIG, "[package]");
        assert_eq!(r, vec![("[package]".into(), Kind::Type)]);
        let r = kinds(&CONFIG, "name = \"x\" # trailing");
        assert!(r.contains(&("name".into(), Kind::Function)));
        assert!(r.contains(&("# trailing".into(), Kind::Comment)));
        let r = kinds(&CONFIG, "url = http://a/b#frag");
        assert!(!r.iter().any(|(_, k)| *k == Kind::Comment), "{r:?}");
        assert_eq!(kinds(&CONFIG, "# whole line"), vec![("# whole line".into(), Kind::Comment)]);
        let r = kinds(&CONFIG, "enabled: true");
        assert!(r.contains(&("true".into(), Kind::Keyword)));
    }

    #[test]
    fn json_keys_values_and_keywords() {
        let r = kinds(&JSON, r#"{"a": [1, true, null, "x"]}"#);
        assert_eq!(r.iter().filter(|(_, k)| *k == Kind::String).count(), 2);
        assert_eq!(r.iter().filter(|(_, k)| *k == Kind::Keyword).count(), 2);
        assert!(r.contains(&("1".into(), Kind::Number)));
    }

    #[test]
    fn state_cache_follows_edits_and_matches_a_full_scan() {
        let text = "a\n/* c\nb\n*/\nlet x = 1;\n";
        let mut buf = Buffer::from_text(text);
        let mut cache = StateCache::default();
        let lang: &'static Language = &RUST;
        assert_eq!(cache.state_at(lang, &buf, 0), State::Normal);
        assert_eq!(cache.state_at(lang, &buf, 2), State::Block);
        assert_eq!(cache.state_at(lang, &buf, 4), State::Normal);
        // 刪掉註解開頭：後面的行不再在區塊註解裡
        buf.remove(2..4);
        assert_eq!(cache.state_at(lang, &buf, 2), State::Normal);
    }

    #[test]
    fn huge_documents_use_the_lookback_estimate() {
        let mut text = String::new();
        for _ in 0..(MAX_FULL_LINES + 10) {
            text.push_str("x\n");
        }
        let mut buf = Buffer::from_text(&text);
        buf.insert(0, "/*\n");
        let mut cache = StateCache::default();
        // 離開頭很遠的行，往回看不到註解開頭：估計成 Normal（這是設計上的取捨）
        assert_eq!(cache.state_at(&RUST, &buf, 15_000), State::Normal);
        // 開頭附近仍然正確
        assert_eq!(cache.state_at(&RUST, &buf, 5), State::Block);
    }

    const PHP_CONF: &str = "\
# 範例
name = Zed
extensions = .ZZ, zzfile
line_comment = //, #
block_comment = /*, */
quotes = \", '
keywords = if else
keywords = while, return
capital_types = true
";

    #[test]
    fn parses_a_user_language_file() {
        let (exts, lang) = parse_language(PHP_CONF).unwrap();
        assert_eq!(exts, vec!["zz", "zzfile"], "副檔名去掉開頭的點並轉小寫");
        assert_eq!(lang.name, "Zed");
        assert_eq!(lang.line_comments, &["//", "#"]);
        assert_eq!(lang.block, Some(("/*", "*/")));
        assert_eq!(lang.quotes, &['"', '\'']);
        assert_eq!(lang.keywords, &["if", "else", "while", "return"], "多行 keywords 會合併");
        assert!(lang.capital_types);
        let r = kinds(&lang, "if foo(Bar, \"s\") # c");
        assert!(r.contains(&("if".into(), Kind::Keyword)));
        assert!(r.contains(&("foo".into(), Kind::Function)));
        assert!(r.contains(&("Bar".into(), Kind::Type)));
        assert!(r.contains(&("# c".into(), Kind::Comment)));
    }

    #[test]
    fn bad_user_language_files_report_what_is_wrong() {
        let err = |t: &str| parse_language(t).err().unwrap();
        assert!(err("extensions = a").contains("name"));
        assert!(err("name = A").contains("extensions"));
        assert!(err("name = A\nextensions = a\nbogus = 1").contains("bogus"));
        assert!(err("name = A\nextensions = a\nmode = fancy").contains("mode"));
        assert!(err("name = A\nextensions = a\nblock_comment = /*").contains("block_comment"));
        assert!(err("name = A\nextensions = a\ncapital_types = maybe").contains("true"));
        assert!(err("name = A\nextensions = a\njust words").contains("第 3 行"));
    }

    #[test]
    fn user_languages_load_from_a_folder_and_win_over_builtin() {
        let dir = std::env::temp_dir().join(format!("feditor-syntax-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("zed.conf"), PHP_CONF).unwrap();
        // 蓋掉內建的 .lua，再放一個壞檔與一個非 .conf 的檔案
        std::fs::write(dir.join("lua.conf"), "name = MyLua\nextensions = lua\nkeywords = foo\n").unwrap();
        std::fs::write(dir.join("broken.conf"), "name = X").unwrap();
        std::fs::write(dir.join("readme.txt"), "ignored").unwrap();
        let messages = load_user_languages(Some(&dir));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(messages.len(), 1, "只有壞檔會有訊息：{messages:?}");
        assert!(messages[0].contains("broken.conf"));
        let p = |s: &str| language_for(Some(Path::new(s))).map(|l| l.name);
        assert_eq!(p("a.zz"), Some("Zed"));
        assert_eq!(p("ZZFILE"), Some("Zed"), "完整檔名也可以");
        assert_eq!(p("x.lua"), Some("MyLua"), "自訂語言優先於內建");
        assert_eq!(p("x.rs"), Some("Rust"));
    }
}
