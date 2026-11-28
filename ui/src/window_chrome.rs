//! 自繪的視窗標題列（目前只有 Windows）：系統標題列拿掉，改由主工具列兼任。
//!
//! - 最左邊是應用程式圖示，最右邊是最小化、最大化（還原）、關閉三顆鈕；
//! - 工具列的空白處可以拖曳視窗，雙按切換最大化。
//!
//! 視窗標題（工作列、Alt+Tab 看到的檔名）仍然照常設定，只是不畫在視窗裡。
//! macOS、Linux 保留系統標題列，不使用這裡的東西：自繪之後要自己處理視窗邊緣縮放，
//! 目前只有 Windows 做了（見 `win_frame`）。技術細節見 `docs/windows-titlebar.md`。

use eframe::egui::{
    self, Color32, CornerRadius, Pos2, Rect, Response, Sense, Stroke, Ui, ViewportCommand, vec2,
};

use crate::icons::BUTTON_HEIGHT;
use feditor_core::i18n::t;

/// 這個平台是否使用自繪標題列。
pub const CUSTOM: bool = cfg!(windows);

/// 視窗的最小寬度：放得下應用程式圖示、「更多」鈕、一點可拖曳的空白和三顆視窗按鈕。
/// 工具列放不下的按鈕會收進「更多」，所以不必寬到放下全部。
pub const MIN_WINDOW_WIDTH: f32 = 480.0;

/// 工具列右邊固定留給三顆視窗按鈕的寬度（只有自繪標題列才有）。
pub const RIGHT_WIDTH: f32 = if CUSTOM {
    3.0 * BUTTON_WIDTH + 12.0
} else {
    0.0
};

/// 視窗按鈕的寬度（高度和工具列按鈕一樣）。
const BUTTON_WIDTH: f32 = 40.0;
/// 應用程式圖示的邊長。
const ICON_SIZE: f32 = 24.0;
/// 關閉鈕滑鼠移上去的底色（Windows 慣用的紅色）。
const CLOSE_HOVER: Color32 = Color32::from_rgb(232, 17, 35);

/// 工具列最左邊的應用程式圖示。
pub fn app_icon(ui: &mut Ui) {
    // 先配一個和按鈕等高的格子，圖示再畫在正中央（直接 add 會貼著列的上緣）
    let (slot, _) = ui.allocate_exact_size(vec2(ICON_SIZE + 4.0, BUTTON_HEIGHT), Sense::hover());
    // 用 256 像素的大圖縮到這麼小，細線會糊掉；ico 裡有手工調過線條粗細的小尺寸，依實際像素數挑最接近的
    // （工作列、Alt+Tab 用的也是這些），而且圖放在整數像素的位置，才不會被插值弄糊
    let ppp = ui.ctx().pixels_per_point();
    let wanted = (ICON_SIZE * ppp).round();
    let source = if wanted <= 24.0 {
        egui::include_image!("../assets/icon-24.png")
    } else if wanted <= 32.0 {
        egui::include_image!("../assets/icon-32.png")
    } else {
        egui::include_image!("../assets/icon-48.png")
    };
    let snap = |v: f32| (v * ppp).round() / ppp;
    let size = wanted / ppp;
    let min = Pos2::new(
        snap(slot.center().x - size / 2.0),
        snap(slot.center().y - size / 2.0),
    );
    egui::Image::new(source).paint_at(ui, Rect::from_min_size(min, vec2(size, size)));
}

/// 把 `rect`（整條工具列）登記成拖曳視窗的區域。要在工具列的其他元件**之前**呼叫，
/// 這樣按鈕在它上面，點到按鈕不會變成拖曳視窗。
pub fn drag_area(ui: &mut Ui, rect: Rect) {
    let response = ui.interact(
        rect,
        ui.make_persistent_id("window_drag"),
        Sense::click_and_drag(),
    );
    let ctx = ui.ctx();
    if response.drag_started_by(egui::PointerButton::Primary) {
        ctx.send_viewport_cmd(ViewportCommand::StartDrag);
    }
    if response.double_clicked() {
        ctx.send_viewport_cmd(ViewportCommand::Maximized(!is_maximized(ctx)));
    }
}

fn is_maximized(ctx: &egui::Context) -> bool {
    ctx.input(|i| i.viewport().maximized.unwrap_or(false))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Minimize,
    Maximize,
    Restore,
    Close,
}

/// 最小化、最大化（還原）、關閉三顆鈕。放在由右往左排的版面裡：先放的在最右邊，所以先畫關閉鈕。
pub fn buttons(ui: &mut Ui) {
    let maximized = is_maximized(ui.ctx());
    if caption_button(ui, Kind::Close, t("關閉視窗")).clicked() {
        // 走一般的關閉流程：有未儲存的修改會先詢問
        ui.ctx().send_viewport_cmd(ViewportCommand::Close);
    }
    let (kind, tip) = if maximized {
        (Kind::Restore, t("還原視窗"))
    } else {
        (Kind::Maximize, t("最大化視窗"))
    };
    if caption_button(ui, kind, tip).clicked() {
        ui.ctx()
            .send_viewport_cmd(ViewportCommand::Maximized(!maximized));
    }
    if caption_button(ui, Kind::Minimize, t("最小化視窗")).clicked() {
        ui.ctx().send_viewport_cmd(ViewportCommand::Minimized(true));
    }
}

fn caption_button(ui: &mut Ui, kind: Kind, tip: &str) -> Response {
    let (rect, response) =
        ui.allocate_exact_size(vec2(BUTTON_WIDTH, BUTTON_HEIGHT), Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, tip));
    if ui.is_rect_visible(rect) {
        let visuals = ui.style().interact(&response);
        // Windows：最大化鈕上的滑鼠訊息由系統接手（為了貼齊版面選單），egui 看不到懸停，要問原生那邊
        let is_max = matches!(kind, Kind::Maximize | Kind::Restore);
        #[cfg(windows)]
        let native_hover = is_max && {
            crate::win_frame::set_max_button(rect, ui.ctx().pixels_per_point());
            crate::win_frame::max_button_hovered()
        };
        #[cfg(not(windows))]
        let native_hover = {
            let _ = is_max;
            false
        };
        let hot = response.hovered() || response.is_pointer_button_down_on() || native_hover;
        let close_hot = kind == Kind::Close && hot;
        if hot {
            let fill = if close_hot {
                CLOSE_HOVER
            } else {
                visuals.weak_bg_fill
            };
            ui.painter()
                .rect_filled(rect.shrink(1.0), CornerRadius::same(4), fill);
        }
        let color = if close_hot {
            Color32::WHITE
        } else {
            visuals.fg_stroke.color
        };
        draw_glyph(ui, kind, rect.center(), Stroke::new(1.0, color));
    }
    response.on_hover_text(tip)
}

/// 在 `c` 為中心的 10×10 範圍畫出圖形。座標取整數再加 0.5，1 像素的線才不會糊掉。
fn draw_glyph(ui: &Ui, kind: Kind, c: Pos2, stroke: Stroke) {
    let c = Pos2::new(c.x.round() + 0.5, c.y.round() + 0.5);
    let p = |dx: f32, dy: f32| Pos2::new(c.x + dx, c.y + dy);
    let painter = ui.painter();
    match kind {
        Kind::Minimize => {
            painter.line_segment([p(-5.0, 0.0), p(5.0, 0.0)], stroke);
        }
        Kind::Maximize => {
            painter.rect_stroke(
                Rect::from_center_size(c, vec2(10.0, 10.0)),
                CornerRadius::ZERO,
                stroke,
                egui::StrokeKind::Middle,
            );
        }
        Kind::Restore => {
            // 前面的方框，加上藏在它後面、露出右上角的那一個
            painter.rect_stroke(
                Rect::from_min_max(p(-5.0, -3.0), p(3.0, 5.0)),
                CornerRadius::ZERO,
                stroke,
                egui::StrokeKind::Middle,
            );
            painter.line(
                vec![
                    p(-3.0, -3.0),
                    p(-3.0, -5.0),
                    p(5.0, -5.0),
                    p(5.0, 3.0),
                    p(3.0, 3.0),
                ],
                stroke,
            );
        }
        Kind::Close => {
            painter.line_segment([p(-5.0, -5.0), p(5.0, 5.0)], stroke);
            painter.line_segment([p(-5.0, 5.0), p(5.0, -5.0)], stroke);
        }
    }
}
