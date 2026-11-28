//! Windows：自繪標題列需要的原生視窗行為（見 `window_chrome`）。
//!
//! 系統標題列拿掉之後，winit 的無框視窗只有最上緣能拖曳縮放，也沒有 Windows 11 的「貼齊版面」選單。
//! 這裡掛一個自己的視窗程序，以 `WM_NCCALCSIZE` 保留原生側邊與下邊框，只移除標題列；
//! 不能把整個非客戶區清空，否則即使回報 `HTMAXBUTTON`，Windows 11 仍不會顯示貼齊版面。
//! 再攔下 `WM_NCHITTEST` 回報：
//!
//! - 視窗四邊四角：縮放區，游標形狀、拖曳縮放都由系統處理；
//! - 自繪的最大化鈕：`HTMAXBUTTON`，滑鼠停上去系統就會跳出貼齊版面選單。
//!   這個區域的滑鼠訊息會變成「非客戶區」訊息，不會到 egui，所以懸停與點擊都在這裡處理。

use eframe::egui;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicIsize, Ordering};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::ScreenToClient;
use windows_sys::Win32::UI::HiDpi::{GetDpiForWindow, GetSystemMetricsForDpi};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallWindowProcW, DefWindowProcW, GWLP_WNDPROC, GetCursorPos, GetWindowRect, HTBOTTOM,
    HTBOTTOMLEFT, HTBOTTOMRIGHT, HTCLIENT, HTLEFT, HTMAXBUTTON, HTRIGHT, HTTOP, HTTOPLEFT,
    HTTOPRIGHT, IsZoomed, NCCALCSIZE_PARAMS, PostMessageW, SC_MAXIMIZE, SC_RESTORE,
    SM_CXPADDEDBORDER, SM_CYSIZEFRAME, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    SWP_NOZORDER, SetWindowLongPtrW, SetWindowPos, WM_MOUSEMOVE, WM_NCCALCSIZE, WM_NCHITTEST,
    WM_NCLBUTTONDOWN, WM_NCLBUTTONUP, WM_NCMOUSELEAVE, WM_NCMOUSEMOVE, WM_SYSCOMMAND,
};

/// 縮放區的寬度（96 DPI 的像素，實際用的會依螢幕縮放放大）。
const RESIZE_BORDER: i32 = 6;

/// 原本的視窗程序（winit 的）。
static ORIGINAL: AtomicIsize = AtomicIsize::new(0);
static CONTEXT: OnceLock<egui::Context> = OnceLock::new();
/// 最大化鈕在視窗客戶區裡的位置（實體像素）：左、上、右、下。全 0 代表還沒畫出來。
static MAX_BUTTON: [AtomicI32; 4] = [
    AtomicI32::new(0),
    AtomicI32::new(0),
    AtomicI32::new(0),
    AtomicI32::new(0),
];
static MAX_HOVER: AtomicBool = AtomicBool::new(false);
/// 掛了視窗程序的視窗。
static HWND_STORED: AtomicIsize = AtomicIsize::new(0);

/// 掛上視窗程序。要在建立視窗的執行緒呼叫，每個視窗只做一次。
pub fn install(hwnd: isize, ctx: egui::Context) {
    if ORIGINAL.load(Ordering::Relaxed) != 0 {
        return;
    }
    let _ = CONTEXT.set(ctx);
    HWND_STORED.store(hwnd, Ordering::Relaxed);
    // SAFETY: hwnd 是 eframe 剛建好、屬於這個行程的視窗；新的視窗程序簽章和系統要求的一致
    let previous =
        unsafe { SetWindowLongPtrW(hwnd as HWND, GWLP_WNDPROC, frame_proc as *const () as isize) };
    ORIGINAL.store(previous, Ordering::Relaxed);
    if previous != 0 {
        let flags = SWP_FRAMECHANGED | SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER;
        // SAFETY: hwnd 屬於這個執行緒；重新計算邊框，不移動、縮放或啟用視窗。
        unsafe {
            SetWindowPos(hwnd as HWND, std::ptr::null_mut(), 0, 0, 0, 0, flags);
        }
    }
}

/// 每個畫面都告訴這裡最大化鈕現在在哪（以 egui 的點為單位，客戶區座標）。
pub fn set_max_button(rect: egui::Rect, pixels_per_point: f32) {
    let px = |v: f32| (v * pixels_per_point).round() as i32;
    for (slot, value) in MAX_BUTTON.iter().zip([
        px(rect.left()),
        px(rect.top()),
        px(rect.right()),
        px(rect.bottom()),
    ]) {
        slot.store(value, Ordering::Relaxed);
    }
}

/// 滑鼠是不是停在最大化鈕上（這時滑鼠訊息不會到 egui，要靠這個旗標畫出懸停的底色）。
/// 旗標是「進入」時設的；離開沒有可靠的通知，所以旗標亮著的時候每個畫面都重新量一次游標位置。
pub fn max_button_hovered() -> bool {
    if !MAX_HOVER.load(Ordering::Relaxed) {
        return false;
    }
    let mut cursor = POINT { x: 0, y: 0 };
    let hwnd = HWND_STORED.load(Ordering::Relaxed) as HWND;
    // SAFETY: cursor 是有效的輸出位置
    let over =
        unsafe { GetCursorPos(&mut cursor) } != 0 && over_max_button(hwnd, cursor.x, cursor.y);
    if over {
        if let Some(ctx) = CONTEXT.get() {
            ctx.request_repaint_after(std::time::Duration::from_millis(60)); // 繼續盯著，直到滑鼠離開
        }
    } else {
        MAX_HOVER.store(false, Ordering::Relaxed);
    }
    over
}

fn set_hover(hovered: bool) {
    if MAX_HOVER.swap(hovered, Ordering::Relaxed) != hovered
        && let Some(ctx) = CONTEXT.get()
    {
        ctx.request_repaint();
    }
}

/// 螢幕座標 (x, y) 落在視窗哪個縮放區；不在縮放區回傳 None。
fn resize_hit(hwnd: HWND, x: i32, y: i32) -> Option<u32> {
    let mut rect = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    // SAFETY: hwnd 是有效的視窗；rect 是有效的輸出位置
    let (ok, dpi) = unsafe { (GetWindowRect(hwnd, &mut rect), GetDpiForWindow(hwnd)) };
    if ok == 0 {
        return None;
    }
    let edge = (RESIZE_BORDER as f32 * dpi.max(96) as f32 / 96.0).round() as i32;
    let corner = edge * 2; // 四個角比邊好抓一點
    let (left, right) = (x - rect.left, rect.right - 1 - x);
    let (top, bottom) = (y - rect.top, rect.bottom - 1 - y);
    let near = |a: i32, b: i32| (a < edge && b < corner) || (a < corner && b < edge);
    Some(if near(left, top) {
        HTTOPLEFT
    } else if near(right, top) {
        HTTOPRIGHT
    } else if near(left, bottom) {
        HTBOTTOMLEFT
    } else if near(right, bottom) {
        HTBOTTOMRIGHT
    } else if left < edge {
        HTLEFT
    } else if right < edge {
        HTRIGHT
    } else if top < edge {
        HTTOP
    } else if bottom < edge {
        HTBOTTOM
    } else {
        return None;
    })
}

fn over_max_button(hwnd: HWND, x: i32, y: i32) -> bool {
    let [left, top, right, bottom] = [0, 1, 2, 3].map(|i| MAX_BUTTON[i].load(Ordering::Relaxed));
    if right <= left {
        return false;
    }
    let mut point = POINT { x, y };
    // SAFETY: hwnd 是有效的視窗；point 是有效的輸入輸出位置
    if unsafe { ScreenToClient(hwnd, &mut point) } == 0 {
        return false;
    }
    point.x >= left && point.x < right && point.y >= top && point.y < bottom
}

unsafe extern "system" fn frame_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // SAFETY: 這個函式只會被系統當視窗程序呼叫，hwnd 有效；ORIGINAL 是 install 存下的原本的視窗程序
    unsafe {
        let is_max_button = wparam == HTMAXBUTTON as usize;
        match msg {
            WM_NCCALCSIZE => {
                if wparam == 0 {
                    return 0;
                }
                // 依 Windows Terminal 的 _OnNcCalcSize，先讓系統計算原生邊框，
                // 保留左右及下邊框，只把標題列收回客戶區供 egui 繪製。
                // https://github.com/microsoft/terminal/blob/main/src/cascadia/WindowsTerminal/NonClientIslandWindow.cpp
                let params = lparam as *mut NCCALCSIZE_PARAMS;
                let top = (*params).rgrc[0].top;
                let result = DefWindowProcW(hwnd, msg, wparam, lparam);
                if result != 0 {
                    return result;
                }
                let border = if IsZoomed(hwnd) != 0 {
                    // 最大化時原生縮放邊框會落在螢幕外，頂部也要扣掉，避免內容被裁切。
                    let dpi = GetDpiForWindow(hwnd);
                    GetSystemMetricsForDpi(SM_CYSIZEFRAME, dpi)
                        + GetSystemMetricsForDpi(SM_CXPADDEDBORDER, dpi)
                } else {
                    0
                };
                (*params).rgrc[0].top = top + border;
                return 0;
            }
            WM_NCHITTEST => {
                // lParam 低 16 位是 x、高 16 位是 y（有號：多螢幕時座標可能是負的）
                let x = (lparam & 0xFFFF) as i16 as i32;
                let y = ((lparam >> 16) & 0xFFFF) as i16 as i32;
                if IsZoomed(hwnd) == 0
                    && let Some(hit) = resize_hit(hwnd, x, y)
                {
                    return hit as LRESULT;
                }
                if over_max_button(hwnd, x, y) {
                    return HTMAXBUTTON as LRESULT;
                }
                // 標題列已由 egui 接管，不讓系統命中原生標題列或隱藏的按鈕。
                return HTCLIENT as LRESULT;
            }
            WM_NCMOUSEMOVE if is_max_button => {
                // 由系統追蹤 hover；繼續轉送非客戶區訊息，讓 Windows 顯示貼齊面板。
                set_hover(true);
            }
            WM_NCMOUSELEAVE | WM_MOUSEMOVE => set_hover(false),
            // 自己處理按下與放開：按下時不交給系統（它會去畫不存在的標題列按鈕），放開時送出最大化或還原
            WM_NCLBUTTONDOWN if is_max_button => return 0,
            WM_NCLBUTTONUP if is_max_button => {
                let command = if IsZoomed(hwnd) != 0 {
                    SC_RESTORE
                } else {
                    SC_MAXIMIZE
                };
                PostMessageW(hwnd, WM_SYSCOMMAND, command as usize, 0);
                return 0;
            }
            _ => {}
        }
        let original: isize = ORIGINAL.load(Ordering::Relaxed);
        // SAFETY: original 是 SetWindowLongPtrW 回傳的原視窗程序
        let original: windows_sys::Win32::UI::WindowsAndMessaging::WNDPROC =
            std::mem::transmute(original);
        CallWindowProcW(original, hwnd, msg, wparam, lparam)
    }
}
