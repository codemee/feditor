//! shell 清單裡每一項前面的小圖示，方便分辨 PowerShell、cmd、Git Bash、WSL…
//!
//! Windows 上向作業系統（Shell API）要執行檔本身的圖示，所以畫出來就是檔案總管裡看到的那個；
//! 要不到（macOS、Linux 的 shell 沒有圖示，或讀取失敗）就畫程式自己的終端機圖示。

use std::collections::HashMap;

use eframe::egui::{self, Color32, Sense, TextureHandle, Ui, vec2};

use crate::icons::{self, Icon};
use crate::terminal::ShellOption;

/// 清單上一列的高度與圖示的大小（邏輯像素）。
const ROW_HEIGHT: f32 = 24.0;
const ICON_SIZE: f32 = 16.0;
const PAD: f32 = 6.0;

/// 每個 shell 的圖示材質；第一次要畫的時候才去讀，結果（包含「要不到」）會記起來。
#[derive(Default)]
pub struct ShellIcons {
    // RefCell：畫面函式只有 &self，讀圖示是「第一次才做」的快取，不算改動 App 的狀態
    textures: std::cell::RefCell<HashMap<String, Option<TextureHandle>>>,
}

impl ShellIcons {
    /// 清單裡某一項的圖示。
    pub fn get(&self, ctx: &egui::Context, option: &ShellOption) -> Option<TextureHandle> {
        self.get_by_command(ctx, &option.command)
    }

    /// 執行這個指令的 shell 的圖示；指令是空字串時用系統預設 shell 的（終端機分頁的標籤用）。
    pub fn get_by_command(&self, ctx: &egui::Context, command: &str) -> Option<TextureHandle> {
        self.textures.borrow_mut().entry(command.to_owned()).or_insert_with(|| load(ctx, command)).clone()
    }
}

#[cfg(windows)]
fn load(ctx: &egui::Context, command: &str) -> Option<TextureHandle> {
    use crate::terminal;
    let effective = if command.trim().is_empty() { terminal::system_default_shell().program } else { command.to_owned() };
    let path = resolve_shim(&terminal::program_path(&effective)?);
    let image = exe_icon(&path)?;
    Some(ctx.load_texture(format!("shell-icon:{command}"), image, egui::TextureOptions::LINEAR))
}

/// Scoop 的 shim（`shims\pwsh.exe`）是個通用的小執行檔，圖示是一個藍色的視窗，不是 shell 本身的；
/// 旁邊同名的 `.shim` 文字檔寫著真正的程式（`path = "C:\...\pwsh.exe"`），圖示要用那個。不是 shim 就原樣回傳。
#[cfg(any(windows, test))]
fn resolve_shim(exe: &std::path::Path) -> std::path::PathBuf {
    let target = std::fs::read_to_string(exe.with_extension("shim")).ok().and_then(|text| {
        text.lines().find_map(|line| {
            let (key, value) = line.split_once('=')?;
            (key.trim() == "path").then(|| std::path::PathBuf::from(value.trim().trim_matches('"')))
        })
    });
    target.filter(|t| t.is_file()).unwrap_or_else(|| exe.to_path_buf())
}

#[cfg(not(windows))]
fn load(_ctx: &egui::Context, _command: &str) -> Option<TextureHandle> {
    None
}

/// 在 `rect` 畫 shell 的圖示：有材質就畫它（`tint` 可以調淡），沒有就畫程式自己的終端機圖示（顏色用 `fallback`）。
pub fn paint_icon(ui: &Ui, texture: Option<&TextureHandle>, rect: egui::Rect, tint: Color32, fallback: Color32) {
    match texture {
        Some(texture) => {
            ui.painter().image(texture.id(), rect, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), tint);
        }
        None => icons::paint(ui, Icon::Terminal, rect, fallback),
    }
}

/// 清單上的一列：圖示加名稱，`selected` 時有選取的底色（和 `selectable_label` 的樣子一致）。
/// `texture` 是 `None` 時畫程式自己的終端機圖示。
pub fn shell_row(ui: &mut Ui, texture: Option<&TextureHandle>, label: &str, selected: bool) -> egui::Response {
    let font = egui::TextStyle::Button.resolve(ui.style());
    let galley = ui.painter().layout_no_wrap(label.to_owned(), font, Color32::WHITE);
    let width = (PAD * 3.0 + ICON_SIZE + galley.size().x).max(ui.available_width());
    let (rect, response) = ui.allocate_exact_size(vec2(width, ROW_HEIGHT), Sense::click());
    if ui.is_rect_visible(rect) {
        let visuals = ui.style().interact_selectable(&response, selected);
        if selected || response.hovered() || response.has_focus() {
            ui.painter().rect_filled(rect, egui::CornerRadius::same(3), visuals.weak_bg_fill);
        }
        let icon_rect = egui::Rect::from_center_size(egui::pos2(rect.left() + PAD + ICON_SIZE / 2.0, rect.center().y), vec2(ICON_SIZE, ICON_SIZE));
        paint_icon(ui, texture, icon_rect, Color32::WHITE, visuals.fg_stroke.color);
        let text_pos = egui::pos2(rect.left() + PAD * 2.0 + ICON_SIZE, rect.center().y - galley.size().y / 2.0);
        ui.painter().galley_with_override_text_color(text_pos, galley, visuals.fg_stroke.color);
    }
    response
}

/// 執行檔的圖示（小圖示，通常是 16×16）。向 Windows 的 Shell 要，轉成 RGBA 的影像。
#[cfg(windows)]
pub fn exe_icon(path: &std::path::Path) -> Option<egui::ColorImage> {
    use std::ffi::c_void;
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Graphics::Gdi::{
        BI_RGB, BITMAP, BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS, DeleteObject, GetDC, GetDIBits, GetObjectW, HBITMAP, ReleaseDC,
    };
    use windows_sys::Win32::UI::Shell::{SHFILEINFOW, SHGFI_ICON, SHGFI_SMALLICON, SHGetFileInfoW};
    use windows_sys::Win32::UI::WindowsAndMessaging::{DestroyIcon, GetIconInfo, ICONINFO};

    /// 把圖示的彩色位圖（與遮罩）讀成 RGBA。
    unsafe fn read(color: HBITMAP, mask: HBITMAP) -> Option<egui::ColorImage> {
        unsafe {
            if color.is_null() {
                return None;
            }
            let mut bm: BITMAP = std::mem::zeroed();
            if GetObjectW(color, std::mem::size_of::<BITMAP>() as i32, &mut bm as *mut _ as *mut c_void) == 0 {
                return None;
            }
            let (w, h) = (bm.bmWidth, bm.bmHeight);
            if w <= 0 || h <= 0 || w > 256 || h > 256 {
                return None;
            }
            let header = BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h, // 負的：由上往下排
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB,
                biSizeImage: 0,
                biXPelsPerMeter: 0,
                biYPelsPerMeter: 0,
                biClrUsed: 0,
                biClrImportant: 0,
            };
            let mut info: BITMAPINFO = std::mem::zeroed();
            info.bmiHeader = header;
            let hdc = GetDC(std::ptr::null_mut());
            let mut bgra = vec![0u8; (w * h * 4) as usize];
            let rows = GetDIBits(hdc, color, 0, h as u32, bgra.as_mut_ptr() as *mut c_void, &mut info, DIB_RGB_COLORS);
            // 沒有透明度資訊的舊式圖示：透明的部分在遮罩位圖裡（黑色是不透明、白色是透明）
            let mut mask_px = vec![0u8; (w * h * 4) as usize];
            let mut have_mask = false;
            if !mask.is_null() {
                let mut mask_info: BITMAPINFO = std::mem::zeroed();
                mask_info.bmiHeader = header;
                have_mask = GetDIBits(hdc, mask, 0, h as u32, mask_px.as_mut_ptr() as *mut c_void, &mut mask_info, DIB_RGB_COLORS) != 0;
            }
            ReleaseDC(std::ptr::null_mut(), hdc);
            if rows == 0 {
                return None;
            }
            let has_alpha = bgra.chunks(4).any(|p| p[3] != 0);
            let mut rgba = Vec::with_capacity(bgra.len());
            for (i, p) in bgra.chunks(4).enumerate() {
                let alpha = if has_alpha {
                    p[3]
                } else if have_mask && mask_px[i * 4] > 127 {
                    0
                } else {
                    255
                };
                rgba.extend_from_slice(&[p[2], p[1], p[0], alpha]);
            }
            Some(egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba))
        }
    }

    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    unsafe {
        let mut file: SHFILEINFOW = std::mem::zeroed();
        let ok = SHGetFileInfoW(wide.as_ptr(), 0, &mut file, std::mem::size_of::<SHFILEINFOW>() as u32, SHGFI_ICON | SHGFI_SMALLICON);
        if ok == 0 || file.hIcon.is_null() {
            return None;
        }
        let mut icon: ICONINFO = std::mem::zeroed();
        let image = if GetIconInfo(file.hIcon, &mut icon) != 0 {
            let image = read(icon.hbmColor, icon.hbmMask);
            if !icon.hbmColor.is_null() {
                DeleteObject(icon.hbmColor);
            }
            if !icon.hbmMask.is_null() {
                DeleteObject(icon.hbmMask);
            }
            image
        } else {
            None
        };
        DestroyIcon(file.hIcon);
        image
    }
}

#[cfg(test)]
mod tests {
    #[cfg(windows)]
    #[test]
    fn the_icon_of_a_real_windows_program_can_be_read_and_is_not_blank() {
        let system = std::env::var_os("SystemRoot").map(std::path::PathBuf::from).unwrap_or_else(|| "C:\\Windows".into());
        let cmd = system.join("System32").join("cmd.exe");
        let image = super::exe_icon(&cmd).expect("cmd.exe 有圖示");
        assert!(image.size[0] >= 8 && image.size[1] >= 8 && image.size[0] <= 64, "{:?}", image.size);
        let visible = image.pixels.iter().filter(|p| p.a() > 0).count();
        assert!(visible > image.pixels.len() / 10, "圖示不該是全透明的：{visible}/{}", image.pixels.len());
        assert!(image.pixels.iter().any(|p| p.a() > 0 && (p.r() != p.g() || p.g() != p.b() || p.r() > 0)), "要有看得見的顏色");
        // 不存在的檔案：沒有圖示，也不會 panic
        assert!(super::exe_icon(&system.join("no-such-program.exe")).is_none());
    }

    #[test]
    fn a_scoop_shim_points_at_the_real_program_for_its_icon() {
        let dir = std::env::temp_dir().join(format!("feditor-shim-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (shim, real) = (dir.join("tool.exe"), dir.join("real.exe"));
        std::fs::write(&shim, "x").unwrap();
        std::fs::write(&real, "x").unwrap();
        assert_eq!(super::resolve_shim(&shim), shim, "沒有 .shim 檔：原樣");
        std::fs::write(dir.join("tool.shim"), format!("path = \"{}\"\r\nargs = \r\n", real.display())).unwrap();
        assert_eq!(super::resolve_shim(&shim), real);
        std::fs::write(dir.join("tool.shim"), "path = \"Z:\\no\\such.exe\"").unwrap();
        assert_eq!(super::resolve_shim(&shim), shim, "目標不存在：原樣");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_shell_without_a_readable_icon_simply_has_no_texture() {
        // 找不到的程式：沒有路徑，也就不會嘗試讀圖示
        assert!(crate::terminal::program_path_in("definitely-not-a-shell", Some(std::ffi::OsStr::new("")), &|_| false).is_none());
    }
}
