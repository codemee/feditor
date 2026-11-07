//! 介面語言：程式裡的介面字串一律以「中文原文」當 key，英文從對照表查。
//!
//! - `t("中文")`：靜態字串。英文模式查表，查不到就退回中文原文。
//! - `tf!("已取代 {n} 筆", n = n)`：有參數的字串。`{名稱}` 會被換成對應的值（英文模式下用英文範本）。
//!
//! 目前語言放在全域，切換語言後下一次繪製就會生效，不用重開程式。

use std::collections::HashMap;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU8, Ordering};

mod en;

/// 使用者選的語言。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum LangChoice {
    /// 跟隨作業系統的語言（中文系統用中文，其餘用英文）。
    #[default]
    System,
    Chinese,
    English,
}

impl LangChoice {
    pub const ALL: [LangChoice; 3] = [LangChoice::System, LangChoice::Chinese, LangChoice::English];

    /// 點一下按鈕時的下一個選項：跟隨系統 → 中文 → English → 跟隨系統。
    pub fn next(self) -> Self {
        match self {
            LangChoice::System => LangChoice::Chinese,
            LangChoice::Chinese => LangChoice::English,
            LangChoice::English => LangChoice::System,
        }
    }

    /// 存進設定檔的值。
    pub fn key(self) -> &'static str {
        match self {
            LangChoice::System => "system",
            LangChoice::Chinese => "zh",
            LangChoice::English => "en",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim_matches('"').trim().to_ascii_lowercase().as_str() {
            "system" => Some(LangChoice::System),
            "zh" | "zh-tw" | "zh-hant" | "chinese" => Some(LangChoice::Chinese),
            "en" | "english" => Some(LangChoice::English),
            _ => None,
        }
    }

    /// 選項本身的顯示名稱：語言名稱用各自的語言寫，這樣在看不懂的介面裡也找得到。
    pub fn label(self) -> &'static str {
        match self {
            LangChoice::System => t("跟隨系統"),
            LangChoice::Chinese => "中文",
            LangChoice::English => "English",
        }
    }
}

/// 實際生效的語言。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    Chinese,
    English,
}

static CURRENT: AtomicU8 = AtomicU8::new(0);

/// 系統語言標籤（例如 `zh-TW`、`en-US`）是不是中文。
pub fn is_chinese_locale(tag: &str) -> bool {
    tag.to_ascii_lowercase().starts_with("zh")
}

/// 依選擇算出實際語言；`system_locale` 是作業系統回報的語言標籤（取不到是 `None`）。
pub fn resolve(choice: LangChoice, system_locale: Option<&str>) -> Lang {
    match choice {
        LangChoice::Chinese => Lang::Chinese,
        LangChoice::English => Lang::English,
        LangChoice::System => match system_locale {
            Some(tag) if is_chinese_locale(tag) => Lang::Chinese,
            _ => Lang::English,
        },
    }
}

/// 套用使用者的選擇（查作業系統語言並設成全域目前語言）。
pub fn apply(choice: LangChoice) {
    let lang = resolve(choice, sys_locale::get_locale().as_deref());
    set(lang);
}

pub fn set(lang: Lang) {
    CURRENT.store(if lang == Lang::English { 1 } else { 0 }, Ordering::Relaxed);
}

pub fn current() -> Lang {
    if CURRENT.load(Ordering::Relaxed) == 1 { Lang::English } else { Lang::Chinese }
}

pub fn is_english() -> bool {
    current() == Lang::English
}

fn table() -> &'static HashMap<&'static str, &'static str> {
    static TABLE: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    TABLE.get_or_init(|| en::ENGLISH.iter().copied().collect())
}

/// 取得目前語言的字串。中文模式原樣回傳；英文模式查表，沒有翻譯就退回中文。
pub fn t(zh: &'static str) -> &'static str {
    translate(current(), zh)
}

/// 指定語言的字串（不看全域設定，測試用）。
pub fn translate(lang: Lang, zh: &'static str) -> &'static str {
    if lang == Lang::English { table().get(zh).copied().unwrap_or(zh) } else { zh }
}

/// 把範本裡的 `{名稱}` 換成對應的值。沒有對應的名稱就原樣保留。
pub fn fill(template: &str, args: &[(&str, String)]) -> String {
    let mut out = String::with_capacity(template.len() + 16);
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        match after.find('}') {
            Some(close) => {
                let name = &after[..close];
                match args.iter().find(|(k, _)| *k == name) {
                    Some((_, v)) => out.push_str(v),
                    None => {
                        out.push('{');
                        out.push_str(name);
                        out.push('}');
                    }
                }
                rest = &after[close + 1..];
            }
            None => {
                out.push_str(&rest[open..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

/// 依序把範本裡的 `{}` 換成 `args`。多出來的 `{}` 原樣保留。
pub fn fill_positional(template: &str, args: &[String]) -> String {
    let mut out = String::with_capacity(template.len() + 16);
    let mut next = args.iter();
    let mut rest = template;
    while let Some(at) = rest.find("{}") {
        out.push_str(&rest[..at]);
        match next.next() {
            Some(v) => out.push_str(v),
            None => out.push_str("{}"),
        }
        rest = &rest[at + 2..];
    }
    out.push_str(rest);
    out
}

/// 有參數的介面字串，兩種寫法：
/// - 具名：`tf!("已取代 {n} 筆", n = count)`
/// - 依序：`tf!("「{}」已儲存", name)`
#[macro_export]
macro_rules! tf {
    ($zh:literal $(, $k:ident = $v:expr)+ $(,)?) => {
        $crate::i18n::fill($crate::i18n::t($zh), &[$((stringify!($k), ($v).to_string())),+])
    };
    ($zh:literal, $($v:expr),+ $(,)?) => {
        $crate::i18n::fill_positional($crate::i18n::t($zh), &[$(($v).to_string()),+])
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_follows_system_only_when_asked() {
        assert_eq!(resolve(LangChoice::Chinese, Some("en-US")), Lang::Chinese);
        assert_eq!(resolve(LangChoice::English, Some("zh-TW")), Lang::English);
        assert_eq!(resolve(LangChoice::System, Some("zh-TW")), Lang::Chinese);
        assert_eq!(resolve(LangChoice::System, Some("zh-Hant-TW")), Lang::Chinese);
        assert_eq!(resolve(LangChoice::System, Some("ZH-CN")), Lang::Chinese);
        assert_eq!(resolve(LangChoice::System, Some("ja-JP")), Lang::English);
        assert_eq!(resolve(LangChoice::System, Some("en-US")), Lang::English);
        assert_eq!(resolve(LangChoice::System, None), Lang::English, "取不到就用英文");
    }

    #[test]
    fn next_cycles_through_all_three_choices() {
        let mut c = LangChoice::System;
        let mut seen = vec![c];
        for _ in 0..3 {
            c = c.next();
            seen.push(c);
        }
        assert_eq!(
            seen,
            [LangChoice::System, LangChoice::Chinese, LangChoice::English, LangChoice::System]
        );
    }

    #[test]
    fn choice_keys_round_trip_and_reject_unknown() {
        for c in LangChoice::ALL {
            assert_eq!(LangChoice::parse(c.key()), Some(c));
        }
        assert_eq!(LangChoice::parse("\"EN\""), Some(LangChoice::English));
        assert_eq!(LangChoice::parse("klingon"), None);
    }

    #[test]
    fn fill_replaces_named_placeholders() {
        let args = [("n", "3".to_owned()), ("name", "a.txt".to_owned())];
        assert_eq!(fill("已取代 {n} 筆", &args), "已取代 3 筆");
        assert_eq!(fill("{name}：{n}", &args), "a.txt：3");
        assert_eq!(fill("{missing} 與 {n}", &args), "{missing} 與 3", "沒有對應的名稱原樣保留");
        assert_eq!(fill("沒有大括號", &args), "沒有大括號");
        assert_eq!(fill("未結束的 {n", &args), "未結束的 {n");
    }

    #[test]
    fn tf_macro_supports_named_and_positional_arguments() {
        set(Lang::Chinese);
        assert_eq!(tf!("已取代 {n} 筆", n = 3), "已取代 3 筆");
        assert_eq!(tf!("「{}」有 {} 個", "a.txt", 2), "「a.txt」有 2 個");
        assert_eq!(fill_positional("{} 與 {}", &["x".to_owned()]), "x 與 {}");
    }

    /// 掃描所有 crate 的原始碼，找出 `t("…")`、`tf!("…")` 用到的中文原文（測試區塊與註解不算）。
    fn used_keys() -> std::collections::BTreeMap<String, String> {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let call = regex::Regex::new(r#"\b(?:t|tf!)\(\s*"((?:[^"\\]|\\.)*)""#).unwrap();
        let mut found = std::collections::BTreeMap::new();
        let mut stack: Vec<std::path::PathBuf> = ["ui", "core", "script", "script-js"]
            .iter()
            .map(|d| root.join(d).join("src"))
            .collect();
        while let Some(path) = stack.pop() {
            if path.is_dir() {
                stack.extend(std::fs::read_dir(&path).unwrap().map(|e| e.unwrap().path()));
                continue;
            }
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            if path.extension().is_none_or(|e| e != "rs") || name == "en.rs" || name == "i18n.rs" {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap();
            // 只看測試模組之前的程式碼（有些檔案的 #[cfg(test)] 會標在單一常數上，所以要連 mod tests 一起比對）
            let test_mod = regex::Regex::new(r"#\[cfg\(test\)\]\s*mod tests").unwrap();
            let body = test_mod.split(&text).next().unwrap();
            let code: String = body
                .lines()
                .filter(|l| !l.trim_start().starts_with("//"))
                .collect::<Vec<_>>()
                .join("\n");
            for m in call.captures_iter(&code) {
                let key = m[1]
                    .replace("\\\\", "\u{0}")
                    .replace("\\n", "\n")
                    .replace("\\t", "\t")
                    .replace("\\\"", "\"")
                    .replace('\u{0}', "\\");
                found.entry(key).or_insert_with(|| name.clone());
            }
        }
        found
    }

    #[test]
    fn every_translatable_string_in_the_source_has_an_english_entry() {
        let keys = used_keys();
        assert!(keys.len() > 200, "掃描到的字串太少（{}），掃描路徑可能不對", keys.len());
        let missing: Vec<String> = keys
            .iter()
            .filter(|(k, _)| !table().contains_key(k.as_str()))
            .map(|(k, f)| format!("{f}: {k}"))
            .collect();
        assert!(missing.is_empty(), "缺少英文翻譯：\n{}", missing.join("\n"));
    }

    #[test]
    fn english_table_has_no_unused_entries() {
        let keys = used_keys();
        let unused: Vec<&str> = en::ENGLISH
            .iter()
            .map(|(k, _)| *k)
            .filter(|k| !keys.contains_key(*k))
            .collect();
        assert!(unused.is_empty(), "對照表有沒用到的字串：\n{}", unused.join("\n"));
    }

    #[test]
    fn english_mode_translates_and_chinese_mode_does_not() {
        assert_eq!(translate(Lang::English, "取消"), "Cancel");
        assert_eq!(translate(Lang::Chinese, "取消"), "取消");
        assert_eq!(translate(Lang::English, "沒有這個字串"), "沒有這個字串", "沒有翻譯就退回中文");
    }

    #[test]
    fn table_has_no_duplicate_keys_and_keeps_placeholders() {
        let mut seen = std::collections::HashSet::new();
        for (zh, en) in en::ENGLISH {
            assert!(seen.insert(*zh), "重複的 key：{zh}");
            assert!(!en.is_empty(), "英文是空的：{zh}");
            let names = |s: &str| {
                // `${名稱}` 是給使用者看的規則表達式語法，不是這裡的參數
                let s = s.replace("${", "");
                let mut v: Vec<String> = s
                    .split('{')
                    .skip(1)
                    .filter_map(|p| p.split_once('}').map(|(n, _)| n.to_owned()))
                    .collect();
                v.sort();
                v
            };
            assert_eq!(names(zh), names(en), "參數名稱要一致：{zh}");
        }
    }

    /// 全形的 ＋ － ／ ＝ 在 Windows 的微軟正黑體裡有，但 Linux 上後備的中文字型常常沒有，介面就會出現方框（缺字）。
    /// 介面文字一律用半形的 + - / =，任何字型都畫得出來。
    #[test]
    fn interface_strings_avoid_fullwidth_math_symbols_that_linux_fonts_often_lack() {
        for (zh, en) in en::ENGLISH {
            for text in [zh, en] {
                if let Some(c) = text.chars().find(|c| "＋－／＝".contains(*c)) {
                    panic!("介面文字不要用全形的「{c}」（有些字型沒有這個字）：{zh}");
                }
            }
        }
    }
}
