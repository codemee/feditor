//! 巨集庫：使用者的巨集放在設定資料夾下的 `macros` 資料夾，每個 `.js` 檔一支巨集。
//!
//! 檔案開頭的註解可以帶兩個標記：
//!
//! ```js
//! // @name 把選取轉成大寫
//! // @shortcut Ctrl+Alt+U
//! ```
//!
//! 沒有 `@name` 就用檔名。與畫面無關，方便單元測試。
//!
//! 還可以用 `@lang` 限定巨集只適用於哪些檔案類型（狀態列「語言」下拉清單裡的名稱，逗號分隔、不分大小寫）：
//!
//! ```js
//! // @lang Rust, Python
//! ```
//!
//! 沒有 `@lang` 的巨集適用於所有檔案，所以舊的巨集不用改。

use eframe::egui::{Key, KeyboardShortcut, Modifiers};
use feditor_core::i18n::t;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct MacroEntry {
    pub path: PathBuf,
    /// 顯示名稱：`@name`，沒有就是檔名（不含副檔名）。
    pub name: String,
    pub shortcut: Option<KeyboardShortcut>,
    /// `@lang` 限定的檔案類型；空的表示適用於所有檔案。
    pub langs: Vec<String>,
}

impl MacroEntry {
    /// 這支巨集能不能用在語言是 `lang` 的檔案上（`None` 是純文字或認不出來的檔案，只有通用的巨集適用）。
    pub fn applies_to(&self, lang: Option<&str>) -> bool {
        applies(&self.langs, lang)
    }
}

/// `langs`（`@lang` 的內容，空的是所有檔案）適不適用語言 `lang`。
pub fn applies(langs: &[String], lang: Option<&str>) -> bool {
    langs.is_empty() || lang.is_some_and(|l| langs.iter().any(|x| x.eq_ignore_ascii_case(l)))
}

/// 兩支巨集的適用範圍有沒有重疊（通用的和任何範圍都重疊）：重疊時不能用同一個快捷鍵，不然同一種檔案上會同時觸發。
pub fn scopes_overlap(a: &[String], b: &[String]) -> bool {
    a.is_empty() || b.is_empty() || a.iter().any(|x| b.iter().any(|y| x.eq_ignore_ascii_case(y)))
}

/// 讀檔案開頭註解裡的 `@lang`（可以有好幾行，合併起來），逗號分隔、去掉空白與重複。
pub fn parse_langs(source: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in source.lines() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        let Some(rest) = t.strip_prefix("//") else { break };
        let Some(value) = rest.trim().strip_prefix("@lang") else { continue };
        // 只認「標記後面接空白或結尾」，免得 @language 之類被當成 @lang
        if !value.is_empty() && !value.starts_with(char::is_whitespace) {
            continue;
        }
        for name in value.split([',', '，', '、']) {
            let name = name.trim();
            if !name.is_empty() && !out.iter().any(|o| o.eq_ignore_ascii_case(name)) {
                out.push(name.to_owned());
            }
        }
    }
    out
}

/// 巨集資料夾；取不到使用者設定目錄時為 `None`。
pub fn macros_dir() -> Option<PathBuf> {
    crate::settings::Settings::default_path()
        .and_then(|p| p.parent().map(|d| d.join("macros")))
}

/// 讀檔案開頭的註解，取出 `@name` 與 `@shortcut`。
pub fn parse_header(source: &str) -> (Option<String>, Option<String>) {
    let (mut name, mut shortcut) = (None, None);
    for line in source.lines() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        let Some(rest) = t.strip_prefix("//") else { break };
        let rest = rest.trim();
        if let Some(v) = rest.strip_prefix("@name") {
            if name.is_none() && !v.trim().is_empty() {
                name = Some(v.trim().to_owned());
            }
        } else if let Some(v) = rest.strip_prefix("@shortcut") {
            if shortcut.is_none() && !v.trim().is_empty() {
                shortcut = Some(v.trim().to_owned());
            }
        }
    }
    (name, shortcut)
}

/// 「Ctrl+Alt+U」這種寫法轉成快捷鍵。至少要有一個修飾鍵（單獨的字母鍵是打字用的），也不認得的就回 `None`。
pub fn parse_shortcut(text: &str) -> Option<KeyboardShortcut> {
    let mut mods = Modifiers::NONE;
    let mut key = None;
    for part in text.split('+') {
        let p = part.trim();
        match p.to_ascii_lowercase().as_str() {
            "ctrl" | "control" | "cmd" | "command" => mods.command = true,
            "alt" | "option" => mods.alt = true,
            "shift" => mods.shift = true,
            "" => return None,
            _ => {
                if key.is_some() {
                    return None;
                }
                key = Some(Key::from_name(p)?);
            }
        }
    }
    let key = key.filter(|k| !is_modifier_key(*k))?;
    (mods.command || mods.alt || key_is_function(key)).then_some(KeyboardShortcut::new(mods, key))
}

/// 修飾鍵本身（Ctrl、Alt、Shift、Super）。egui 0.37 會把它們當成一般按鍵回報，
/// 所以設定快捷鍵時要略過，等使用者按下真正的那個鍵。
pub fn is_modifier_key(key: Key) -> bool {
    use Key::*;
    matches!(
        key,
        ShiftLeft | ShiftRight | ControlLeft | ControlRight | AltLeft | AltRight | SuperLeft | SuperRight
    )
}

fn key_is_function(key: Key) -> bool {
    use Key::*;
    matches!(
        key,
        F1 | F2 | F3 | F4 | F5 | F6 | F7 | F8 | F9 | F10 | F11 | F12
    )
}

/// 快捷鍵轉成「Ctrl+Alt+U」的寫法。
pub fn format_shortcut(s: &KeyboardShortcut) -> String {
    let mut out = String::new();
    if s.modifiers.command {
        out.push_str("Ctrl+");
    }
    if s.modifiers.alt {
        out.push_str("Alt+");
    }
    if s.modifiers.shift {
        out.push_str("Shift+");
    }
    out.push_str(s.logical_key.name());
    out
}

/// 掃描資料夾裡所有 `.js`，依顯示名稱排序。
pub fn scan(dir: &Path) -> Vec<MacroEntry> {
    let Ok(read) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<MacroEntry> = read
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("js")))
        .map(|path| {
            let stem = path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            let source = std::fs::read_to_string(&path).unwrap_or_default();
            let (name, shortcut) = parse_header(&source);
            MacroEntry {
                name: name.unwrap_or(stem),
                shortcut: shortcut.as_deref().and_then(parse_shortcut),
                langs: parse_langs(&source),
                path,
            }
        })
        .collect();
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    out
}

/// 把原始碼開頭的 `@shortcut` 換成 `shortcut`（`None` 表示拿掉）。
pub fn set_header_shortcut(source: &str, shortcut: Option<&str>) -> String {
    set_header_tag(source, "@shortcut", shortcut)
}

/// 把原始碼開頭的 `@lang` 換成 `langs`（空的表示拿掉，也就是適用於所有檔案）。
pub fn set_header_langs(source: &str, langs: &[String]) -> String {
    let joined = langs.iter().map(|l| l.replace(['\r', '\n', ','], " ")).collect::<Vec<_>>().join(", ");
    set_header_tag(source, "@lang", (!joined.trim().is_empty()).then_some(joined.as_str()))
}

/// 把原始碼開頭的 `@name` 換成 `name`（換行會變成空白）。
pub fn set_header_name(source: &str, name: &str) -> String {
    let name = name.replace(['\r', '\n'], " ");
    set_header_tag(source, "@name", Some(name.trim()))
}

/// 把檔頭註解裡的 `tag`（例如 `@name`）換成 `value`，沒有值就拿掉；新的一行放在最前面。
fn set_header_tag(source: &str, tag: &str, value: Option<&str>) -> String {
    let mut out = String::with_capacity(source.len() + 32);
    if let Some(v) = value {
        out.push_str(&format!("// {tag} {v}\n"));
    }
    let mut in_header = true;
    for line in source.split_inclusive('\n') {
        let t = line.trim();
        if in_header {
            if t.is_empty() {
                // 檔頭裡的空行照留
            } else if let Some(rest) = t.strip_prefix("//") {
                let rest = rest.trim();
                // 只認「標記後面接空白或結尾」，免得 @name 把 @namespace 也吃掉
                if rest.strip_prefix(tag).is_some_and(|r| r.is_empty() || r.starts_with(char::is_whitespace)) {
                    continue; // 舊的拿掉
                }
            } else {
                in_header = false;
            }
        }
        out.push_str(line);
    }
    out
}

/// 由顯示名稱做出安全的檔名主體：換掉 Windows／macOS／Linux 不能用的字元與結尾的點、空白，
/// 太長的截斷，空的或是 Windows 保留字（CON、NUL…）就補上預設名稱。
pub fn file_stem_for(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| if c.is_control() || "\\/:*?\"<>|".contains(c) { '_' } else { c })
        .collect();
    let cleaned: String = cleaned.trim().trim_end_matches(['.', ' ']).chars().take(60).collect();
    let upper = cleaned.to_ascii_uppercase();
    let reserved = matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (upper.len() == 4
            && (upper.starts_with("COM") || upper.starts_with("LPT"))
            && upper.ends_with(|c: char| c.is_ascii_digit()));
    if cleaned.is_empty() {
        t("錄製的巨集").to_owned()
    } else if reserved {
        format!("{cleaned}_")
    } else {
        cleaned
    }
}

/// 在 `dir` 底下找一個還沒被用的檔名：`base.js`、`base-2.js`…
pub fn unique_path(dir: &Path, base: &str) -> PathBuf {
    let first = dir.join(format!("{base}.js"));
    if !first.exists() {
        return first;
    }
    (2..)
        .map(|n| dir.join(format!("{base}-{n}.js")))
        .find(|p| !p.exists())
        .expect("檔名一定找得到")
}

/// 把錄好的巨集以 `name` 為名存進資料夾（必要時建立），回傳檔案路徑。
/// 檔案開頭寫入 `// @name`，所以巨集面板顯示的就是 `name` 原樣（含空白與中文）；檔名則用安全的版本，
/// 重複時自動加 `-2`、`-3`。`langs` 不是空的就同時寫入 `// @lang`（只適用於那些檔案類型）。
pub fn save_named(dir: &Path, name: &str, source: &str, langs: &[String]) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let path = unique_path(dir, &file_stem_for(name));
    // 先寫 @lang 再寫 @name：新的標記放在最前面，所以名稱在最上面、適用範圍在它下面
    std::fs::write(&path, set_header_name(&set_header_langs(source, langs), name))?;
    Ok(path)
}

/// 重新命名巨集：改寫檔案開頭的 `@name`（檔名不動，快捷鍵等其他標記保留）。
pub fn rename(path: &Path, new_name: &str) -> std::io::Result<()> {
    let source = std::fs::read_to_string(path)?;
    std::fs::write(path, set_header_name(&source, new_name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_reads_name_and_shortcut_from_leading_comments_only() {
        let src = "// @name 轉大寫\n// 其他說明\n// @shortcut Ctrl+Alt+U\nconst x = 1;\n// @name 不算\n";
        assert_eq!(
            parse_header(src),
            (Some("轉大寫".to_owned()), Some("Ctrl+Alt+U".to_owned()))
        );
        assert_eq!(parse_header("editor.insert(0,'x');"), (None, None));
        assert_eq!(parse_header("\n\n// @name  A \n"), (Some("A".to_owned()), None));
        assert_eq!(parse_header("// @name\n"), (None, None), "空的標記忽略");
    }

    #[test]
    fn shortcut_round_trips() {
        let s = parse_shortcut("Ctrl+Alt+U").unwrap();
        assert!(s.modifiers.command && s.modifiers.alt && !s.modifiers.shift);
        assert_eq!(s.logical_key, Key::U);
        assert_eq!(format_shortcut(&s), "Ctrl+Alt+U");
        let s = parse_shortcut("ctrl + shift + 1").unwrap();
        assert_eq!(format_shortcut(&s), "Ctrl+Shift+1");
        let s = parse_shortcut("F6").unwrap();
        assert_eq!(format_shortcut(&s), "F6");
        for t in ["U", "Shift+U", "Ctrl+", "Ctrl+Alt", "Ctrl+Foo", "Ctrl+A+B", "", "Ctrl+ControlLeft"] {
            assert!(parse_shortcut(t).is_none(), "{t:?}");
        }
    }

    #[test]
    fn set_header_shortcut_replaces_adds_and_removes() {
        let src = "// @name A\n// @shortcut Ctrl+1\nbody();\n";
        assert_eq!(
            set_header_shortcut(src, Some("Ctrl+2")),
            "// @shortcut Ctrl+2\n// @name A\nbody();\n"
        );
        assert_eq!(set_header_shortcut(src, None), "// @name A\nbody();\n");
        assert_eq!(set_header_shortcut("body();\n", Some("F6")), "// @shortcut F6\nbody();\n");
        // 程式碼裡面的同樣文字不能被動到
        let tricky = "run();\n// @shortcut Ctrl+9\n";
        assert_eq!(set_header_shortcut(tricky, None), tricky);
    }

    #[test]
    fn scan_lists_js_files_sorted_with_names_and_shortcuts() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("b.js"), "// @shortcut Ctrl+Alt+B\n1;").unwrap();
        std::fs::write(dir.path().join("a.js"), "// @name Zeta\n1;").unwrap();
        std::fs::write(dir.path().join("c.JS"), "1;").unwrap();
        std::fs::write(dir.path().join("note.txt"), "x").unwrap();
        let list = scan(dir.path());
        let names: Vec<&str> = list.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, vec!["b", "c", "Zeta"]);
        assert_eq!(format_shortcut(&list[0].shortcut.unwrap()), "Ctrl+Alt+B");
        assert!(list[1].shortcut.is_none());
        assert!(scan(&dir.path().join("missing")).is_empty());
    }

    #[test]
    fn save_named_writes_the_name_header_and_makes_unique_safe_file_names() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("macros");
        let a = save_named(&sub, "轉換: 大寫/小寫?", "1;", &[]).unwrap();
        let b = save_named(&sub, "轉換: 大寫/小寫?", "2;", &[]).unwrap();
        assert_ne!(a, b);
        assert_eq!(a.file_name().unwrap().to_string_lossy(), "轉換_ 大寫_小寫_.js");
        assert!(b.file_name().unwrap().to_string_lossy().contains("-2"));
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "// @name 轉換: 大寫/小寫?\n1;");
        // 面板顯示的是原樣的名稱，不是被清理過的檔名
        let list = scan(&sub);
        assert!(list.iter().all(|m| m.name == "轉換: 大寫/小寫?"));
    }

    #[test]
    fn rename_rewrites_only_the_name_tag_and_keeps_the_shortcut() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("m.js");
        std::fs::write(&p, "// @name 舊名\n// @shortcut Ctrl+Alt+K\n// @namespace x\nrun();\n").unwrap();
        rename(&p, "新名\n稱").unwrap();
        let text = std::fs::read_to_string(&p).unwrap();
        assert_eq!(text, "// @name 新名 稱\n// @shortcut Ctrl+Alt+K\n// @namespace x\nrun();\n");
        let list = scan(dir.path());
        assert_eq!(list[0].name, "新名 稱");
        assert!(list[0].shortcut.is_some());
        // 沒有 @name 的巨集（例如以前錄的）也能命名
        std::fs::write(&p, "run();\n").unwrap();
        rename(&p, "A").unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "// @name A\nrun();\n");
        assert!(rename(&dir.path().join("missing.js"), "x").is_err());
    }

    fn langs(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn lang_tag_is_read_from_the_header_only_and_split_on_commas() {
        assert_eq!(parse_langs("// @name A\n// @lang Rust, python ,  Go\nbody();\n"), langs(&["Rust", "python", "Go"]));
        assert_eq!(parse_langs("// @lang Rust\n// @lang rust，Go、Lua\n1;"), langs(&["Rust", "Go", "Lua"]), "多行合併、全形逗號與頓號、不分大小寫去重複");
        assert!(parse_langs("1;\n// @lang Rust\n").is_empty(), "程式碼後面的不算");
        assert!(parse_langs("// @lang\n// @language Rust\n// @langs Rust\n1;").is_empty(), "空的、別的標記都不算");
        assert!(parse_langs("// @name A\n").is_empty(), "沒有標記就是所有檔案");
    }

    #[test]
    fn a_macro_without_lang_applies_everywhere_and_one_with_lang_only_to_those_types() {
        let entry = |l: &[&str]| MacroEntry { path: PathBuf::new(), name: String::new(), shortcut: None, langs: langs(l) };
        assert!(entry(&[]).applies_to(Some("Rust")) && entry(&[]).applies_to(None), "通用的：什麼檔案都適用，包含純文字");
        let rust = entry(&["Rust", "Python"]);
        assert!(rust.applies_to(Some("rust")) && rust.applies_to(Some("Python")), "不分大小寫");
        assert!(!rust.applies_to(Some("Go")) && !rust.applies_to(None), "別的類型與純文字不適用");
    }

    #[test]
    fn scopes_overlap_when_either_is_universal_or_they_share_a_type() {
        assert!(scopes_overlap(&[], &[]) && scopes_overlap(&[], &langs(&["Rust"])) && scopes_overlap(&langs(&["Go"]), &[]));
        assert!(scopes_overlap(&langs(&["Rust", "Go"]), &langs(&["go"])));
        assert!(!scopes_overlap(&langs(&["Rust"]), &langs(&["Python", "Go"])), "沒有共同的類型：可以共用快捷鍵");
    }

    #[test]
    fn set_header_langs_replaces_adds_and_removes_and_save_named_writes_it_under_the_name() {
        let src = "// @name A\n// @lang Rust\nbody();\n";
        assert_eq!(set_header_langs(src, &langs(&["Go", "Lua"])), "// @lang Go, Lua\n// @name A\nbody();\n");
        assert_eq!(set_header_langs(src, &[]), "// @name A\nbody();\n", "空的就拿掉，回到所有檔案");
        assert_eq!(set_header_langs("body();\n", &langs(&["Rust"])), "// @lang Rust\nbody();\n");
        assert_eq!(set_header_langs("body();\n", &langs(&["a,b\nc"])), "// @lang a b c\nbody();\n", "名稱裡的逗號與換行不能破壞格式");
        let dir = tempfile::tempdir().unwrap();
        let path = save_named(dir.path(), "加註解", "1;", &langs(&["Rust"])).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "// @name 加註解\n// @lang Rust\n1;");
        let entry = &scan(dir.path())[0];
        assert_eq!((entry.name.as_str(), entry.langs.as_slice()), ("加註解", langs(&["Rust"]).as_slice()));
        // 改名、設快捷鍵都保留適用範圍
        rename(&path, "新名").unwrap();
        std::fs::write(&path, set_header_shortcut(&std::fs::read_to_string(&path).unwrap(), Some("Ctrl+Alt+K"))).unwrap();
        let entry = &scan(dir.path())[0];
        assert_eq!((entry.name.as_str(), entry.langs.as_slice()), ("新名", langs(&["Rust"]).as_slice()));
        assert!(entry.shortcut.is_some());
    }

    #[test]
    fn file_stems_are_safe_on_every_platform() {
        assert_eq!(file_stem_for("  a:b*c  "), "a_b_c");
        assert_eq!(file_stem_for("結尾是點..."), "結尾是點");
        assert_eq!(file_stem_for(""), "錄製的巨集");
        assert_eq!(file_stem_for("   "), "錄製的巨集");
        assert_eq!(file_stem_for("con"), "con_");
        assert_eq!(file_stem_for("LPT1"), "LPT1_");
        assert_eq!(file_stem_for("console"), "console");
        assert_eq!(file_stem_for(&"長".repeat(100)).chars().count(), 60);
    }
}
