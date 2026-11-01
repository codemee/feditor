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

use eframe::egui::{Key, KeyboardShortcut, Modifiers};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct MacroEntry {
    pub path: PathBuf,
    /// 顯示名稱：`@name`，沒有就是檔名（不含副檔名）。
    pub name: String,
    pub shortcut: Option<KeyboardShortcut>,
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
                path,
            }
        })
        .collect();
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    out
}

/// 把原始碼開頭的 `@shortcut` 換成 `shortcut`（`None` 表示拿掉）。
pub fn set_header_shortcut(source: &str, shortcut: Option<&str>) -> String {
    let mut out = String::with_capacity(source.len() + 32);
    if let Some(s) = shortcut {
        out.push_str(&format!("// @shortcut {s}\n"));
    }
    let mut in_header = true;
    for line in source.split_inclusive('\n') {
        let t = line.trim();
        if in_header {
            if t.is_empty() {
                // 檔頭裡的空行照留
            } else if let Some(rest) = t.strip_prefix("//") {
                if rest.trim().starts_with("@shortcut") {
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

/// 把錄好的巨集存進資料夾（必要時建立），回傳檔案路徑。
pub fn save_new(dir: &Path, source: &str) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let path = unique_path(dir, "錄製的巨集");
    std::fs::write(&path, source)?;
    Ok(path)
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
    fn save_new_makes_unique_files() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("macros");
        let a = save_new(&sub, "1;").unwrap();
        let b = save_new(&sub, "2;").unwrap();
        assert_ne!(a, b);
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "1;");
        assert!(b.file_name().unwrap().to_string_lossy().contains("-2"));
    }
}
