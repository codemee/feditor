//! 快捷鍵一覽：設定面板的「快捷鍵」頁顯示內建的快捷鍵與使用者替巨集設的快捷鍵。
//!
//! 有常數的（`NEW`、`SAVE`…）直接用常數轉成文字，所以改了快捷鍵這裡自動跟著變；編輯區的按鍵（在
//! `editor/view.rs` 的按鍵對照表）沒有常數，照那張表寫在這裡。與畫面無關，方便單元測試。

use crate::macro_lib::format_shortcut;
use eframe::egui::KeyboardShortcut;
use feditor_core::i18n::t;

/// 一列：做什麼、按什麼。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub action: String,
    pub keys: String,
}

/// 一組（檔案、編輯、搜尋…）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Group {
    pub title: String,
    pub rows: Vec<Row>,
}

fn sc(shortcut: &KeyboardShortcut) -> String {
    format_shortcut(shortcut)
}

fn group(title: &str, rows: Vec<(&str, String)>) -> Group {
    Group { title: title.to_owned(), rows: rows.into_iter().map(|(action, keys)| Row { action: action.to_owned(), keys }).collect() }
}

/// 內建的快捷鍵，依用途分組（文字依目前的介面語言）。
pub fn builtin_groups() -> Vec<Group> {
    use crate::{CLOSE_TAB, FIND, FIND_IN_FILES, FIND_NEXT, FIND_PREV, NEW, NEXT_TAB, OPEN, PREV_TAB, REDO, REDO_Y, REPLACE, RERUN, SAVE, SAVE_AS, TOGGLE_PREVIEW, UNDO};
    let two = |a: &KeyboardShortcut, b: &KeyboardShortcut| format!("{} / {}", sc(a), sc(b));
    vec![
        group(
            t("檔案"),
            vec![
                (t("新增空白文件"), sc(&NEW)),
                (t("開啟檔案"), sc(&OPEN)),
                (t("儲存"), sc(&SAVE)),
                (t("另存新檔"), sc(&SAVE_AS)),
                (t("關閉分頁"), sc(&CLOSE_TAB)),
            ],
        ),
        group(
            t("分頁與終端機"),
            vec![
                (t("下一個分頁"), sc(&NEXT_TAB)),
                (t("上一個分頁"), sc(&PREV_TAB)),
                // 反引號的按鍵名稱是 "Backtick"，直接寫符號比較看得懂
                (t("在編輯區與各終端機之間切換焦點"), "Ctrl+`".to_owned()),
                (t("新增終端機"), "Ctrl+Shift+`".to_owned()),
            ],
        ),
        group(
            t("編輯"),
            vec![
                (t("復原"), sc(&UNDO)),
                (t("重做"), two(&REDO, &REDO_Y)),
                (t("剪下、複製、貼上"), "Ctrl+X / Ctrl+C / Ctrl+V".to_owned()),
                (t("全選"), "Ctrl+A".to_owned()),
                (t("選取下一個相同的字（多重游標）"), "Ctrl+D".to_owned()),
                (t("在上方、下方加一個游標"), "Ctrl+Alt+Up / Ctrl+Alt+Down".to_owned()),
                (t("上移、下移這一行"), "Alt+Up / Alt+Down".to_owned()),
                (t("複製這一行"), "Alt+Shift+Up / Alt+Shift+Down".to_owned()),
                (t("刪除這一行"), "Ctrl+Shift+K".to_owned()),
                (t("切換註解"), "Ctrl+/".to_owned()),
                (t("增加縮排"), "Ctrl+]".to_owned()),
                (t("減少縮排"), "Ctrl+[".to_owned()),
                (t("排列格式"), "Alt+Shift+F".to_owned()),
            ],
        ),
        group(
            t("搜尋"),
            vec![
                (t("尋找"), sc(&FIND)),
                (t("取代"), sc(&REPLACE)),
                (t("下一筆結果"), sc(&FIND_NEXT)),
                (t("上一筆結果"), sc(&FIND_PREV)),
                (t("在資料夾的檔案中尋找"), sc(&FIND_IN_FILES)),
            ],
        ),
        group(
            t("檢視"),
            vec![
                (t("開啟或關閉預覽"), sc(&TOGGLE_PREVIEW)),
                (t("放大字級"), "Ctrl+= / Ctrl++".to_owned()),
                (t("縮小字級"), "Ctrl+-".to_owned()),
                (t("回到預設字級"), "Ctrl+0".to_owned()),
            ],
        ),
        group(t("巨集"), vec![(t("重新執行上一個巨集"), sc(&RERUN))]),
    ]
}

/// 使用者替巨集設的快捷鍵：（巨集名稱, 適用的檔案類型, 快捷鍵, 適不適用於 `lang` 這種檔案）。
pub fn macro_rows(macros: &[crate::macro_lib::MacroEntry], lang: Option<&str>) -> Vec<(String, Vec<String>, String, bool)> {
    let mut rows: Vec<_> = macros
        .iter()
        .filter_map(|m| m.shortcut.as_ref().map(|s| (m.name.clone(), m.langs.clone(), format_shortcut(s), m.applies_to(lang))))
        .collect();
    // 適用於目前檔案的在前面，其餘依快捷鍵排
    rows.sort_by(|a, b| b.3.cmp(&a.3).then_with(|| a.2.cmp(&b.2)));
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::macro_lib::MacroEntry;
    use std::path::PathBuf;

    fn all_rows() -> Vec<Row> {
        builtin_groups().into_iter().flat_map(|g| g.rows).collect()
    }

    #[test]
    fn built_in_rows_follow_the_shortcut_constants() {
        let rows = all_rows();
        let keys_of = |action: &str| rows.iter().find(|r| r.action == action).map(|r| r.keys.clone());
        assert_eq!(keys_of("儲存").as_deref(), Some("Ctrl+S"));
        assert_eq!(keys_of("另存新檔").as_deref(), Some("Ctrl+Shift+S"));
        assert_eq!(keys_of("重做").as_deref(), Some("Ctrl+Shift+Z / Ctrl+Y"));
        assert_eq!(keys_of("下一筆結果").as_deref(), Some("F3"));
        assert_eq!(keys_of("重新執行上一個巨集").as_deref(), Some("F5"));
        assert_eq!(keys_of("在編輯區與各終端機之間切換焦點").as_deref(), Some("Ctrl+`"));
    }

    #[test]
    fn every_built_in_shortcut_that_macros_may_not_take_is_listed() {
        // 巨集不能用的內建快捷鍵（`builtin_shortcuts`）都要在一覽裡出現，免得漏列
        let listed: String = all_rows().iter().map(|r| r.keys.clone()).collect::<Vec<_>>().join(" ; ");
        for shortcut in crate::App::builtin_shortcuts() {
            let text = format_shortcut(&shortcut);
            assert!(listed.contains(&text), "一覽少了 {text}");
        }
    }

    #[test]
    fn no_row_is_empty_and_groups_are_not_empty() {
        for group in builtin_groups() {
            assert!(!group.title.is_empty() && !group.rows.is_empty(), "{group:?}");
            assert!(group.rows.iter().all(|r| !r.action.is_empty() && !r.keys.is_empty()), "{group:?}");
        }
    }

    #[test]
    fn macro_rows_list_only_macros_with_a_shortcut_and_put_the_applicable_ones_first() {
        let entry = |name: &str, shortcut: Option<&str>, langs: &[&str]| MacroEntry {
            path: PathBuf::new(),
            name: name.to_owned(),
            shortcut: shortcut.and_then(crate::macro_lib::parse_shortcut),
            langs: langs.iter().map(|s| (*s).to_owned()).collect(),
        };
        let macros = vec![
            entry("無快捷鍵", None, &[]),
            entry("Python 專用", Some("Alt+B"), &["Python"]),
            entry("通用", Some("Alt+C"), &[]),
            entry("Rust 專用", Some("Alt+A"), &["Rust"]),
        ];
        let rows = macro_rows(&macros, Some("Rust"));
        let names: Vec<&str> = rows.iter().map(|r| r.0.as_str()).collect();
        assert_eq!(names, ["Rust 專用", "通用", "Python 專用"], "適用於目前檔案的在前面，其餘依快捷鍵排");
        assert_eq!(rows.iter().map(|r| r.3).collect::<Vec<_>>(), [true, true, false]);
        assert!(macro_rows(&macros, None).iter().filter(|r| r.3).count() == 1, "純文字檔：只有通用的適用");
    }
}
