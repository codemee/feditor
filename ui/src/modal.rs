//! 程式內的確認對話框：扁平、跟整體配色一致，取代系統的 MessageBox。
//!
//! 對話框只是資料（[`Spec`]，有需要時再加一個 [`Input`] 文字框）：每個影格由 [`show`] 畫出來，
//! 使用者按了按鈕就回傳 [`Answer`]。呼叫端把「按下之後要做什麼」自己記著，不會卡住整個程式。

use eframe::egui;
use egui::text::CCursor;
use egui::text_selection::CCursorRange;
use std::ops::Range;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Answer {
    Yes,
    No,
    /// 「全部都否」：關閉視窗時有好幾份未儲存的文件，一次都不儲存。
    NoToAll,
    Cancel,
}

#[derive(Clone, Debug)]
pub struct Spec {
    pub title: String,
    pub message: String,
    /// 由左到右的按鈕。第一個是預設按鈕（按 Enter 就是它）。
    pub buttons: Vec<(String, Answer)>,
    /// 輸入有誤時顯示在文字框下面的紅字。
    pub error: Option<String>,
    /// 按 Esc 或點到對話框外面時的答案；`None` 表示這些動作不關閉對話框（避免誤丟資料）。
    pub dismiss: Option<Answer>,
}

impl Spec {
    pub fn new(title: impl Into<String>, message: impl Into<String>) -> Self {
        Spec {
            title: title.into(),
            message: message.into(),
            buttons: Vec::new(),
            error: None,
            dismiss: None,
        }
    }

    /// 加一個按鈕。沒有明確指定 [`Spec::no_dismiss`] 時，按 Esc 的答案是「取消」（有的話）或「否」。
    pub fn button(mut self, label: &str, answer: Answer) -> Self {
        self.buttons.push((label.to_owned(), answer));
        self.dismiss = Some(self.default_dismiss());
        self
    }

    fn default_dismiss(&self) -> Answer {
        if self.buttons.iter().any(|(_, a)| *a == Answer::Cancel) {
            Answer::Cancel
        } else {
            Answer::No
        }
    }

    /// 按 Esc 或點到外面都不關閉：只能按按鈕。
    pub fn no_dismiss(mut self) -> Self {
        self.dismiss = None;
        self
    }
}

/// 對話框裡的單行文字輸入。
#[derive(Clone, Debug)]
pub struct Input {
    pub text: String,
    /// 一開始要選取的字元範圍（例如檔名的主檔名，不含副檔名）；`None` 表示全選。
    select: Option<Range<usize>>,
    focused: bool,
    /// 文字框下面的單選項目（沒有就不畫）；`choice` 是選到的那一個。
    pub choices: Vec<String>,
    pub choice: usize,
}

impl Input {
    pub fn new(text: impl Into<String>) -> Self {
        Input {
            text: text.into(),
            select: None,
            focused: false,
            choices: Vec::new(),
            choice: 0,
        }
    }

    /// 文字框下面加一排單選項目，一開始選 `selected`。
    pub fn with_choices(mut self, labels: Vec<String>, selected: usize) -> Self {
        self.choice = selected.min(labels.len().saturating_sub(1));
        self.choices = labels;
        self
    }

    pub fn select(mut self, range: Range<usize>) -> Self {
        self.select = Some(range);
        self
    }
}

/// 畫出對話框。使用者做出選擇時回傳答案；有 `input` 時，按 Enter 等於按第一個按鈕。
pub fn show(ctx: &egui::Context, spec: &Spec, mut input: Option<&mut Input>) -> Option<Answer> {
    let mut answer = None;
    let has_input = input.is_some();
    let input_id = egui::Id::unique("modal_input");
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
        if let Some(input) = input.as_deref_mut() {
            ui.add_space(8.0);
            let out = egui::TextEdit::singleline(&mut input.text)
                .id(input_id)
                .desired_width(f32::INFINITY)
                .show(ui);
            if !input.focused {
                input.focused = true;
                out.response.request_focus();
                let len = input.text.chars().count();
                let range = input.select.clone().unwrap_or(0..len);
                let (a, b) = (range.start.min(len), range.end.min(len));
                let mut state = out.state;
                state
                    .cursor
                    .set_char_range(Some(CCursorRange::two(CCursor::new(a), CCursor::new(b))));
                state.store(ui.ctx(), input_id);
                ui.ctx().request_repaint();
            } else if out.response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                answer = spec.buttons.first().map(|(_, a)| *a);
            }
            if !input.choices.is_empty() {
                ui.add_space(6.0);
                let choices = input.choices.clone();
                for (i, label) in choices.iter().enumerate() {
                    ui.radio_value(&mut input.choice, i, label.as_str());
                }
            }
        }
        if let Some(err) = &spec.error {
            ui.add_space(6.0);
            ui.label(egui::RichText::new(err).color(ui.visuals().error_fg_color));
        }
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
                // 沒有文字框時，預設按鈕取得焦點（按 Enter 就是它）
                if primary && !has_input && ui.memory(|m| m.focused().is_none()) {
                    r.request_focus();
                }
                if r.clicked() {
                    answer = Some(*a);
                }
            }
        });
    });
    if answer.is_none() && response.should_close() {
        answer = spec.dismiss;
    }
    answer
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dismiss_prefers_cancel_over_no_and_can_be_disabled() {
        let three = Spec::new("t", "m")
            .button("是", Answer::Yes)
            .button("否", Answer::No)
            .button("取消", Answer::Cancel);
        assert_eq!(three.dismiss, Some(Answer::Cancel));
        let two = Spec::new("t", "m").button("是", Answer::Yes).button("否", Answer::No);
        assert_eq!(two.dismiss, Some(Answer::No));
        assert_eq!(two.no_dismiss().dismiss, None);
    }

    fn run(spec: &Spec, input: Option<&mut Input>) -> Option<Answer> {
        let ctx = egui::Context::default();
        let mut out = None;
        let mut input = input;
        let mut output = ctx.run_ui(Default::default(), |ui| {
            out = show(ui.ctx(), spec, input.as_deref_mut());
        });
        output.textures_delta.clear();
        out
    }

    #[test]
    fn shows_buttons_and_returns_no_answer_until_something_is_clicked() {
        let spec = Spec::new("標題", "訊息").button("確定", Answer::Yes);
        assert_eq!(run(&spec, None), None, "沒人按就不會有答案");
    }

    #[test]
    fn input_dialog_draws_error_and_selects_the_requested_range_on_first_frame() {
        let mut spec = Spec::new("改名", "新名稱").button("好", Answer::Yes);
        spec.error = Some("名稱不能是空的".to_owned());
        let mut input = Input::new("report.txt").select(0..6);
        assert_eq!(run(&spec, Some(&mut input)), None);
        assert!(input.focused, "第一個影格就取得焦點");
        assert_eq!(input.text, "report.txt");
    }
}
