//! Windows：視窗開啟後，明確把 exe 內嵌的圖示設成視窗的大圖示與小圖示。
//!
//! winit 只替標題列設了小圖示（用 PNG 轉出來的），工作列按鈕用的大圖示沒有人設，
//! 結果工作列顯示成 Windows 預設的「空白視窗」圖示。這裡直接用 exe 的圖示資源（見 `build.rs`），
//! 並依視窗所在螢幕的縮放比例選大小，工作列在高 DPI 下才不會糊。

use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicBool, Ordering};
use windows_sys::Win32::Foundation::{HWND, LPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::GetCurrentProcessId;
use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GW_OWNER, GetWindow, GetWindowThreadProcessId, ICON_BIG, ICON_SMALL, IMAGE_ICON, IsWindowVisible,
    LR_SHARED, LoadImageW, SendMessageW, WM_SETICON,
};

/// `build.rs` 內嵌的圖示資源編號。
const ICON_RESOURCE_ID: usize = 1;

static APPLIED: AtomicBool = AtomicBool::new(false);

/// 找出這個行程的第一個可見的最上層視窗（沒有擁有者）。
unsafe extern "system" fn find_own_window(hwnd: HWND, found: LPARAM) -> i32 {
    let mut pid = 0;
    // SAFETY: hwnd 是系統列舉出來的有效視窗；found 是呼叫端傳進來的 HWND 指標
    unsafe {
        GetWindowThreadProcessId(hwnd, &mut pid);
        if pid == GetCurrentProcessId() && IsWindowVisible(hwnd) != 0 && GetWindow(hwnd, GW_OWNER).is_null() {
            *(found as *mut HWND) = hwnd;
            return 0; // 找到了，停止列舉
        }
    }
    1
}

/// 設好視窗的大小圖示；視窗還沒出現就先不動，下次再試。成功之後不會重複做。
pub fn apply_once() {
    if APPLIED.load(Ordering::Relaxed) {
        return;
    }
    let mut hwnd: HWND = null_mut();
    // SAFETY: 回呼函式只會寫入 hwnd 這個區域變數
    unsafe { EnumWindows(Some(find_own_window), &mut hwnd as *mut HWND as LPARAM) };
    if hwnd.is_null() {
        return;
    }
    // SAFETY: hwnd 是剛找到的自己的視窗；圖示資源在 exe 裡，LR_SHARED 的圖示由系統管理，不用釋放
    unsafe {
        let dpi = GetDpiForWindow(hwnd).max(96) as i32;
        let instance = GetModuleHandleW(null());
        for (kind, base) in [(ICON_BIG, 32), (ICON_SMALL, 16)] {
            let size = base * dpi / 96;
            let icon = LoadImageW(instance, ICON_RESOURCE_ID as *const u16, IMAGE_ICON, size, size, LR_SHARED);
            if !icon.is_null() {
                SendMessageW(hwnd, WM_SETICON, kind as usize, icon as LPARAM);
            }
        }
    }
    APPLIED.store(true, Ordering::Relaxed);
}
