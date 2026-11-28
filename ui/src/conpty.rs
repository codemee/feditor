//! 隨附的新版 ConPTY（Windows 專用）。
//!
//! Windows 內建的 ConPTY 會吃掉程式問終端機的問題（OSC 10／11 問前景、背景色等），Codex、vim 等程式就
//! 不知道終端機是深色還是淺色。微軟 Windows Terminal 附的 `conpty.dll` 加 `OpenConsole.exe` 沒有這個問題，
//! 而 `alacritty_terminal` 開 shell 時會先試著載入名叫 `conpty.dll` 的檔案，找不到才用內建的。
//!
//! 所以：兩個檔案內嵌在執行檔裡（MIT 授權，見 `ui/assets/conpty/LICENSE`），啟動時解開到
//! `%LOCALAPPDATA%\feditor\conpty\<版本>\`，再用 `SetDllDirectoryW` 告訴系統去那裡找 `conpty.dll`。
//! 這只影響這個程序載入 DLL 的位置，不會動 `PATH` 等環境變數，所以 shell 裡的環境不變。
//! 任何一步失敗都安靜地退回內建的 ConPTY（只是 Codex 之類的程式偵測不到淺色）。

/// 隨附的 ConPTY 的版本（NuGet 套件 `Microsoft.Windows.Console.ConPTY`），資料夾名稱用它區分。
pub const VERSION: &str = "1.25.261109001";

#[cfg(all(windows, target_arch = "x86_64"))]
const CONPTY_DLL: &[u8] = include_bytes!("../assets/conpty/conpty.dll");
#[cfg(all(windows, target_arch = "x86_64"))]
const OPEN_CONSOLE: &[u8] = include_bytes!("../assets/conpty/OpenConsole.exe");

/// 把 `files`（檔名、內容）寫進 `dir`：已經有而且內容一樣的就不動。回傳有沒有全部就位。
#[cfg_attr(not(all(windows, target_arch = "x86_64")), allow(dead_code))]
fn unpack(dir: &std::path::Path, files: &[(&str, &[u8])]) -> bool {
    if std::fs::create_dir_all(dir).is_err() {
        return false;
    }
    files.iter().all(|(name, bytes)| {
        let path = dir.join(name);
        if std::fs::read(&path).is_ok_and(|existing| existing == *bytes) {
            return true;
        }
        // 先寫成暫存檔再改名，同時有好幾個視窗啟動時不會讀到寫到一半的檔案
        let tmp = dir.join(format!("{name}.{}.tmp", std::process::id()));
        let ok = std::fs::write(&tmp, bytes).is_ok() && std::fs::rename(&tmp, &path).is_ok();
        if !ok {
            let _ = std::fs::remove_file(&tmp);
            // 改名失敗，但目標已經是正確的內容（別的視窗剛好寫好）也算成功
            return std::fs::read(&path).is_ok_and(|existing| existing == *bytes);
        }
        true
    })
}

/// 第一次要開終端機時才呼叫（之後的呼叫什麼都不做）：解開隨附的 ConPTY 並讓這個程序從那裡載入。
/// 延到這個時候才做，沒開過終端機的人完全不會碰到，檔案總管轉送開檔要求、`--help` 等也不受影響。
/// 測試不做，免得測試寫到使用者的資料夾。
pub fn ensure_installed() {
    #[cfg(not(test))]
    {
        static ONCE: std::sync::Once = std::sync::Once::new();
        ONCE.call_once(install);
    }
}

/// 解開隨附的 ConPTY 並讓這個程序從那裡載入。
#[cfg(all(windows, target_arch = "x86_64"))]
fn install() {
    use std::os::windows::ffi::OsStrExt;
    let Some(base) = std::env::var_os("LOCALAPPDATA").map(std::path::PathBuf::from) else {
        return;
    };
    let dir = base.join("feditor").join("conpty").join(VERSION);
    if !unpack(
        &dir,
        &[
            ("conpty.dll", CONPTY_DLL),
            ("OpenConsole.exe", OPEN_CONSOLE),
        ],
    ) {
        return;
    }
    let wide: Vec<u16> = dir
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    // SAFETY: `wide` 是以 0 結尾的路徑字串，呼叫期間一直有效
    unsafe { windows_sys::Win32::System::LibraryLoader::SetDllDirectoryW(wide.as_ptr()) };
}

#[cfg(not(all(windows, target_arch = "x86_64")))]
#[allow(dead_code)]
fn install() {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unpacking_writes_missing_files_repairs_changed_ones_and_leaves_good_ones_alone() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("a").join("b");
        let files: [(&str, &[u8]); 2] = [("x.dll", b"dll-bytes"), ("y.exe", b"exe-bytes")];
        assert!(unpack(&dir, &files));
        assert_eq!(std::fs::read(dir.join("x.dll")).unwrap(), b"dll-bytes");
        assert_eq!(std::fs::read(dir.join("y.exe")).unwrap(), b"exe-bytes");
        // 內容被改掉：修回來
        std::fs::write(dir.join("x.dll"), b"tampered").unwrap();
        assert!(unpack(&dir, &files));
        assert_eq!(std::fs::read(dir.join("x.dll")).unwrap(), b"dll-bytes");
        // 內容正確：不重寫（修改時間不變）
        let before = std::fs::metadata(dir.join("y.exe"))
            .unwrap()
            .modified()
            .unwrap();
        assert!(unpack(&dir, &files));
        assert_eq!(
            std::fs::metadata(dir.join("y.exe"))
                .unwrap()
                .modified()
                .unwrap(),
            before
        );
        // 沒有留下暫存檔
        assert!(
            std::fs::read_dir(&dir).unwrap().all(|e| !e
                .unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with(".tmp"))
        );
    }

    #[test]
    fn unpacking_into_an_impossible_place_fails_quietly() {
        let root = tempfile::tempdir().unwrap();
        let blocker = root.path().join("file");
        std::fs::write(&blocker, "").unwrap();
        assert!(
            !unpack(&blocker.join("sub"), &[("x", b"1")]),
            "資料夾建不起來：回傳失敗，不當機"
        );
    }

    /// 隨附的檔案真的是微軟的 ConPTY（簡單的完整性檢查：檔頭 `MZ`、大小合理）。
    #[cfg(all(windows, target_arch = "x86_64"))]
    #[test]
    fn the_embedded_files_look_like_windows_executables() {
        for bytes in [CONPTY_DLL, OPEN_CONSOLE] {
            assert!(bytes.starts_with(b"MZ") && bytes.len() > 50_000);
        }
    }
}
