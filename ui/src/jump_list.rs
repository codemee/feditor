//! Windows 工作列的跳躍清單：在 FEditor 的圖示上按右鍵，多一個「最近開啟」類別，列出最近開啟的檔案。
//!
//! 用 `ICustomDestinationList` 建一個自訂類別，每一項是一個 `IShellLink`：目標是 FEditor 自己，參數是檔案路徑。
//! 所以點下去是執行 `feditor.exe "檔案"`，已經有 FEditor 在執行時由 `single_instance` 轉給那個視窗，
//! 不管這個檔案平常是用哪個程式開的（用系統的「最近」清單就會跟著檔案關聯走，開到別的程式）。
//!
//! COM 呼叫要幾十毫秒，所以每次更新都開一個背景執行緒做，失敗就算了（見 [`update`]）。
//! 不設定 AppUserModelID：用 Windows 依執行檔路徑算出的預設值，和釘選在工作列的捷徑是同一個。

use std::path::{Path, PathBuf};

use windows::Win32::Foundation::{E_FAIL, RPC_E_CHANGED_MODE};
use windows::Win32::Storage::EnhancedStorage::PKEY_Title;
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize};
use windows::Win32::UI::Shell::Common::{IObjectArray, IObjectCollection};
use windows::Win32::UI::Shell::PropertiesSystem::IPropertyStore;
use windows::Win32::UI::Shell::{DestinationList, EnumerableObjectCollection, ICustomDestinationList, IShellLinkW, ShellLink};
use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
use windows::core::{Interface, PCWSTR, Result};

/// 命令列的一個參數：用雙引號包起來（Windows 的檔名不能有雙引號，所以不用跳脫）。
pub fn quote_argument(path: &str) -> String {
    format!("\"{path}\"")
}

/// 在背景更新跳躍清單：`list` 是最近開啟的檔案（最近的在前面），不存在的檔案不列；空的就清掉這個類別。
pub fn update(list: &[String]) {
    let list = list.to_vec();
    std::thread::spawn(move || {
        let _ = rebuild(&list);
    });
}

/// COM 初始化的守衛：這個執行緒有成功初始化才在結束時還原。
struct ComGuard(bool);

impl ComGuard {
    fn new() -> Result<Self> {
        // SAFETY: 標準的 COM 初始化；單一執行緒公寓。這個執行緒是我們自己開的，不會已經是別的模式；
        // 真的遇到（RPC_E_CHANGED_MODE）就直接用現有的，不重複初始化。
        let hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        if hr == RPC_E_CHANGED_MODE {
            return Ok(ComGuard(false));
        }
        hr.ok()?;
        Ok(ComGuard(true))
    }
}

impl Drop for ComGuard {
    fn drop(&mut self) {
        if self.0 {
            // SAFETY: 與 CoInitializeEx 成對。
            unsafe { CoUninitialize() };
        }
    }
}

/// 把字串轉成以 0 結尾的 UTF-16（給 `PCWSTR` 用）。
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

fn rebuild(list: &[String]) -> Result<()> {
    let _com = ComGuard::new()?;
    let exe = std::env::current_exe().map_err(|_| windows::core::Error::from(E_FAIL))?;
    // SAFETY: 全是標準的 COM 呼叫，指標都指向這個函式裡活著的緩衝區。
    unsafe {
        let dest: ICustomDestinationList = CoCreateInstance(&DestinationList, None, CLSCTX_INPROC_SERVER)?;
        let mut slots = 0u32;
        // 使用者在跳躍清單上按「從這份清單移除」的項目：不能再加回去，不然整個類別會失敗
        let removed: IObjectArray = dest.BeginList(&mut slots)?;
        let removed_args = removed_arguments(&removed);

        let titles = crate::recent::jump_titles(list);
        let collection: IObjectCollection = CoCreateInstance(&EnumerableObjectCollection, None, CLSCTX_INPROC_SERVER)?;
        let mut added = 0usize;
        for (path, title) in list.iter().zip(&titles) {
            if added >= slots as usize || !Path::new(path).exists() {
                continue;
            }
            let args = quote_argument(path);
            if removed_args.iter().any(|r| r.eq_ignore_ascii_case(&args)) {
                continue;
            }
            if let Ok(link) = make_link(&exe, &args, title, path) {
                collection.AddObject(&link)?;
                added += 1;
            }
        }
        if added > 0 {
            let items: IObjectArray = collection.cast()?;
            let category = wide(feditor_core::i18n::t("最近開啟"));
            dest.AppendCategory(PCWSTR(category.as_ptr()), &items)?;
        }
        dest.CommitList()?;
    }
    Ok(())
}

/// 使用者已經從跳躍清單移除的項目的參數（`IShellLink` 的參數字串）。
unsafe fn removed_arguments(removed: &IObjectArray) -> Vec<String> {
    let mut out = Vec::new();
    // SAFETY: 呼叫端保證 `removed` 有效；緩衝區夠大且是這裡配的。
    unsafe {
        let count = removed.GetCount().unwrap_or(0);
        for i in 0..count {
            if let Ok(link) = removed.GetAt::<IShellLinkW>(i) {
                let mut buffer = [0u16; 1024];
                if link.GetArguments(&mut buffer).is_ok() {
                    let len = buffer.iter().position(|c| *c == 0).unwrap_or(buffer.len());
                    out.push(String::from_utf16_lossy(&buffer[..len]));
                }
            }
        }
    }
    out
}

/// 一個跳躍清單項目：執行 `exe args`，標題 `title`，提示文字是完整路徑，圖示用 FEditor 的。
unsafe fn make_link(exe: &PathBuf, args: &str, title: &str, description: &str) -> Result<IShellLinkW> {
    // SAFETY: 標準的 COM 呼叫；寬字串在呼叫期間都活著。
    unsafe {
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
        let exe_text = wide(&exe.to_string_lossy());
        link.SetPath(PCWSTR(exe_text.as_ptr()))?;
        link.SetIconLocation(PCWSTR(exe_text.as_ptr()), 0)?;
        let args = wide(args);
        link.SetArguments(PCWSTR(args.as_ptr()))?;
        let description = wide(description);
        link.SetDescription(PCWSTR(description.as_ptr()))?;
        // 清單上顯示的名稱要用屬性（PKEY_Title）設定，IShellLink 本身沒有「名稱」
        let store: IPropertyStore = link.cast()?;
        let value = PROPVARIANT::from(title);
        store.SetValue(&PKEY_Title, &value)?;
        store.Commit()?;
        Ok(link)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arguments_are_quoted_so_paths_with_spaces_stay_one_argument() {
        assert_eq!(quote_argument(r"C:\My Docs\a b.txt"), r#""C:\My Docs\a b.txt""#);
    }

    /// 真的動到這個測試程式自己的跳躍清單（不是 FEditor 的，AppID 依執行檔路徑算，所以互不影響），
    /// 確認整串 COM 呼叫能跑完。平常不跑（CI 的非互動工作階段不保證有殼層）：`cargo test -- --ignored jump_list`。
    #[test]
    #[ignore = "會改動使用者的跳躍清單；手動執行"]
    fn the_whole_com_sequence_runs_against_the_real_shell() {
        let dir = std::env::temp_dir();
        let file = dir.join("feditor jump list 測試.txt");
        std::fs::write(&file, "x").unwrap();
        let list = vec![file.to_string_lossy().into_owned(), r"C:\這個檔案不存在.txt".to_owned()];
        rebuild(&list).expect("跳躍清單更新失敗");
        rebuild(&[]).expect("清除跳躍清單失敗");
        let _ = std::fs::remove_file(file);
    }
}
