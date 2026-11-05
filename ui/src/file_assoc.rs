//! 整合系統的「預設開啟」：雙按某一類檔案時，直接用 FEditor 開啟。
//!
//! 檔案分成幾類（程式碼、設定檔、標記語言、純文字），使用者在設定面板勾選要讓 FEditor 接手的類別。
//!
//! - **Windows**：寫在 `HKEY_CURRENT_USER`（不需要系統管理員權限）。登錄 FEditor 的檔案類型（ProgID），
//!   每個副檔名的「開啟方式」清單加上 FEditor，並在「預設應用程式」登記 FEditor 能開這些副檔名；
//!   還沒有預設程式的副檔名，直接把預設設成 FEditor。**使用者已經自己選過預設程式的副檔名，系統（UserChoice 有雜湊保護）
//!   不允許程式改掉**，這些副檔名會列在 [`Outcome::blocked`]，由呼叫端提示使用者到系統的「預設應用程式」自己確認。
//! - **macOS**：用 Launch Services 把這些副檔名對應的內容類型（UTI）的預設處理程式設成 FEditor，
//!   原本的處理程式記在備份檔裡，取消時還原。FEditor 必須是 `.app`（打包出來的 `FEditor.app`）才登記得到。
//! - **Linux**：寫 `~/.local/share/applications/feditor.desktop`，並在 `~/.config/mimeapps.list` 的
//!   `[Default Applications]` 把這些 MIME 類型的預設程式排成 FEditor 在最前面（原本的留在後面，取消時拿掉就還原）。
//!
//! 預設不會碰 `.bat`、`.cmd` 這類「雙按就是執行」的檔案。

// 各平台用得到的部分不同，但內容產生的邏輯每個平台都編譯（為了測試）
#![allow(dead_code)]

use feditor_core::i18n::t;
use std::collections::BTreeSet;
use std::path::Path;

/// 一類檔案。
pub struct Group {
    /// 設定檔裡的名稱。
    pub key: &'static str,
    /// 副檔名（小寫、不含點）。
    pub extensions: &'static [&'static str],
    /// Linux 用的 MIME 類型。
    pub mime_types: &'static [&'static str],
    /// 第一次開啟這個功能時，預設有沒有勾。
    pub default_on: bool,
}

impl Group {
    /// 顯示在設定面板的名稱。
    pub fn label(&self) -> &'static str {
        match self.key {
            "code" => t("程式碼"),
            "config" => t("設定檔"),
            "markup" => t("標記語言（HTML、XML、Markdown）"),
            _ => t("純文字（.txt、.log、.csv）"),
        }
    }
}

pub const GROUPS: &[Group] = &[
    Group {
        key: "code",
        extensions: &[
            "py", "pyw", "rs", "c", "h", "cc", "cpp", "cxx", "hpp", "hh", "ino", "js", "mjs", "cjs", "jsx", "ts", "tsx",
            "java", "cs", "kt", "kts", "scala", "dart", "swift", "go", "lua", "sql", "sh", "bash", "zsh", "ps1", "psm1",
            "css", "scss", "less", "slint",
        ],
        mime_types: &[
            "text/x-python", "text/x-python3", "text/rust", "text/x-rust", "text/x-csrc", "text/x-chdr", "text/x-c++src",
            "text/x-c++hdr", "text/x-java", "text/javascript", "application/javascript", "text/x-javascript",
            "application/typescript", "text/x-typescript", "text/x-go", "text/x-lua", "text/x-sql", "application/sql",
            "application/x-shellscript", "text/x-shellscript", "application/x-powershell", "text/css", "text/x-scss",
            "text/x-less", "text/x-csharp", "text/x-kotlin", "text/x-scala", "text/x-dart", "text/x-swift",
        ],
        default_on: true,
    },
    Group {
        key: "config",
        extensions: &["json", "jsonc", "toml", "ini", "cfg", "conf", "yml", "yaml", "properties"],
        mime_types: &[
            "application/json", "application/toml", "text/x-toml", "application/x-yaml", "application/yaml", "text/yaml",
            "text/x-ini", "application/x-wine-extension-ini", "text/x-properties", "text/x-java-properties",
        ],
        default_on: true,
    },
    Group {
        key: "markup",
        extensions: &["html", "htm", "xml", "svg", "xaml", "vue", "md", "markdown"],
        mime_types: &[
            "text/html", "application/xhtml+xml", "application/xml", "text/xml", "image/svg+xml", "text/markdown",
            "text/x-markdown",
        ],
        default_on: false,
    },
    Group {
        key: "text",
        extensions: &["txt", "log", "csv", "tsv"],
        mime_types: &["text/plain", "text/csv", "text/tab-separated-values", "text/x-log"],
        default_on: false,
    },
];

/// 第一次開啟這個功能時，預設勾選的類別。
pub fn default_groups() -> BTreeSet<String> {
    GROUPS.iter().filter(|g| g.default_on).map(|g| g.key.to_owned()).collect()
}

/// 選到的類別的所有副檔名（不含點）。不認得的類別名稱略過。
pub fn extensions_for(selected: &BTreeSet<String>) -> Vec<&'static str> {
    GROUPS.iter().filter(|g| selected.contains(g.key)).flat_map(|g| g.extensions.iter().copied()).collect()
}

/// 所有類別的所有副檔名。
pub fn all_extensions() -> Vec<&'static str> {
    GROUPS.iter().flat_map(|g| g.extensions.iter().copied()).collect()
}

/// 選到的類別的所有 MIME 類型。
pub fn mime_types_for(selected: &BTreeSet<String>) -> Vec<&'static str> {
    GROUPS.iter().filter(|g| selected.contains(g.key)).flat_map(|g| g.mime_types.iter().copied()).collect()
}

pub fn all_mime_types() -> Vec<&'static str> {
    GROUPS.iter().flat_map(|g| g.mime_types.iter().copied()).collect()
}

/// 套用之後的結果。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Outcome {
    /// 沒辦法由程式設成預設的副檔名（Windows：使用者已經自己選過預設程式；macOS：系統拒絕）。
    /// 要請使用者到系統的設定自己確認。
    pub blocked: Vec<String>,
}

/// 這個系統支不支援。
pub fn supported() -> bool {
    cfg!(any(windows, target_os = "macos", target_os = "linux"))
}

/// 讓 `selected` 這些類別的檔案預設用 FEditor 開啟（`on` 為 `false` 就全部還原）；
/// 沒選到的類別（以前選過的）會一併還原。重複呼叫沒有副作用，所以程式搬了位置也可以再呼叫一次更新。
pub fn apply(selected: &BTreeSet<String>, on: bool) -> Result<Outcome, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let selected: BTreeSet<String> = if on { selected.clone() } else { BTreeSet::new() };
    imp::apply(&exe, &selected)
}

/// 備份檔放在設定資料夾（macOS 記原本的處理程式用）。
fn backup_path() -> Option<std::path::PathBuf> {
    crate::settings::Settings::default_path().and_then(|p| p.parent().map(|d| d.join("file-assoc-backup.txt")))
}

// ───────── Linux：.desktop 與 mimeapps.list 的內容（純文字邏輯，每個平台都能測） ─────────

/// 桌面項目的檔名。
pub const DESKTOP_FILE: &str = "feditor.desktop";

/// `.desktop` 的 `Exec` 欄位裡的引號：用雙引號包起來，裡面的 `"` `` ` `` `$` `\` 前面加反斜線。
fn desktop_quote(text: &str) -> String {
    let mut out = String::from("\"");
    for c in text.chars() {
        if matches!(c, '"' | '`' | '$' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
    out
}

/// `feditor.desktop` 的內容：能開所有類別的 MIME 類型（所以「開啟方式」清單裡找得到）。
pub fn desktop_entry(exe: &Path) -> String {
    let mimes: String = all_mime_types().iter().map(|m| format!("{m};")).collect();
    format!(
        "[Desktop Entry]\nType=Application\nName=FEditor\nGenericName=Text Editor\nComment=Lightweight cross-platform text editor\n\
         Exec={} %F\nTerminal=false\nCategories=Utility;TextEditor;\nMimeType={mimes}\n",
        desktop_quote(&exe.to_string_lossy())
    )
}

const DEFAULT_SECTION: &str = "[Default Applications]";

/// 把 `mimeapps.list` 的 `[Default Applications]` 裡這些 MIME 類型的預設程式排成 `desktop` 在最前面
/// （原本的留在後面當備案）；本來就排第一的不動。
pub fn set_default_apps(text: &str, mimes: &[&str], desktop: &str) -> String {
    edit_default_apps(text, mimes, |current| match current {
        Some(v) if v.split(';').next() == Some(desktop) => Some(v.to_owned()),
        Some(v) if !v.trim().is_empty() => Some(format!("{desktop};{}", v.trim_start_matches(';'))),
        _ => Some(format!("{desktop};")),
    })
}

/// 拿掉 `desktop` 排在最前面的這個安排（還原成原本的）；拿掉之後沒東西了就整行刪掉。
pub fn clear_default_apps(text: &str, mimes: &[&str], desktop: &str) -> String {
    edit_default_apps(text, mimes, |current| {
        let v = current?;
        match v.strip_prefix(desktop) {
            // 不是 FEditor 排在最前面（使用者自己改過、或根本不是我們設的）：不動
            None => Some(v.to_owned()),
            Some(rest) => {
                let rest = rest.trim_start_matches(';');
                (!rest.trim().is_empty()).then(|| rest.to_owned())
            }
        }
    })
}

/// 逐一處理 `[Default Applications]` 裡這些 MIME 類型那一行：`change` 收到現在的值（沒有這一行是 `None`），
/// 回傳新的值（`None` 表示不要這一行）。其他段落與行原樣保留；沒有這個段落時，需要新增才建立。
fn edit_default_apps(text: &str, mimes: &[&str], change: impl Fn(Option<&str>) -> Option<String>) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let new_lines = |handled: &[&str]| -> Vec<String> {
        mimes.iter().filter(|m| !handled.contains(m)).filter_map(|m| change(None).map(|v| format!("{m}={v}"))).collect()
    };
    let mut out: Vec<String> = Vec::new();
    match lines.iter().position(|l| l.trim() == DEFAULT_SECTION) {
        None => {
            out.extend(lines.iter().map(|l| (*l).to_owned()));
            let adds = new_lines(&[]);
            if !adds.is_empty() {
                if out.last().is_some_and(|l| !l.trim().is_empty()) {
                    out.push(String::new());
                }
                out.push(DEFAULT_SECTION.to_owned());
                out.extend(adds);
            }
        }
        Some(start) => {
            let end = lines[start + 1..].iter().position(|l| l.trim_start().starts_with('[')).map_or(lines.len(), |p| start + 1 + p);
            out.extend(lines[..=start].iter().map(|l| (*l).to_owned()));
            let mut handled: Vec<&str> = Vec::new();
            for line in &lines[start + 1..end] {
                if let Some((key, value)) = line.split_once('=')
                    && mimes.contains(&key.trim())
                {
                    handled.push(key.trim());
                    if let Some(new) = change(Some(value.trim())) {
                        out.push(format!("{}={new}", key.trim()));
                    }
                    continue; // `None`：這一行不要了
                }
                out.push((*line).to_owned());
            }
            // 新增的放在段落最後一個有內容的行後面（段落尾巴的空行是和下一段的分隔）
            let mut at = out.len();
            while at > start + 1 && out[at - 1].trim().is_empty() {
                at -= 1;
            }
            let adds = new_lines(&handled);
            out.splice(at..at, adds);
            out.extend(lines[end..].iter().map(|l| (*l).to_owned()));
        }
    }
    let mut result = out.join("\n");
    if !result.is_empty() {
        result.push('\n');
    }
    result
}
// ───────── 各平台的實作 ─────────

#[cfg(windows)]
mod imp {
    use super::*;
    use feditor_core::tf;
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND, ERROR_SUCCESS};
    use windows_sys::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_WRITE, REG_OPTION_NON_VOLATILE, REG_SZ, RRF_RT_REG_SZ, RegCloseKey, RegCreateKeyExW,
        RegDeleteKeyValueW, RegDeleteTreeW, RegGetValueW, RegSetValueExW,
    };
    use windows_sys::Win32::UI::Shell::{SHCNE_ASSOCCHANGED, SHCNF_IDLIST, SHChangeNotify};

    /// 登錄用的名稱（測試用另一組，才不會動到使用者真正的設定）。
    pub struct Names {
        /// 檔案類型（ProgID）。
        pub progid: &'static str,
        /// `HKCU\Software` 底下放能力宣告的機碼。
        pub app_key: &'static str,
        /// 在 `RegisteredApplications` 裡的值名稱。
        pub registered: &'static str,
    }

    pub const NAMES: Names = Names { progid: "FEditor.File", app_key: r"Software\FEditor", registered: "FEditor" };

    /// 顯示用的文字（由呼叫端翻譯好）。
    pub struct Texts {
        pub type_name: String,
        pub description: String,
    }

    const CLASSES: &str = r"Software\Classes";
    const SOFTWARE: &str = "Software";

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn check(code: u32, what: &str) -> Result<(), String> {
        if code == ERROR_SUCCESS { Ok(()) } else { Err(format!("{what}（錯誤碼 {code}）")) }
    }

    fn full(base: &str, key: &str) -> Vec<u16> {
        wide(&if key.is_empty() { base.to_owned() } else { format!(r"{base}\{key}") })
    }

    /// 寫字串值到 `HKCU\<base>\<key>`（`name` 空字串是預設值）。
    fn set(base: &str, key: &str, name: &str, data: &str) -> Result<(), String> {
        let path = full(base, key);
        let mut hkey: HKEY = null_mut();
        // SAFETY: 參數都是以 0 結尾的 UTF-16 字串或有效的輸出指標
        let created = unsafe {
            RegCreateKeyExW(HKEY_CURRENT_USER, path.as_ptr(), 0, null(), REG_OPTION_NON_VOLATILE, KEY_WRITE, null(), &mut hkey, null_mut())
        };
        check(created, "無法建立登錄機碼")?;
        let name = wide(name);
        let data = wide(data);
        // SAFETY: hkey 剛建立成功；資料長度以位元組計，含結尾的 0
        let code = unsafe { RegSetValueExW(hkey, name.as_ptr(), 0, REG_SZ, data.as_ptr().cast(), (data.len() * 2) as u32) };
        // SAFETY: 關閉剛開啟的 key
        unsafe { RegCloseKey(hkey) };
        check(code, "無法寫入登錄值")
    }

    /// 讀 `HKCU\<base>\<key>` 的字串值；沒有就 `None`。
    fn get(base: &str, key: &str, name: &str) -> Option<String> {
        let path = full(base, key);
        let name = wide(name);
        let mut buf = vec![0u16; 2048];
        let mut bytes = (buf.len() * 2) as u32;
        // SAFETY: buf 夠大，bytes 是它的位元組數；函式會回報實際長度
        let code = unsafe {
            RegGetValueW(HKEY_CURRENT_USER, path.as_ptr(), name.as_ptr(), RRF_RT_REG_SZ, null_mut(), buf.as_mut_ptr().cast(), &mut bytes)
        };
        if code != ERROR_SUCCESS {
            return None;
        }
        let len = (bytes as usize / 2).saturating_sub(1).min(buf.len());
        Some(String::from_utf16_lossy(&buf[..len]))
    }

    /// 刪掉一個值（`name` 空字串是預設值）；本來就沒有算成功。
    fn delete_value(base: &str, key: &str, name: &str) -> Result<(), String> {
        let path = full(base, key);
        let name_w = wide(name);
        let name_ptr = if name.is_empty() { null() } else { name_w.as_ptr() };
        // SAFETY: 以 0 結尾的 UTF-16 字串（或預設值用的空指標）
        let code = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, path.as_ptr(), name_ptr) };
        if code == ERROR_FILE_NOT_FOUND || code == ERROR_PATH_NOT_FOUND {
            return Ok(());
        }
        check(code, "無法刪除登錄值")
    }

    /// 刪掉整個機碼；本來就沒有算成功。
    fn delete_tree(base: &str, key: &str) -> Result<(), String> {
        let path = full(base, key);
        // SAFETY: 以 0 結尾的 UTF-16 字串
        let code = unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, path.as_ptr()) };
        if code == ERROR_FILE_NOT_FOUND || code == ERROR_PATH_NOT_FOUND {
            return Ok(());
        }
        check(code, "無法刪除登錄機碼")
    }

    /// 使用者在系統設定自己選過的這個副檔名的預設程式（有的話系統就不讓程式改）。
    fn user_choice(ext: &str) -> Option<String> {
        get(SOFTWARE, &format!(r"Microsoft\Windows\CurrentVersion\Explorer\FileExts\{ext}\UserChoice"), "ProgId")
    }

    /// 這個副檔名是不是被使用者自己選的別的程式佔住了。
    pub fn is_blocked(user_choice: Option<&str>, progid: &str) -> bool {
        user_choice.is_some_and(|c| !c.is_empty() && !c.eq_ignore_ascii_case(progid))
    }

    /// 請檔案總管重新讀取檔案關聯（不然圖示與預設程式要過一陣子才更新）。
    fn notify() {
        // SAFETY: 單純的通知，沒有要傳的項目
        unsafe { SHChangeNotify(SHCNE_ASSOCCHANGED as i32, SHCNF_IDLIST, null(), null()) };
    }

    fn unregister_ext(names: &Names, ext: &str) -> Result<(), String> {
        delete_value(CLASSES, &format!(r"{ext}\OpenWithProgids"), names.progid)?;
        if get(CLASSES, ext, "").is_some_and(|d| d.eq_ignore_ascii_case(names.progid)) {
            delete_value(CLASSES, ext, "")?;
        }
        Ok(())
    }

    fn remove_all(names: &Names) -> Result<(), String> {
        delete_tree(CLASSES, names.progid)?;
        delete_tree(SOFTWARE, names.app_key.strip_prefix(r"Software\").unwrap_or(names.app_key))?;
        delete_value(SOFTWARE, "RegisteredApplications", names.registered)
    }

    /// 讓 `selected`（含點的副檔名）預設用 `exe` 開啟，`all` 裡其他的還原。`selected` 是空的就全部移除。
    pub fn sync(names: &Names, selected: &[String], all: &[String], exe: &Path, texts: &Texts) -> Result<Outcome, String> {
        for ext in all.iter().filter(|e| !selected.contains(e)) {
            unregister_ext(names, ext)?;
        }
        if selected.is_empty() {
            remove_all(names)?;
            notify();
            return Ok(Outcome::default());
        }
        let exe = exe.display().to_string();
        // 檔案類型：雙按時執行 "exe" "檔案"
        set(CLASSES, names.progid, "", &texts.type_name)?;
        set(CLASSES, &format!(r"{}\DefaultIcon", names.progid), "", &format!("\"{exe}\",0"))?;
        set(CLASSES, &format!(r"{}\shell\open\command", names.progid), "", &format!("\"{exe}\" \"%1\""))?;
        // 能力宣告：讓 FEditor 出現在系統設定的「預設應用程式」，使用者可以一次選好
        let app = names.app_key.strip_prefix(r"Software\").unwrap_or(names.app_key);
        let caps = format!(r"{app}\Capabilities");
        delete_tree(SOFTWARE, &format!(r"{caps}\FileAssociations"))?;
        set(SOFTWARE, &caps, "ApplicationName", "FEditor")?;
        set(SOFTWARE, &caps, "ApplicationDescription", &texts.description)?;
        for ext in selected {
            set(SOFTWARE, &format!(r"{caps}\FileAssociations"), ext, names.progid)?;
        }
        set(SOFTWARE, "RegisteredApplications", names.registered, &format!(r"{}\Capabilities", names.app_key))?;
        let mut blocked = Vec::new();
        for ext in selected {
            // 「開啟方式」清單裡有 FEditor
            set(CLASSES, &format!(r"{ext}\OpenWithProgids"), names.progid, "")?;
            // 還沒有預設程式的：直接設成 FEditor（使用者在 HKCU 已經指定別的程式就不動它）
            if get(CLASSES, ext, "").is_none_or(|d| d.is_empty()) {
                set(CLASSES, ext, "", names.progid)?;
            }
            if is_blocked(user_choice(ext).as_deref(), names.progid) {
                blocked.push(ext.clone());
            }
        }
        notify();
        Ok(Outcome { blocked })
    }

    pub fn apply(exe: &Path, selected: &BTreeSet<String>) -> Result<Outcome, String> {
        let dotted = |e: &&str| format!(".{e}");
        let chosen: Vec<String> = extensions_for(selected).iter().map(dotted).collect();
        let all: Vec<String> = all_extensions().iter().map(dotted).collect();
        let texts = Texts { type_name: t("FEditor 文字檔案").to_owned(), description: t("FEditor：輕量的跨平台文字編輯器").to_owned() };
        sync(&NAMES, &chosen, &all, exe, &texts).map_err(|e| tf!("無法更新檔案關聯：{}", e))
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        const TEST: Names = Names { progid: "FEditorTest.File", app_key: r"Software\FEditorTest", registered: "FEditorTest" };
        const EXT: &str = ".feditortest";

        fn texts() -> Texts {
            Texts { type_name: "FEditor test file".into(), description: "test".into() }
        }

        #[test]
        fn blocked_only_when_the_user_picked_another_program() {
            assert!(!is_blocked(None, "FEditor.File"));
            assert!(!is_blocked(Some(""), "FEditor.File"));
            assert!(!is_blocked(Some("feditor.file"), "FEditor.File"), "大小寫不分，是自己就不算");
            assert!(is_blocked(Some("Applications\\notepad.exe"), "FEditor.File"));
        }

        /// 真的寫進登錄檔再還原（用一個不存在的副檔名和另一組名稱，不影響真正的設定）。
        #[test]
        fn registering_and_unregistering_round_trips_in_the_registry() {
            let all = vec![EXT.to_owned()];
            let exe = Path::new(r"C:\Apps\FEditor\feditor.exe");
            let _ = sync(&TEST, &[], &all, exe, &texts()); // 先清乾淨
            let outcome = sync(&TEST, &all, &all, exe, &texts()).unwrap();
            assert!(outcome.blocked.is_empty(), "沒有人選過這個副檔名");
            assert_eq!(get(CLASSES, EXT, "").as_deref(), Some("FEditorTest.File"), "沒有預設程式：直接設成 FEditor");
            assert_eq!(get(CLASSES, &format!(r"{EXT}\OpenWithProgids"), "FEditorTest.File").as_deref(), Some(""));
            assert_eq!(
                get(CLASSES, r"FEditorTest.File\shell\open\command", "").as_deref(),
                Some(r#""C:\Apps\FEditor\feditor.exe" "%1""#)
            );
            assert_eq!(get(SOFTWARE, r"FEditorTest\Capabilities\FileAssociations", EXT).as_deref(), Some("FEditorTest.File"));
            assert_eq!(get(SOFTWARE, "RegisteredApplications", "FEditorTest").as_deref(), Some(r"Software\FEditorTest\Capabilities"));
            // 取消：全部還原（我們設的預設程式、開啟方式、能力宣告都拿掉）
            sync(&TEST, &[], &all, exe, &texts()).unwrap();
            assert_eq!(get(CLASSES, EXT, ""), None, "我們設的預設程式要拿掉");
            assert_eq!(get(CLASSES, &format!(r"{EXT}\OpenWithProgids"), "FEditorTest.File"), None);
            assert_eq!(get(CLASSES, r"FEditorTest.File\shell\open\command", ""), None);
            assert_eq!(get(SOFTWARE, "RegisteredApplications", "FEditorTest"), None);
            assert_eq!(get(SOFTWARE, r"FEditorTest\Capabilities", "ApplicationName"), None);
            // 副檔名本來就有別的預設程式：不動它，但「開啟方式」清單還是加上；取消時也只拿掉我們加的
            set(CLASSES, EXT, "", "SomeoneElse.File").unwrap();
            sync(&TEST, &all, &all, exe, &texts()).unwrap();
            assert_eq!(get(CLASSES, EXT, "").as_deref(), Some("SomeoneElse.File"));
            assert_eq!(get(CLASSES, &format!(r"{EXT}\OpenWithProgids"), "FEditorTest.File").as_deref(), Some(""));
            sync(&TEST, &[], &all, exe, &texts()).unwrap();
            assert_eq!(get(CLASSES, EXT, "").as_deref(), Some("SomeoneElse.File"), "別人的預設程式不能被我們刪掉");
            assert_eq!(get(CLASSES, &format!(r"{EXT}\OpenWithProgids"), "FEditorTest.File"), None);            let _ = delete_tree(CLASSES, EXT);
        }
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use super::*;
    use std::collections::BTreeMap;
    use std::ffi::{CStr, CString, c_char, c_void};

    type CFStringRef = *const c_void;
    const UTF8: u32 = 0x0800_0100;
    const ROLES_ALL: u32 = 0xFFFF_FFFF;
    /// `Info.plist` 的 `CFBundleIdentifier`。
    const BUNDLE_ID: &str = "io.github.codemee.feditor";

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFStringCreateWithCString(alloc: *const c_void, c_str: *const c_char, encoding: u32) -> CFStringRef;
        fn CFStringGetCString(string: CFStringRef, buffer: *mut c_char, size: isize, encoding: u32) -> u8;
        fn CFRelease(cf: *const c_void);
    }

    #[link(name = "CoreServices", kind = "framework")]
    unsafe extern "C" {
        fn UTTypeCreatePreferredIdentifierForTag(tag_class: CFStringRef, tag: CFStringRef, conforming_to: CFStringRef) -> CFStringRef;
        fn LSSetDefaultRoleHandlerForContentType(content_type: CFStringRef, roles: u32, handler: CFStringRef) -> i32;
        fn LSCopyDefaultRoleHandlerForContentType(content_type: CFStringRef, roles: u32) -> CFStringRef;
    }

    /// 擁有一個 CFString（用完自動釋放）。
    struct Cf(CFStringRef);

    impl Cf {
        fn new(text: &str) -> Option<Cf> {
            let c = CString::new(text).ok()?;
            // SAFETY: c 是以 0 結尾的 UTF-8 字串；回傳的字串由我們擁有（Create 規則）
            let s = unsafe { CFStringCreateWithCString(std::ptr::null(), c.as_ptr(), UTF8) };
            (!s.is_null()).then_some(Cf(s))
        }

        /// 接手一個函式回傳的（已經由我們擁有的）CFString；空指標是 `None`。
        fn owned(raw: CFStringRef) -> Option<Cf> {
            (!raw.is_null()).then_some(Cf(raw))
        }

        fn to_text(&self) -> Option<String> {
            let mut buf = vec![0 as c_char; 512];
            // SAFETY: buf 夠大，長度如實告訴系統；成功時內容以 0 結尾
            let ok = unsafe { CFStringGetCString(self.0, buf.as_mut_ptr(), buf.len() as isize, UTF8) };
            // SAFETY: 成功時 buf 是以 0 結尾的字串
            (ok != 0).then(|| unsafe { CStr::from_ptr(buf.as_ptr()) }.to_string_lossy().into_owned())
        }
    }

    impl Drop for Cf {
        fn drop(&mut self) {
            // SAFETY: 釋放自己擁有的 CFString
            unsafe { CFRelease(self.0) };
        }
    }

    /// 副檔名對應的內容類型（UTI）。
    fn uti_for(ext: &str) -> Option<String> {
        let class = Cf::new("public.filename-extension")?;
        let tag = Cf::new(ext)?;
        // SAFETY: 兩個參數都是有效的 CFString；回傳的 UTI 由我們擁有
        let uti = unsafe { UTTypeCreatePreferredIdentifierForTag(class.0, tag.0, std::ptr::null()) };
        Cf::owned(uti)?.to_text()
    }

    fn current_handler(uti: &str) -> Option<String> {
        let uti = Cf::new(uti)?;
        // SAFETY: uti 是有效的 CFString；回傳的處理程式識別碼由我們擁有
        let handler = unsafe { LSCopyDefaultRoleHandlerForContentType(uti.0, ROLES_ALL) };
        Cf::owned(handler)?.to_text()
    }

    fn set_handler(uti: &str, bundle: &str) -> Result<(), i32> {
        let (Some(uti), Some(bundle)) = (Cf::new(uti), Cf::new(bundle)) else { return Err(-1) };
        // SAFETY: 兩個參數都是有效的 CFString
        let status = unsafe { LSSetDefaultRoleHandlerForContentType(uti.0, ROLES_ALL, bundle.0) };
        if status == 0 { Ok(()) } else { Err(status) }
    }

    /// 備份檔：每行「UTI<Tab>原本的處理程式」（原本沒有就空白）。
    fn read_backup(path: &Path) -> BTreeMap<String, String> {
        std::fs::read_to_string(path)
            .unwrap_or_default()
            .lines()
            .filter_map(|l| l.split_once('\t'))
            .map(|(k, v)| (k.to_owned(), v.to_owned()))
            .collect()
    }

    fn write_backup(path: &Path, map: &BTreeMap<String, String>) -> Result<(), String> {
        if map.is_empty() {
            let _ = std::fs::remove_file(path);
            return Ok(());
        }
        let text: String = map.iter().map(|(k, v)| format!("{k}\t{v}\n")).collect();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        std::fs::write(path, text).map_err(|e| e.to_string())
    }

    pub fn apply(_exe: &Path, selected: &BTreeSet<String>) -> Result<Outcome, String> {
        let backup_file = backup_path().ok_or_else(|| "找不到設定資料夾".to_owned())?;
        let mut backup = read_backup(&backup_file);
        let chosen: Vec<&str> = extensions_for(selected);
        let mut blocked = Vec::new();
        // 沒選到的（以前選過的）還原成原本的處理程式
        for ext in all_extensions().into_iter().filter(|e| !chosen.contains(e)) {
            let Some(uti) = uti_for(ext) else { continue };
            if let Some(previous) = backup.remove(&uti)
                && current_handler(&uti).as_deref() == Some(BUNDLE_ID)
                && !previous.is_empty()
            {
                let _ = set_handler(&uti, &previous);
            }
        }
        for ext in chosen {
            let Some(uti) = uti_for(ext) else { continue };
            let current = current_handler(&uti);
            if current.as_deref() == Some(BUNDLE_ID) {
                continue;
            }
            match set_handler(&uti, BUNDLE_ID) {
                Ok(()) => {
                    backup.entry(uti).or_insert(current.unwrap_or_default());
                }
                Err(_) => blocked.push(format!(".{ext}")),
            }
        }
        write_backup(&backup_file, &backup)?;
        Ok(Outcome { blocked })
    }
}

#[cfg(target_os = "linux")]
mod imp {
    use super::*;
    use std::path::PathBuf;

    fn dir_from(var: &str, fallback: &str) -> Result<PathBuf, String> {
        if let Some(d) = std::env::var_os(var).filter(|d| !d.is_empty()) {
            return Ok(PathBuf::from(d));
        }
        std::env::var_os("HOME")
            .filter(|h| !h.is_empty())
            .map(|h| PathBuf::from(h).join(fallback))
            .ok_or_else(|| "找不到使用者的家目錄".to_owned())
    }

    pub fn apply(exe: &Path, selected: &BTreeSet<String>) -> Result<Outcome, String> {
        let applications = dir_from("XDG_DATA_HOME", ".local/share")?.join("applications");
        let mimeapps = dir_from("XDG_CONFIG_HOME", ".config")?.join("mimeapps.list");
        let desktop = applications.join(DESKTOP_FILE);
        let old = std::fs::read_to_string(&mimeapps).unwrap_or_default();
        // 先把所有類別都還原，再依選到的重排：取消勾選的類別就會還原
        let mut text = clear_default_apps(&old, &all_mime_types(), DESKTOP_FILE);
        if selected.is_empty() {
            let _ = std::fs::remove_file(&desktop);
        } else {
            std::fs::create_dir_all(&applications).map_err(|e| e.to_string())?;
            std::fs::write(&desktop, desktop_entry(exe)).map_err(|e| e.to_string())?;
            text = set_default_apps(&text, &mime_types_for(selected), DESKTOP_FILE);
        }
        if text != old {
            if let Some(dir) = mimeapps.parent() {
                std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
            }
            std::fs::write(&mimeapps, text).map_err(|e| e.to_string())?;
        }
        // 請桌面環境重新讀取（沒有這個指令也沒關係）
        let _ = std::process::Command::new("update-desktop-database").arg(&applications).status();
        Ok(Outcome::default())
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
mod imp {
    use super::*;

    pub fn apply(_: &Path, selected: &BTreeSet<String>) -> Result<Outcome, String> {
        if selected.is_empty() { Ok(Outcome::default()) } else { Err("這個系統不支援".to_owned()) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set_of(keys: &[&str]) -> BTreeSet<String> {
        keys.iter().map(|k| (*k).to_owned()).collect()
    }

    #[test]
    fn groups_cover_distinct_lowercase_extensions_and_default_to_code_and_config() {
        let all = all_extensions();
        let unique: BTreeSet<&str> = all.iter().copied().collect();
        assert_eq!(all.len(), unique.len(), "同一個副檔名只屬於一類");
        assert!(all.iter().all(|e| !e.starts_with('.') && *e == e.to_ascii_lowercase() && !e.is_empty()));
        assert_eq!(default_groups(), set_of(&["code", "config"]));
        // 「雙按就是執行」的檔案不在裡面
        for risky in ["bat", "cmd", "exe", "com", "msi", "vbs", "lnk"] {
            assert!(!all.contains(&risky), "{risky} 不該被接手");
        }
        assert_eq!(extensions_for(&set_of(&["text"])), vec!["txt", "log", "csv", "tsv"]);
        assert!(extensions_for(&set_of(&["nonsense"])).is_empty(), "不認得的類別略過");
        assert!(extensions_for(&BTreeSet::new()).is_empty());
    }

    #[test]
    fn code_config_and_markup_extensions_are_all_known_to_the_syntax_table() {
        // 設定面板列出的類別，FEditor 自己也認得（語法上色）：副檔名表和語言表不能各唱各的
        for group in GROUPS.iter().filter(|g| g.key != "text") {
            for ext in group.extensions {
                assert!(crate::syntax::language_for_name(&format!("x.{ext}")).is_some(), "{ext} 沒有對應的語言");
            }
        }
    }

    #[test]
    fn every_group_has_a_label_and_mime_types() {
        for g in GROUPS {
            assert!(!g.label().is_empty() && !g.mime_types.is_empty(), "{}", g.key);
        }
        assert!(mime_types_for(&set_of(&["config"])).contains(&"application/json"));
        assert!(all_mime_types().len() > mime_types_for(&set_of(&["config"])).len());
    }

    #[test]
    fn the_desktop_entry_lists_every_mime_type_and_quotes_the_exe() {
        let entry = desktop_entry(Path::new("/opt/my apps/feditor"));
        assert!(entry.starts_with("[Desktop Entry]\nType=Application\n"));
        assert!(entry.contains("Exec=\"/opt/my apps/feditor\" %F\n"), "{entry}");
        assert!(entry.contains("MimeType=text/x-python;") && entry.contains("text/plain;"));
        assert!(entry.contains("Categories=Utility;TextEditor;"));
        assert!(desktop_entry(Path::new("/a\"b$c")).contains(r#"Exec="/a\"b\$c" %F"#));
    }

    const APP: &str = "feditor.desktop";

    #[test]
    fn default_apps_put_feditor_first_and_keep_the_previous_ones_behind_it() {
        let start = "[Added Associations]\ntext/plain=gedit.desktop;\n\n[Default Applications]\ntext/plain=gedit.desktop;\nimage/png=eog.desktop;\n\n[Removed Associations]\ntext/x-python=vim.desktop;\n";
        let set = set_default_apps(start, &["text/plain", "text/x-python"], APP);
        assert_eq!(
            set,
            "[Added Associations]\ntext/plain=gedit.desktop;\n\n[Default Applications]\ntext/plain=feditor.desktop;gedit.desktop;\nimage/png=eog.desktop;\ntext/x-python=feditor.desktop;\n\n[Removed Associations]\ntext/x-python=vim.desktop;\n"
        );
        // 已經排第一：不重複加
        assert_eq!(set_default_apps(&set, &["text/plain"], APP), set);
        // 取消：回到原本的（原本沒有這行的就整行刪掉）
        let cleared = clear_default_apps(&set, &["text/plain", "text/x-python"], APP);
        assert_eq!(
            cleared,
            "[Added Associations]\ntext/plain=gedit.desktop;\n\n[Default Applications]\ntext/plain=gedit.desktop;\nimage/png=eog.desktop;\n\n[Removed Associations]\ntext/x-python=vim.desktop;\n"
        );
        // 沒選到的類別不動
        assert_eq!(clear_default_apps(&set, &["image/png"], APP), set);
    }

    #[test]
    fn default_apps_create_the_section_when_missing_and_leave_other_apps_alone() {
        assert_eq!(set_default_apps("", &["text/plain"], APP), "[Default Applications]\ntext/plain=feditor.desktop;\n");
        assert_eq!(
            set_default_apps("[Added Associations]\na/b=x.desktop;\n", &["text/plain"], APP),
            "[Added Associations]\na/b=x.desktop;\n\n[Default Applications]\ntext/plain=feditor.desktop;\n"
        );
        // 別的程式排第一的，FEditor 插到最前面；取消後原樣回來
        let other = "[Default Applications]\ntext/plain=kate.desktop;\n";
        let set = set_default_apps(other, &["text/plain"], APP);
        assert_eq!(set, "[Default Applications]\ntext/plain=feditor.desktop;kate.desktop;\n");
        assert_eq!(clear_default_apps(&set, &["text/plain"], APP), other);
        // 沒有東西要改：不動、也不憑空變出段落
        assert_eq!(clear_default_apps("", &["text/plain"], APP), "");
        assert_eq!(clear_default_apps("[Added Associations]\na/b=x;\n", &["text/plain"], APP), "[Added Associations]\na/b=x;\n");
    }
}
