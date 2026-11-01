// iced IME 驗證原型：使用內建 text_editor 元件
use iced::widget::{column, text, text_editor};
use iced::{Element, Font, Length};
use std::borrow::Cow;

#[derive(Default)]
struct App {
    content: text_editor::Content,
}

#[derive(Debug, Clone)]
enum Message {
    Edit(text_editor::Action),
}

fn update(app: &mut App, message: Message) {
    match message {
        Message::Edit(action) => app.content.perform(action),
    }
}

fn view(app: &App) -> Element<'_, Message> {
    column![
        text("iced IME 測試：請用注音／倉頡／拼音輸入，觀察候選字窗位置與組字中文字"),
        text_editor(&app.content)
            .on_action(Message::Edit)
            .height(Length::Fill),
    ]
    .spacing(8)
    .padding(8)
    .into()
}

fn main() -> iced::Result {
    // 載入微軟正黑體並設為預設字型
    let mut app = iced::application(App::default, update, view).title("IME 驗證 - iced");
    if let Ok(bytes) = std::fs::read(r"C:\Windows\Fonts\msjh.ttc") {
        let leaked: &'static [u8] = Box::leak(bytes.into_boxed_slice());
        app = app
            .font(Cow::Borrowed(leaked))
            .default_font(Font::with_name("Microsoft JhengHei"));
    } else {
        eprintln!("找不到 msjh.ttc，中文字形將無法顯示");
    }
    app.run()
}
