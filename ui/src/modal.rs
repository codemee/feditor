//! 程式內的確認對話框：扁平、跟整體配色一致，取代系統的 MessageBox。
//!
//! 對話框只是資料（[`Spec`]）：每個影格由 [`show`] 畫出來，使用者按了按鈕就回傳
//! [`Answer`]。呼叫端把「按下之後要做什麼」自己記著，不會卡住整個程式。

use eframe::egui;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Answer {
    Yes,
    No,
    Cancel,
}

#[derive(Clone, Debug)]
pub struct Spec {
    pub title: String,
    pub message: String,
    /// 由左到右的按鈕。第一個是預設按鈕（會取得焦點，按 Enter 就是它）。
    pub buttons: Vec<(String, Answer)>,
}

impl Spec {
    pub fn new(title: impl Into<String>, message: impl Into<String>) -> Self {
        Spec {
            title: title.into(),
            message: message.into(),
            buttons: Vec::new(),
        }
    }

    pub fn button(mut self, label: &str, answer: Answer) -> Self {
        self.buttons.push((label.to_owned(), answer));
        self
    }

    /// 按 Esc 或點到對話框外面時的答案：有「取消」就是取消，否則是「否」。
    pub fn dismiss_answer(&self) -> Answer {
        if self.buttons.iter().any(|(_, a)| *a == Answer::Cancel) {
            Answer::Cancel
        } else {
            Answer::No
        }
    }
}

/// 畫出對話框。使用者做出選擇時回傳答案。
pub fn show(ctx: &egui::Context, spec: &Spec) -> Option<Answer> {
    let mut answer = None;
    let response = egui::Modal::new(egui::Id::unique("app_modal")).show(ctx, |ui| {
        ui.set_width(360.0);
        {
            // 按鈕 hover 時不畫外框，只留底色（與其他地方一致）
            let w = &mut ui.visuals_mut().widgets;
            for s in [&mut w.hovered, &mut w.active, &mut w.open] {
                s.bg_stroke = egui::Stroke::NONE;
            }
        }
        ui.label(egui::RichText::new(&spec.title).strong().size(15.0));
        ui.add_space(8.0);
        ui.label(&spec.message);
        ui.add_space(16.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            ui.spacing_mut().button_padding = egui::vec2(14.0, 5.0);
            // 右到左排列：先放最後一個，第一個按鈕最後放、落在最左邊
            for (i, (label, a)) in spec.buttons.iter().enumerate().rev() {
                let primary = i == 0;
                let mut button = egui::Button::new(label.as_str());
                if primary {
                    button = button.fill(ui.visuals().selection.bg_fill);
                }
                let r = ui.add(button);
                if primary && ui.memory(|m| m.focused().is_none()) {
                    r.request_focus();
                }
                if r.clicked() {
                    answer = Some(*a);
                }
            }
        });
    });
    if answer.is_none() && response.should_close() {
        answer = Some(spec.dismiss_answer());
    }
    answer
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dismiss_prefers_cancel_over_no() {
        let three = Spec::new("t", "m")
            .button("是", Answer::Yes)
            .button("否", Answer::No)
            .button("取消", Answer::Cancel);
        assert_eq!(three.dismiss_answer(), Answer::Cancel);
        let two = Spec::new("t", "m").button("是", Answer::Yes).button("否", Answer::No);
        assert_eq!(two.dismiss_answer(), Answer::No);
    }

    #[test]
    fn shows_buttons_and_returns_the_clicked_answer() {
        let ctx = egui::Context::default();
        let spec = Spec::new("標題", "訊息").button("確定", Answer::Yes);
        let mut out = None;
        let mut output = ctx.run_ui(Default::default(), |ui| {
            out = show(ui.ctx(), &spec);
        });
        output.textures_delta.clear();
        assert_eq!(out, None, "沒人按就不會有答案");
    }
}
