// egui IME 驗證原型：只有一個多行文字框，用來測試繁體中文輸入法
use eframe::egui;
use std::sync::Arc;

fn install_cjk_font(ctx: &egui::Context) {
    // 載入微軟正黑體；找不到就維持預設字型（中文會變方塊）
    let Ok(bytes) = std::fs::read(r"C:\Windows\Fonts\msjh.ttc") else {
        eprintln!("找不到 msjh.ttc，中文字形將無法顯示");
        return;
    };
    let mut fonts = egui::FontDefinitions::default();
    fonts
        .font_data
        .insert("cjk".into(), Arc::new(egui::FontData::from_owned(bytes)));
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        fonts.families.entry(family).or_default().push("cjk".into());
    }
    ctx.set_fonts(fonts);
}

#[derive(Default)]
struct App {
    text: String,
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        ui.label("egui IME 測試：請用注音／倉頡／拼音輸入，觀察候選字窗位置與組字中文字");
        ui.separator();
        ui.add_sized(
            ui.available_size(),
            egui::TextEdit::multiline(&mut self.text).code_editor(),
        );
    }
}

fn main() -> eframe::Result {
    eframe::run_native(
        "IME 驗證 - egui",
        eframe::NativeOptions::default(),
        Box::new(|cc| {
            install_cjk_font(&cc.egui_ctx);
            Ok(Box::new(App::default()))
        }),
    )
}
