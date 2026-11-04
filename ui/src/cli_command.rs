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

/// 這個系統支援嗎（要有 `~/.local/bin` 慣例的系統；Windows 不用這個）。
pub fn supported() -> bool {
    cfg!(unix)
}

/// 放指令的資料夾：`~/.local/bin`。
pub fn bin_dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").filter(|h| !h.is_empty())?;
    Some(PathBuf::from(home).join(".local").join("bin"))
}

fn quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

/// 指令腳本的內容：把所有參數交給 `exe`，放到背景執行。
pub fn script(exe: &Path) -> String {
    format!(
        "#!/bin/sh\n{MARKER}\nnohup {} \"$@\" >/dev/null 2>&1 &\n",
        quote(&exe.to_string_lossy())
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

/// 真正去加入（`on`）或移除 `~/.local/bin/feditor`。
pub fn apply(on: bool) -> Result<(), String> {
    let dir = bin_dir().ok_or_else(|| t("找不到使用者的家目錄").to_owned())?;
    if on {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        install(&dir, &exe).map(|_| ())
    } else {
        remove(&dir).map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_script_launches_the_exe_in_the_background_and_quotes_odd_paths() {
        let s = script(Path::new("/Applications/FEditor.app/Contents/MacOS/feditor"));
        assert_eq!(s.lines().next(), Some("#!/bin/sh"));
        assert_eq!(s.lines().nth(1), Some(MARKER));
        assert!(s.contains("nohup '/Applications/FEditor.app/Contents/MacOS/feditor' \"$@\" >/dev/null 2>&1 &"), "{s}");
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
