//! 在 `~/.local/bin` 加入／移除 `feditor` 指令（macOS、Linux），讓終端機可以直接輸入 `feditor 檔案` 開啟。
//!
//! 加入的是一個很短的 shell 腳本，不是符號連結：腳本把程式放到背景啟動，終端機馬上回到提示字元；
//! 也不會因為程式在 `.app` 套件裡面（`FEditor.app/Contents/MacOS/feditor`）而讓使用者自己去找路徑。
//! 腳本第二行有固定的標記，移除或更新時只動帶標記的檔案，不會碰到使用者自己的同名檔案。

use feditor_core::i18n::t;
use feditor_core::tf;
use std::path::{Path, PathBuf};

/// 指令的檔名。
pub const COMMAND_NAME: &str = "feditor";

/// 腳本裡的標記行：靠它認出這個檔案是 FEditor 建立的。
const MARKER: &str = "# feditor-managed-command: created by FEditor, remove it from FEditor's settings";

/// 這個系統支援嗎（macOS、Linux：在 `~/.local/bin` 放腳本；Windows：把程式所在資料夾加進使用者的 `PATH`）。
pub fn supported() -> bool {
    cfg!(any(unix, windows))
}

/// 指令所在的資料夾：macOS、Linux 是 `~/.local/bin`；Windows 是程式（`feditor.exe`）所在的資料夾。
pub fn bin_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        std::env::current_exe().ok()?.parent().map(Path::to_path_buf)
    }
    #[cfg(not(windows))]
    {
        let home = std::env::var_os("HOME").filter(|h| !h.is_empty())?;
        Some(PathBuf::from(home).join(".local").join("bin"))
    }
}

// ───────── PATH 字串的處理（純函式，各平台都能測）─────────

/// 展開 `%NAME%`（找不到的變數原樣保留），讓登錄裡的 `%USERPROFILE%\bin` 能和實際路徑比較。
fn expand_vars(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find('%') {
            Some(end) if end > 0 => match std::env::var(&after[..end]) {
                Ok(value) => {
                    out.push_str(&value);
                    rest = &after[end + 1..];
                }
                Err(_) => {
                    out.push('%');
                    rest = after;
                }
            },
            _ => {
                out.push('%');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// 比較用的正規化：展開變數、去掉引號與結尾的斜線、`/` 當成 `\`、不分大小寫。
fn normalize_entry(entry: &str) -> String {
    let expanded = expand_vars(entry.trim());
    expanded.trim_matches('"').trim().replace('/', "\\").trim_end_matches('\\').to_lowercase()
}

fn entries(path: &str) -> impl Iterator<Item = &str> {
    path.split(';').filter(|e| !e.trim().is_empty())
}

/// `path`（用 `;` 分隔）裡是不是已經有 `dir`。
pub fn path_has(path: &str, dir: &str) -> bool {
    let wanted = normalize_entry(dir);
    !wanted.is_empty() && entries(path).any(|e| normalize_entry(e) == wanted)
}

/// 把 `dir` 加到 `path` 最後面；已經有就回傳 `None`（不重複加入）。
pub fn path_with(path: &str, dir: &str) -> Option<String> {
    if path_has(path, dir) {
        return None;
    }
    let base = path.trim_end_matches(';');
    Some(if base.trim().is_empty() { dir.to_owned() } else { format!("{base};{dir}") })
}

/// 把 `path` 裡的 `dir` 全部拿掉（其他項目原樣保留）；本來就沒有就回傳 `None`。
pub fn path_without(path: &str, dir: &str) -> Option<String> {
    if !path_has(path, dir) {
        return None;
    }
    let wanted = normalize_entry(dir);
    Some(entries(path).filter(|e| normalize_entry(e) != wanted).collect::<Vec<_>>().join(";"))
}

#[cfg(windows)]
mod win {
    use super::*;
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_WRITE, REG_EXPAND_SZ, REG_OPTION_NON_VOLATILE, RRF_NOEXPAND, RRF_RT_REG_EXPAND_SZ,
        RRF_RT_REG_SZ, RegCloseKey, RegCreateKeyExW, RegGetValueW, RegSetValueExW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{HWND_BROADCAST, SMTO_ABORTIFHUNG, SendMessageTimeoutW, WM_SETTINGCHANGE};

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// 讀 `HKCU\Environment` 的 `Path`（不展開 `%變數%`），連同它的型別；沒有這個值就 `None`。
    fn read_user_path() -> Result<Option<(String, u32)>, String> {
        let key = wide("Environment");
        let name = wide("Path");
        let flags = RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ | RRF_NOEXPAND;
        let mut bytes = 0u32;
        // SAFETY: 第一次只問大小（資料緩衝區給空指標）
        let code = unsafe { RegGetValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr(), flags, null_mut(), null_mut(), &mut bytes) };
        if code != ERROR_SUCCESS {
            return Ok(None); // 沒有使用者的 Path 值
        }
        let mut buf = vec![0u16; bytes as usize / 2 + 2];
        let mut bytes = (buf.len() * 2) as u32;
        let mut kind = 0u32;
        // SAFETY: buf 夠大，bytes 是它的位元組數；函式會回報實際長度與型別
        let code = unsafe { RegGetValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr(), flags, &mut kind, buf.as_mut_ptr().cast(), &mut bytes) };
        if code != ERROR_SUCCESS {
            return Err(format!("無法讀取使用者的 PATH（錯誤碼 {code}）"));
        }
        let len = (bytes as usize / 2).saturating_sub(1).min(buf.len());
        Ok(Some((String::from_utf16_lossy(&buf[..len]), kind)))
    }

    fn write_user_path(value: &str, kind: u32) -> Result<(), String> {
        let key = wide("Environment");
        let mut hkey: HKEY = null_mut();
        // SAFETY: 參數都是以 0 結尾的 UTF-16 字串或有效的輸出指標
        let code = unsafe { RegCreateKeyExW(HKEY_CURRENT_USER, key.as_ptr(), 0, null(), REG_OPTION_NON_VOLATILE, KEY_WRITE, null(), &mut hkey, null_mut()) };
        if code != ERROR_SUCCESS {
            return Err(format!("無法開啟使用者的環境變數（錯誤碼 {code}）"));
        }
        let name = wide("Path");
        let data = wide(value);
        // SAFETY: hkey 剛開啟成功；資料長度以位元組計，含結尾的 0
        let code = unsafe { RegSetValueExW(hkey, name.as_ptr(), 0, kind, data.as_ptr().cast(), (data.len() * 2) as u32) };
        // SAFETY: 關閉剛開啟的 key
        unsafe { RegCloseKey(hkey) };
        if code != ERROR_SUCCESS {
            return Err(format!("無法寫入使用者的 PATH（錯誤碼 {code}）"));
        }
        // 通知檔案總管等程式重新讀環境變數，之後新開的終端機才拿得到
        let env = wide("Environment");
        // SAFETY: lParam 指向以 0 結尾的 UTF-16 字串，呼叫期間一直有效
        unsafe { SendMessageTimeoutW(HWND_BROADCAST, WM_SETTINGCHANGE, 0, env.as_ptr() as isize, SMTO_ABORTIFHUNG, 5000, null_mut()) };
        Ok(())
    }

    /// 加入（`on`）或移除使用者 `PATH` 裡的 `dir`。加入前先看使用者的 PATH 與目前行程的 PATH
    /// （含系統的）有沒有這個資料夾，有就什麼也不做；移除只動使用者的 PATH。
    pub fn apply(on: bool, dir: &Path) -> Result<(), String> {
        let dir = dir.to_string_lossy();
        let current = read_user_path()?;
        let user_path = current.as_ref().map_or("", |(p, _)| p.as_str());
        if on {
            let process_path = std::env::var("PATH").unwrap_or_default();
            if path_has(&process_path, &dir) {
                return Ok(());
            }
            match path_with(user_path, &dir) {
                Some(new) => write_user_path(&new, current.as_ref().map_or(REG_EXPAND_SZ, |(_, k)| *k)),
                None => Ok(()),
            }
        } else {
            match (path_without(user_path, &dir), current.as_ref()) {
                (Some(new), Some((_, kind))) => write_user_path(&new, *kind),
                _ => Ok(()),
            }
        }
    }
}

fn quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

/// 指令腳本的內容：把所有參數交給 `exe`，放到背景執行；要等結果的（`--wait`、`--help`、`--version`）留在前景。
pub fn script(exe: &Path) -> String {
    format!(
        "#!/bin/sh\n{MARKER}\n\
         [ \"$1\" = --send ] && exec {exe} \"$@\"\n\
         for a in \"$@\"; do\n\
         \x20 case \"$a\" in\n\
         \x20   --) break ;;\n\
         \x20   -w|--wait|-h|--help|-v|--version) exec {exe} \"$@\" ;;\n\
         \x20 esac\n\
         done\n\
         nohup {exe} \"$@\" >/dev/null 2>&1 &\n",
        exe = quote(&exe.to_string_lossy())
    )
}

/// 這個檔案是不是 FEditor 建立的指令腳本。
fn is_ours(path: &Path) -> bool {
    std::fs::read_to_string(path).is_ok_and(|text| text.lines().nth(1) == Some(MARKER))
}

/// 在 `dir` 建立（或更新）指令。已經有同名的、不是 FEditor 建立的檔案就不動它，回傳錯誤。
/// 成功回傳指令檔的完整路徑。
pub fn install(dir: &Path, exe: &Path) -> Result<PathBuf, String> {
    let target = dir.join(COMMAND_NAME);
    if (target.exists() || target.symlink_metadata().is_ok()) && !is_ours(&target) {
        return Err(tf!("{} 已經存在，而且不是 FEditor 建立的，所以沒有動它", target.display()));
    }
    std::fs::create_dir_all(dir).map_err(|e| format!("{}：{e}", dir.display()))?;
    std::fs::write(&target, script(exe)).map_err(|e| format!("{}：{e}", target.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o755)).map_err(|e| format!("{}：{e}", target.display()))?;
    }
    Ok(target)
}

/// 移除 `dir` 裡 FEditor 建立的指令。檔案不存在、或不是 FEditor 建立的，什麼也不做（回傳 `false`）。
pub fn remove(dir: &Path) -> Result<bool, String> {
    let target = dir.join(COMMAND_NAME);
    if !is_ours(&target) {
        return Ok(false);
    }
    std::fs::remove_file(&target).map_err(|e| format!("{}：{e}", target.display()))?;
    Ok(true)
}

/// 真正去加入（`on`）或移除 feditor 指令：macOS、Linux 是 `~/.local/bin/feditor` 腳本，Windows 是使用者 `PATH` 裡的程式資料夾。
pub fn apply(on: bool) -> Result<(), String> {
    let dir = bin_dir().ok_or_else(|| t("找不到使用者的家目錄").to_owned())?;
    #[cfg(windows)]
    {
        win::apply(on, &dir)
    }
    #[cfg(not(windows))]
    {
        if on {
            let exe = std::env::current_exe().map_err(|e| e.to_string())?;
            install(&dir, &exe).map(|_| ())
        } else {
            remove(&dir).map(|_| ())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_is_only_added_when_missing_and_comparison_ignores_case_slashes_and_variables() {
        let dir = r"C:\Tools\FEditor";
        assert_eq!(path_with(r"C:\Windows;C:\Bin", dir).as_deref(), Some(r"C:\Windows;C:\Bin;C:\Tools\FEditor"));
        assert_eq!(path_with("", dir).as_deref(), Some(dir));
        assert_eq!(path_with(r"C:\Bin;", dir).as_deref(), Some(r"C:\Bin;C:\Tools\FEditor"), "結尾的分號不重複");
        for existing in [r"c:\tools\feditor", r"C:\Tools\FEditor\", r"C:/Tools/FEditor", r#""C:\Tools\FEditor""#] {
            assert!(path_has(&format!(r"C:\Bin;{existing};C:\X"), dir), "{existing}");
            assert_eq!(path_with(&format!(r"C:\Bin;{existing}"), dir), None, "{existing}");
        }
        assert!(!path_has(r"C:\Tools\FEditor2;C:\Tools", dir), "只差一部分的不算");
        // %變數% 展開後相同也算已經有
        unsafe { std::env::set_var("FEDITOR_TEST_ROOT", r"C:\Tools") };
        assert!(path_has(r"%FEDITOR_TEST_ROOT%\FEditor", dir));
        assert!(!path_has(r"%FEDITOR_NO_SUCH_VAR%\FEditor", dir));
    }

    #[test]
    fn path_without_removes_only_the_given_folder() {
        let dir = r"C:\Tools\FEditor";
        assert_eq!(path_without(r"C:\A;c:\tools\feditor\;C:\B", dir).as_deref(), Some(r"C:\A;C:\B"));
        assert_eq!(path_without(r"C:\A;C:\B", dir), None, "本來就沒有");
        assert_eq!(path_without(dir, dir).as_deref(), Some(""));
    }

    #[test]
    fn the_script_launches_the_exe_in_the_background_and_quotes_odd_paths() {
        let s = script(Path::new("/Applications/FEditor.app/Contents/MacOS/feditor"));
        assert_eq!(s.lines().next(), Some("#!/bin/sh"));
        assert_eq!(s.lines().nth(1), Some(MARKER));
        assert!(s.contains("nohup '/Applications/FEditor.app/Contents/MacOS/feditor' \"$@\" >/dev/null 2>&1 &"), "{s}");
        assert!(s.contains("-w|--wait|-h|--help|-v|--version) exec '/Applications/FEditor.app/Contents/MacOS/feditor' \"$@\""), "{s}");
        // --send 要看得到錯誤訊息、拿得到結束碼、讀得到標準輸入：前景執行
        assert!(s.contains("[ \"$1\" = --send ] && exec '/Applications/FEditor.app/Contents/MacOS/feditor' \"$@\""), "{s}");
        let odd = script(Path::new("/tmp/it's here/feditor"));
        assert!(odd.contains(r"'/tmp/it'\''s here/feditor'"), "{odd}");
    }

    #[test]
    fn install_creates_an_executable_script_updates_it_and_remove_deletes_only_ours() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("nested").join("bin"); // 資料夾還不存在
        let path = install(&bin, Path::new("/old/feditor")).unwrap();
        assert_eq!(path, bin.join("feditor"));
        assert!(std::fs::read_to_string(&path).unwrap().contains("/old/feditor"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o111, 0o111, "要能執行");
        }
        // 程式搬了位置：再裝一次就更新成新路徑
        install(&bin, Path::new("/new/feditor")).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("/new/feditor") && !text.contains("/old/feditor"));
        assert_eq!(remove(&bin), Ok(true));
        assert!(!path.exists());
        assert_eq!(remove(&bin), Ok(false), "已經沒有了");
    }

    #[test]
    fn a_file_that_is_not_ours_is_never_overwritten_or_removed() {
        let dir = tempfile::tempdir().unwrap();
        let mine = dir.path().join("feditor");
        std::fs::write(&mine, "#!/bin/sh\necho 使用者自己的腳本\n").unwrap();
        let err = install(dir.path(), Path::new("/x/feditor")).unwrap_err();
        assert!(err.contains("不是 FEditor 建立的"), "{err}");
        assert_eq!(remove(dir.path()), Ok(false));
        assert!(std::fs::read_to_string(&mine).unwrap().contains("使用者自己的腳本"));
    }
}
