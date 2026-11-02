//! 把配色方案（[`crate::scheme`]）套到 egui：淺色與深色各自套一套。
//!
//! 語法上色的顏色不屬於 egui 的 `Visuals`，所以另外放在 egui 的暫存資料裡（淺色、深色各一份），
//! 編輯區每個影格依目前的主題取用，見 [`syntax_colors`]。

use crate::scheme::{self, Scheme};
use eframe::egui::{self, Color32, Theme};

fn syntax_id(dark: bool) -> egui::Id {
    egui::Id::unique(if dark { "syntax_colors_dark" } else { "syntax_colors_light" })
}

/// 套用配色：`light` 套到淺色主題、`dark` 套到深色主題，兩邊互不影響。
/// 目前是哪個主題（含「跟隨系統」）由 egui 決定，這裡兩邊都先設好。
pub fn apply(ctx: &egui::Context, light: &Scheme, dark: &Scheme) {
    ctx.style_mut_of(Theme::Light, |style| style.visuals = scheme::visuals_for(light));
    ctx.style_mut_of(Theme::Dark, |style| style.visuals = scheme::visuals_for(dark));
    ctx.data_mut(|d| {
        d.insert_temp(syntax_id(false), light.syntax_colors());
        d.insert_temp(syntax_id(true), dark.syntax_colors());
    });
    ctx.request_repaint();
}

/// 語法上色的顏色，依 [`crate::syntax::Kind`] 的順序：
/// 一般、註解、字串、數字、關鍵字、型別、函式。還沒套用過配色時（例如測試）用預設配色的。
pub fn syntax_colors(ctx: &egui::Context, dark: bool) -> [Color32; 7] {
    ctx.data(|d| d.get_temp(syntax_id(dark)))
        .unwrap_or_else(|| scheme::default_for(dark).syntax_colors())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scheme::Role;

    #[test]
    fn apply_sets_light_and_dark_independently() {
        let ctx = egui::Context::default();
        let all = scheme::builtins();
        let light = all.iter().find(|s| s.name == "GitHub Light").unwrap();
        let dark = all.iter().find(|s| s.name == "Dracula").unwrap();
        apply(&ctx, light, dark);
        assert_eq!(ctx.style_of(Theme::Light).visuals.panel_fill, light.get(Role::PanelBg));
        assert_eq!(ctx.style_of(Theme::Dark).visuals.panel_fill, dark.get(Role::PanelBg));
        assert_eq!(syntax_colors(&ctx, false), light.syntax_colors());
        assert_eq!(syntax_colors(&ctx, true), dark.syntax_colors());
        // 只換深色：淺色那一邊原封不動
        let other_dark = all.iter().find(|s| s.name == "Nord").unwrap();
        apply(&ctx, light, other_dark);
        assert_eq!(ctx.style_of(Theme::Light).visuals.panel_fill, light.get(Role::PanelBg));
        assert_eq!(ctx.style_of(Theme::Dark).visuals.extreme_bg_color, other_dark.get(Role::EditorBg));
    }

    #[test]
    fn syntax_colors_fall_back_to_the_defaults_before_anything_is_applied() {
        let ctx = egui::Context::default();
        assert_eq!(syntax_colors(&ctx, true), scheme::default_for(true).syntax_colors());
        assert_eq!(syntax_colors(&ctx, false), scheme::default_for(false).syntax_colors());
    }
}
