//! 深色配色：《新世紀福音戰士》（EVA）初號機與 NERV 的視覺語彙。
//!
//! - 底色：帶紫調的近黑
//! - 初號機紫：選取、hover、按下
//! - 螢光綠：強調色（游標、超連結、被選取的圖示）
//! - NERV 橘：警告（也用在搜尋結果的高亮）
//! - 警示紅：錯誤
//!
//! 淺色配色維持 egui 預設。

use eframe::egui::{self, Color32, CornerRadius, Stroke, Theme, Visuals};

/// 色票。用 `const fn` 讓測試可以直接引用同一組數值。
pub mod palette {
    use super::Color32;

    const fn rgb(hex: u32) -> Color32 {
        Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
    }

    // 底色（由深到淺）
    pub const BG_EDITOR: Color32 = rgb(0x0B0814);
    pub const BG_PANEL: Color32 = rgb(0x150F24);
    pub const BG_POPUP: Color32 = rgb(0x1A1230);
    pub const BG_FAINT: Color32 = rgb(0x1D1530);
    pub const BG_CODE: Color32 = rgb(0x241A3B);

    // 初號機紫
    pub const PURPLE_DARK: Color32 = rgb(0x2A1E45);
    pub const PURPLE_MID: Color32 = rgb(0x4A2F7A);
    pub const PURPLE_SELECT: Color32 = rgb(0x5B2A9E);
    pub const PURPLE_LINE: Color32 = rgb(0x3A2A5C);
    pub const PURPLE_LIGHT: Color32 = rgb(0x9B6DE0);

    // 強調色
    pub const GREEN: Color32 = rgb(0x8FEA00);
    pub const ORANGE: Color32 = rgb(0xFF8A00);
    pub const RED: Color32 = rgb(0xFF4D5E);

    // 文字
    pub const TEXT: Color32 = rgb(0xD9D3EA);
    pub const TEXT_STRONG: Color32 = rgb(0xF2EEFF);
    pub const TEXT_WEAK: Color32 = rgb(0x948BB2);
}

/// EVA 風格的深色 `Visuals`。以 egui 的深色為底，只覆蓋顏色，不動間距與圓角以外的形狀。
pub fn eva_dark_visuals() -> Visuals {
    use palette::*;

    let mut v = Visuals::dark();
    v.dark_mode = true;

    v.panel_fill = BG_PANEL;
    v.window_fill = BG_POPUP;
    v.window_stroke = Stroke::new(1.0, PURPLE_LINE);
    v.extreme_bg_color = BG_EDITOR; // 文字框與捲動區的底色：編輯區
    v.faint_bg_color = BG_FAINT;
    v.code_bg_color = BG_CODE;

    v.hyperlink_color = GREEN;
    v.warn_fg_color = ORANGE;
    v.error_fg_color = RED;
    v.weak_text_color = Some(TEXT_WEAK);

    // 選取：初號機紫，被選取的圖示與文字用螢光綠
    v.selection.bg_fill = PURPLE_SELECT;
    v.selection.stroke = Stroke::new(1.0, GREEN);
    v.text_cursor.stroke = Stroke::new(2.0, GREEN);

    let w = &mut v.widgets;
    w.noninteractive.bg_fill = BG_PANEL;
    w.noninteractive.weak_bg_fill = BG_PANEL;
    w.noninteractive.bg_stroke = Stroke::new(1.0, PURPLE_LINE);
    w.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);

    w.inactive.bg_fill = PURPLE_DARK;
    w.inactive.weak_bg_fill = PURPLE_DARK;
    w.inactive.bg_stroke = Stroke::NONE;
    w.inactive.fg_stroke = Stroke::new(1.0, TEXT);

    w.hovered.bg_fill = PURPLE_MID;
    w.hovered.weak_bg_fill = PURPLE_MID;
    w.hovered.bg_stroke = Stroke::new(1.0, PURPLE_LIGHT);
    w.hovered.fg_stroke = Stroke::new(1.5, TEXT_STRONG);

    w.active.bg_fill = PURPLE_SELECT;
    w.active.weak_bg_fill = PURPLE_SELECT;
    w.active.bg_stroke = Stroke::new(1.0, GREEN);
    w.active.fg_stroke = Stroke::new(2.0, TEXT_STRONG);

    w.open.bg_fill = PURPLE_MID;
    w.open.weak_bg_fill = PURPLE_MID;
    w.open.bg_stroke = Stroke::new(1.0, PURPLE_LIGHT);
    w.open.fg_stroke = Stroke::new(1.0, TEXT_STRONG);

    // 與 egui 預設一致的小圓角
    for state in [
        &mut w.noninteractive,
        &mut w.inactive,
        &mut w.hovered,
        &mut w.active,
        &mut w.open,
    ] {
        state.corner_radius = CornerRadius::same(4);
    }
    v
}

/// 把配色套到 egui：只改深色那一組，淺色維持預設。
pub fn apply(ctx: &egui::Context) {
    ctx.style_mut_of(Theme::Dark, |style| {
        style.visuals = eva_dark_visuals();
    });
}

#[cfg(test)]
mod tests {
    use super::palette::*;
    use super::*;

    /// WCAG 相對亮度。
    fn luminance(c: Color32) -> f64 {
        fn channel(v: u8) -> f64 {
            let s = v as f64 / 255.0;
            if s <= 0.03928 {
                s / 12.92
            } else {
                ((s + 0.055) / 1.055).powf(2.4)
            }
        }
        0.2126 * channel(c.r()) + 0.7152 * channel(c.g()) + 0.0722 * channel(c.b())
    }

    /// WCAG 對比值（1 到 21）。
    fn contrast(a: Color32, b: Color32) -> f64 {
        let (la, lb) = (luminance(a), luminance(b));
        let (hi, lo) = if la > lb { (la, lb) } else { (lb, la) };
        (hi + 0.05) / (lo + 0.05)
    }

    #[test]
    fn body_text_is_readable_on_every_background() {
        // 一般文字至少 4.5:1（WCAG AA）
        for bg in [BG_EDITOR, BG_PANEL, BG_POPUP, BG_FAINT, BG_CODE, PURPLE_DARK] {
            for fg in [TEXT, TEXT_STRONG] {
                let c = contrast(fg, bg);
                assert!(c >= 4.5, "{fg:?} 在 {bg:?} 上只有 {c:.2}:1");
            }
        }
    }

    #[test]
    fn secondary_text_and_accents_are_readable_on_panels() {
        for bg in [BG_EDITOR, BG_PANEL, BG_POPUP] {
            for (name, fg) in [
                ("次要文字", TEXT_WEAK),
                ("超連結／游標綠", GREEN),
                ("警告橘", ORANGE),
                ("錯誤紅", RED),
            ] {
                let c = contrast(fg, bg);
                assert!(c >= 4.5, "{name} 在 {bg:?} 上只有 {c:.2}:1");
            }
        }
    }

    #[test]
    fn selected_and_hovered_states_keep_icons_and_text_visible() {
        // 被選取的開關：綠色圖示在紫色底上（圖形元件至少 3:1）
        assert!(contrast(GREEN, PURPLE_SELECT) >= 3.0);
        // 選取的文字：一般文字在選取底色上仍要能讀
        assert!(contrast(TEXT, PURPLE_SELECT) >= 4.5);
        // hover、按下
        assert!(contrast(TEXT_STRONG, PURPLE_MID) >= 4.5);
        assert!(contrast(TEXT_STRONG, PURPLE_SELECT) >= 4.5);
    }

    #[test]
    fn backgrounds_get_lighter_from_editor_to_popup() {
        // 編輯區最暗，面板其次，浮動視窗再亮一點：層次才分得出來
        assert!(luminance(BG_EDITOR) < luminance(BG_PANEL));
        assert!(luminance(BG_PANEL) < luminance(BG_POPUP));
    }

    #[test]
    fn visuals_use_the_palette_and_stay_dark() {
        let v = eva_dark_visuals();
        assert!(v.dark_mode);
        assert_eq!(v.panel_fill, BG_PANEL);
        assert_eq!(v.extreme_bg_color, BG_EDITOR);
        assert_eq!(v.hyperlink_color, GREEN);
        assert_eq!(v.warn_fg_color, ORANGE);
        assert_eq!(v.error_fg_color, RED);
        assert_eq!(v.selection.bg_fill, PURPLE_SELECT);
        assert_eq!(v.widgets.noninteractive.fg_stroke.color, TEXT);
    }

    #[test]
    fn apply_changes_only_the_dark_style() {
        let ctx = egui::Context::default();
        let light_before = ctx.style_of(Theme::Light).visuals.panel_fill;
        apply(&ctx);
        assert_eq!(ctx.style_of(Theme::Dark).visuals.panel_fill, BG_PANEL);
        assert_eq!(ctx.style_of(Theme::Light).visuals.panel_fill, light_before);
    }
}

/// 語法上色的顏色，依 [`crate::syntax::Kind`] 的順序：
/// 一般（不使用）、註解、字串、數字、關鍵字、型別、函式。
pub fn syntax_colors(dark: bool) -> [Color32; 7] {
    let c = Color32::from_rgb;
    if dark {
        [
            palette::TEXT,
            c(0x6F, 0x80, 0x9C), // 註解：偏灰藍
            c(0xA5, 0xE0, 0x75), // 字串：柔和綠
            c(0xF7, 0x8C, 0x6C), // 數字：橘
            c(0xC7, 0x92, 0xEA), // 關鍵字：初號機紫的亮色
            c(0xFF, 0xCB, 0x6B), // 型別：黃
            c(0x82, 0xAA, 0xFF), // 函式：藍
        ]
    } else {
        [
            Color32::BLACK,
            c(0x6E, 0x77, 0x81),
            c(0x0A, 0x7B, 0x34),
            c(0xB5, 0x53, 0x0E),
            c(0x82, 0x50, 0xDF),
            c(0x95, 0x38, 0x00),
            c(0x05, 0x50, 0xAE),
        ]
    }
}
