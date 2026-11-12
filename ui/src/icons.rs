//! 自繪向量圖示：不依賴圖示字型，任何 DPI 都清晰，並自動跟隨深淺色主題。
//!
//! 每個圖示都畫在 16×16 的格子上，再依實際大小縮放。

use eframe::egui::{
    Color32, CornerRadius, Painter, Pos2, Rect, Response, Role, Sense, Shape, Stroke, StrokeKind,
    Ui, Vec2, WidgetInfo, style::WidgetVisuals, vec2,
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    New,
    Open,
    Save,
    SaveAs,
    Run,
    Rerun,
    Clear,
    Reload,
    Search,
    CaseSensitive,
    WholeWord,
    Regex,
    ReplaceOne,
    ReplaceAll,
    ChevronUp,
    ChevronDown,
    ChevronRight,
    ChevronLeft,
    Close,
    /// 配色：跟隨系統（半黑半白的圓）。
    ThemeSystem,
    /// 配色：淺色（太陽）。
    ThemeLight,
    /// 配色：深色（月亮）。
    ThemeDark,
    /// 設定（橫排三個點）。
    More,
    /// 顯示空白字元（段落符號）。
    Whitespace,
    /// 顯示行號。
    LineNumbers,
    /// 目前行高亮。
    CurrentLine,
    /// 自動換行。
    WordWrap,
    /// 用文字包住選取（一對括號夾著一段文字）。
    Surround,
    /// 搜尋範圍：資料夾（闔起來的資料夾）。
    Folder,
    /// 搜尋範圍：包含子資料夾（資料夾底下再一層資料夾）。
    Subfolders,
    /// 語法上色（大括號）。
    Syntax,
    /// 開始錄製巨集（圓點）。
    Record,
    /// 停止錄製（方塊）。
    Stop,
    /// 編輯（鉛筆）。
    Edit,
    /// 設定快捷鍵（鍵盤）。
    Keyboard,
    /// 重新命名（文字游標 I 形）。
    Rename,
    /// 把 Tab 換成空白（Tab 符號指向空白符號）。
    TabsToSpaces,
    /// 把空白換成 Tab（空白符號指向 Tab 符號）。
    SpacesToTabs,
    /// 排列格式（長短不一的行，縮排成階梯）。
    Reformat,
    /// 關閉窗格（窗格外框裡一個 ×）。
    ClosePane,
    /// 終端機（提示字元 >_）。
    Terminal,
    /// 在終端機執行目前的檔案（終端機的提示字元，右下角一個播放三角形）。
    RunFile,
    /// 預覽（眼睛）。
    Preview,
    /// 介面語言：跟隨系統（地球）。
    LangSystem,
    /// 介面語言：中文（「中」字）。
    LangChinese,
    /// 介面語言：英文（「En」）。
    LangEnglish,
    /// 通知（鈴鐺）：終端機在背景收到通知時標在分頁上。
    Bell,
}

/// 按鈕尺寸。工具列用 `Normal`，狀態列用 `Small`。
#[derive(Clone, Copy)]
enum Size {
    Normal,
    Small,
    /// 工具列上附在別的按鈕旁邊的下拉箭頭：和一般按鈕一樣高（灰底才會對齊），但比較窄。
    Dropdown,
}

/// 一般按鈕的外框高度；表單列裡要讓文字標籤和按鈕垂直置中時用。
pub const BUTTON_HEIGHT: f32 = 30.0;

impl Size {
    /// （按鈕外框的寬與高, 圖示本體）邏輯像素。
    fn px(self) -> (Vec2, f32) {
        match self {
            Size::Normal => (Vec2::splat(BUTTON_HEIGHT), 18.0),
            Size::Small => (Vec2::splat(22.0), 14.0),
            Size::Dropdown => (vec2(18.0, BUTTON_HEIGHT), 14.0),
        }
    }
}

/// 一般按鈕。`enabled` 為 false 時變灰且不可點。
pub fn button(ui: &mut Ui, icon: Icon, tip: &str, enabled: bool) -> Response {
    widget(ui, icon, tip, enabled, false, Size::Normal).on_hover_text(tip)
}

/// 開關按鈕，`on` 為 true 時以選取色顯示。
pub fn toggle(ui: &mut Ui, icon: Icon, tip: &str, on: bool) -> Response {
    widget(ui, icon, tip, true, on, Size::Normal).on_hover_text(tip)
}

/// 緊湊版按鈕（狀態列用）。
pub fn small_button(ui: &mut Ui, icon: Icon, tip: &str, enabled: bool) -> Response {
    widget(ui, icon, tip, enabled, false, Size::Small).on_hover_text(tip)
}

/// 工具列上附在別的按鈕旁邊的下拉箭頭：高度和一般按鈕一樣（滑鼠移過的灰底才會對齊），寬度比較窄。
pub fn dropdown_button(ui: &mut Ui, icon: Icon, tip: &str, enabled: bool) -> Response {
    widget(ui, icon, tip, enabled, false, Size::Dropdown).on_hover_text(tip)
}

/// 在指定的位置畫一個圖示（不是按鈕），例如清單裡文字前面的小圖示。
pub fn paint(ui: &Ui, icon: Icon, rect: Rect, color: Color32) {
    draw(ui, icon, rect, color, ui.visuals().panel_fill);
}

/// 緊湊版開關按鈕，`on` 為 true 時以選取色顯示。`enabled` 為 false 時變灰且不可點。
pub fn small_toggle(ui: &mut Ui, icon: Icon, tip: &str, on: bool, enabled: bool) -> Response {
    widget(ui, icon, tip, enabled, on, Size::Small).on_hover_text(tip)
}

/// 緊湊版開關按鈕，tooltip 的內容由呼叫端自訂：可以放多行說明與可點擊的連結
/// （egui 的 tooltip 含互動元件時，滑鼠移到 tooltip 上也會保持開啟）。
/// `label` 只用在無障礙描述。
pub fn small_toggle_rich(
    ui: &mut Ui,
    icon: Icon,
    label: &str,
    on: bool,
    tooltip: impl FnOnce(&mut Ui),
) -> Response {
    widget(ui, icon, label, true, on, Size::Small).on_hover_ui(tooltip)
}

// 圖示按鈕能不能用 Tab 鍵聚焦（預設可以）。只在畫面執行緒用，由 `mouse_only` 暫時關掉。
thread_local! {
    static TAB_FOCUSABLE: std::cell::Cell<bool> = const { std::cell::Cell::new(true) };
}

/// 在 `f` 裡畫出來的圖示按鈕都不能用 Tab 鍵聚焦，只能用滑鼠點（例如尋找列：Tab 只留給兩個輸入框）。
pub fn mouse_only<R>(f: impl FnOnce() -> R) -> R {
    let before = TAB_FOCUSABLE.replace(false);
    let result = f();
    TAB_FOCUSABLE.set(before);
    result
}

/// 配置並繪製按鈕本體，不含 tooltip（由各個包裝函式決定 tooltip 的形式）。
fn widget(
    ui: &mut Ui,
    icon: Icon,
    tip: &str,
    enabled: bool,
    selected: bool,
    size: Size,
) -> Response {
    let (button_size, icon_px) = size.px();
    // `Sense::click()` 同時是「可以用 Tab 聚焦」的；只能用滑鼠點的按鈕改用 `Sense::CLICK`
    let sense = match (enabled, TAB_FOCUSABLE.get()) {
        (false, _) => Sense::hover(),
        (true, true) => Sense::click(),
        (true, false) => Sense::CLICK,
    };
    let (rect, response) = ui.allocate_exact_size(button_size, sense);
    response.widget_info(|| WidgetInfo::labeled(Role::Button, enabled, tip));

    if ui.is_rect_visible(rect) {
        let visuals = ui.style().interact_selectable(&response, selected);
        let highlighted = enabled && (response.hovered() || response.has_focus() || selected);
        if highlighted {
            ui.painter()
                .rect_filled(rect.shrink(1.0), CornerRadius::same(4), visuals.weak_bg_fill);
        }
        // 圖示底下目前實際的底色：有些圖示（月亮、徽章）要用它「挖」出形狀
        let bg = if highlighted {
            visuals.weak_bg_fill
        } else {
            ui.visuals().panel_fill
        };
        let color = if enabled {
            visuals.fg_stroke.color
        } else {
            ui.visuals().widgets.noninteractive.fg_stroke.color.gamma_multiply(0.4)
        };
        let icon_rect = Rect::from_center_size(rect.center(), Vec2::splat(icon_px));
        draw(ui, icon, icon_rect, color, bg);
    }

    response
}

/// 用介面字型把文字寫在圖示格子正中間（語言按鈕的「中」「En」）。
/// 直接用字型而不是手畫線條，字形才會和旁邊的文字一模一樣。
fn glyph(painter: &Painter, rect: Rect, text: &str, size: f32, color: Color32) {
    painter.text(
        rect.center(),
        eframe::egui::Align2::CENTER_CENTER,
        text,
        eframe::egui::FontId::proportional(size),
        color,
    );
}

/// 畫出圖示。`bg` 是圖示底下的底色，供需要「挖洞」的圖示使用。
fn draw(ui: &Ui, icon: Icon, rect: Rect, color: Color32, bg: Color32) {
    let painter = ui.painter();
    let s = rect.width() / 16.0;
    let p = |x: f32, y: f32| -> Pos2 { rect.min + vec2(x * s, y * s) };
    // 約 1 邏輯像素的細線；再細在 100% 縮放下會變得模糊
    let stroke = Stroke::new((0.9 * s).max(1.0), color);
    let line = |pts: &[(f32, f32)]| {
        painter.add(Shape::line(
            pts.iter().map(|&(x, y)| p(x, y)).collect(),
            stroke,
        ));
    };
    let closed = |pts: &[(f32, f32)]| {
        painter.add(Shape::closed_line(
            pts.iter().map(|&(x, y)| p(x, y)).collect(),
            stroke,
        ));
    };
    let rect_of = |x0: f32, y0: f32, x1: f32, y1: f32| Rect::from_min_max(p(x0, y0), p(x1, y1));

    match icon {
        Icon::New => {
            // 摺角的紙張 + 加號
            closed(&[(3.0, 1.5), (10.0, 1.5), (13.0, 4.5), (13.0, 14.5), (3.0, 14.5)]);
            line(&[(10.0, 1.5), (10.0, 4.5), (13.0, 4.5)]);
            line(&[(8.0, 7.0), (8.0, 12.0)]);
            line(&[(5.5, 9.5), (10.5, 9.5)]);
        }
        Icon::Open => {
            // 開啟的資料夾
            line(&[
                (1.5, 13.5),
                (1.5, 3.5),
                (6.0, 3.5),
                (7.5, 5.5),
                (13.0, 5.5),
                (13.0, 8.0),
            ]);
            closed(&[(1.5, 13.5), (4.0, 8.0), (15.0, 8.0), (12.5, 13.5)]);
        }
        Icon::Save => floppy(painter, &p, stroke, &rect_of),
        Icon::SaveAs => {
            floppy(painter, &p, stroke, &rect_of);
            // 右下角的「+」徽章；用當下的底色把後方線條蓋掉（hover 時底色不同，所以不能寫死）
            let c = p(12.5, 12.5);
            painter.circle_filled(c, 4.0 * s, bg);
            painter.circle_stroke(c, 3.6 * s, stroke);
            line(&[(12.5, 10.8), (12.5, 14.2)]);
            line(&[(10.8, 12.5), (14.2, 12.5)]);
        }
        Icon::Run => {
            // 空心三角形：和其他按鈕一樣只有線條、沒有填色
            closed(&[(4.0, 2.5), (13.0, 8.0), (4.0, 13.5)]);
        }
        Icon::Rerun => {
            circular_arrow(painter, p(8.0, 8.0), 6.0 * s, -50.0, 270.0, stroke, color, s);
            // 中間的小三角形也是空心，與「執行」一致
            closed(&[(6.6, 5.8), (10.2, 8.0), (6.6, 10.2)]);
        }
        Icon::Reload => {
            // 與「開啟」同樣的資料夾（縮在左下），右上角加一個半圓箭頭：重新讀取這個檔案
            line(&[(1.0, 14.5), (1.0, 6.5), (4.5, 6.5), (5.8, 8.0), (8.5, 8.0)]);
            closed(&[(1.0, 14.5), (3.4, 10.0), (12.5, 10.0), (10.1, 14.5)]);
            circular_arrow(painter, p(11.6, 4.6), 3.4 * s, 165.0, 205.0, stroke, color, s * 0.85);
        }
        Icon::Clear => {
            // 垃圾桶
            line(&[(2.5, 4.5), (13.5, 4.5)]);
            line(&[(6.0, 4.5), (6.0, 2.5), (10.0, 2.5), (10.0, 4.5)]);
            line(&[(3.8, 4.5), (4.6, 14.0), (11.4, 14.0), (12.2, 4.5)]);
            line(&[(6.6, 7.0), (6.8, 12.0)]);
            line(&[(9.4, 7.0), (9.2, 12.0)]);
        }
        Icon::Search => {
            // 放大鏡
            painter.circle_stroke(p(6.8, 6.8), 4.6 * s, stroke);
            line(&[(10.2, 10.2), (14.2, 14.2)]);
        }
        Icon::CaseSensitive => {
            // 大寫 A 與小寫 a：區分大小寫
            line(&[(0.8, 13.0), (4.4, 3.0), (8.0, 13.0)]);
            line(&[(2.2, 9.4), (6.6, 9.4)]);
            painter.circle_stroke(p(11.6, 10.3), 2.6 * s, stroke);
            line(&[(14.2, 7.6), (14.2, 13.0)]);
        }
        Icon::WholeWord => {
            // "ab" 加上底下的括弧：完整單字
            painter.circle_stroke(p(4.2, 7.6), 1.9 * s, stroke);
            line(&[(6.1, 5.7), (6.1, 9.5)]);
            line(&[(9.0, 2.8), (9.0, 9.5)]);
            painter.circle_stroke(p(10.9, 7.6), 1.9 * s, stroke);
            line(&[(1.8, 11.4), (1.8, 13.6), (14.2, 13.6), (14.2, 11.4)]);
        }
        Icon::Regex => {
            // 「.*」：規則表達式
            painter.circle_filled(p(3.8, 11.8), 1.2 * s, color);
            line(&[(10.2, 3.1), (10.2, 9.7)]);
            line(&[(7.34, 8.05), (13.06, 4.75)]);
            line(&[(13.06, 8.05), (7.34, 4.75)]);
        }
        Icon::ReplaceOne => {
            // 上下兩個方向相反的箭頭：取代
            line(&[(2.0, 5.0), (13.0, 5.0)]);
            line(&[(10.0, 2.4), (13.0, 5.0), (10.0, 7.6)]);
            line(&[(14.0, 11.0), (3.0, 11.0)]);
            line(&[(6.0, 8.4), (3.0, 11.0), (6.0, 13.6)]);
        }
        Icon::ReplaceAll => {
            // 四條交錯的箭頭：全部取代
            for (i, y) in [2.4, 6.0, 9.6, 13.2].into_iter().enumerate() {
                if i % 2 == 0 {
                    line(&[(2.0, y), (13.5, y)]);
                    line(&[(11.4, y - 1.5), (13.5, y), (11.4, y + 1.5)]);
                } else {
                    line(&[(14.0, y), (2.5, y)]);
                    line(&[(4.6, y - 1.5), (2.5, y), (4.6, y + 1.5)]);
                }
            }
        }
        Icon::ChevronUp => line(&[(3.5, 10.5), (8.0, 6.0), (12.5, 10.5)]),
        Icon::ChevronDown => line(&[(3.5, 6.0), (8.0, 10.5), (12.5, 6.0)]),
        Icon::ChevronRight => line(&[(6.0, 3.5), (10.5, 8.0), (6.0, 12.5)]),
        Icon::ChevronLeft => line(&[(10.0, 3.5), (5.5, 8.0), (10.0, 12.5)]),
        Icon::Close => {
            line(&[(3.5, 3.5), (12.5, 12.5)]);
            line(&[(12.5, 3.5), (3.5, 12.5)]);
        }
        Icon::Terminal => {
            // 提示字元：向右的 > 加底線
            line(&[(3.0, 4.5), (7.2, 8.0), (3.0, 11.5)]);
            line(&[(8.8, 12.0), (13.0, 12.0)]);
        }
        Icon::RunFile => {
            // 終端機的提示字元（> 加底線，和「新增終端機」的圖示一樣），右下角加一個實心的播放三角形：在終端機執行
            line(&[(2.0, 3.2), (6.0, 6.7), (2.0, 10.2)]);
            line(&[(2.0, 13.2), (6.2, 13.2)]);
            painter.add(Shape::convex_polygon(vec![p(8.6, 8.2), p(8.6, 14.6), p(14.6, 11.4)], color, Stroke::NONE));
        }
        Icon::Preview => {
            // 眼睛：上下眼瞼的弧線加瞳孔
            line(&[(1.2, 8.0), (4.2, 4.8), (8.0, 3.4), (11.8, 4.8), (14.8, 8.0)]);
            line(&[(1.2, 8.0), (4.2, 11.2), (8.0, 12.6), (11.8, 11.2), (14.8, 8.0)]);
            painter.circle_stroke(p(8.0, 8.0), 2.4 * s, stroke);
        }
        Icon::LangSystem => {
            // 地球：圓、赤道、一條直的子午線（橢圓）與上下兩條緯線
            let c = p(8.0, 8.0);
            painter.circle_stroke(c, 6.2 * s, stroke);
            line(&[(1.8, 8.0), (14.2, 8.0)]);
            let meridian: Vec<(f32, f32)> = (0..=24)
                .map(|i| {
                    let a = std::f32::consts::TAU * i as f32 / 24.0;
                    (8.0 + 2.8 * a.cos(), 8.0 + 6.2 * a.sin())
                })
                .collect();
            line(&meridian);
            line(&[(3.1, 4.6), (12.9, 4.6)]);
            line(&[(3.1, 11.4), (12.9, 11.4)]);
        }
        Icon::LangChinese => glyph(painter, rect, "中", 13.0 * s, color),
        Icon::LangEnglish => glyph(painter, rect, "En", 12.0 * s, color),
        Icon::ClosePane => {
            // 窗格的外框，裡面一個 ×
            painter.rect_stroke(rect_of(1.8, 2.8, 14.2, 13.2), CornerRadius::same(1), stroke, StrokeKind::Inside);
            line(&[(5.8, 6.0), (10.2, 10.0)]);
            line(&[(10.2, 6.0), (5.8, 10.0)]);
        }
        Icon::ThemeSystem => {
            // 半黑半白的圓：左半邊填滿，右半邊只有外框
            use std::f32::consts::{FRAC_PI_2, PI};
            let c = p(8.0, 8.0);
            let r = 5.8 * s;
            // 螢幕座標 y 向下：角度從 90°（下）經 180°（左）到 270°（上）就是左半圓
            let half: Vec<Pos2> = (0..=24)
                .map(|i| {
                    let a = FRAC_PI_2 + PI * (i as f32 / 24.0);
                    c + r * vec2(a.cos(), a.sin())
                })
                .collect();
            painter.add(Shape::convex_polygon(half, color, Stroke::NONE));
            painter.circle_stroke(c, r, stroke);
        }
        Icon::ThemeLight => {
            // 太陽：中間一個圓，周圍八道光芒
            use std::f32::consts::FRAC_PI_4;
            let c = p(8.0, 8.0);
            painter.circle_stroke(c, 2.9 * s, stroke);
            for k in 0..8 {
                let a = k as f32 * FRAC_PI_4;
                let d = vec2(a.cos(), a.sin());
                painter.line_segment([c + d * (4.7 * s), c + d * (6.6 * s)], stroke);
            }
        }
        Icon::ThemeDark => {
            // 月亮：實心大圓，再用底色在右上方挖掉一個圓，留下彎月
            painter.circle_filled(p(7.6, 8.4), 5.6 * s, color);
            painter.circle_filled(p(10.6, 6.0), 4.7 * s, bg);
        }
        Icon::More => {
            // 橫排三個點
            for x in [3.0, 8.0, 13.0] {
                painter.circle_filled(p(x, 8.0), 1.4 * s, color);
            }
        }
        Icon::Whitespace => {
            // 段落符號 ¶：空白字元的標記
            painter.circle_stroke(p(6.6, 5.2), 2.9 * s, stroke);
            line(&[(6.6, 8.1), (6.6, 14.0)]);
            line(&[(10.0, 2.3), (10.0, 14.0)]);
            line(&[(6.6, 2.3), (13.0, 2.3)]);
        }
        Icon::LineNumbers => {
            // 左邊一排短橫線（行號），右邊一排長橫線（文字）
            for y in [3.5, 8.0, 12.5] {
                line(&[(1.5, y), (4.5, y)]);
                line(&[(7.0, y), (14.5, y)]);
            }
        }
        Icon::CurrentLine => {
            // 三行文字，中間那行畫成實心底色
            painter.rect_filled(rect_of(1.0, 5.8, 15.0, 10.2), CornerRadius::same(1), color.gamma_multiply(0.35));
            line(&[(2.5, 3.0), (13.5, 3.0)]);
            line(&[(2.5, 8.0), (13.5, 8.0)]);
            line(&[(2.5, 13.0), (13.5, 13.0)]);
        }
        Icon::Folder => {
            // 有分頁耳朵的資料夾外框
            closed(&[(1.5, 13.5), (1.5, 3.0), (6.0, 3.0), (7.5, 5.0), (14.5, 5.0), (14.5, 13.5)]);
        }
        Icon::Subfolders => {
            // 上層資料夾，底下接一個縮排的子資料夾
            closed(&[(1.5, 7.0), (1.5, 1.5), (4.5, 1.5), (5.8, 3.2), (10.5, 3.2), (10.5, 7.0)]);
            line(&[(3.0, 7.0), (3.0, 12.5), (6.0, 12.5)]);
            closed(&[(6.0, 14.5), (6.0, 9.5), (8.8, 9.5), (9.9, 11.0), (14.5, 11.0), (14.5, 14.5)]);
        }
        Icon::Syntax => {
            // { }：程式碼
            line(&[(6.0, 2.5), (4.8, 2.5), (4.2, 3.2), (4.2, 6.6), (3.0, 8.0), (4.2, 9.4), (4.2, 12.8), (4.8, 13.5), (6.0, 13.5)]);
            line(&[(10.0, 2.5), (11.2, 2.5), (11.8, 3.2), (11.8, 6.6), (13.0, 8.0), (11.8, 9.4), (11.8, 12.8), (11.2, 13.5), (10.0, 13.5)]);
        }
        Icon::Record => {
            painter.circle_stroke(p(8.0, 8.0), 6.0 * s, stroke);
            painter.circle_filled(p(8.0, 8.0), 3.2 * s, color);
        }
        Icon::Stop => {
            painter.rect_filled(rect_of(3.5, 3.5, 12.5, 12.5), CornerRadius::same(1), color);
        }
        Icon::Edit => {
            closed(&[(2.5, 13.5), (3.3, 10.2), (10.6, 2.9), (13.1, 5.4), (5.8, 12.7)]);
            line(&[(9.0, 4.5), (11.5, 7.0)]);
        }
        Icon::TabsToSpaces | Icon::SpacesToTabs => {
            // 上面是 Tab 符號（箭頭＋豎線），下面是空白符號（開口向上的括號），右邊的箭頭由換出的一邊指向換進的一邊
            line(&[(2.0, 4.5), (9.0, 4.5)]);
            line(&[(6.8, 2.7), (9.0, 4.5), (6.8, 6.3)]);
            line(&[(11.0, 2.7), (11.0, 6.3)]);
            line(&[(2.0, 10.0), (2.0, 12.5), (11.0, 12.5), (11.0, 10.0)]);
            if icon == Icon::TabsToSpaces {
                line(&[(14.0, 3.0), (14.0, 13.0)]);
                line(&[(12.6, 11.4), (14.0, 13.0), (15.4, 11.4)]);
            } else {
                line(&[(14.0, 13.0), (14.0, 3.0)]);
                line(&[(12.6, 4.6), (14.0, 3.0), (15.4, 4.6)]);
            }
        }
        Icon::Reformat => {
            // 四行文字，縮排依序為 0、1、2、1：像是排好的程式碼
            line(&[(2.5, 3.0), (10.5, 3.0)]);
            line(&[(5.0, 6.3), (13.5, 6.3)]);
            line(&[(7.5, 9.7), (12.0, 9.7)]);
            line(&[(5.0, 13.0), (9.5, 13.0)]);
        }
        Icon::Rename => {
            // 文字游標（I 形）加底線：改名稱
            line(&[(5.0, 2.5), (11.0, 2.5)]);
            line(&[(8.0, 2.5), (8.0, 10.5)]);
            line(&[(5.0, 10.5), (11.0, 10.5)]);
            line(&[(2.5, 13.5), (13.5, 13.5)]);
        }
        Icon::Keyboard => {
            painter.rect_stroke(rect_of(1.5, 3.5, 14.5, 12.5), CornerRadius::same(2), stroke, StrokeKind::Inside);
            for x in [4.0, 6.7, 9.4, 12.1] {
                painter.circle_filled(p(x, 6.3), 0.7 * s, color);
            }
            line(&[(4.5, 10.0), (11.5, 10.0)]);
        }

        Icon::Surround => {
            // [ - ]：一對括號夾著一段文字
            line(&[(5.5, 2.5), (3.0, 2.5), (3.0, 13.5), (5.5, 13.5)]);
            line(&[(10.5, 2.5), (13.0, 2.5), (13.0, 13.5), (10.5, 13.5)]);
            line(&[(6.0, 8.0), (10.0, 8.0)]);
        }
        Icon::WordWrap => {
            // 一行文字走到右邊後折回下一行：折返箭頭
            line(&[(1.5, 3.5), (14.5, 3.5)]);
            line(&[(1.5, 12.5), (6.0, 12.5)]);
            line(&[(1.5, 8.0), (12.0, 8.0), (14.0, 9.0), (14.0, 11.0), (12.0, 12.0), (9.5, 12.0)]);
            line(&[(11.0, 10.2), (9.2, 12.0), (11.0, 13.8)]);
        }
        Icon::Bell => {
            // 鈴鐺：圓頂、向外張開的下緣，頂端一小段柄，底下一個鈴舌
            closed(&[
                (2.5, 12.0),
                (4.0, 10.5),
                (4.0, 7.0),
                (5.0, 4.8),
                (8.0, 3.6),
                (11.0, 4.8),
                (12.0, 7.0),
                (12.0, 10.5),
                (13.5, 12.0),
            ]);
            line(&[(8.0, 1.5), (8.0, 3.6)]);
            line(&[(6.5, 13.6), (8.0, 14.8), (9.5, 13.6)]);
        }
    }
}

/// 下拉選單右側的箭頭：細線向下的 chevron（展開時朝上），扁平現代風格。
/// 給 `ComboBox::icon` 使用。
pub fn combo_chevron(ui: &Ui, rect: Rect, visuals: &WidgetVisuals, is_open: bool) {
    let c = rect.center();
    let (half_w, half_h) = (3.6, 2.0);
    let dy = if is_open { half_h } else { -half_h };
    let points = vec![
        Pos2::new(c.x - half_w, c.y + dy),
        Pos2::new(c.x, c.y - dy),
        Pos2::new(c.x + half_w, c.y + dy),
    ];
    ui.painter()
        .add(Shape::line(points, Stroke::new(1.1, visuals.fg_stroke.color)));
}

/// 磁碟片：外框、上方的金屬片、下方的標籤。
fn floppy(
    painter: &Painter,
    p: &impl Fn(f32, f32) -> Pos2,
    stroke: Stroke,
    rect_of: &impl Fn(f32, f32, f32, f32) -> Rect,
) {
    painter.add(Shape::closed_line(
        [(2.0, 2.0), (12.0, 2.0), (14.0, 4.0), (14.0, 14.0), (2.0, 14.0)]
            .iter()
            .map(|&(x, y)| p(x, y))
            .collect(),
        stroke,
    ));
    painter.rect_stroke(
        rect_of(5.0, 2.0, 11.0, 6.0),
        CornerRadius::ZERO,
        stroke,
        StrokeKind::Middle,
    );
    painter.rect_stroke(
        rect_of(4.5, 9.0, 11.5, 14.0),
        CornerRadius::ZERO,
        stroke,
        StrokeKind::Middle,
    );
}

/// 圓弧加箭頭。角度單位為度，螢幕座標 y 向下，所以正的 `sweep_deg` 是順時針、負的是逆時針。
/// `head_scale` 是箭頭大小（以像素為單位的基準）。
#[allow(clippy::too_many_arguments)]
fn circular_arrow(
    painter: &Painter,
    center: Pos2,
    radius: f32,
    start_deg: f32,
    sweep_deg: f32,
    stroke: Stroke,
    color: Color32,
    head_scale: f32,
) {
    let start = start_deg.to_radians();
    let sweep = sweep_deg.to_radians();
    let steps = 24;
    let pts: Vec<Pos2> = (0..=steps)
        .map(|i| {
            let a = start + sweep * (i as f32 / steps as f32);
            center + radius * vec2(a.cos(), a.sin())
        })
        .collect();
    painter.add(Shape::line(pts.clone(), stroke));

    // 箭頭：頂點沿切線方向，底邊沿半徑方向
    let end_angle = start + sweep;
    let dir = sweep.signum();
    let tangent = dir * vec2(-end_angle.sin(), end_angle.cos());
    let normal = vec2(end_angle.cos(), end_angle.sin());
    let tip_base = *pts.last().unwrap();
    let head = 3.2 * head_scale;
    let tip = tip_base + tangent * head;
    let a = tip_base + normal * (head * 0.9);
    let b = tip_base - normal * (head * 0.9);
    painter.add(Shape::convex_polygon(vec![tip, a, b], color, Stroke::NONE));
}

/// 把文件（docs）裡要用的圖示用實際的繪製程式畫出來，輸出成 SVG（`docs/images/icons/`）。
/// 文件裡的圖示因此和程式裡看到的一模一樣，改了圖示之後重新產生即可：
/// `cargo test -p feditor --bin feditor export_icons_for_docs -- --ignored`
#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui;

    /// 文件裡用到的圖示與檔名。
    const DOC_ICONS: &[(&str, Icon)] = &[
        ("run", Icon::Run),
        ("rerun", Icon::Rerun),
        ("record", Icon::Record),
        ("stop", Icon::Stop),
        ("keyboard", Icon::Keyboard),
        ("edit", Icon::Edit),
        ("rename", Icon::Rename),
        ("trash", Icon::Clear),
        ("dropdown", Icon::ChevronDown),
    ];

    /// 圖示畫在 32×32 的格子裡（程式裡是 14 到 18 像素，這裡放大一倍好讓線條在高解析度螢幕上也清楚）。
    const SIZE: f32 = 32.0;
    /// 在淺色與深色的文件底下都看得清楚的灰色。
    const INK: Color32 = Color32::from_rgb(0x6e, 0x77, 0x81);

    fn hex(c: Color32) -> String {
        format!("#{:02x}{:02x}{:02x}", c.r(), c.g(), c.b())
    }

    fn opacity(c: Color32) -> f32 {
        c.a() as f32 / 255.0
    }

    fn svg_of(shape: &Shape, out: &mut String) {
        let stroke_attrs = |s: &Stroke| {
            if s.is_empty() {
                "stroke=\"none\"".to_owned()
            } else {
                format!("stroke=\"{}\" stroke-opacity=\"{:.2}\" stroke-width=\"{:.2}\" stroke-linecap=\"round\" stroke-linejoin=\"round\"", hex(s.color), opacity(s.color), s.width)
            }
        };
        let fill_attrs = |c: Color32| {
            if c == Color32::TRANSPARENT {
                "fill=\"none\"".to_owned()
            } else {
                format!("fill=\"{}\" fill-opacity=\"{:.2}\"", hex(c), opacity(c))
            }
        };
        match shape {
            Shape::Vec(shapes) => shapes.iter().for_each(|s| svg_of(s, out)),
            Shape::Path(path) => {
                let points: Vec<String> = path.points.iter().map(|p| format!("{:.2},{:.2}", p.x, p.y)).collect();
                let stroke = match &path.stroke.color {
                    egui::epaint::ColorMode::Solid(c) => Stroke::new(path.stroke.width, *c),
                    _ => Stroke::NONE,
                };
                let tag = if path.closed { "polygon" } else { "polyline" };
                out.push_str(&format!("<{tag} points=\"{}\" {} {}/>\n", points.join(" "), fill_attrs(path.fill), stroke_attrs(&stroke)));
            }
            Shape::Circle(c) => {
                out.push_str(&format!("<circle cx=\"{:.2}\" cy=\"{:.2}\" r=\"{:.2}\" {} {}/>\n", c.center.x, c.center.y, c.radius, fill_attrs(c.fill), stroke_attrs(&c.stroke)));
            }
            Shape::Rect(r) => {
                let rect = match r.stroke_kind {
                    StrokeKind::Inside => r.rect.shrink(r.stroke.width / 2.0),
                    StrokeKind::Outside => r.rect.expand(r.stroke.width / 2.0),
                    StrokeKind::Middle => r.rect,
                };
                out.push_str(&format!(
                    "<rect x=\"{:.2}\" y=\"{:.2}\" width=\"{:.2}\" height=\"{:.2}\" rx=\"{:.2}\" {} {}/>\n",
                    rect.min.x, rect.min.y, rect.width(), rect.height(), r.corner_radius.nw as f32, fill_attrs(r.fill), stroke_attrs(&r.stroke)
                ));
            }
            Shape::LineSegment { points, stroke } => {
                out.push_str(&format!("<line x1=\"{:.2}\" y1=\"{:.2}\" x2=\"{:.2}\" y2=\"{:.2}\" {}/>\n", points[0].x, points[0].y, points[1].x, points[1].y, stroke_attrs(stroke)));
            }
            other => panic!("圖示用到了還不會轉成 SVG 的形狀：{other:?}"),
        }
    }

    /// 用真正的繪製程式畫出一個圖示，轉成 SVG 文字。
    fn render(icon: Icon) -> String {
        let ctx = egui::Context::default();
        let mut raw = egui::RawInput::default();
        raw.screen_rect = Some(Rect::from_min_size(Pos2::ZERO, vec2(SIZE * 2.0, SIZE * 2.0)));
        let mut output = ctx.run_ui(raw, |ui| {
            paint(ui, icon, Rect::from_min_size(Pos2::ZERO, Vec2::splat(SIZE)), INK);
        });
        output.textures_delta.clear();
        let mut body = String::new();
        for clipped in &output.shapes {
            svg_of(&clipped.shape, &mut body);
        }
        format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{SIZE}\" height=\"{SIZE}\" viewBox=\"0 0 {SIZE} {SIZE}\">\n{body}</svg>\n")
    }

    #[test]
    fn every_doc_icon_renders_to_a_non_empty_svg() {
        for (name, icon) in DOC_ICONS {
            let svg = render(*icon);
            assert!(svg.contains("<polyline") || svg.contains("<polygon") || svg.contains("<circle") || svg.contains("<rect"), "{name} 沒有畫出任何東西");
        }
    }

    #[test]
    #[ignore = "產生文件用的圖示檔，要更新時才手動執行"]
    fn export_icons_for_docs() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../docs/images/icons");
        std::fs::create_dir_all(&dir).unwrap();
        for (name, icon) in DOC_ICONS {
            std::fs::write(dir.join(format!("{name}.svg")), render(*icon)).unwrap();
        }
    }
}
