//! 把最近開啟的檔案告訴作業系統：
//! - **Windows**：工作列上的程式圖示按右鍵的「跳躍清單」，多一個「最近開啟」類別，點一下就用 FEditor 開那個檔案
//!   （已經有 FEditor 在執行時，檔案會交給那個視窗，見 `single_instance`）。
//! - **macOS**：Dock 圖示按右鍵（以及系統的最近項目）列出最近開啟的文件，點一下會送「開啟文件」事件，
//!   由 `macos_open` 接手。macOS 自己管那份清單，我們只負責「通知有新的文件」與「清除」。
//! - **其他系統**：沒有對應的功能，什麼也不做。
//!
//! 都在背景或呼叫端很快的地方做，失敗一律忽略（跳躍清單壞了不能影響編輯）。

use std::path::Path;

/// 清單變了（開了新檔案、清除、程式啟動時同步一次）：更新作業系統那邊。
/// `list` 是完整的清單（最近的在最前面）；`added` 是這次新加進去的檔案（沒有就是 `None`）。
pub fn update(list: &[String], added: Option<&Path>) {
    #[cfg(windows)]
    {
        let _ = added;
        crate::jump_list::update(list);
    }
    #[cfg(target_os = "macos")]
    {
        let _ = list;
        if let Some(path) = added {
            macos::note(path);
        }
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = (list, added);
    }
}

/// 使用者清除了清單。
pub fn clear() {
    #[cfg(windows)]
    crate::jump_list::update(&[]);
    #[cfg(target_os = "macos")]
    macos::clear();
}

#[cfg(target_os = "macos")]
mod macos {
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};
    use objc2_foundation::{NSString, NSURL};
    use std::path::Path;

    /// 通知 NSDocumentController 有一個新的最近文件：Dock 選單與系統的最近項目會自己更新。
    pub fn note(path: &Path) {
        let text = NSString::from_str(&path.to_string_lossy());
        // SAFETY: 標準的 Objective-C 呼叫；sharedDocumentController 在主執行緒之外也可以用，參數是有效的 NSURL。
        unsafe {
            let url = NSURL::fileURLWithPath(&text);
            let controller: Retained<AnyObject> = msg_send![class!(NSDocumentController), sharedDocumentController];
            let _: () = msg_send![&*controller, noteNewRecentDocumentURL: &*url];
        }
    }

    /// 清除系統的最近文件。
    pub fn clear() {
        // SAFETY: 同上；參數是 nil（發送者）。
        unsafe {
            let controller: Retained<AnyObject> = msg_send![class!(NSDocumentController), sharedDocumentController];
            let _: () = msg_send![&*controller, clearRecentDocuments: std::ptr::null::<AnyObject>()];
        }
    }
}
