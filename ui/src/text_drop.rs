//! 從別的程式把「文字」拖進視窗（Windows）。
//!
//! winit 登記的 OLE 放置目標只收檔案（`CF_HDROP`），文字會被拒絕（游標變成禁止）。
//! 這裡換上自己的放置目標：資料是檔案就原封不動轉給 winit 原本的那一個（行為不變），
//! 是文字就自己讀出來，放進佇列，由 `App` 在下一個畫面取走。

use std::cell::Cell;
use std::sync::Mutex;

use eframe::egui;
use windows::Win32::Foundation::{HANDLE, HWND, POINT, POINTL};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, CoCreateInstance, DVASPECT_CONTENT, FORMATETC, IDataObject, TYMED_HGLOBAL,
};
use windows::Win32::System::Memory::{GlobalLock, GlobalUnlock};
use windows::Win32::System::Ole::{
    DROPEFFECT, DROPEFFECT_COPY, DROPEFFECT_NONE, IDropTarget, IDropTarget_Impl, RegisterDragDrop,
    ReleaseStgMedium, RevokeDragDrop,
};
use windows::Win32::System::SystemServices::MODIFIERKEYS_FLAGS;
use windows::Win32::UI::Shell::{CLSID_DragDropHelper, IDropTargetHelper};
use windows::Win32::UI::WindowsAndMessaging::GetPropW;
use windows::core::{Interface, Ref, Result, implement, w};

const CF_UNICODETEXT: u16 = 13;
const CF_HDROP: u16 = 15;

/// 放下的文字與放開時的螢幕座標（像素）。
static PENDING: Mutex<Vec<(String, (i32, i32))>> = Mutex::new(Vec::new());

/// 取走放下的文字，位置換算成視窗內的座標（點）。
pub fn take(ctx: &egui::Context) -> Vec<(String, egui::Pos2)> {
    let items = std::mem::take(&mut *PENDING.lock().unwrap_or_else(|e| e.into_inner()));
    items
        .into_iter()
        .map(|(text, (x, y))| (text, crate::screen_to_window(ctx, x, y)))
        .collect()
}

/// 文字正拖在視窗上方時，游標的螢幕座標（像素）；沒有在拖就是 `None`。
static HOVER: Mutex<Option<(i32, i32)>> = Mutex::new(None);

fn set_hover(value: Option<(i32, i32)>, ctx: &egui::Context) {
    *HOVER.lock().unwrap_or_else(|e| e.into_inner()) = value;
    ctx.request_repaint();
}

/// 文字正拖在視窗上方時，游標在視窗內的位置（點）：畫面用來標出放開後會落在哪裡。
pub fn hover(ctx: &egui::Context) -> Option<egui::Pos2> {
    let (x, y) = (*HOVER.lock().unwrap_or_else(|e| e.into_inner()))?;
    Some(crate::screen_to_window(ctx, x, y))
}

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    None,
    Files,
    Text,
}

#[implement(IDropTarget)]
struct TextDropTarget {
    /// winit 原本登記的放置目標：處理檔案。
    files: IDropTarget,
    ctx: egui::Context,
    mode: Cell<Mode>,
    hwnd: HWND,
    /// 系統的拖曳影像協助物件：放置目標要呼叫它，來源程式（瀏覽器等）的半透明文字影像才會顯示在這個視窗上。
    helper: Option<IDropTargetHelper>,
}

fn has_format(data: &IDataObject, format: u16) -> bool {
    let fmt = FORMATETC {
        cfFormat: format,
        ptd: std::ptr::null_mut(),
        dwAspect: DVASPECT_CONTENT.0 as u32,
        lindex: -1,
        tymed: TYMED_HGLOBAL.0 as u32,
    };
    // SAFETY: `fmt` 在呼叫期間有效
    unsafe { data.QueryGetData(&fmt) }.is_ok()
}

fn read_text(data: &IDataObject) -> Option<String> {
    let fmt = FORMATETC {
        cfFormat: CF_UNICODETEXT,
        ptd: std::ptr::null_mut(),
        dwAspect: DVASPECT_CONTENT.0 as u32,
        lindex: -1,
        tymed: TYMED_HGLOBAL.0 as u32,
    };
    // SAFETY: 依 COM 的規則取資料、鎖住全域記憶體讀到結尾的 0、解鎖並釋放
    unsafe {
        let mut medium = data.GetData(&fmt).ok()?;
        let hglobal = medium.u.hGlobal;
        let ptr = GlobalLock(hglobal) as *const u16;
        let text = if ptr.is_null() {
            None
        } else {
            let mut len = 0;
            while *ptr.add(len) != 0 {
                len += 1;
            }
            Some(String::from_utf16_lossy(std::slice::from_raw_parts(
                ptr, len,
            )))
        };
        let _ = GlobalUnlock(hglobal);
        ReleaseStgMedium(&mut medium);
        text
    }
}

impl IDropTarget_Impl for TextDropTarget_Impl {
    fn DragEnter(
        &self,
        data: Ref<'_, IDataObject>,
        keys: MODIFIERKEYS_FLAGS,
        pt: &POINTL,
        effect: *mut DROPEFFECT,
    ) -> Result<()> {
        let Some(data_object) = data.as_ref() else {
            return Ok(());
        };
        if has_format(data_object, CF_HDROP) {
            self.mode.set(Mode::Files);
            return unsafe { self.files.DragEnter(data_object, keys, *pt, effect) };
        }
        let text = has_format(data_object, CF_UNICODETEXT);
        self.mode.set(if text { Mode::Text } else { Mode::None });
        if text {
            set_hover(Some((pt.x, pt.y)), &self.ctx);
            if let Some(helper) = &self.helper {
                // SAFETY: 參數都在呼叫期間有效
                let _ = unsafe {
                    helper.DragEnter(
                        self.hwnd,
                        data_object,
                        &POINT { x: pt.x, y: pt.y },
                        DROPEFFECT_COPY,
                    )
                };
            }
        }
        // SAFETY: OLE 給的輸出參數
        unsafe {
            *effect = if text {
                DROPEFFECT_COPY
            } else {
                DROPEFFECT_NONE
            }
        };
        Ok(())
    }

    fn DragOver(
        &self,
        keys: MODIFIERKEYS_FLAGS,
        pt: &POINTL,
        effect: *mut DROPEFFECT,
    ) -> Result<()> {
        match self.mode.get() {
            Mode::Files => unsafe { self.files.DragOver(keys, *pt, effect) },
            mode => {
                if mode == Mode::Text {
                    set_hover(Some((pt.x, pt.y)), &self.ctx);
                    if let Some(helper) = &self.helper {
                        // SAFETY: 參數都在呼叫期間有效
                        let _ = unsafe {
                            helper.DragOver(&POINT { x: pt.x, y: pt.y }, DROPEFFECT_COPY)
                        };
                    }
                }
                // SAFETY: OLE 給的輸出參數
                unsafe {
                    *effect = if mode == Mode::Text {
                        DROPEFFECT_COPY
                    } else {
                        DROPEFFECT_NONE
                    }
                };
                Ok(())
            }
        }
    }

    fn DragLeave(&self) -> Result<()> {
        let mode = self.mode.replace(Mode::None);
        if mode == Mode::Files {
            unsafe { self.files.DragLeave() }
        } else {
            set_hover(None, &self.ctx);
            if mode == Mode::Text
                && let Some(helper) = &self.helper
            {
                let _ = unsafe { helper.DragLeave() };
            }
            Ok(())
        }
    }

    fn Drop(
        &self,
        data: Ref<'_, IDataObject>,
        keys: MODIFIERKEYS_FLAGS,
        pt: &POINTL,
        effect: *mut DROPEFFECT,
    ) -> Result<()> {
        let mode = self.mode.replace(Mode::None);
        set_hover(None, &self.ctx);
        let Some(data_object) = data.as_ref() else {
            return Ok(());
        };
        if mode == Mode::Files {
            return unsafe { self.files.Drop(data_object, keys, *pt, effect) };
        }
        if mode == Mode::Text
            && let Some(helper) = &self.helper
        {
            let _ =
                unsafe { helper.Drop(data_object, &POINT { x: pt.x, y: pt.y }, DROPEFFECT_COPY) };
        }
        if mode == Mode::Text
            && let Some(text) = read_text(data_object).filter(|t| !t.is_empty())
        {
            PENDING
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push((text, (pt.x, pt.y)));
            self.ctx.request_repaint();
            // SAFETY: OLE 給的輸出參數
            unsafe { *effect = DROPEFFECT_COPY };
        } else {
            unsafe { *effect = DROPEFFECT_NONE };
        }
        Ok(())
    }
}

/// 把視窗 `hwnd` 的放置目標換成能收文字的那一個。找不到 winit 登記的目標（或登記失敗）就維持原狀。
pub fn install(hwnd: isize, ctx: egui::Context) {
    let hwnd = HWND(hwnd as *mut _);
    // SAFETY: OLE 把放置目標記在視窗屬性 `OleDropTargetInterface` 裡；先取一份（加參考計數）再換掉登記
    unsafe {
        let prop: HANDLE = GetPropW(hwnd, w!("OleDropTargetInterface"));
        if prop.0.is_null() {
            return;
        }
        let Some(original) = IDropTarget::from_raw_borrowed(&prop.0).cloned() else {
            return;
        };
        if RevokeDragDrop(hwnd).is_err() {
            return;
        }
        let helper = CoCreateInstance::<_, IDropTargetHelper>(
            &CLSID_DragDropHelper,
            None,
            CLSCTX_INPROC_SERVER,
        )
        .ok();
        let ours: IDropTarget = TextDropTarget {
            files: original.clone(),
            ctx,
            mode: Cell::new(Mode::None),
            hwnd,
            helper,
        }
        .into();
        if RegisterDragDrop(hwnd, &ours).is_err() {
            // 登記失敗：把 winit 的放回去
            let _ = RegisterDragDrop(hwnd, &original);
        }
    }
}
