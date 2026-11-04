// FEditor UI：以 egui 自繪。編輯區是自繪的（見 editor 模組），不用 egui 的 TextEdit。
// 操作以工具列按鈕為主，不使用功能表。

// Windows 的 release 版是視窗程式：不要在背後多開一個主控台視窗（debug 版保留，方便看輸出）
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod document;
mod editor;
mod find;
mod folder_search;
mod fonts;
mod icons;
mod language;
mod macro_lib;
mod macros;
mod modal;
mod pane_ops;
mod panes;
mod preview;
mod preview_pane;
mod preview_ui;
mod scheme;
mod session;
mod settings;
mod syntax;
mod tabs;
mod terminal;
mod terminal_pane;
mod terminal_text;
mod shell_icons;
mod watcher;
mod theme;

use document::{Document, SaveDocError};
use eframe::egui;
use feditor_core::i18n::t;
use feditor_core::tf;
use egui::text::CCursor;
use egui::text_selection::CCursorRange;
use encoding_rs::Encoding;
use editor::model::EditorState;
use feditor_core::fileio::{LineEnding, common_encodings, format_choices, format_label};
use find::{FindState, Summary};
use icons::Icon;
use settings::{
    DEFAULT_FONT_SIZE, DEFAULT_PREVIEW_FONT_SIZE, DEFAULT_TERMINAL_FONT_SIZE, DEFAULT_TERMINAL_MIN_CONTRAST, MAX_FONT_SIZE, MAX_TERMINAL_CONTRAST, MIN_FONT_SIZE, MIN_TERMINAL_CONTRAST, Settings, ThemeChoice, adjust_font_size,
    clamp_font_size, zoom_steps,
};
use std::ops::Range;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// 窗格 `pane` 的編輯畫面的 id（鍵盤焦點、捲動位置都掛在它上面）。
fn editor_id(pane: panes::PaneId) -> egui::Id {
    egui::Id::unique(("editor", pane))
}

fn find_input_id() -> egui::Id {
    egui::Id::unique("find_input")
}

fn folder_query_id() -> egui::Id {
    egui::Id::unique("folder_query")
}

fn replace_input_id() -> egui::Id {
    egui::Id::unique("replace_input")
}

/// 下拉選單剛好容納最長項目所需的寬度：文字寬度 + 右側箭頭 + 內距。
fn combo_width<'a>(ui: &egui::Ui, labels: impl IntoIterator<Item = &'a str>) -> f32 {
    let font = egui::TextStyle::Button.resolve(ui.style());
    let text_w = labels
        .into_iter()
        .map(|s| {
            ui.painter()
                .layout_no_wrap(s.to_owned(), font.clone(), egui::Color32::PLACEHOLDER)
                .size()
                .x
        })
        .fold(0.0_f32, f32::max);
    let spacing = ui.spacing();
    text_w + spacing.icon_width + spacing.button_padding.x * 2.0 + spacing.item_spacing.x
}

/// 搜尋框或取代框有鍵盤焦點時，讓它自己留住 Tab：egui 預設會在畫面一開始就把 Tab 當成「移到下一個元件」，
/// 留住之後 Tab 才會留在事件裡，由 [`App::tab_between_find_and_replace`] 在搜尋與取代之間切換。
/// 方向鍵照原本的樣子留給輸入框（游標左右移動）。
fn keep_tab_in_find_inputs(ui: &egui::Ui) {
    let filter = egui::EventFilter { tab: true, horizontal_arrows: true, vertical_arrows: true, escape: false };
    ui.memory_mut(|m| {
        for id in [find_input_id(), replace_input_id()] {
            if m.has_focus(id) {
                m.set_focus_lock_filter(id, filter);
            }
        }
    });
}

/// 顯示系統的「開啟檔案」交談窗（可多選），從 `dir` 開始；取消回傳空清單。
fn pick_files_in_dialog(dir: &std::path::Path) -> Vec<PathBuf> {
    rfd::FileDialog::new()
        .set_title(t("開啟檔案"))
        .set_directory(dir)
        .pick_files()
        .unwrap_or_default()
}

/// 配色狀態鈕目前要顯示的圖示。
fn theme_icon(choice: ThemeChoice) -> Icon {
    match choice {
        ThemeChoice::System => Icon::ThemeSystem,
        ThemeChoice::Light => Icon::ThemeLight,
        ThemeChoice::Dark => Icon::ThemeDark,
    }
}

/// 介面語言狀態鈕目前要顯示的圖示。
fn language_icon(choice: feditor_core::i18n::LangChoice) -> Icon {
    use feditor_core::i18n::LangChoice;
    match choice {
        LangChoice::System => Icon::LangSystem,
        LangChoice::Chinese => Icon::LangChinese,
        LangChoice::English => Icon::LangEnglish,
    }
}

fn theme_label(choice: ThemeChoice) -> &'static str {
    match choice {
        ThemeChoice::System => t("跟隨系統"),
        ThemeChoice::Light => t("淺色"),
        ThemeChoice::Dark => t("深色"),
    }
}

/// 確認對話框回答之後要接著做的事。
enum Then {
    CloseTab,
    CloseWindow,
    Reload(&'static Encoding),
    DeleteMacro(usize),
    External,
    /// 錄好的巨集原始碼，等使用者輸入名稱後存檔。
    SaveRecording(String),
    /// 重新命名巨集庫裡的第 i 支巨集（改檔頭的 `@name`）。
    RenameMacro(usize),
    /// 重新命名作用中文件的檔案（磁碟上的檔名）。
    RenameFile,
    /// 幫目前的終端機分頁取名字。
    RenameTerminal,
    /// 把正在編輯的配色另存成自訂配色。
    SaveSchemeAs,
}

struct Pending {
    spec: modal::Spec,
    then: Then,
    /// 有的話，對話框裡會有一個文字框（重新命名、命名巨集）。
    input: Option<modal::Input>,
}

impl Pending {
    fn new(spec: modal::Spec, then: Then) -> Self {
        Pending { spec, then, input: None }
    }

    fn with_input(mut self, input: modal::Input) -> Self {
        self.input = Some(input);
        self
    }
}

/// 觸發快捷鍵時，系統同時送來的那個字元事件（Alt+M 會多一個 "m"）不能讓編輯區當成打字收走。
/// 只丟掉文字等於這個鍵的字元，回傳丟掉幾個。
fn drop_key_text(events: &mut Vec<egui::Event>, key: egui::Key) -> usize {
    let before = events.len();
    let (symbol, name) = (key.symbol_or_name().to_lowercase(), key.name().to_lowercase());
    // egui 的減號符號是 Unicode 的「−」，實際打出來的是 ASCII 的 "-"
    let ascii_minus = key == egui::Key::Minus;
    events.retain(|e| match e {
        egui::Event::Text(t) => {
            let t = t.to_lowercase();
            t != symbol && t != name && !(ascii_minus && t == "-")
        }
        _ => true,
    });
    before - events.len()
}

/// 圖示按鈕之間的間距（像素）。
const BUTTON_GAP: f32 = 4.0;

/// 把這一列的元件間距縮成 [`BUTTON_GAP`]。放圖示按鈕的橫列開頭呼叫。
fn tight_buttons(ui: &mut egui::Ui) {
    ui.spacing_mut().item_spacing.x = BUTTON_GAP;
}

/// 設定面板的一個群組：兩欄的表單。所有群組的標籤欄一樣寬，控制項才會左緣對齊。
fn settings_grid(ui: &mut egui::Ui, id: &str, add_rows: impl FnOnce(&mut egui::Ui)) {
    egui::Grid::new(id)
        .num_columns(2)
        .min_col_width(96.0)
        .spacing([14.0, 4.0])
        .show(ui, add_rows);
}

/// 設定面板群組的標題。
fn group_title(ui: &mut egui::Ui, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .strong()
            .color(ui.visuals().weak_text_color()),
    );
    ui.add_space(2.0);
}

/// 兩個群組之間的分隔。
fn group_gap(ui: &mut egui::Ui) {
    ui.add_space(4.0);
    ui.separator();
    ui.add_space(2.0);
}

/// 表單的一列：左邊文字標籤，右邊控制項。每一列的高度一律和圖示按鈕一樣高，
/// 標籤與控制項都垂直置中，列與列的間距才會一致（不會因為有沒有按鈕而忽大忽小）。
fn form_row(ui: &mut egui::Ui, label: &str, add: impl FnOnce(&mut egui::Ui)) {
    let height = icons::BUTTON_HEIGHT;
    ui.horizontal(|ui| {
        ui.set_min_height(height);
        ui.label(label);
    });
    ui.horizontal(|ui| {
        ui.set_min_height(height);
        add(ui);
    });
    ui.end_row();
}

/// 這一區的元件 hover／按下／展開時不畫外框，只保留底色。
fn no_hover_outline(ui: &mut egui::Ui) {
    let w = &mut ui.visuals_mut().widgets;
    for s in [&mut w.hovered, &mut w.active, &mut w.open] {
        s.bg_stroke = egui::Stroke::NONE;
        s.expansion = 0.0; // hover 時底色不要比原本的範圍大一圈
    }
}

/// 檔案拖到視窗上、還沒放開時的提示：整個視窗蓋一層淡色，中間寫「放開以開啟 N 個檔案」。
fn drop_overlay(ctx: &egui::Context, count: usize) {
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::unique("drop_overlay"),
    ));
    let rect = ctx.content_rect();
    let accent = ctx.global_style().visuals.selection.bg_fill;
    painter.rect_filled(rect, 0.0, accent.gamma_multiply(0.35));
    painter.rect_stroke(
        rect.shrink(8.0),
        egui::CornerRadius::same(8),
        egui::Stroke::new(2.0, accent),
        egui::StrokeKind::Inside,
    );
    let text = if count == 1 {
        t("放開以開啟檔案").to_owned()
    } else {
        tf!("放開以開啟 {count} 個檔案", count = count)
    };
    let color = ctx.global_style().visuals.strong_text_color();
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        text,
        egui::FontId::proportional(20.0),
        color,
    );
}

/// 下拉清單裡面（選項）的樣式：選項 hover 時不畫外框，並在右邊留出捲軸的位置，
/// 這樣捲軸不會壓在選項（特別是被選取的那一項）的底色上。放在 `show_ui` 的開頭呼叫。
fn flat_list(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    no_hover_outline(ui);
    egui::Frame::NONE
        .inner_margin(egui::Margin { right: 14, ..Default::default() })
        .show(ui, add);
}

/// 中央面板（放窗格的地方）的左右內縮。
const CENTRAL_SIDE_MARGIN: i8 = 8;

/// 下方面板（狀態列、輸出面板）用的外框：預設的面板外框，但左右內縮改成和窗格卡片的外緣一樣
///（中央面板的內縮加上編輯區的空隙），內容的兩端才會和圓角框對齊。
fn aligned_panel_frame(style: &egui::Style) -> egui::Frame {
    let frame = egui::Frame::side_top_panel(style);
    let side = CENTRAL_SIDE_MARGIN + pane_ops::AREA_INSET_X.round() as i8;
    let m = frame.inner_margin;
    frame.inner_margin(egui::Margin { left: side, right: side, top: m.top, bottom: m.bottom })
}

/// 工具列的群組分隔線：只畫一小段，比按鈕 hover 時的底色還矮。
/// （`ui.separator()` 在橫向排列裡會撐滿整列高度，看起來太長。）
fn group_separator(ui: &mut egui::Ui) {
    // 配置的高度與按鈕相同，這樣不會改變整列的對齊；寬度只留線本身，
    // 加上兩側各一個按鈕間距，群組之間就是 4 + 1 + 4 = 9 像素
    let (rect, _) = ui.allocate_exact_size(egui::vec2(1.0, 30.0), egui::Sense::hover());
    let x = rect.center().x;
    let half = 8.0;
    let color = ui.visuals().widgets.noninteractive.bg_stroke.color;
    ui.painter().line_segment(
        [
            egui::pos2(x, rect.center().y - half),
            egui::pos2(x, rect.center().y + half),
        ],
        egui::Stroke::new(1.0, color),
    );
}

/// 尋找列的選項開關：點擊時翻轉並回傳 true。
fn option_toggle(ui: &mut egui::Ui, icon: Icon, tip: &str, value: &mut bool) -> bool {
    if icons::small_toggle(ui, icon, tip, *value, true).clicked() {
        *value = !*value;
        true
    } else {
        false
    }
}

/// 套用字型設定：編輯區用指定的等寬字型（`None` 表示系統預設）。
fn install_fonts(ctx: &egui::Context, family: Option<&str>) {
    ctx.set_fonts(fonts::build_fonts(family));
}

/// 規則表達式按鈕的 tooltip：基本語法速查，加上完整說明的連結。
fn regex_help(ui: &mut egui::Ui) {
    ui.set_max_width(400.0);
    ui.strong(t("規則表達式（Rust regex 語法）"));
    ui.add_space(2.0);
    egui::Grid::new("regex_help")
        .num_columns(2)
        .spacing([14.0, 2.0])
        .show(ui, |ui| {
            for (syntax, meaning) in [
                (".", t("任意字元（不含換行）")),
                ("\\d  \\w  \\s", t("數字、字母數字底線、空白")),
                ("^  $", t("行首、行尾（以「行」為單位）")),
                ("[abc]  [^abc]", t("其中之一、排除這些")),
                ("*  +  ?", t("重複 0 次以上、1 次以上、0 或 1 次")),
                ("{2,5}", t("重複 2 到 5 次（後面加 ? 為非貪婪）")),
                ("(…)  |", t("群組（取代時用 $1）、或")),
                ("\\.  \\(", t("跳脫特殊字元")),
                ("(?i)", t("不分大小寫")),
            ] {
                ui.label(egui::RichText::new(syntax).monospace());
                ui.label(meaning);
                ui.end_row();
            }
        });
    ui.add_space(4.0);
    ui.label(t("取代字串：$1 代入群組，\\n 換行，\\t 跳位，$$ 為字面的 $"));
    ui.add_space(4.0);
    ui.hyperlink_to(
        t("完整語法說明（docs.rs/regex）"),
        "https://docs.rs/regex/latest/regex/#syntax",
    );
}

#[derive(Clone, Copy, PartialEq)]
enum LogKind {
    Info,
    Output,
    Warn,
    Error,
}

/// 工具列或快捷鍵觸發的動作。先收集、等畫面畫完再處理，避免在繪製中途跳出對話框。
enum Action {
    New,
    Open,
    Save,
    SaveAs,
    RunMacro,
    RerunMacro,
    ClearLog,
    ToggleOutput,
    Reload(&'static Encoding),
    /// 開啟尋找列；`replace` 為 true 時一併展開取代欄。
    Find { replace: bool },
    FindNext,
    FindPrev,
    /// 搜尋字串或選項被改了：即時重新搜尋。
    FindChanged,
    ReplaceOne,
    ReplaceAll,
    CloseFind,
    /// 復原／重做。編輯區有焦點時由編輯區自己處理；焦點不在任何輸入框時（例如剛按完「全部取代」）走這裡。
    Undo,
    Redo,
    /// 切換配色（跟隨系統、淺色、深色），並記到設定檔。
    SetTheme(ThemeChoice),
    /// 選用配色：（是不是深色主題, 配色名稱）。
    SetScheme(bool, String),
    /// 編輯目前主題正在用的配色。
    EditScheme,
    OpenThemesDir,
    /// 設定編輯區的字級（點）。設定面板的字級欄與快捷鍵都走這一條，所以兩邊永遠同步。
    SetFontSize(f32),
    /// 設定預覽窗格的字級（點）。
    SetPreviewFontSize(f32),
    SetTerminalFontSize(f32),
    SetTerminalMinContrast(f32),
    SetTerminalShell(String),
    /// 設定編輯區的字型家族；`None` 表示作業系統預設的等寬字型。
    SetFontFamily(Option<String>),
    /// 編輯區的顯示選項（空白字元、行號、目前行、自動換行）。
    SetView(settings::ViewOptions),
    /// 狀態列欄號裡，全形字（中日韓文字等）算 1 欄還是 2 欄。
    SetWideCols(u8),
    /// 介面語言：跟隨系統、中文、英文。
    SetLanguage(feditor_core::i18n::LangChoice),
    CloseTab(usize),
    NextTab,
    PrevTab,
    /// 某個窗格的分頁列上的操作（切換、關閉、改名）。先讓那個窗格取得焦點，再照一般的分頁操作處理。
    PaneTab(panes::PaneId, pane_ops::TabCmd),
    /// 關閉窗格：裡面的分頁移到相鄰的窗格。
    ClosePane(panes::PaneId),
    /// 開啟或關閉有焦點的窗格的預覽。
    TogglePreview,
    /// 新增一個終端機（用設定裡的預設終端機）。
    NewTerminal,
    /// 新增一個終端機，指定要執行的 shell（程式加參數）。
    NewTerminalWith(String),
    /// 關閉一個預覽窗格。
    ClosePreview(panes::PaneId),
    /// 開啟預覽外掛的資料夾。
    OpenPreviewsDir,
    /// 預覽裡點了指向本機檔案的連結：在編輯器開啟。
    OpenLocalFile(PathBuf),
    /// 開始拖曳一個預覽窗格。
    BeginPreviewDrag(panes::PaneId),
    /// 把預覽窗格（第一個）搬到一般窗格（第二個）的某一側。
    MovePreview(panes::PaneId, panes::PaneId, panes::Side),
    /// 開始拖曳某個窗格的某個分頁。
    BeginTabDrag(panes::PaneId, usize),
    /// 把某個窗格的某個分頁搬到別處（放進另一個窗格，或切出新窗格）。
    MoveTab(panes::PaneId, usize, pane_ops::Target),
    /// 展開或收起巨集面板。
    ToggleMacroMenu,
    RunMacroEntry(usize),
    EditMacro(usize),
    RenameMacro(usize),
    DeleteMacro(usize),
    /// 開始（或取消）等待使用者按下這支巨集的新快捷鍵。
    CaptureShortcut(usize),
    ToggleRecord,
    OpenMacroDir,
    /// 開關跨檔搜尋面板。
    ToggleFolderSearch,
    RunFolderSearch,
    StopFolderSearch,
    PickSearchFolder,
    /// 打開搜尋結果的第幾筆。
    GotoHit(usize),
}

/// 背景掃描出來的字型資訊。
struct FontScan {
    /// 系統上所有等寬字型家族的名稱。
    families: Vec<String>,
    /// 作業系統預設的等寬字型家族名稱。
    default_name: Option<String>,
}

/// 設定編輯區（與輸出窗格）使用的等寬字級。
fn apply_font_size(ctx: &egui::Context, size: f32) {
    ctx.all_styles_mut(|style| {
        if let Some(font) = style.text_styles.get_mut(&egui::TextStyle::Monospace) {
            font.size = size;
        }
    });
}

const OPEN: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::O);
const SAVE: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::S);
const SAVE_AS: egui::KeyboardShortcut = egui::KeyboardShortcut::new(
    egui::Modifiers {
        shift: true,
        ..egui::Modifiers::COMMAND
    },
    egui::Key::S,
);
const NEW: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::N);
const RERUN: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::NONE, egui::Key::F5);
const FIND: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::F);
const REPLACE: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::H);
const FIND_NEXT: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::NONE, egui::Key::F3);
const UNDO: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::Z);
const REDO: egui::KeyboardShortcut = egui::KeyboardShortcut::new(
    egui::Modifiers {
        shift: true,
        ..egui::Modifiers::COMMAND
    },
    egui::Key::Z,
);
const REDO_Y: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::Y);
const FIND_PREV: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::SHIFT, egui::Key::F3);
/// 開關預覽。不用 Ctrl+Shift+V：egui 會把它當成貼上。
const TOGGLE_PREVIEW: egui::KeyboardShortcut = egui::KeyboardShortcut::new(
    egui::Modifiers {
        shift: true,
        ..egui::Modifiers::COMMAND
    },
    egui::Key::P,
);
const FIND_IN_FILES: egui::KeyboardShortcut = egui::KeyboardShortcut::new(
    egui::Modifiers {
        shift: true,
        ..egui::Modifiers::COMMAND
    },
    egui::Key::F,
);
const CLOSE_TAB: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::W);
const NEXT_TAB: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::CTRL, egui::Key::Tab);
const PREV_TAB: egui::KeyboardShortcut = egui::KeyboardShortcut::new(
    egui::Modifiers {
        shift: true,
        ..egui::Modifiers::CTRL
    },
    egui::Key::Tab,
);

struct App {
    /// 有焦點的窗格、它目前作用中分頁的文件與編輯狀態；這個窗格的其他分頁放在 `tabs` 裡（見 `tabs.rs`）。
    doc: Document,
    /// 有焦點的窗格的分頁集。沒有焦點的窗格放在 `layout.others`（見 `pane_ops.rs`）。
    tabs: tabs::TabSet,
    /// 窗格版面（分割視窗）。
    layout: panes::Layout<tabs::TabSet>,
    /// 正在拖曳的分頁。
    tab_drag: Option<pane_ops::TabDrag>,
    /// 換了有焦點的窗格：下一個畫面把鍵盤焦點也移過去。
    pending_editor_focus: bool,
    /// 鍵盤焦點在預覽窗格上（點了預覽窗格）：縮放快捷鍵調的是預覽的字級，不是編輯區的。
    preview_focused: bool,
    /// 拖進來的是資料夾時，用這個函式讓使用者從該資料夾選要開的檔案（預設是系統的「開啟檔案」交談窗；測試時換掉）。
    pick_files_in: fn(&std::path::Path) -> Vec<PathBuf>,
    /// 預覽窗格（窗格編號 → 狀態）。預覽窗格也是版面裡的葉子，但不在 `layout.others`。
    previews: std::collections::HashMap<panes::PaneId, preview_pane::PreviewPane>,
    /// 按 Ctrl+` 從編輯區跳進終端機之前所在的（窗格, 分頁），繞完一圈之後回到這裡。
    terminal_return: Option<(panes::PaneId, usize)>,
    /// 這台機器上偵測到的 shell（工具列的下拉清單與設定面板用）。
    shells: Vec<terminal::ShellOption>,
    /// 每個 shell 的圖示材質（第一次畫的時候才讀）。
    shell_icons: shell_icons::ShellIcons,
    /// 使用者的預覽外掛（設定資料夾的 `previews/*.js`）。
    preview_plugins: Vec<std::sync::Arc<preview::Plugin>>,
    /// 上一個畫面每個窗格的位置（切預覽時用來判斷放不放得下）。
    pane_rects: Vec<(panes::PaneId, egui::Rect)>,
    last_macro: Option<PathBuf>,
    log: Vec<(LogKind, String)>,
    last_title: String,
    allow_close: bool,
    /// 目前顯示中的確認對話框，以及使用者回答之後要接著做的事。
    modal: Option<Pending>,
    /// 關閉視窗時，使用者已經選了「不儲存」的分頁：（窗格, 分頁編號）。
    close_skipped: Vec<(panes::PaneId, usize)>,
    find: FindState,
    /// 下一個畫面要把鍵盤焦點放到搜尋框。
    focus_find: bool,
    /// 編輯區的游標、選取與輸入法狀態。
    editor: EditorState,
    settings: Settings,
    /// 背景掃描出來的等寬字型清單；掃描完成前是 `None`。
    font_scan: Arc<Mutex<Option<FontScan>>>,
    /// 設定有尚未寫入磁碟的變更（最後一次變更的時間）。字級會連續變動，所以延遲一下再存。
    settings_dirty_since: Option<Instant>,
    /// Ctrl＋滾輪縮放量的累積值（見 [`zoom_steps`]）。
    zoom_accum: f32,
    /// 設定面板是否展開。
    settings_open: bool,
    /// 輸出面板收合時，因為新的錯誤或巨集輸出而暫時展開。只在這次執行有效，不寫進設定。
    output_peek: bool,
    /// 剛觸發過的巨集快捷鍵的按鍵：接下來幾個影格裡，等於這個鍵的字元事件要丟掉（見 [`drop_key_text`]）。
    swallow_text: Option<(egui::Key, u8)>,
    /// 所有可選的配色（內建加上 `themes` 資料夾裡的自訂配色）。
    schemes: Vec<scheme::Scheme>,
    /// 正在編輯的配色：工作副本，改動立刻預覽，按「儲存」才寫成檔案。
    scheme_edit: Option<scheme::Scheme>,
    /// 設定按鈕在畫面上的位置（面板要貼在它的下方）。
    settings_anchor: Option<egui::Rect>,
    /// 使用者的巨集清單（來自巨集資料夾）。
    macros: Vec<macro_lib::MacroEntry>,
    /// 巨集面板是否展開，以及巨集按鈕的位置。
    macro_open: bool,
    macro_anchor: Option<egui::Rect>,
    /// 正在等使用者替第幾支巨集按下新的快捷鍵。
    capturing_shortcut: Option<usize>,
    /// 跨檔搜尋。
    folder: folder_search::FolderSearch,
    /// 要不要把開啟的分頁記到工作階段檔（測試時關掉，免得寫到使用者的設定資料夾）。
    session_enabled: bool,
    /// 上次檢查外部變更的時間，以及當時視窗有沒有焦點。
    last_disk_check: Instant,
    was_focused: bool,
    /// 監看已開啟檔案所在的資料夾（第一次畫面更新時才建立，測試裡沒有）。
    watcher: Option<watcher::FileWatcher>,
}

impl App {
    fn new(initial: Option<PathBuf>, settings: Settings) -> Self {
        let mut app = Self {
            doc: Document::new(),
            tabs: tabs::TabSet::new(),
            layout: panes::Layout::default(),
            tab_drag: None,
            pending_editor_focus: false,
            preview_focused: false,
            pick_files_in: pick_files_in_dialog,
            previews: Default::default(),
            terminal_return: None,
            shells: terminal::detect_shells(),
            shell_icons: Default::default(),
            preview_plugins: Vec::new(),
            pane_rects: Vec::new(),
            last_macro: None,
            log: Vec::new(),
            last_title: String::new(),
            allow_close: false,
            modal: None,
            close_skipped: Vec::new(),
            find: FindState::default(),
            focus_find: false,
            editor: EditorState::default(),
            settings,
            font_scan: Arc::new(Mutex::new(None)),
            settings_dirty_since: None,
            zoom_accum: 0.0,
            settings_open: false,
            output_peek: false,
            swallow_text: None,
            schemes: scheme::builtins(),
            scheme_edit: None,
            settings_anchor: None,
            macros: Vec::new(),
            macro_open: false,
            macro_anchor: None,
            capturing_shortcut: None,
            folder: folder_search::FolderSearch::default(),
            session_enabled: false,
            last_disk_check: Instant::now(),
            was_focused: true,
            watcher: None,
        };
        app.refresh_macros();
        app.refresh_schemes();
        app.refresh_preview_plugins();
        if let Some(path) = initial {
            app.open_path(&path);
        }
        app
    }

    fn push(&mut self, kind: LogKind, text: impl Into<String>) {
        // 輸出面板收合時，錯誤與巨集的輸出（console.log）不能讓人錯過：暫時展開
        if matches!(kind, LogKind::Error | LogKind::Output) {
            self.output_peek = true;
        }
        self.log.push((kind, text.into()));
    }

    /// 輸出面板現在是不是展開的（使用者展開了，或是剛有錯誤／巨集輸出而暫時展開）。
    fn output_expanded(&self) -> bool {
        self.settings.show_output || self.output_peek
    }

    /// 展開／收合輸出面板。暫時展開中按下去就是收合，並一併清掉暫時展開。
    fn toggle_output(&mut self) {
        let next = !self.output_expanded();
        self.output_peek = false;
        if self.settings.show_output != next {
            self.settings.show_output = next;
            self.mark_settings_dirty();
        }
    }

    // ── 設定 ────────────────────────────────────────────────

    /// 在背景執行緒掃描系統上的等寬字型。要逐一開啟字型檔檢查，家族多時要花幾秒，
    /// 所以不能擋住啟動；掃完會要求重畫，設定面板就會出現清單。
    fn start_font_scan(&self, ctx: egui::Context) {
        let slot = Arc::clone(&self.font_scan);
        std::thread::spawn(move || {
            let scan = FontScan {
                families: fonts::list_monospace_families(),
                default_name: fonts::default_monospace_name(),
            };
            if let Ok(mut guard) = slot.lock() {
                *guard = Some(scan);
            }
            ctx.request_repaint();
        });
    }

    /// 記下「設定有變更、還沒存檔」。字級會隨著拖曳連續變動，所以不是每次都寫磁碟，
    /// 而是停下來一小段時間後才寫（見 `ui()`），關閉視窗時一定會寫。
    fn mark_settings_dirty(&mut self) {
        self.settings_dirty_since = Some(Instant::now());
    }

    /// 把尚未存檔的設定寫入磁碟。
    fn flush_settings(&mut self) {
        if self.settings_dirty_since.take().is_some() {
            if let Err(e) = self.settings.save() {
                self.push(LogKind::Warn, tf!("無法儲存設定：{e}", e = e));
            }
        }
    }

    // ── 尋找與取代 ──────────────────────────────────────────

    /// 整份文字被換掉（新文件、開檔、重新載入）：游標與搜尋狀態都要重來。
    fn text_replaced(&mut self) {
        self.find.invalidate();
        // 錄製中換了文件：錄製不中斷
        let recording = self.editor.recording.take();
        self.editor = EditorState::default();
        self.editor.recording = recording;
    }

    /// 編輯區目前的選取範圍（位元組）。
    fn editor_selection(&self) -> Range<usize> {
        let buf = &self.doc.buffer;
        let range = self.editor.sel.range();
        buf.char_to_byte(range.start.min(buf.len_chars()))
            ..buf.char_to_byte(range.end.min(buf.len_chars()))
    }

    /// 把編輯區的選取設成 `chars`（字元範圍），並讓畫面捲到那裡。
    fn select_chars(&mut self, chars: Range<usize>) {
        let buf = &mut self.doc.buffer;
        self.editor.set_caret(buf, chars.start, false);
        self.editor.set_caret(buf, chars.end, true);
        // 由搜尋的「捲到可見處」負責捲動（置中），不要再追著游標捲
        self.editor.scroll_to_caret = false;
    }

    /// 搜尋後把找到的那一筆選起來，這樣接著打字就是取代它。
    fn select_found(&mut self) {
        if let Some(chars) = self.find.current_chars.clone() {
            self.select_chars(chars);
        }
    }

    /// 搜尋的起點：向前找從選取的尾端算起，向後找從選取的開頭算起。
    fn search_anchor(&mut self, backward: bool) -> usize {
        let sel = self.editor_selection();
        if backward {
            return sel.start;
        }
        // 目前選的正好是一個空比對：要多跳過一個字元，否則會一直停在原地
        if sel.is_empty() && self.find.current.as_ref() == Some(&sel) {
            return self.doc.text()[sel.end..]
                .chars()
                .next()
                .map_or(sel.end, |c| sel.end + c.len_utf8());
        }
        sel.end
    }

    fn find_step(&mut self, backward: bool) {
        if self.find.query.is_empty() {
            self.find.open = true;
            self.focus_find = true;
            return;
        }
        let from = self.search_anchor(backward);
        let App { doc, find, .. } = self;
        find.find(doc.text(), from, backward);
        self.select_found();
    }

    /// 搜尋字串或選項改變：從目前選取的起點重新找，這樣邊打字邊縮小時不會跳走。
    fn find_changed(&mut self) {
        self.find.mark_stats_stale();
        let from = self.editor_selection().start;
        let App { doc, find, .. } = self;
        find.find(doc.text(), from, false);
        self.select_found();
    }

    fn open_find(&mut self, ctx: &egui::Context, replace: bool) {
        // 在編輯區按快捷鍵時，若選取了一小段單行文字，就帶入搜尋框
        let editor_focused = ctx.memory(|m| m.has_focus(editor_id(self.layout.focus)));
        let picked = editor_focused
            .then(|| self.editor.selected_text(&self.doc.buffer))
            .flatten()
            .filter(|s| !s.is_empty() && !s.contains('\n') && s.chars().count() <= 200);

        self.find.open = true;
        if replace {
            self.find.show_replace = true;
        }
        self.focus_find = true;
        if let Some(q) = picked {
            self.find.query = q;
            self.find_changed();
        }
    }

    fn close_find(&mut self, ctx: &egui::Context) {
        self.find.open = false;
        // 找到的那一筆已經是選取狀態；把鍵盤焦點帶回編輯區，方便直接打字取代
        ctx.memory_mut(|m| m.request_focus(editor_id(self.layout.focus)));
    }

    fn replace_one(&mut self) {
        let selection = self.editor_selection();
        let App { doc, find, editor, .. } = self;
        if let Some(replaced) = find.replace_current(&mut doc.buffer, selection) {
            doc.sync();
            editor.set_caret(&mut doc.buffer, replaced.end, false);
        }
        self.select_found();
    }

    fn replace_all(&mut self) {
        let App { doc, find, .. } = self;
        match find.replace_all(&mut doc.buffer) {
            Ok(0) => self.push(LogKind::Info, t("沒有可取代的內容")),
            Ok(n) => {
                self.doc.sync();
                // 整份文字都換過了，原本的選取範圍已經沒有意義：收成游標
                let head = self.editor.sel.head;
                self.editor.set_caret(&mut self.doc.buffer, head, false);
                self.push(LogKind::Info, tf!("已取代 {n} 筆", n = n));
            }
            // 搜尋字串是空的或規則有誤：交給搜尋流程去顯示錯誤
            Err(_) => self.find_changed(),
        }
    }
    // ── 檔案 ────────────────────────────────────────────────

    /// 對作用中的文件顯示「要儲存嗎？」對話框；答案由 [`App::resolve_modal`] 處理。
    fn ask_save(&mut self, then: Then, before: &str) {
        let spec = modal::Spec::new(
            t("尚未儲存"),
            tf!("「{}」有尚未儲存的修改。要在{}之前儲存嗎？", self.doc.name(), before),
        )
        .button(t("儲存"), modal::Answer::Yes)
        .button(t("不儲存"), modal::Answer::No)
        .button(t("取消"), modal::Answer::Cancel);
        self.modal = Some(Pending::new(spec, then));
    }

    /// 錄製結束：請使用者為巨集命名（預設名稱不與現有的重複）。`text`／`error` 用在輸入有誤、重新詢問時。
    fn ask_recording_name(&mut self, source: String, text: Option<String>, error: Option<String>) {
        let text = text.unwrap_or_else(|| self.unused_macro_name());
        let mut spec = modal::Spec::new(t("儲存錄好的巨集"), t("為這支巨集取個名字："))
            .button(t("儲存"), modal::Answer::Yes)
            .button(t("放棄"), modal::Answer::No)
            .no_dismiss(); // Esc 或點到外面不關閉，免得錄好的東西一不小心就丟了
        spec.error = error;
        self.modal = Some(Pending::new(spec, Then::SaveRecording(source)).with_input(modal::Input::new(text)));
    }

    /// 「錄製的巨集」、「錄製的巨集 2」…：第一個還沒被用掉的顯示名稱。
    fn unused_macro_name(&self) -> String {
        let taken = |n: &str| self.macros.iter().any(|m| m.name == n);
        let base = t("錄製的巨集");
        if !taken(base) {
            return base.to_owned();
        }
        (2..).map(|n| format!("{base} {n}")).find(|n| !taken(n)).expect("名稱一定找得到")
    }

    fn ask_rename_macro(&mut self, i: usize, text: Option<String>, error: Option<String>) {
        let Some(entry) = self.macros.get(i) else { return };
        let text = text.unwrap_or_else(|| entry.name.clone());
        let mut spec = modal::Spec::new(t("重新命名巨集"), tf!("「{}」的新名稱：", entry.name))
            .button(t("重新命名"), modal::Answer::Yes)
            .button(t("取消"), modal::Answer::Cancel);
        spec.error = error;
        self.modal = Some(Pending::new(spec, Then::RenameMacro(i)).with_input(modal::Input::new(text)));
    }

    /// 雙按分頁：沒有檔案的文件走「另存新檔」，有檔案的就問新檔名（只改檔名，留在同一個資料夾）。
    fn start_rename_tab(&mut self, i: usize) {
        if i >= self.tabs.len() {
            return;
        }
        self.switch_tab(i);
        if self.doc.terminal.is_some() {
            self.ask_rename_terminal(None);
        } else if self.doc.path.is_none() {
            if self.save_as() {
                self.save_session();
            }
        } else {
            self.ask_rename_file(None, None);
        }
    }

    /// 雙按終端機分頁：問新名字（留空就改回預設的「終端機 N」）。
    fn ask_rename_terminal(&mut self, text: Option<String>) {
        let Some(terminal) = self.doc.terminal.as_ref() else { return };
        let (old, number) = (terminal.title(), terminal.number());
        let text = text.unwrap_or_else(|| old.clone());
        let len = text.chars().count();
        let spec = modal::Spec::new(
            t("重新命名終端機"),
            tf!("「{old}」的新名稱（留空就改回預設的「終端機 {n}」）：", old = old, n = number),
        )
        .button(t("重新命名"), modal::Answer::Yes)
        .button(t("取消"), modal::Answer::Cancel);
        self.modal = Some(Pending::new(spec, Then::RenameTerminal).with_input(modal::Input::new(text).select(0..len)));
    }

    fn ask_rename_file(&mut self, text: Option<String>, error: Option<String>) {
        let old = self.doc.name();
        let text = text.unwrap_or_else(|| old.clone());
        // 一開始只選主檔名，改名時副檔名不會被蓋掉
        let stem = match text.rfind('.') {
            Some(p) if p > 0 => text[..p].chars().count(),
            _ => text.chars().count(),
        };
        let mut spec = modal::Spec::new(t("重新命名檔案"), tf!("「{old}」的新檔名（檔案留在同一個資料夾）：", old = old))
            .button(t("重新命名"), modal::Answer::Yes)
            .button(t("取消"), modal::Answer::Cancel);
        spec.error = error;
        self.modal = Some(Pending::new(spec, Then::RenameFile).with_input(modal::Input::new(text).select(0..stem)));
    }

    /// 把作用中文件的檔案改名。別的分頁已經開著同名檔案時拒絕，免得兩個分頁指到同一個檔案。
    fn rename_active_file(&mut self, name: &str) -> Result<(), String> {
        let old = self.doc.path.clone().ok_or(t("這份文件還沒有存成檔案"))?;
        document::validate_file_name(name)?;
        let target = old.with_file_name(name);
        if target != old {
            if let Some(other) = self.find_open_path(&target) {
                if other != (self.layout.focus, self.tabs.active()) {
                    return Err(tf!("另一個分頁已經開著「{name}」", name = name));
                }
            }
        }
        let renamed = self.doc.rename_file(name)?;
        if renamed != old {
            self.push(LogKind::Info, tf!("已將檔案重新命名為 {name}", name = name));
            self.find.invalidate();
            self.save_session();
        }
        Ok(())
    }

    /// 錄好的巨集以 `name` 為名存進巨集庫。
    fn save_recording(&mut self, source: &str, name: &str) {
        let Some(dir) = macro_lib::macros_dir() else {
            self.push(LogKind::Error, t("找不到設定資料夾，無法儲存錄好的巨集"));
            return;
        };
        match macro_lib::save_named(&dir, name, source) {
            Ok(path) => {
                self.last_macro = Some(path);
                self.refresh_macros();
                self.push(
                    LogKind::Info,
                    tf!("已儲存錄好的巨集「{name}」；按 F5 重新執行，或到巨集面板重新命名、設快捷鍵、編輯", name = name),
                );
            }
            Err(e) => self.push(LogKind::Error, tf!("無法儲存錄好的巨集：{e}", e = e)),
        }
    }

    /// 使用者回答了確認對話框：接著做原本被擋住的事。`text` 是對話框文字框裡最後的內容。
    fn resolve_modal(&mut self, then: Then, answer: modal::Answer, text: Option<String>, ctx: &egui::Context) {
        use modal::Answer::{No, Yes};
        let typed = text.as_deref().map(str::trim).unwrap_or_default().to_owned();
        match then {
            Then::SaveRecording(source) => match answer {
                Yes if typed.is_empty() => {
                    self.ask_recording_name(source, Some(String::new()), Some(t("名稱不能是空的").to_owned()));
                }
                Yes => self.save_recording(&source, &typed),
                _ => self.push(LogKind::Info, t("已放棄錄好的巨集")),
            },
            Then::RenameMacro(i) => {
                if answer == Yes {
                    if typed.is_empty() {
                        self.ask_rename_macro(i, Some(String::new()), Some(t("名稱不能是空的").to_owned()));
                    } else if let Some(entry) = self.macros.get(i).cloned() {
                        match macro_lib::rename(&entry.path, &typed) {
                            Ok(()) => {
                                self.refresh_macros();
                                self.push(LogKind::Info, tf!("已將巨集「{}」重新命名為「{}」", entry.name, typed));
                            }
                            Err(e) => self.push(LogKind::Error, tf!("無法重新命名巨集：{e}", e = e)),
                        }
                    }
                }
            }
            Then::SaveSchemeAs => {
                if answer == Yes {
                    let result = match scheme::themes_dir() {
                        Some(dir) => self.save_scheme_as(&dir, &typed),
                        None => Err(t("找不到設定資料夾，無法儲存配色").to_owned()),
                    };
                    if let Err(e) = result {
                        self.ask_scheme_name(Some(typed), Some(e));
                    } else {
                        self.apply_schemes(ctx);
                    }
                }
            }
            Then::RenameTerminal => {
                if answer == Yes {
                    if let Some(terminal) = self.doc.terminal.as_mut() {
                        terminal.set_custom_name(&typed);
                    }
                    self.save_session(); // 名字也要記進工作階段
                }
            }
            Then::RenameFile => {
                if answer == Yes {
                    if let Err(e) = self.rename_active_file(&typed) {
                        self.ask_rename_file(Some(typed), Some(e));
                    }
                }
            }
            Then::CloseTab => match answer {
                Yes if self.save() => self.close_active_tab(),
                No => self.close_active_tab(),
                _ => {}
            },
            Then::CloseWindow => {
                match answer {
                    Yes if self.save() => {}
                    No => self.close_skipped.push((self.layout.focus, self.tabs.active())),
                    _ => return, // 取消，或儲存失敗／另存新檔被取消：不關了
                }
                self.next_close_prompt(ctx);
            }
            Then::Reload(enc) => {
                if answer == Yes {
                    self.do_reload(enc);
                }
            }
            Then::DeleteMacro(i) => {
                if answer == Yes {
                    self.do_delete_macro(i);
                }
            }
            // 檔案被別的程式改了，而且自己也有修改：「是」重新載入，其他一律保留自己的
            Then::External => self.handle_external_change(|_| answer == Yes),
        }
    }

    /// 關閉視窗：依序問每個有未儲存修改的分頁；都處理完了才真的關。
    fn next_close_prompt(&mut self, ctx: &egui::Context) {
        let next = self.all_dirty().into_iter().find(|k| !self.close_skipped.contains(k));
        match next {
            Some((pane, i)) => {
                self.focus_pane(pane);
                self.switch_tab(i);
                self.ask_save(Then::CloseWindow, t("關閉"));
            }
            None => {
                self.close_skipped.clear();
                self.allow_close = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }

    /// 作用中的是空白、沒存過、沒改過的文件：開新檔案時直接用掉它，不要多出一個空分頁。
    fn is_pristine(&self) -> bool {
        self.doc.terminal.is_none() && self.doc.path.is_none() && !self.doc.dirty && self.doc.buffer.is_empty()
    }

    /// 分頁換了：搜尋結果屬於上一個分頁，作廢。
    fn tab_changed(&mut self) {
        // 換了分頁（切換、新增、開啟、關閉都算）：鍵盤焦點回到編輯區，可以直接打字
        self.pending_editor_focus = true;
        self.find.invalidate();
        self.find.reveal = false;
        self.save_session();
    }

    // ── 工作階段與外部變更 ──────────────────────────────────

    fn save_session(&self) {
        if self.session_enabled {
            self.session_all().save();
        }
    }

    /// 把上次開著的檔案開回來（游標也放回原位）。
    fn restore_session(&mut self) {
        self.restore_from(&session::Session::load());
    }

    fn restore_from(&mut self, session: &session::Session) {
        // 有存窗格版面：整個版面還原（已經不存在的檔案略過）
        if self.restore_session_layout(session) {
            return;
        }
        // 沒有版面（舊版的工作階段）或只有一個窗格：全部開在目前的窗格
        let focus = session.panes.iter().find(|p| p.id == session.focus).or(session.panes.first());
        let mut active_tab = None;
        for pane in &session.panes {
            for (i, (path, caret)) in pane.files.iter().enumerate() {
                if !path.exists() {
                    continue;
                }
                self.open_path(path);
                let n = self.doc.buffer.len_chars();
                self.editor.sel = editor::model::Selection::collapsed((*caret).min(n));
                self.editor.scroll_to_caret = true;
                if focus.is_some_and(|f| std::ptr::eq(f, pane)) && i == pane.active {
                    active_tab = Some(self.tabs.active());
                }
            }
        }
        if let Some(i) = active_tab {
            self.switch_tab(i);
        }
    }
    /// 作用中的檔案被別的程式改了：沒有未儲存的修改就直接重新載入；有的話問使用者
    /// （`ask` 收到檔名，回傳 `true` 表示要重新載入、捨棄自己的修改）。
    fn handle_external_change(&mut self, ask: impl FnOnce(&str) -> bool) {
        if !self.doc.changed_on_disk() {
            return;
        }
        let name = self.doc.name();
        if self.doc.dirty && !ask(&name) {
            // 使用者要保留自己的版本：記下磁碟版本，不要每秒都再問一次
            self.doc.accept_disk_version();
            self.push(LogKind::Warn, tf!("{name} 已被其他程式修改，目前保留編輯器裡的版本", name = name));
            return;
        }
        let sel = self.editor.sel;
        match self.doc.reload_with(self.doc.format.encoding) {
            Ok(warnings) => {
                self.find.invalidate();
                self.editor.sel = sel;
                self.editor.extra.clear();
                self.editor.clamp(&self.doc.buffer);
                self.push(LogKind::Info, tf!("{name} 已被其他程式修改，已自動重新載入", name = name));
                for w in warnings {
                    self.push(LogKind::Warn, w);
                }
            }
            Err(e) => {
                self.doc.accept_disk_version();
                self.push(LogKind::Error, tf!("無法重新載入 {name}：{e}", name = name, e = e));
            }
        }
    }

    /// 所有窗格裡所有有檔案的分頁，檔案的路徑。
    fn open_paths(&self) -> Vec<PathBuf> {
        let mut paths: Vec<PathBuf> =
            self.tabs.session_entries(Some((&self.doc, &self.editor))).into_iter().map(|(p, ..)| p).collect();
        for set in self.layout.others.values() {
            paths.extend(set.session_entries(None).into_iter().map(|(p, ..)| p));
        }
        paths
    }

    /// 讓監看的資料夾跟著開著的檔案走，並回傳有沒有（穩定下來的）檔案系統事件。
    fn watch_files(&mut self, ctx: &egui::Context) -> bool {
        let paths = self.open_paths();
        let w = self.watcher.get_or_insert_with(|| watcher::FileWatcher::new(ctx.clone()));
        w.sync(watcher::watch_dirs(paths.iter().map(PathBuf::as_path)));
        let changed = w.take_changes();
        if w.pending() {
            ctx.request_repaint_after(Duration::from_millis(250));
        }
        changed
    }

    /// 沒有顯示在畫面上的分頁（其他窗格、同窗格的其他分頁）：檔案被刪了就標記；被改了，沒有未儲存的修改就直接重新載入，
    /// 有的話標記起來，切換到那個分頁時再問（見 [App::check_external_changes]）。
    fn check_background_docs(&mut self) {
        let mut logs: Vec<(LogKind, String)> = Vec::new();
        let tabs = self.tabs.background_mut(true);
        let others = self.layout.others.values_mut().flat_map(|s| s.background_mut(false));
        for tab in tabs.chain(others) {
            watcher::refresh_background_tab(tab, &mut logs);
        }
        for (kind, msg) in logs {
            self.push(kind, msg);
        }
    }

    fn check_external_changes(&mut self) {
        if self.doc.poll_disk() == document::DiskChange::Gone {
            let name = self.doc.name();
            self.push(LogKind::Warn, tf!("{name} 已被刪除或移走，分頁裡的內容仍然保留；存檔會重新建立這個檔案", name = name));
        }
        if self.modal.is_some() {
            return;
        }
        if self.doc.dirty && self.doc.changed_on_disk() {
            let spec = modal::Spec::new(
                t("檔案已被修改"),
                tf!(
                    "「{}」已被其他程式修改，而且你在編輯器裡也有尚未儲存的修改。要重新載入嗎？（會捨棄你的修改）",
                    self.doc.name()
                ),
            )
            .button(t("重新載入"), modal::Answer::Yes)
            .button(t("保留我的版本"), modal::Answer::No);
            self.modal = Some(Pending::new(spec, Then::External));
        } else {
            // 沒有未儲存的修改：直接重新載入，不會詢問
            self.handle_external_change(|_| false);
        }
    }

    fn new_document(&mut self) {
        if self.is_pristine() {
            return;
        }
        self.tabs
            .push(&mut self.doc, &mut self.editor, Document::new());
        self.tab_changed();
    }

    fn switch_tab(&mut self, i: usize) {
        if self.tabs.switch(&mut self.doc, &mut self.editor, i) {
            self.tab_changed();
            self.check_external_changes();
        }
    }

    /// 關閉分頁；有未儲存的修改時先切過去詢問。
    fn close_tab(&mut self, i: usize) {
        if i >= self.tabs.len() {
            return;
        }
        self.switch_tab(i);
        if self.doc.dirty {
            self.ask_save(Then::CloseTab, t("關閉分頁"));
        } else {
            self.close_active_tab();
        }
    }

    fn close_active_tab(&mut self) {
        // 有多個窗格時，窗格裡最後一個分頁關掉就連窗格一起關（不留空白文件）
        if self.tabs.len() == 1 && self.tab_pane_count() > 1 {
            self.remove_focused_pane();
            return;
        }
        let i = self.tabs.active();
        self.tabs.close(&mut self.doc, &mut self.editor, i);
        self.tab_changed();
    }

    fn open(&mut self) {
        if let Some(paths) = rfd::FileDialog::new().set_title(t("開啟檔案")).pick_files() {
            for path in paths {
                self.open_path(&path);
            }
        }
    }

    /// 開啟指定路徑：已經開著就切過去，否則開在新分頁（作用中是空白文件時直接用它）。
    fn open_path(&mut self, path: &std::path::Path) {
        if let Some((pane, i)) = self.find_open_path(path) {
            self.focus_pane(pane);
            self.switch_tab(i);
            self.push(LogKind::Info, tf!("{} 已經開啟，切換到該分頁", self.doc.name()));
            return;
        }
        match Document::open(path) {
            Ok((doc, warnings)) => {
                if self.is_pristine() {
                    self.doc = doc;
                    self.text_replaced();
                } else {
                    self.tabs.push(&mut self.doc, &mut self.editor, doc);
                    self.tab_changed();
                }
                self.push(
                    LogKind::Info,
                    tf!(
                        "已開啟 {}（{}，{}）",
                        self.doc.name(),
                        self.doc.format.encoding_label(),
                        self.doc.format.line_ending.label()
                    ),
                );
                for w in warnings {
                    self.push(LogKind::Warn, w);
                }
                self.save_session();
            }
            Err(e) => self.push(LogKind::Error, tf!("無法開啟 {}：{}", path.display(), e)),
        }
    }
    /// 從檔案總管拖進視窗的檔案：每個都用 [`App::open_path`] 開（已開著的切過去，最後一個成為作用中分頁）。
    /// 拖進來的是資料夾：以它為起點顯示「開啟檔案」交談窗，讓使用者選要開哪些檔案（取消就什麼都不開）。
    /// 同一個路徑只處理一次；有對話框正在等回答時不處理，免得換了分頁。
    fn open_dropped(&mut self, paths: Vec<PathBuf>) {
        if self.modal.is_some() {
            return;
        }
        let mut seen: Vec<PathBuf> = Vec::new();
        for path in paths {
            if seen.contains(&path) {
                continue;
            }
            seen.push(path.clone());
            if path.is_dir() {
                for picked in (self.pick_files_in)(&path) {
                    self.open_path(&picked);
                }
            } else {
                self.open_path(&path);
            }
        }
    }

    /// 回傳是否成功儲存。
    fn save(&mut self) -> bool {
        match self.doc.save() {
            Ok(()) => {
                self.push(LogKind::Info, tf!("已儲存 {}", self.doc.name()));
                true
            }
            Err(SaveDocError::NoPath) => self.save_as(),
            Err(e) => {
                self.push(LogKind::Error, tf!("儲存失敗：{e}", e = e));
                false
            }
        }
    }

    fn save_as(&mut self) -> bool {
        let Some(path) = rfd::FileDialog::new()
            .set_title(t("另存新檔"))
            .set_file_name(self.doc.name())
            .save_file()
        else {
            return false;
        };
        match self.doc.save_as(path) {
            Ok(()) => {
                self.push(LogKind::Info, tf!("已儲存 {}", self.doc.name()));
                true
            }
            Err(e) => {
                self.push(LogKind::Error, tf!("儲存失敗：{e}", e = e));
                false
            }
        }
    }

    fn reload(&mut self, encoding: &'static Encoding) {
        if self.doc.dirty {
            let spec = modal::Spec::new(t("重新載入"), t("重新載入會捨棄尚未儲存的修改，確定嗎？"))
                .button(t("重新載入"), modal::Answer::Yes)
                .button(t("取消"), modal::Answer::Cancel);
            self.modal = Some(Pending::new(spec, Then::Reload(encoding)));
        } else {
            self.do_reload(encoding);
        }
    }

    fn do_reload(&mut self, encoding: &'static Encoding) {
        match self.doc.reload_with(encoding) {
            Ok(warnings) => {
                self.text_replaced();
                self.push(
                    LogKind::Info,
                    tf!("已以 {} 重新載入", self.doc.format.encoding_label()),
                );
                for w in warnings {
                    self.push(LogKind::Warn, w);
                }
            }
            Err(e) => self.push(LogKind::Error, tf!("重新載入失敗：{e}", e = e)),
        }
    }

    // ── 巨集 ────────────────────────────────────────────────

    fn pick_and_run_macro(&mut self) {
        let picked = rfd::FileDialog::new()
            .set_title(t("選擇要執行的 JavaScript 巨集"))
            .add_filter("JavaScript", &["js"])
            .pick_file();
        if let Some(path) = picked {
            self.run_macro_file(path);
        }
    }

    fn run_macro_file(&mut self, path: PathBuf) {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        self.last_macro = Some(path.clone());

        let source = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) => {
                self.push(LogKind::Error, tf!("無法讀取 {name}：{e}", name = name, e = e));
                return;
            }
        };

        self.push(LogKind::Info, tf!("執行 {name}", name = name));
        let path_text = self.doc.path.as_ref().map(|p| p.display().to_string());
        let report =
            macros::run_js_macro_in(&source, &mut self.doc.buffer, &mut self.editor, path_text, true);

        for line in report.output {
            self.push(LogKind::Output, line);
        }
        match report.error {
            None => {
                if self.doc.sync() {
                    self.find.invalidate();
                }
                self.push(LogKind::Info, t("完成"));
            }
            Some(e) => {
                self.push(LogKind::Error, e);
                if report.rolled_back {
                    // 修改已經全部退回，文字與執行前相同，不算有未儲存的修改
                    self.doc.mark_synced();
                    self.push(LogKind::Info, t("巨集失敗，已還原所有修改"));
                }
            }
        }
    }
    // ── 動作分派 ────────────────────────────────────────────

    fn handle(&mut self, action: Action, ctx: &egui::Context) {
        // 作用中的分頁是終端機：這些針對文字文件的動作沒有意義
        if self.doc.terminal.is_some()
            && matches!(
                action,
                Action::Save | Action::SaveAs | Action::Reload(_) | Action::Undo | Action::Redo | Action::ReplaceOne | Action::ReplaceAll
            )
        {
            return;
        }
        match action {
            Action::New => self.new_document(),
            Action::Open => self.open(),
            Action::Save => {
                self.save();
            }
            Action::SaveAs => {
                self.save_as();
            }
            Action::RunMacro => self.pick_and_run_macro(),
            Action::RerunMacro => {
                if let Some(path) = self.last_macro.clone() {
                    self.run_macro_file(path);
                }
            }
            Action::ClearLog => self.log.clear(),
            Action::ToggleOutput => self.toggle_output(),
            Action::Reload(enc) => self.reload(enc),
            // 作用中的分頁是終端機：尋找就是搜尋終端機的畫面
            Action::Find { .. } if self.doc.terminal.is_some() => {
                if let Some(terminal) = self.doc.terminal.as_mut() {
                    terminal.open_search();
                }
            }
            Action::Find { replace } => self.open_find(ctx, replace),
            Action::FindNext => self.find_step(false),
            Action::FindPrev => self.find_step(true),
            Action::FindChanged => self.find_changed(),
            Action::ReplaceOne => self.replace_one(),
            Action::ReplaceAll => self.replace_all(),
            Action::CloseFind => self.close_find(ctx),
            Action::Undo => {
                self.editor.undo(&mut self.doc.buffer);
                self.doc.sync();
                self.find.invalidate();
            }
            Action::Redo => {
                self.editor.redo(&mut self.doc.buffer);
                self.doc.sync();
                self.find.invalidate();
            }
            Action::SetTheme(choice) => {
                self.settings.theme = choice;
                ctx.set_theme(choice);
                self.mark_settings_dirty();
            }
            Action::SetScheme(dark, name) => {
                self.settings.set_scheme_name(dark, &name);
                self.scheme_edit = None; // 換了配色，編輯到一半的就放掉
                self.apply_schemes(ctx);
                self.mark_settings_dirty();
            }
            Action::EditScheme => self.start_scheme_edit(ctx),
            Action::OpenThemesDir => self.open_folder(scheme::themes_dir()),
            Action::SetFontSize(size) => {
                let size = clamp_font_size(size);
                if size != self.settings.font_size {
                    self.settings.font_size = size;
                    apply_font_size(ctx, size);
                    self.mark_settings_dirty();
                }
            }
            Action::ToggleFolderSearch => self.toggle_folder_search(ctx),
            Action::RunFolderSearch => self.folder.start(ctx),
            Action::StopFolderSearch => self.folder.stop(),
            Action::PickSearchFolder => self.pick_search_folder(),
            Action::GotoHit(i) => self.goto_hit(i),
            Action::ToggleMacroMenu => {
                self.macro_open = !self.macro_open;
                if self.macro_open {
                    self.settings_open = false;
                    self.refresh_macros();
                } else {
                    self.capturing_shortcut = None;
                }
            }
            Action::RunMacroEntry(i) => {
                self.macro_open = false;
                self.run_macro_entry(i);
            }
            Action::EditMacro(i) => {
                self.macro_open = false;
                if let Some(path) = self.macros.get(i).map(|m| m.path.clone()) {
                    self.open_path(&path);
                }
            }
            Action::RenameMacro(i) => {
                self.macro_open = false;
                self.ask_rename_macro(i, None, None);
            }
            Action::DeleteMacro(i) => self.delete_macro(i),
            Action::CaptureShortcut(i) => {
                self.capturing_shortcut = if self.capturing_shortcut == Some(i) { None } else { Some(i) };
            }
            Action::ToggleRecord => self.toggle_record(),
            Action::OpenMacroDir => self.open_macro_dir(),
            Action::PaneTab(pane, cmd) => {
                self.focus_pane(pane);
                match cmd {
                    pane_ops::TabCmd::Switch(i) => {
                        self.switch_tab(i);
                        // 點的就是目前的分頁時 switch_tab 什麼也不做，也要把鍵盤焦點還給編輯區
                        self.pending_editor_focus = true;
                    }
                    pane_ops::TabCmd::Close(i) => self.close_tab(i),
                    pane_ops::TabCmd::Rename(i) => self.start_rename_tab(i),
                }
            }
            Action::ClosePane(pane) => self.close_pane(pane),
            Action::TogglePreview => self.toggle_preview(),
            Action::NewTerminal => self.new_terminal(),
            Action::NewTerminalWith(shell) => self.new_terminal_with(Some(shell)),

            Action::ClosePreview(id) => self.close_preview(id),
            Action::OpenPreviewsDir => self.open_folder(preview::previews_dir()),
            Action::OpenLocalFile(path) => {
                if path.is_dir() {
                    self.open_folder(Some(path));
                } else if path.is_file() {
                    self.open_path(&path);
                } else {
                    self.push(LogKind::Warn, tf!("找不到連結指向的檔案：{path}", path = path.display()));
                }
            }
            Action::BeginPreviewDrag(id) => self.begin_preview_drag(id),
            Action::MovePreview(id, target, side) => self.move_preview(id, target, side),
            Action::BeginTabDrag(pane, i) => self.begin_tab_drag(pane, i),
            Action::MoveTab(pane, i, target) => self.move_tab(pane, i, target),
            Action::CloseTab(i) => self.close_tab(if i == usize::MAX { self.tabs.active() } else { i }),
            Action::NextTab => self.switch_tab(self.tabs.neighbor(true)),
            Action::PrevTab => self.switch_tab(self.tabs.neighbor(false)),
            Action::SetView(view) => {
                if view != self.settings.view {
                    self.settings.view = view;
                    self.mark_settings_dirty();
                }
            }
            Action::SetLanguage(choice) => {
                if choice != self.settings.language {
                    self.settings.language = choice;
                    feditor_core::i18n::apply(choice);
                    self.mark_settings_dirty();
                }
            }
            Action::SetWideCols(n) => {
                let n = n.clamp(1, 2);
                if n != self.settings.wide_cols {
                    self.settings.wide_cols = n;
                    self.mark_settings_dirty();
                }
            }
            Action::SetTerminalFontSize(size) => {
                let size = if size.is_finite() { clamp_font_size(size) } else { DEFAULT_TERMINAL_FONT_SIZE };
                if size != self.settings.terminal_font_size {
                    self.settings.terminal_font_size = size;
                    self.mark_settings_dirty();
                }
            }
            Action::SetTerminalMinContrast(ratio) => {
                let ratio = if ratio.is_finite() { ratio.clamp(MIN_TERMINAL_CONTRAST, MAX_TERMINAL_CONTRAST) } else { DEFAULT_TERMINAL_MIN_CONTRAST };
                if ratio != self.settings.terminal_min_contrast {
                    self.settings.terminal_min_contrast = ratio;
                    self.mark_settings_dirty();
                }
            }
            Action::SetTerminalShell(shell) => {
                // 不去掉前後空白：使用者正在輸入框裡打字，空白是下一個詞的開頭（用到的時候才會忽略）
                let shell = shell.replace(['\r', '\n'], " ");
                if shell != self.settings.terminal_shell {
                    self.settings.terminal_shell = shell;
                    self.mark_settings_dirty();
                }
            }
            Action::SetPreviewFontSize(size) => {
                let size = if size.is_finite() { clamp_font_size(size) } else { DEFAULT_PREVIEW_FONT_SIZE };
                if size != self.settings.preview_font_size {
                    self.settings.preview_font_size = size;
                    self.mark_settings_dirty();
                }
            }
            Action::SetFontFamily(family) => {
                if family != self.settings.font_family {
                    self.settings.font_family = family;
                    install_fonts(ctx, self.settings.font_family.as_deref());
                    self.mark_settings_dirty();
                }
            }
        }
    }

    // ── 畫面 ────────────────────────────────────────────────

    fn toolbar(&mut self, ui: &mut egui::Ui) -> Option<Action> {
        let mut action = None;
        ui.horizontal(|ui| {
            tight_buttons(ui);
            // 所有按鈕只顯示圖示，文字說明放在 tooltip
            if icons::button(ui, Icon::New, t("新增空白文件 (Ctrl+N)"), true).clicked() {
                action = Some(Action::New);
            }
            if icons::button(ui, Icon::Open, t("開啟檔案 (Ctrl+O)"), true).clicked() {
                action = Some(Action::Open);
            }
            if icons::button(ui, Icon::Save, t("儲存 (Ctrl+S)"), true).clicked() {
                action = Some(Action::Save);
            }
            if icons::button(ui, Icon::SaveAs, t("另存新檔 (Ctrl+Shift+S)"), true).clicked() {
                action = Some(Action::SaveAs);
            }

            group_separator(ui);

            let find_tip = t("尋找與取代 (Ctrl+F，取代 Ctrl+H)");
            if icons::toggle(ui, Icon::Search, find_tip, self.find.open).clicked() {
                action = Some(if self.find.open {
                    Action::CloseFind
                } else {
                    Action::Find { replace: false }
                });
            }

            let grep_tip = t("在資料夾的所有檔案中尋找 (Ctrl+Shift+F)");
            if icons::toggle(ui, Icon::FindInFiles, grep_tip, self.folder.open).clicked() {
                action = Some(Action::ToggleFolderSearch);
            }

            group_separator(ui);

            // 這是「新增終端機」的動作鈕，不是顯示終端機的開關，所以不會有啟用狀態的樣式；
            // 按下去用設定裡的預設終端機，旁邊的下拉清單可以改選別的 shell 來新增
            if icons::button(ui, Icon::Terminal, t("新增終端機 (Ctrl+Shift+`)；Ctrl+` 在編輯區與各終端機之間切換焦點"), true).clicked() {
                action = Some(Action::NewTerminal);
            }
            if !self.shells.is_empty() {
                // 下拉箭頭的高度和其他工具按鈕一樣（灰底對齊），每一項前面是那個 shell 的圖示
                let choose = icons::dropdown_button(ui, Icon::ChevronDown, t("選擇 shell 來新增終端機"), true);
                let (configured, system) = (self.settings.terminal_shell.clone(), terminal::system_default_shell());
                let ctx = ui.ctx().clone();
                egui::Popup::menu(&choose).show(|ui| {
                    no_hover_outline(ui);
                    flat_list(ui, |ui| {
                        for option in &self.shells {
                            let selected = terminal::is_default_shell(option, &configured, &system); // 目前的預設終端機有選取的底色
                            let texture = self.shell_icons.get(&ctx, option);
                            if shell_icons::shell_row(ui, texture.as_ref(), &option.label, selected).clicked() {
                                action = Some(Action::NewTerminalWith(option.command.clone()));
                                ui.close();
                            }
                        }
                    });
                });
            }

            group_separator(ui);

            let preview_on = self.preview_id().is_some();
            if icons::toggle(ui, Icon::Preview, t("預覽 (Ctrl+Shift+P)"), preview_on).clicked() {
                action = Some(Action::TogglePreview);
            }

            group_separator(ui);

            let run = icons::toggle(ui, Icon::Run, t("巨集：執行、設快捷鍵、編輯"), self.macro_open);
            self.macro_anchor = Some(run.rect);
            if run.clicked() {
                action = Some(Action::ToggleMacroMenu);
            }
            let can_rerun = self.last_macro.is_some();
            if icons::button(ui, Icon::Rerun, t("重新執行上一個巨集 (F5)"), can_rerun).clicked() {
                action = Some(Action::RerunMacro);
            }
            let recording = self.editor.is_recording();
            let (icon, tip) = if recording {
                (Icon::Stop, t("停止錄製，並把錄下的操作存成巨集"))
            } else {
                (Icon::Record, t("錄製巨集：記下之後的按鍵與輸入，可重複播放（滑鼠操作不會錄）"))
            };
            if icons::toggle(ui, icon, tip, recording).clicked() {
                action = Some(Action::ToggleRecord);
            }

            // 最右邊、單獨一組：設定。按下後展開設定面板（配色、字型、字級）。
            // 面板不用 egui 的 Popup：Popup 與下拉選單共用「同一時間只開一個」的狀態，
            // 面板裡一打開字型下拉，整個面板就被關掉了。所以面板用 Area 畫，自己決定何時關閉。
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let more = icons::toggle(
                    ui,
                    Icon::More,
                    t("設定（配色、字型、字級）"),
                    self.settings_open,
                );
                group_separator(ui);
                self.settings_anchor = Some(more.rect);
                if more.clicked() {
                    self.settings_open = !self.settings_open;
                    if self.settings_open {
                        // 使用者可能剛在 themes 資料夾放了新的配色檔
                        self.refresh_schemes();
                        self.refresh_preview_plugins();
                        self.shells = terminal::detect_shells(); // 使用者可能剛裝了新的 shell
                        self.apply_schemes(ui.ctx());
                    }
                }
            });
        });
        action
    }

    // ── 配色 ────────────────────────────────────────────────

    /// 重新讀取 `themes` 資料夾裡的自訂配色（開設定面板時、儲存後都會呼叫）。壞掉的檔案略過並提示。
    fn refresh_schemes(&mut self) {
        self.load_schemes(scheme::themes_dir());
    }

    fn load_schemes(&mut self, dir: Option<PathBuf>) {
        let (custom, messages) = dir.map(|d| scheme::load_custom(&d)).unwrap_or_default();
        self.schemes = scheme::combined(&custom);
        for m in messages {
            self.push(LogKind::Warn, m);
        }
    }

    /// 把目前選的配色套到 egui：淺色、深色各用自己選的；正在編輯的配色蓋過它所屬主題的選擇。
    fn apply_schemes(&self, ctx: &egui::Context) {
        let mut light = scheme::pick(&self.schemes, false, self.settings.scheme_name(false)).clone();
        let mut dark = scheme::pick(&self.schemes, true, self.settings.scheme_name(true)).clone();
        if let Some(edit) = &self.scheme_edit {
            if edit.dark {
                dark = edit.clone();
            } else {
                light = edit.clone();
            }
        }
        theme::apply(ctx, &light, &dark);
    }

    /// 開始編輯目前主題（深色或淺色）正在用的配色。
    fn start_scheme_edit(&mut self, ctx: &egui::Context) {
        let dark = ctx.global_style().visuals.dark_mode;
        let current = scheme::pick(&self.schemes, dark, self.settings.scheme_name(dark)).clone();
        self.scheme_edit = Some(current);
        self.settings_open = false;
    }

    /// 編輯中的配色在清單裡原本的樣子（「還原」用）。
    fn original_scheme(&self, edit: &scheme::Scheme) -> scheme::Scheme {
        scheme::pick(&self.schemes, edit.dark, &edit.name).clone()
    }

    /// 儲存正在編輯的自訂配色（寫回原檔）。
    fn save_scheme_edit(&mut self) {
        let Some(edit) = self.scheme_edit.clone() else { return };
        let Some(dir) = scheme::themes_dir() else {
            self.push(LogKind::Error, t("找不到設定資料夾，無法儲存配色"));
            return;
        };
        match scheme::save_custom(&dir, &edit) {
            Ok(_) => {
                self.refresh_schemes();
                self.push(LogKind::Info, tf!("已儲存配色「{}」", edit.name));
            }
            Err(e) => self.push(LogKind::Error, tf!("無法儲存配色：{e}", e = e)),
        }
    }

    fn ask_scheme_name(&mut self, text: Option<String>, error: Option<String>) {
        let Some(edit) = &self.scheme_edit else { return };
        let text = text.unwrap_or_else(|| tf!("{} 自訂", edit.name));
        let mode = if edit.dark { t("深色") } else { t("淺色") };
        let mut spec = modal::Spec::new(t("另存為自訂配色"), tf!("為這套{mode}配色取個名字：", mode = mode))
            .button(t("儲存"), modal::Answer::Yes)
            .button(t("取消"), modal::Answer::Cancel);
        spec.error = error;
        self.modal = Some(Pending::new(spec, Then::SaveSchemeAs).with_input(modal::Input::new(text)));
    }

    /// 把正在編輯的配色以 `name` 另存成自訂配色檔（存在 `dir`），並設為這個主題使用的配色。
    /// 不能與內建配色同名（那樣會分不清），也不會覆蓋另一個自訂配色。
    fn save_scheme_as(&mut self, dir: &std::path::Path, name: &str) -> Result<(), String> {
        let edit = self.scheme_edit.clone().ok_or(t("沒有正在編輯的配色"))?;
        if name.trim().is_empty() {
            return Err(t("名稱不能是空的").to_owned());
        }
        if scheme::builtins().iter().any(|s| s.dark == edit.dark && s.name == name) {
            return Err(tf!("「{name}」是內建配色的名稱，請換一個", name = name));
        }
        let mut new = edit.clone();
        new.name = name.to_owned();
        new.builtin = false;
        // 與正在編輯的是同一套自訂配色就寫回原檔；否則一律另開新檔，絕不覆蓋別的配色
        let same_as_edit = !edit.builtin && edit.name == name;
        if !same_as_edit {
            new.path = None;
            let taken = self.schemes.iter().any(|s| s.dark == edit.dark && !s.builtin && s.name == name);
            if taken {
                return Err(tf!("已經有名為「{name}」的自訂配色", name = name));
            }
        }
        scheme::save_custom(dir, &new).map_err(|e| tf!("無法儲存配色：{e}", e = e))?;
        self.load_schemes(Some(dir.to_path_buf()));
        self.settings.set_scheme_name(edit.dark, name);
        self.mark_settings_dirty();
        self.scheme_edit = None;
        self.push(LogKind::Info, tf!("已儲存自訂配色「{}」，並設為{}主題的配色", name, if edit.dark { t("深色") } else { t("淺色") }));
        Ok(())
    }

    /// 配色編輯視窗：每種顏色一個色塊，改了立刻預覽。
    fn scheme_editor(&mut self, ctx: &egui::Context) {
        use scheme::{Group, Role};
        let Some(mut edit) = self.scheme_edit.take() else { return };
        let mut open = true;
        let (mut changed, mut save, mut save_as, mut revert) = (false, false, false, false);
        let mode = if edit.dark { t("深色") } else { t("淺色") };
        // 預設開在右側：左邊的編輯區才看得到改色的預覽
        let right = ctx.content_rect().right();
        egui::Window::new(tf!("自訂{}配色：{}", mode, scheme::display_name(&edit.name)))
            .id(egui::Id::unique("scheme_editor"))
            .default_pos(egui::pos2((right - 380.0).max(8.0), 70.0))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                no_hover_outline(ui);
                ui.label(
                    egui::RichText::new(tf!(
                        "這是目前{mode}主題用的配色。改動會立刻預覽，按「儲存」或「另存為」才會留下來。\n要編輯另一種主題的配色，請先在設定面板切換外觀。",
                        mode = mode
                    ))
                    .small()
                    .color(ui.visuals().weak_text_color()),
                );
                ui.add_space(6.0);
                egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                    for group in Group::ALL {
                        ui.add_space(4.0);
                        ui.label(egui::RichText::new(group.label()).strong());
                        egui::Grid::new(("scheme_group", group.label())).num_columns(2).spacing([12.0, 4.0]).show(ui, |ui| {
                            for role in Role::ALL.into_iter().filter(|r| r.group() == group) {
                                ui.label(role.label());
                                let c = edit.get(role);
                                let mut rgb = [c.r(), c.g(), c.b()];
                                if ui.color_edit_button_srgb(&mut rgb).changed() {
                                    edit.set(role, egui::Color32::from_rgb(rgb[0], rgb[1], rgb[2]));
                                    changed = true;
                                }
                                ui.end_row();
                            }
                        });
                    }
                });
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    // 內建配色不能直接覆寫，只能另存；自訂配色可以存回原檔
                    if ui.add_enabled(!edit.builtin, egui::Button::new(t("儲存"))).on_disabled_hover_text(t("內建配色不能覆寫，請用「另存為」")).clicked() {
                        save = true;
                    }
                    if ui.button(t("另存為…")).clicked() {
                        save_as = true;
                    }
                    if ui.button(t("還原")).on_hover_text(t("放棄這次的修改")).clicked() {
                        revert = true;
                    }
                });
            });
        if revert {
            edit = self.original_scheme(&edit);
            changed = true;
        }
        self.scheme_edit = open.then_some(edit);
        if changed || !open {
            self.apply_schemes(ctx); // 視窗關掉：scheme_edit 已清空，會回到清單裡選的配色
        }
        if save {
            self.save_scheme_edit();
        }
        if save_as {
            self.ask_scheme_name(None, None);
        }
    }

    // ── 巨集庫與錄製 ────────────────────────────────────────

    /// 重新掃描巨集資料夾。
    fn refresh_macros(&mut self) {
        self.macros = macro_lib::macros_dir()
            .map(|d| macro_lib::scan(&d))
            .unwrap_or_default();
    }

    fn toggle_record(&mut self) {
        if !self.editor.is_recording() {
            self.editor.start_recording();
            self.push(
                LogKind::Info,
                t("開始錄製巨集：之後的按鍵與輸入都會記下來（滑鼠操作不會）。再按一次錄製鈕結束"),
            );
            return;
        }
        let Some(source) = self.editor.stop_recording() else {
            self.push(LogKind::Info, t("沒有錄到任何操作"));
            return;
        };
        if macro_lib::macros_dir().is_none() {
            // 沒有設定資料夾可存：把原始碼開在新分頁，讓使用者自己決定存哪裡
            let mut doc = Document::new();
            doc.buffer = feditor_core::Buffer::from_text(&source);
            self.tabs.push(&mut self.doc, &mut self.editor, doc);
            self.tab_changed();
            self.push(LogKind::Warn, t("找不到設定資料夾，錄好的巨集開在新分頁，請自行另存"));
            return;
        }
        // 先問名稱再存檔（按「放棄」就不存）
        self.ask_recording_name(source, None, None);
    }

    fn run_macro_entry(&mut self, i: usize) {
        if let Some(path) = self.macros.get(i).map(|m| m.path.clone()) {
            self.run_macro_file(path);
        }
    }

    fn delete_macro(&mut self, i: usize) {
        let Some(entry) = self.macros.get(i) else {
            return;
        };
        let spec = modal::Spec::new(
            t("刪除巨集"),
            tf!("確定要刪除巨集「{}」嗎？檔案會被刪除。", entry.name),
        )
        .button(t("刪除"), modal::Answer::Yes)
        .button(t("取消"), modal::Answer::Cancel);
        self.modal = Some(Pending::new(spec, Then::DeleteMacro(i)));
    }

    fn do_delete_macro(&mut self, i: usize) {
        let Some(entry) = self.macros.get(i).cloned() else {
            return;
        };
        match std::fs::remove_file(&entry.path) {
            Ok(()) => self.push(LogKind::Info, tf!("已刪除巨集 {}", entry.name)),
            Err(e) => self.push(LogKind::Error, tf!("無法刪除 {}：{}", entry.path.display(), e)),
        }
        self.refresh_macros();
    }

    fn open_macro_dir(&mut self) {
        self.open_folder(macro_lib::macros_dir());
    }

    /// 用系統的檔案管理員開啟資料夾（不存在就先建立）。
    fn open_folder(&mut self, dir: Option<PathBuf>) {
        let Some(dir) = dir else {
            self.push(LogKind::Warn, t("找不到設定資料夾"));
            return;
        };
        if let Err(e) = std::fs::create_dir_all(&dir) {
            self.push(LogKind::Error, tf!("無法建立 {}：{}", dir.display(), e));
            return;
        }
        let opener = if cfg!(windows) {
            "explorer"
        } else if cfg!(target_os = "macos") {
            "open"
        } else {
            "xdg-open"
        };
        if let Err(e) = std::process::Command::new(opener).arg(&dir).spawn() {
            self.push(LogKind::Error, tf!("無法開啟 {}：{}", dir.display(), e));
        }
    }

    /// 內建的全域快捷鍵：巨集不能用，不然永遠觸發不到。
    fn builtin_shortcuts() -> [egui::KeyboardShortcut; 16] {
        [
            NEW, OPEN, SAVE, SAVE_AS, FIND, REPLACE, UNDO, REDO, REDO_Y, CLOSE_TAB, RERUN,
            FIND_NEXT, FIND_PREV, NEXT_TAB, PREV_TAB, FIND_IN_FILES,
        ]
    }

    /// 替第 `i` 支巨集設定（或拿掉）快捷鍵：改寫檔案開頭的 `@shortcut`。
    fn set_macro_shortcut(&mut self, i: usize, shortcut: Option<egui::KeyboardShortcut>) {
        let Some(entry) = self.macros.get(i).cloned() else {
            return;
        };
        if let Some(sc) = shortcut {
            let same = |a: &egui::KeyboardShortcut| {
                a.logical_key == sc.logical_key && a.modifiers == sc.modifiers
            };
            if Self::builtin_shortcuts().iter().any(same) {
                self.push(
                    LogKind::Warn,
                    tf!("{} 是內建快捷鍵，請換一個", macro_lib::format_shortcut(&sc)),
                );
                return;
            }
            if let Some(other) = self
                .macros
                .iter()
                .enumerate()
                .find(|(j, m)| *j != i && m.shortcut.as_ref().is_some_and(same))
            {
                self.push(
                    LogKind::Warn,
                    tf!(
                        "{} 已經是巨集「{}」的快捷鍵，請換一個",
                        macro_lib::format_shortcut(&sc),
                        other.1.name
                    ),
                );
                return;
            }
        }
        let text = shortcut.as_ref().map(macro_lib::format_shortcut);
        let result = std::fs::read_to_string(&entry.path).and_then(|src| {
            std::fs::write(&entry.path, macro_lib::set_header_shortcut(&src, text.as_deref()))
        });
        match result {
            Ok(()) => {
                let msg = match &text {
                    Some(t) => tf!("巨集「{}」的快捷鍵設為 {}", entry.name, t),
                    None => tf!("已清除巨集「{}」的快捷鍵", entry.name),
                };
                self.push(LogKind::Info, msg);
            }
            Err(e) => self.push(LogKind::Error, tf!("無法更新 {}：{}", entry.path.display(), e)),
        }
        self.refresh_macros();
    }

    /// 正在等使用者按下新的快捷鍵：吃掉這個畫面的按鍵事件，Esc 取消、Delete 清除。
    fn capture_shortcut(&mut self, ctx: &egui::Context) {
        let Some(i) = self.capturing_shortcut else {
            return;
        };
        let pressed = ctx.input(|inp| {
            inp.events.iter().find_map(|e| match e {
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } if !macro_lib::is_modifier_key(*key) => Some((*key, *modifiers)),
                _ => None,
            })
        });
        let Some((key, mods)) = pressed else {
            return;
        };
        ctx.input_mut(|inp| {
            inp.events
                .retain(|e| !matches!(e, egui::Event::Key { .. } | egui::Event::Text(_)));
        });
        self.capturing_shortcut = None;
        match key {
            egui::Key::Escape => {}
            egui::Key::Delete | egui::Key::Backspace if mods == egui::Modifiers::NONE => {
                self.set_macro_shortcut(i, None);
            }
            _ => {
                let sc = egui::KeyboardShortcut::new(mods, key);
                if macro_lib::parse_shortcut(&macro_lib::format_shortcut(&sc)).is_some() {
                    self.set_macro_shortcut(i, Some(sc));
                } else {
                    self.push(
                        LogKind::Warn,
                        t("快捷鍵要有 Ctrl 或 Alt（功能鍵 F1–F12 除外），單獨的字母鍵是打字用的"),
                    );
                }
            }
        }
    }

    /// 巨集面板：執行、設快捷鍵、編輯、刪除。
    fn macro_panel(&self, ui: &mut egui::Ui) -> Option<Action> {
        let mut action = None;
        ui.set_min_width(380.0);
        no_hover_outline(ui);
        if self.macros.is_empty() {
            ui.label(
                egui::RichText::new(t("還沒有巨集。按工具列的錄製鈕錄一個，或把 .js 檔放進巨集資料夾。"))
                    .color(ui.visuals().weak_text_color()),
            );
        }
        for (i, entry) in self.macros.iter().enumerate() {
            ui.horizontal(|ui| {
                tight_buttons(ui);
                if icons::small_button(ui, Icon::Run, t("執行這支巨集"), true).clicked() {
                    action = Some(Action::RunMacroEntry(i));
                }
                ui.add_space(4.0);
                // 名稱雙按也可以重新命名
                let name = ui
                    .add(egui::Label::new(&entry.name).sense(egui::Sense::click()))
                    .on_hover_text(t("雙按重新命名"));
                if name.double_clicked() {
                    action = Some(Action::RenameMacro(i));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if icons::small_button(ui, Icon::Clear, t("刪除這支巨集"), true).clicked() {
                        action = Some(Action::DeleteMacro(i));
                    }
                    if icons::small_button(ui, Icon::Rename, t("重新命名這支巨集"), true).clicked() {
                        action = Some(Action::RenameMacro(i));
                    }
                    if icons::small_button(ui, Icon::Edit, t("編輯原始碼（開在新分頁）"), true).clicked() {
                        action = Some(Action::EditMacro(i));
                    }
                    let capturing = self.capturing_shortcut == Some(i);
                    let tip = t("設定快捷鍵：點一下再按下組合鍵（Esc 取消、Delete 清除）");
                    if icons::small_toggle(ui, Icon::Keyboard, tip, capturing, true).clicked() {
                        action = Some(Action::CaptureShortcut(i));
                    }
                    let label = if capturing {
                        t("請按下快捷鍵…").to_owned()
                    } else {
                        entry
                            .shortcut
                            .as_ref()
                            .map(macro_lib::format_shortcut)
                            .unwrap_or_default()
                    };
                    ui.label(egui::RichText::new(label).color(ui.visuals().weak_text_color()));
                });
            });
        }
        ui.separator();
        ui.horizontal(|ui| {
            if ui.button(t("從檔案執行…")).clicked() {
                action = Some(Action::RunMacro);
            }
            if ui.button(t("開啟巨集資料夾")).clicked() {
                action = Some(Action::OpenMacroDir);
            }
        });
        action
    }

    /// 在巨集按鈕下方展開巨集面板；點到面板與按鈕以外的地方、或按 Esc 關閉。
    fn show_macro_area(&mut self, ctx: &egui::Context, action: &mut Option<Action>) {
        let Some(anchor) = self.macro_anchor else {
            return;
        };
        let shown = egui::Area::new(egui::Id::unique("macro_area"))
            .order(egui::Order::Foreground)
            .pivot(egui::Align2::LEFT_TOP)
            .fixed_pos(egui::pos2(anchor.left(), anchor.bottom() + 4.0))
            .constrain(true)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style())
                    .show(ui, |ui| self.macro_panel(ui))
                    .inner
            });
        if let Some(a) = shown.inner {
            *action = Some(a);
        }
        let panel_rect = shown.response.rect;
        let (clicked, pointer, escape) = ctx.input(|i| {
            (
                i.pointer.any_click(),
                i.pointer.interact_pos(),
                i.key_pressed(egui::Key::Escape),
            )
        });
        let clicked_outside =
            clicked && pointer.is_some_and(|p| !panel_rect.contains(p) && !anchor.contains(p));
        // 等著按快捷鍵時，Esc 是「取消設定」，不要連面板一起關掉
        if (clicked_outside || escape) && self.capturing_shortcut.is_none() {
            self.macro_open = false;
        }
    }

    // ── 跨檔搜尋 ────────────────────────────────────────────

    fn toggle_folder_search(&mut self, ctx: &egui::Context) {
        if self.folder.open {
            self.folder.stop();
            self.folder.open = false;
            return;
        }
        self.folder.open = true;
        self.folder.focus_query = true;
        // 搜尋文字：選取的單行文字；資料夾：目前檔案所在的資料夾
        if let Some(sel) = self.editor.selected_text(&self.doc.buffer) {
            if !sel.contains('\n') && !sel.is_empty() {
                self.folder.query = sel;
            }
        }
        if self.folder.dir.is_empty() {
            let dir = self
                .doc
                .path
                .as_ref()
                .and_then(|p| p.parent().map(|d| d.to_owned()))
                .or_else(|| std::env::current_dir().ok());
            self.folder.dir = dir.map(|d| d.display().to_string()).unwrap_or_default();
        }
        ctx.request_repaint();
    }

    /// 打開搜尋結果第 `i` 筆：開啟（或切到）那個檔案，並選取符合的文字。
    fn goto_hit(&mut self, i: usize) {
        let Some(hit) = self.folder.hits.get(i).cloned() else {
            return;
        };
        self.open_path(&hit.path);
        // 檔案開失敗（open_path 已經記錄原因）：目前文件不是那個檔案就不要亂選
        let same = self.doc.path.as_deref().is_some_and(|p| {
            p.canonicalize().ok() == hit.path.canonicalize().ok()
        });
        if !same {
            return;
        }
        let buf = &self.doc.buffer;
        let line = hit.line.min(buf.line_count().saturating_sub(1));
        let start = buf.line_start(line);
        let len = buf.line_len_chars(line);
        let (a, b) = (hit.range.start.min(len), hit.range.end.min(len));
        self.select_chars(start + a..start + b);
    }

    fn pick_search_folder(&mut self) {
        let mut dialog = rfd::FileDialog::new().set_title(t("選擇要搜尋的資料夾"));
        let current = std::path::PathBuf::from(self.folder.dir.trim());
        if current.is_dir() {
            dialog = dialog.set_directory(current);
        }
        if let Some(dir) = dialog.pick_folder() {
            self.folder.dir = dir.display().to_string();
        }
    }

    /// 跨檔搜尋面板：搜尋框、資料夾、檔名過濾、結果清單（點一下跳到那一行）。
    fn folder_search_panel(&mut self, ui: &mut egui::Ui) -> Option<Action> {
        let mut action = None;
        let running = self.folder.is_running();
        ui.horizontal(|ui| {
            tight_buttons(ui);
            ui.label(t("在檔案中尋找"));
            let response = ui.add(
                egui::TextEdit::singleline(&mut self.folder.query)
                    .id(folder_query_id())
                    .hint_text(t("搜尋文字"))
                    .desired_width(240.0),
            );
            if std::mem::take(&mut self.folder.focus_query) {
                response.request_focus();
            }
            let enter = response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if response.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                action = Some(Action::ToggleFolderSearch);
            }
            option_toggle(ui, Icon::CaseSensitive, t("區分大小寫"), &mut self.folder.case_sensitive);
            option_toggle(ui, Icon::WholeWord, t("完整單字"), &mut self.folder.whole_word);
            option_toggle(ui, Icon::Regex, t("規則表達式"), &mut self.folder.regex);
            if running {
                if icons::button(ui, Icon::Stop, t("停止搜尋"), true).clicked() {
                    action = Some(Action::StopFolderSearch);
                }
            } else if icons::button(ui, Icon::Search, t("開始搜尋 (Enter)"), true).clicked() || enter {
                action = Some(Action::RunFolderSearch);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if icons::small_button(ui, Icon::Close, t("關閉 (Esc)"), true).clicked() {
                    action = Some(Action::ToggleFolderSearch);
                }
            });
        });
        ui.horizontal(|ui| {
            ui.label(t("資料夾"));
            ui.add(
                egui::TextEdit::singleline(&mut self.folder.dir)
                    .desired_width(ui.available_width() - 330.0),
            );
            if icons::small_button(ui, Icon::Open, t("選擇資料夾…"), true).clicked() {
                action = Some(Action::PickSearchFolder);
            }
            ui.label(t("檔名"));
            ui.add(
                egui::TextEdit::singleline(&mut self.folder.filter)
                    .hint_text("*.rs;*.toml")
                    .desired_width(150.0),
            );
        });
        ui.label(egui::RichText::new(&self.folder.status).color(ui.visuals().weak_text_color()));
        ui.separator();
        let row_h = ui.text_style_height(&egui::TextStyle::Monospace) + ui.spacing().item_spacing.y;
        let total = self.folder.hits.len();
        egui::ScrollArea::vertical()
            .id_salt("folder_hits")
            .auto_shrink([false, false])
            .show_rows(ui, row_h, total, |ui, range| {
                // 一筆結果固定佔一列：太長就截斷，不換行（否則列高不一，捲動會錯位）
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
                for i in range {
                    let hit = &self.folder.hits[i];
                    let text = format!(
                        "{}:{}  {}",
                        self.folder.display_path(hit),
                        hit.line + 1,
                        hit.text.trim_start()
                    );
                    let label = egui::RichText::new(text).monospace();
                    if ui.selectable_label(false, label).clicked() {
                        action = Some(Action::GotoHit(i));
                    }
                }
            });
        action
    }

    /// 在設定按鈕下方展開設定面板，並處理關閉：點到面板與設定按鈕以外的地方、或按 Esc 才關。
    /// 面板裡的下拉選單開著時不關閉（選下拉的項目也是「點在面板外」，不能因此把面板關掉）。
    fn show_settings_area(&mut self, ctx: &egui::Context, action: &mut Option<Action>) {
        let Some(anchor) = self.settings_anchor else {
            return;
        };
        // 在畫面開始時就記下下拉選單是否開著：這個畫面裡點選項會讓它關閉，之後再問就晚了
        let dropdown_open = egui::Popup::is_any_open(ctx);

        let shown = egui::Area::new(egui::Id::unique("settings_area"))
            .order(egui::Order::Foreground)
            .pivot(egui::Align2::RIGHT_TOP)
            .fixed_pos(egui::pos2(anchor.right(), anchor.bottom() + 4.0))
            .constrain(true)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style())
                    .show(ui, |ui| self.settings_panel(ui))
                    .inner
            });
        if let Some(a) = shown.inner {
            *action = Some(a);
        }

        let panel_rect = shown.response.rect;
        let (clicked, pointer, escape) = ctx.input(|i| {
            (
                i.pointer.any_click(),
                i.pointer.interact_pos(),
                i.key_pressed(egui::Key::Escape),
            )
        });
        let clicked_outside = clicked
            && pointer.is_some_and(|p| !panel_rect.contains(p) && !anchor.contains(p));
        if !dropdown_open && (clicked_outside || escape) {
            self.settings_open = false;
        }
    }

    /// 某個主題（深色或淺色）可以選的配色清單。
    fn scheme_combo(&self, ui: &mut egui::Ui, dark: bool) -> Option<Action> {
        let mut action = None;
        let current = scheme::pick(&self.schemes, dark, self.settings.scheme_name(dark)).name.clone();
        egui::ComboBox::from_id_salt(if dark { "scheme_dark" } else { "scheme_light" })
            .icon(icons::combo_chevron)
            .width(190.0)
            .selected_text(scheme::display_name(&current))
            .show_ui(ui, |ui| {
                flat_list(ui, |ui| {
                    for s in scheme::names_for(&self.schemes, dark) {
                        let shown = scheme::display_name(&s.name);
                        let label = if s.builtin { shown } else { tf!("{}（自訂）", shown) };
                        if ui.selectable_label(s.name == current, label).clicked() {
                            action = Some(Action::SetScheme(dark, s.name.clone()));
                        }
                    }
                })
            });
        action
    }

    /// 設定面板：外觀、配色、字型、字級。
    fn settings_panel(&self, ui: &mut egui::Ui) -> Option<Action> {
        let mut action = None;
        ui.set_min_width(340.0);
        no_hover_outline(ui);
        // 掃描結果複製一份出來，避免整個繪製過程一直握著鎖
        let scan = self.font_scan.lock().ok().and_then(|guard| {
            guard
                .as_ref()
                .map(|s| (s.families.clone(), s.default_name.clone()))
        });

        // ── 介面 ──
        group_title(ui, t("介面"));
        settings_grid(ui, "settings_ui", |ui| {
            // 介面語言：三態狀態鈕（跟隨系統＝地球、中文＝「中」、英文＝「En」），旁邊寫出目前的選擇。
            // 選項名稱用各自的語言寫（中文、English），在看不懂的介面裡也找得到
            form_row(ui, t("介面語言"), |ui| {
                let current = self.settings.language;
                let next = current.next();
                let tip = tf!("介面語言：{}（點擊切換為「{}」）", current.label(), next.label());
                if icons::button(ui, language_icon(current), &tip, true).clicked() {
                    action = Some(Action::SetLanguage(next));
                }
                ui.label(current.label());
            });

            // 外觀：三態狀態鈕（從工具列搬進來），旁邊寫出目前的狀態
            form_row(ui, t("外觀"), |ui| {
                let current = self.settings.theme;
                let next = current.next();
                let tip = tf!(
                    "外觀：{}（點擊切換為「{}」）",
                    theme_label(current),
                    theme_label(next)
                );
                if icons::button(ui, theme_icon(current), &tip, true).clicked() {
                    action = Some(Action::SetTheme(next));
                }
                ui.label(theme_label(current));
            });
        });
        group_gap(ui);

        // ── 配色：淺色、深色各選一套，互不影響 ──
        group_title(ui, t("配色"));
        settings_grid(ui, "settings_schemes", |ui| {
            for dark in [false, true] {
                form_row(ui, if dark { t("深色配色") } else { t("淺色配色") }, |ui| {
                    if let Some(a) = self.scheme_combo(ui, dark) {
                        action = Some(a);
                    }
                });
            }
            form_row(ui, "", |ui| {
                ui.spacing_mut().item_spacing.x = BUTTON_GAP;
                if ui
                    .button(t("編輯目前配色…"))
                    .on_hover_text(t("改目前主題（深色或淺色）正在用的配色，可以另存成自訂配色"))
                    .clicked()
                {
                    action = Some(Action::EditScheme);
                }
                if ui
                    .button(t("配色資料夾"))
                    .on_hover_text(t("自訂配色檔（*.conf）放在這裡；開啟設定面板時會重新讀取"))
                    .clicked()
                {
                    action = Some(Action::OpenThemesDir);
                }
            });
        });
        group_gap(ui);

        // ── 編輯區 ──
        group_title(ui, t("編輯區"));
        settings_grid(ui, "settings_editor", |ui| {
            // 字型：只列等寬字型
            form_row(ui, t("字型"), |ui| match &scan {
                None => {
                    ui.label(
                        egui::RichText::new(t("正在找出系統上的等寬字型…"))
                            .color(ui.visuals().weak_text_color()),
                    );
                    ui.ctx().request_repaint_after(Duration::from_millis(150));
                }
                Some((families, default_name)) => {
                    let default_label = match default_name {
                        Some(name) => tf!("系統預設（{name}）", name = name),
                        None => t("系統預設").to_owned(),
                    };
                    // 面板要顯示「實際生效」的字型：設定檔裡的字型若不在清單裡（沒安裝了，
                    // 或是畫不出拉丁字母的老字型），編輯區實際用的是系統預設，這裡也要顯示預設。
                    let current = self
                        .settings
                        .font_family
                        .as_deref()
                        .filter(|name| families.iter().any(|f| f == name));
                    let shown = current.map_or_else(|| default_label.clone(), str::to_owned);
                    egui::ComboBox::from_id_salt("font_family")
                        .icon(icons::combo_chevron)
                        .width(250.0)
                        .height(320.0)
                        .selected_text(shown)
                        .show_ui(ui, |ui| flat_list(ui, |ui| {
                            if ui.selectable_label(current.is_none(), &default_label).clicked() {
                                action = Some(Action::SetFontFamily(None));
                            }
                            for name in families {
                                let selected = current == Some(name.as_str());
                                if ui.selectable_label(selected, name).clicked() {
                                    action = Some(Action::SetFontFamily(Some(name.clone())));
                                }
                            }
                        }));
                }
            });

            // 字級：與快捷鍵共用同一個設定值，所以兩邊永遠同步
            form_row(ui, t("字級"), |ui| {
                let mut size = self.settings.font_size;
                let response = ui.add(
                    egui::DragValue::new(&mut size)
                        .range(MIN_FONT_SIZE..=MAX_FONT_SIZE)
                        .speed(0.1)
                        .max_decimals(1)
                        .suffix(" pt"),
                );
                if response.changed() {
                    action = Some(Action::SetFontSize(size));
                }
            });

            // 安全界線：在這一欄的右緣畫一條淺淺的垂直線；0 是不畫
            form_row(ui, t("安全界線"), |ui| {
                let v = self.settings.view;
                let mut column = v.ruler_column;
                let response = ui
                    .add(
                        egui::DragValue::new(&mut column)
                            .range(0..=settings::MAX_RULER_COLUMN)
                            .speed(0.5)
                            .custom_formatter(|n, _| if n < 0.5 { t("不畫界線").to_owned() } else { tf!("{n} 欄", n = n as u32) }),
                    )
                    .on_hover_text(t("在編輯區的這一欄畫一條淺淺的垂直線，提醒一行不要寫得比它寬（全形字算兩欄）；拉到 0 就不畫"));
                if response.changed() {
                    action = Some(Action::SetView(settings::ViewOptions { ruler_column: column, ..v }));
                }
            });

            // 顯示：開關按鈕，圖示加 tooltip
            form_row(ui, t("顯示"), |ui| {
                tight_buttons(ui);
                let v = self.settings.view;
                let mut next = v;
                if icons::toggle(ui, Icon::LineNumbers, t("顯示行號"), v.show_line_numbers).clicked() {
                    next.show_line_numbers = !v.show_line_numbers;
                }
                if icons::toggle(ui, Icon::CurrentLine, t("標示目前這一行"), v.highlight_line).clicked() {
                    next.highlight_line = !v.highlight_line;
                }
                if icons::toggle(ui, Icon::Whitespace, t("顯示空白、Tab 與換行符號"), v.show_whitespace).clicked() {
                    next.show_whitespace = !v.show_whitespace;
                }
                if icons::toggle(ui, Icon::Syntax, t("語法上色（依副檔名）"), v.syntax).clicked() {
                    next.syntax = !v.syntax;
                }
                let too_big = self.doc.buffer.line_count() > editor::wrap::MAX_LINES;
                let tip = if too_big {
                    tf!("自動換行（超過 {} 行的檔案不支援）", editor::wrap::MAX_LINES)
                } else {
                    t("自動換行").to_owned()
                };
                let on = v.word_wrap && !too_big;
                if icons::toggle(ui, Icon::WordWrap, &tip, on).clicked() && !too_big {
                    next.word_wrap = !v.word_wrap;
                }
                if next != v {
                    action = Some(Action::SetView(next));
                }
            });
        });
        ui.add_space(2.0);
        ui.label(
            egui::RichText::new(t("調整字級：Ctrl ＋／－ 或 Ctrl＋滾輪；Ctrl＋0 還原預設（點在預覽窗格或終端機上時，調的是它們各自的字級）"))
                .small()
                .color(ui.visuals().weak_text_color()),
        );
        group_gap(ui);

        // ── 預覽 ──
        group_title(ui, t("預覽"));
        settings_grid(ui, "settings_preview", |ui| {
            form_row(ui, t("預覽外掛"), |ui| {
                if ui
                    .button(t("預覽資料夾"))
                    .on_hover_text(t("預覽外掛（*.js）放在這裡；開啟設定面板時會重新讀取"))
                    .clicked()
                {
                    action = Some(Action::OpenPreviewsDir);
                }
                ui.label(egui::RichText::new(tf!("已載入 {} 個外掛", self.preview_plugins.len())).weak());
            });
            // 預覽字級：點在預覽窗格上時，縮放快捷鍵調的也是它，所以兩邊永遠同步
            form_row(ui, t("預覽字級"), |ui| {
                let mut size = self.settings.preview_font_size;
                let response = ui.add(
                    egui::DragValue::new(&mut size)
                        .range(MIN_FONT_SIZE..=MAX_FONT_SIZE)
                        .speed(0.1)
                        .max_decimals(1)
                        .suffix(" pt"),
                );
                if response.changed() {
                    action = Some(Action::SetPreviewFontSize(size));
                }
            });
        });
        group_gap(ui);

        // ── 終端機 ──
        group_title(ui, t("終端機"));
        settings_grid(ui, "settings_terminal", |ui| {
            // 終端機字級：點在終端機上時，縮放快捷鍵調的也是它
            form_row(ui, t("終端機字級"), |ui| {
                let mut size = self.settings.terminal_font_size;
                let response = ui.add(
                    egui::DragValue::new(&mut size).range(MIN_FONT_SIZE..=MAX_FONT_SIZE).speed(0.1).max_decimals(1).suffix(" pt"),
                );
                if response.changed() {
                    action = Some(Action::SetTerminalFontSize(size));
                }
            });
            form_row(ui, t("最低對比度"), |ui| {
                let mut ratio = self.settings.terminal_min_contrast;
                let response = ui
                    .add(
                        egui::DragValue::new(&mut ratio)
                            .range(MIN_TERMINAL_CONTRAST..=MAX_TERMINAL_CONTRAST)
                            .speed(0.05)
                            .max_decimals(1)
                            .custom_formatter(|v, _| if v <= 1.0 { t("不調整").to_owned() } else { format!("{v:.1} : 1") }),
                    )
                    .on_hover_text(t("終端機文字和底色的對比低於這個值時，自動把文字調整到看得清楚；拉到最小就是不調整"));
                if response.changed() {
                    action = Some(Action::SetTerminalMinContrast(ratio));
                }
            });
            // 預設終端機：按工具列的終端機鈕時執行的 shell，從偵測到的清單選，也可以在下面自己寫指令
            form_row(ui, t("預設終端機"), |ui| {
                let configured = self.settings.terminal_shell.clone();
                let system = terminal::system_default_shell();
                let in_use = !configured.trim().is_empty();
                let selected = if !in_use {
                    t("系統預設").to_owned()
                } else {
                    self.shells
                        .iter()
                        .find(|o| terminal::is_default_shell(o, &configured, &system))
                        .map(|o| o.label.clone())
                        .unwrap_or_else(|| tf!("自訂：{cmd}", cmd = configured.trim()))
                };
                egui::ComboBox::from_id_salt("terminal_shell")
                    .icon(icons::combo_chevron)
                    .width(250.0)
                    .selected_text(selected)
                    .show_ui(ui, |ui| {
                        flat_list(ui, |ui| {
                            if shell_icons::shell_row(ui, None, t("系統預設"), !in_use).clicked() {
                                action = Some(Action::SetTerminalShell(String::new()));
                            }
                            let ctx = ui.ctx().clone();
                            for option in &self.shells {
                                let selected = in_use && terminal::is_default_shell(option, &configured, &system);
                                let texture = self.shell_icons.get(&ctx, option);
                                if shell_icons::shell_row(ui, texture.as_ref(), &option.label, selected).clicked() {
                                    action = Some(Action::SetTerminalShell(option.command.clone()));
                                }
                            }
                        })
                    })
                    .response
                    .on_hover_text(t("按工具列的終端機鈕時執行的 shell；工具列旁的下拉清單可以臨時改選別的"));
            });
            form_row(ui, t("自訂指令"), |ui| {
                let mut shell = self.settings.terminal_shell.clone();
                let response = ui
                    .add(egui::TextEdit::singleline(&mut shell).hint_text(t("留空使用系統預設")).desired_width(250.0))
                    .on_hover_text(t("新開的終端機要執行的程式，可以加參數（例如 pwsh -NoLogo）；路徑有空白要用雙引號包起來。已經開著的終端機不受影響"));
                if response.changed() {
                    action = Some(Action::SetTerminalShell(shell));
                }
            });
        });
        group_gap(ui);

        // ── 狀態列 ──
        group_title(ui, t("狀態列"));
        settings_grid(ui, "settings_status", |ui| {
            // 欄號：狀態列的欄位置，全形字算 1 欄還是 2 欄
            form_row(ui, t("欄號"), |ui| {
                let wide = self.settings.wide_cols;
                let wide_label = |n: u8| if n == 2 { t("全形字算 2 欄") } else { t("全形字算 1 欄") };
                egui::ComboBox::from_id_salt("wide_cols")
                    .icon(icons::combo_chevron)
                    .width(250.0)
                    .selected_text(wide_label(wide))
                    .show_ui(ui, |ui| flat_list(ui, |ui| {
                        for n in [1u8, 2] {
                            if ui.selectable_label(wide == n, wide_label(n)).clicked() {
                                action = Some(Action::SetWideCols(n));
                            }
                        }
                    }))
                    .response
                    .on_hover_text(t("狀態列「行、欄」的欄號算法：1 是一個字一欄（多數編輯器），2 是和畫面寬度一致"));
            });
        });
        action
    }

    /// 尋找列：搜尋框、三個選項（區分大小寫、完整單字、規則表達式）、上一筆／下一筆，
    /// 以及可展開的取代欄。
    ///
    /// 鍵盤只管兩個輸入框：Tab 只在搜尋框與取代框之間切換（見 [`App::tab_between_find_and_replace`]），
    /// 其他按鈕（選項、上一筆／下一筆、取代、關閉…）一律用滑鼠點，不會被 Tab 聚焦。
    fn find_bar(&mut self, ui: &mut egui::Ui) -> Option<Action> {
        self.tab_between_find_and_replace(ui);
        let action = icons::mouse_only(|| self.find_bar_contents(ui));
        // 要在兩個輸入框畫完之後才設：TextEdit 每個畫面畫完自己都會重設鍵盤過濾條件，先設會被蓋掉
        keep_tab_in_find_inputs(ui);
        action
    }

    /// 搜尋框或取代框有鍵盤焦點時，Tab（或 Shift+Tab）在兩者之間切換：
    /// 搜尋框 → 取代框（取代列收著就先展開）、取代框 → 搜尋框。Tab 不會跳到按鈕或別的元件。
    fn tab_between_find_and_replace(&mut self, ui: &egui::Ui) {
        let find_focused = ui.memory(|m| m.has_focus(find_input_id()));
        let replace_focused = ui.memory(|m| m.has_focus(replace_input_id()));
        if !(find_focused || replace_focused) {
            return;
        }
        let pressed = ui.input_mut(|i| {
            let tab = i.consume_key(egui::Modifiers::NONE, egui::Key::Tab);
            let back = i.consume_key(egui::Modifiers::SHIFT, egui::Key::Tab);
            tab || back
        });
        if pressed {
            if find_focused {
                self.find.show_replace = true;
                ui.memory_mut(|m| m.request_focus(replace_input_id()));
            } else {
                ui.memory_mut(|m| m.request_focus(find_input_id()));
            }
        }
    }

    fn find_bar_contents(&mut self, ui: &mut egui::Ui) -> Option<Action> {
        let mut action = None;
        if self.find.stats_pending() {
            let App { doc, find, .. } = self;
            find.refresh_stats(doc.text());
        }
        let query_empty = self.find.query.is_empty();

        ui.horizontal(|ui| {
            tight_buttons(ui);
            let chevron = if self.find.show_replace {
                Icon::ChevronDown
            } else {
                Icon::ChevronRight
            };
            if icons::small_button(ui, chevron, t("顯示或隱藏取代 (Ctrl+H)"), true).clicked() {
                self.find.show_replace = !self.find.show_replace;
            }

            if self.focus_find {
                // 把搜尋框內的文字全選，直接輸入就能取代舊的搜尋字串
                let len = self.find.query.chars().count();
                let mut state =
                    egui::TextEdit::load_state(ui.ctx(), find_input_id()).unwrap_or_default();
                state.cursor.set_char_range(Some(CCursorRange::two(
                    CCursor::new(0),
                    CCursor::new(len),
                )));
                state.store(ui.ctx(), find_input_id());
                ui.memory_mut(|m| m.request_focus(find_input_id()));
                self.focus_find = false;
            }

            let input = ui.add(
                egui::TextEdit::singleline(&mut self.find.query)
                    .id(find_input_id())
                    .hint_text(t("尋找"))
                    .desired_width(220.0),
            );
            if input.changed() {
                action = Some(Action::FindChanged);
            }
            if input.lost_focus() {
                let (enter, shift, escape) = ui.input(|i| {
                    (
                        i.key_pressed(egui::Key::Enter),
                        i.modifiers.shift,
                        i.key_pressed(egui::Key::Escape),
                    )
                });
                if enter {
                    action = Some(if shift {
                        Action::FindPrev
                    } else {
                        Action::FindNext
                    });
                    input.request_focus(); // 連按 Enter 可以一路找下去
                } else if escape {
                    action = Some(Action::CloseFind);
                }
            }

            let mut changed = false;
            changed |= option_toggle(
                ui,
                Icon::CaseSensitive,
                t("區分大小寫"),
                &mut self.find.opts.case_sensitive,
            );
            changed |= option_toggle(
                ui,
                Icon::WholeWord,
                t("完整單字（前後不可緊鄰字母、數字或底線）"),
                &mut self.find.opts.whole_word,
            );
            // 規則表達式：tooltip 是基本語法速查加完整說明的連結（滑鼠移到 tooltip 上也不會消失）
            let regex_on = self.find.opts.regex;
            if icons::small_toggle_rich(ui, Icon::Regex, t("規則表達式"), regex_on, regex_help)
                .clicked()
            {
                self.find.opts.regex = !regex_on;
                changed = true;
            }
            if changed {
                action = Some(Action::FindChanged);
            }

            if icons::small_button(ui, Icon::ChevronUp, t("上一筆 (Shift+Enter 或 Shift+F3)"), !query_empty)
                .clicked()
            {
                action = Some(Action::FindPrev);
            }
            if icons::small_button(ui, Icon::ChevronDown, t("下一筆 (Enter 或 F3)"), !query_empty)
                .clicked()
            {
                action = Some(Action::FindNext);
            }

            // 結果摘要
            match self.find.summary() {
                Summary::Idle => {}
                Summary::Error(msg) => {
                    ui.label(egui::RichText::new(t("規則有誤")).color(ui.visuals().error_fg_color))
                        .on_hover_text(msg);
                }
                Summary::NoMatch => {
                    ui.label(egui::RichText::new(t("找不到")).color(ui.visuals().error_fg_color));
                }
                Summary::Count {
                    index,
                    total,
                    capped,
                } => {
                    let plus = if capped { "+" } else { "" };
                    let text = match index {
                        Some(i) => format!("{}/{total}{plus}", i + 1),
                        None => tf!("{total}{plus} 筆", total = total, plus = plus),
                    };
                    ui.label(egui::RichText::new(text).color(ui.visuals().weak_text_color()));
                }
                Summary::TooLarge => {
                    ui.label(egui::RichText::new("—").color(ui.visuals().weak_text_color()))
                        .on_hover_text(t("檔案很大，不統計筆數"));
                }
            }
            if self.find.wrapped {
                ui.label(egui::RichText::new(t("已繞回")).color(ui.visuals().weak_text_color()));
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if icons::small_button(ui, Icon::Close, t("關閉 (Esc)"), true).clicked() {
                    action = Some(Action::CloseFind);
                }
            });
        });

        if self.find.show_replace {
            ui.horizontal(|ui| {
                tight_buttons(ui); // 要先縮間距，下面的對齊寬度才會跟上一列一致
                // 與上一列的展開鈕對齊
                ui.add_space(22.0 + ui.spacing().item_spacing.x);
                let input = ui.add(
                    egui::TextEdit::singleline(&mut self.find.replacement)
                        .id(replace_input_id())
                        .hint_text(t("取代為"))
                        .desired_width(220.0),
                );
                let input = if self.find.opts.regex {
                    input.on_hover_text(t(
                        "規則表達式模式：可用 $1、${名稱} 代入群組，\\n、\\t 代表換行與跳位，字面的 $ 請寫成 $$",
                    ))
                } else {
                    input
                };
                if input.lost_focus() {
                    let (enter, escape) = ui.input(|i| {
                        (
                            i.key_pressed(egui::Key::Enter),
                            i.key_pressed(egui::Key::Escape),
                        )
                    });
                    if enter {
                        action = Some(Action::ReplaceOne);
                        input.request_focus();
                    } else if escape {
                        action = Some(Action::CloseFind);
                    }
                }
                if icons::small_button(ui, Icon::ReplaceOne, t("取代這一筆 (Enter)"), !query_empty)
                    .clicked()
                {
                    action = Some(Action::ReplaceOne);
                }
                if icons::small_button(ui, Icon::ReplaceAll, t("全部取代"), !query_empty).clicked() {
                    action = Some(Action::ReplaceAll);
                }
            });
        }
        action
    }

    /// 狀態列的游標位置文字，例如「行 12，欄 5」；欄的算法依設定的 `wide_cols`。
    fn caret_position_text(&self) -> String {
        let buffer = &self.doc.buffer;
        let caret = self.editor.sel.head.min(buffer.len_chars());
        let line = buffer.line_of_char(caret);
        let col = feditor_core::motion::caret_column(
            buffer.rope(),
            buffer.line_start(line),
            caret,
            usize::from(self.settings.wide_cols),
        );
        tf!("行 {}，欄 {}", line + 1, col)
    }

    /// 狀態列：左邊是檔案狀態，右邊是這個檔案的格式設定（語言、編碼與 BOM、換行、重新載入）。
    /// 這些控制項不放標題，說明都在 tooltip。
    fn status_bar(&mut self, ui: &mut egui::Ui) -> Option<Action> {
        if self.doc.terminal.is_some() {
            // 終端機分頁沒有編碼、換行這些文字文件的設定，只顯示名稱
            ui.horizontal(|ui| ui.label(self.doc.name()));
            return None;
        }
        let mut action = None;
        ui.horizontal(|ui| {
            tight_buttons(ui);
            let path = self
                .doc
                .path
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| t("未命名").to_owned());
            // 整列由右往左排：先放右側的格式控制項，最後讓路徑吃掉剩下的空間，放不下時截斷。
            // 這樣路徑很長時，也不會和右邊的控制項重疊。（未儲存的標示在標題列，這裡不重複。）
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // 由右往左加入，所以畫面上由左到右的順序是：語言、編碼（含 BOM）、換行、重新載入

                // 重新載入：點擊後彈出編碼清單
                let can_reload = self.doc.path.is_some();
                let reload = icons::small_button(
                    ui,
                    Icon::Reload,
                    t("以指定編碼重新讀取目前的檔案（自動偵測錯誤時使用）"),
                    can_reload,
                );
                egui::Popup::menu(&reload).show(|ui| {
                    no_hover_outline(ui);
                    ui.label(t("重新載入為："));
                    for enc in common_encodings() {
                        if ui.button(enc.name()).clicked() {
                            action = Some(Action::Reload(enc));
                            ui.close();
                        }
                    }
                });

                no_hover_outline(ui);
                let le_width = combo_width(ui, LineEnding::ALL.iter().map(|le| le.label()));
                egui::ComboBox::from_id_salt("line_ending")
                    .icon(icons::combo_chevron)
                    .width(le_width)
                    .selected_text(self.doc.format.line_ending.label())
                    .show_ui(ui, |ui| flat_list(ui, |ui| {
                        for le in LineEnding::ALL {
                            let selected = self.doc.format.line_ending == le;
                            if ui.selectable_label(selected, le.label()).clicked() {
                                self.doc.set_line_ending(le);
                            }
                        }
                    }))
                    .response
                    .on_hover_text(t("換行格式（存檔時使用）"));

                // 編碼與 BOM 合在同一個清單：UTF-8、UTF-16 的「帶 BOM」各自是獨立的一項
                let mut chosen = None;
                let labels: Vec<String> = format_choices()
                    .iter()
                    .map(|(e, b)| format_label(e, *b))
                    .collect();
                let enc_width = combo_width(ui, labels.iter().map(String::as_str));
                let (cur_enc, cur_bom) = (self.doc.format.encoding, self.doc.format.bom);
                let cur_label = format_label(cur_enc, cur_bom);
                egui::ComboBox::from_id_salt("encoding")
                    .icon(icons::combo_chevron)
                    .width(enc_width)
                    .selected_text(cur_label.as_str())
                    .show_ui(ui, |ui| flat_list(ui, |ui| {
                        for ((enc, bom), label) in format_choices().into_iter().zip(&labels) {
                            let selected = *label == cur_label;
                            if ui.selectable_label(selected, label).clicked() {
                                chosen = Some((enc, bom));
                            }
                        }
                    }))
                    .response
                    .on_hover_text(t("編碼與 BOM（存檔時使用）。無法表示的字元會讓存檔失敗，不會被替換"));
                if let Some((enc, bom)) = chosen {
                    self.doc.set_encoding(enc);
                    self.doc.set_bom(bom);
                }

                // 語言（語法上色）：「自動」依副檔名，沒有檔名時看內容猜
                let current = self.doc.lang;
                let resolved = syntax::resolve(current, self.doc.path.as_deref(), &self.doc.buffer);
                let shown = resolved.map_or(t("純文字"), |l| l.name);
                let langs = syntax::all_languages();
                let lang_width = combo_width(ui, langs.iter().map(|l| l.name).chain([t("純文字"), t("自動")]));
                let mut picked = None;
                egui::ComboBox::from_id_salt("language")
                    .icon(icons::combo_chevron)
                    .width(lang_width)
                    .selected_text(shown)
                    .show_ui(ui, |ui| flat_list(ui, |ui| {
                        if ui.selectable_label(current == syntax::LangChoice::Auto, t("自動")).clicked() {
                            picked = Some(syntax::LangChoice::Auto);
                        }
                        if ui.selectable_label(current == syntax::LangChoice::Plain, t("純文字")).clicked() {
                            picked = Some(syntax::LangChoice::Plain);
                        }
                        for l in &langs {
                            let choice = syntax::LangChoice::Named(l.name);
                            if ui.selectable_label(current == choice, l.name).clicked() {
                                picked = Some(choice);
                            }
                        }
                    }))
                    .response
                    .on_hover_text(t("語言（語法上色）。自動：依副檔名，沒有檔名時看內容判斷；也可以手動指定"));
                if let Some(choice) = picked {
                    self.doc.lang = choice;
                }

                // 游標位置（畫面上在語言的左邊）：行、欄都從 1 起算
                ui.label(self.caret_position_text())
                    .on_hover_text(t("游標位置（行、欄）。全形字（中日韓文字等）算 1 欄或 2 欄，可在設定面板的「欄號」調整"));

                // 左側：路徑（剩下的空間全給它，放不下就截斷，完整路徑看 tooltip）
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.add(egui::Label::new(&path).truncate())
                        .on_hover_text(&path);
                });
            });
        });
        action
    }

    /// 輸出面板的標題列：展開／收合鈕、標題、（收合時）最後一則訊息，右邊是清除鈕。
    fn output_header(&self, ui: &mut egui::Ui, expanded: bool) -> Option<Action> {
        let mut action = None;
        ui.horizontal(|ui| {
            tight_buttons(ui);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let has_log = !self.log.is_empty();
                if icons::small_button(ui, Icon::Clear, t("清除輸出"), has_log).clicked() {
                    action = Some(Action::ClearLog);
                }
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    let (icon, tip) = if expanded {
                        (Icon::ChevronDown, t("收合輸出"))
                    } else {
                        (Icon::ChevronUp, t("展開輸出"))
                    };
                    if icons::small_button(ui, icon, tip, true).clicked() {
                        action = Some(Action::ToggleOutput);
                    }
                    ui.label(t("輸出"));
                    // 收合時還是看得到最後一則訊息（例如「已儲存」），一行放不下就截斷
                    if !expanded {
                        if let Some((kind, text)) = self.log.last() {
                            let color = match kind {
                                LogKind::Error => ui.visuals().error_fg_color,
                                LogKind::Warn => ui.visuals().warn_fg_color,
                                _ => ui.visuals().weak_text_color(),
                            };
                            ui.add(egui::Label::new(egui::RichText::new(text).color(color)).truncate())
                                .on_hover_text(text);
                        }
                    }
                });
            });
        });
        action
    }

    fn output_panel(&self, ui: &mut egui::Ui) -> Option<Action> {
        let action = self.output_header(ui, true);
        egui::ScrollArea::vertical()
            .stick_to_bottom(true)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for (kind, text) in &self.log {
                    let rich = egui::RichText::new(text).monospace();
                    let rich = match kind {
                        LogKind::Error => rich.color(ui.visuals().error_fg_color),
                        LogKind::Warn => rich.color(ui.visuals().warn_fg_color),
                        LogKind::Info => rich.color(ui.visuals().weak_text_color()),
                        LogKind::Output => rich,
                    };
                    ui.label(rich);
                }
            });
        action
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        // 組字內容已經被直接寫進編輯區：輸入法稍後送來的同內容 Commit 要丟掉（見 swallow_ime）
        if let Some((text, left)) = self.editor.swallow_ime.take() {
            let mut seen = false;
            ctx.input_mut(|i| {
                i.events.retain(|e| match e {
                    egui::Event::Ime(egui::ImeEvent::Commit(t)) if *t == text => {
                        seen = true;
                        false
                    }
                    egui::Event::Ime(egui::ImeEvent::Preedit { text: t, .. }) => !t.is_empty(),
                    _ => true,
                });
            });
            if !seen && left > 1 {
                self.editor.swallow_ime = Some((text, left - 1));
            }
        }

        // 從檔案總管拖進來的檔案：開起來；拖曳途中在視窗上蓋一層提示
        let dropped: Vec<PathBuf> =
            ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).collect());
        if !dropped.is_empty() {
            self.open_dropped(dropped);
        }
        let hovering = ctx.input(|i| i.raw.hovered_files.len());
        if hovering > 0 {
            drop_overlay(&ctx, hovering);
        }

        // 要關閉視窗了：先把還沒寫入的設定存起來，並記住開著的分頁
        if ctx.input(|i| i.viewport().close_requested()) {
            self.flush_settings();
            self.save_session();
        }

        // 檔案被別的程式改了、刪了？資料夾監看一有事件、視窗拿回焦點時就檢查；
        // 另外每 5 秒檢查一次當後備（網路磁碟、監看失效的情況）
        let focused = ctx.input(|i| i.focused);
        let changed = self.watch_files(&ctx);
        if changed || (focused && !self.was_focused) || self.last_disk_check.elapsed() >= Duration::from_secs(5) {
            self.last_disk_check = Instant::now();
            self.check_external_changes();
            self.check_background_docs();
        }
        self.was_focused = focused;
        ctx.request_repaint_after(Duration::from_secs(1));

        // 關閉視窗前，若有未儲存的修改先詢問
        if ctx.input(|i| i.viewport().close_requested())
            && !self.all_dirty().is_empty()
            && !self.allow_close
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            if self.modal.is_none() {
                self.close_skipped.clear();
                self.next_close_prompt(&ctx);
            }
        }

        // 終端機有鍵盤焦點時，按鍵先給它（要在其他快捷鍵之前）
        self.terminal_input(&ctx);
        self.reap_terminals(&ctx);
        if ctx.input_mut(|i| i.consume_shortcut(&terminal_pane::NEW_TERMINAL)) {
            self.new_terminal();
        } else if ctx.input_mut(|i| i.consume_shortcut(&terminal_pane::TOGGLE_TERMINAL)) {
            self.focus_terminal_shortcut(&ctx);
        }
        self.capture_shortcut(&ctx);
        if self.folder.poll() {
            ctx.request_repaint();
        }

        // 快捷鍵（Ctrl+Shift+S 要先於 Ctrl+S 檢查，因為 egui 比對修飾鍵時 Ctrl+S 也會吃到它）
        let font_size = self.settings.font_size;
        // 點了預覽窗格之後，縮放調的是預覽的字級；否則調編輯區的
        let preview_zoom = self.preview_focused && self.preview_id().is_some();
        // 點了終端機之後（它有鍵盤焦點），縮放調的是終端機的字級
        let terminal_zoom = self.doc.terminal.as_ref().is_some_and(|t| t.has_focus(&ctx));
        let (zoom_base, zoom_default) = if terminal_zoom {
            (self.settings.terminal_font_size, DEFAULT_TERMINAL_FONT_SIZE)
        } else if preview_zoom {
            (self.settings.preview_font_size, DEFAULT_PREVIEW_FONT_SIZE)
        } else {
            (font_size, DEFAULT_FONT_SIZE)
        };
        let zoom_action = |size: f32| {
            if terminal_zoom {
                Action::SetTerminalFontSize(size)
            } else if preview_zoom {
                Action::SetPreviewFontSize(size)
            } else {
                Action::SetFontSize(size)
            }
        };
        let zoom_delta = ctx.input(|i| i.zoom_delta());
        // 沒有任何輸入框有焦點時，Ctrl+Z／Y 由我們負責（有焦點時由該輸入框自己處理）
        let nothing_focused = ctx.memory(|m| m.focused().is_none());
        let mut action = ctx.input_mut(|i| {
            if nothing_focused && i.consume_shortcut(&REDO) {
                Some(Action::Redo)
            } else if nothing_focused && i.consume_shortcut(&REDO_Y) {
                Some(Action::Redo)
            } else if nothing_focused && i.consume_shortcut(&UNDO) {
                Some(Action::Undo)
            } else if i.consume_shortcut(&FIND_IN_FILES) {
                Some(Action::ToggleFolderSearch)
            } else if i.consume_shortcut(&TOGGLE_PREVIEW) {
                Some(Action::TogglePreview)
            } else if i.consume_shortcut(&PREV_TAB) {
                Some(Action::PrevTab)
            } else if i.consume_shortcut(&NEXT_TAB) {
                Some(Action::NextTab)
            } else if i.consume_shortcut(&CLOSE_TAB) {
                Some(Action::CloseTab(usize::MAX))
            } else if i.consume_shortcut(&SAVE_AS) {
                Some(Action::SaveAs)
            } else if i.consume_shortcut(&SAVE) {
                Some(Action::Save)
            } else if i.consume_shortcut(&OPEN) {
                Some(Action::Open)
            } else if i.consume_shortcut(&NEW) {
                Some(Action::New)
            } else if i.consume_shortcut(&RERUN) {
                Some(Action::RerunMacro)
            } else if i.consume_shortcut(&FIND) {
                Some(Action::Find { replace: false })
            } else if i.consume_shortcut(&REPLACE) {
                Some(Action::Find { replace: true })
            } else if i.consume_shortcut(&FIND_PREV) {
                // Shift+F3 要先於 F3 檢查，原因同上
                Some(Action::FindPrev)
            } else if i.consume_shortcut(&FIND_NEXT) {
                Some(Action::FindNext)
            } else if i.consume_shortcut(&egui::gui_zoom::kb_shortcuts::ZOOM_IN)
                || i.consume_shortcut(&egui::gui_zoom::kb_shortcuts::ZOOM_IN_SECONDARY)
            {
                // Ctrl ＋（含不按 Shift 的 Ctrl ＝）：放大字級
                Some(zoom_action(adjust_font_size(zoom_base, 1.0)))
            } else if i.consume_shortcut(&egui::gui_zoom::kb_shortcuts::ZOOM_OUT) {
                Some(zoom_action(adjust_font_size(zoom_base, -1.0)))
            } else if i.consume_shortcut(&egui::gui_zoom::kb_shortcuts::ZOOM_RESET) {
                Some(zoom_action(zoom_default))
            } else {
                None
            }
        });
        // 使用者替巨集設定的快捷鍵。觸發時，系統同時送來的字元（Alt+M 的 "m"）要丟掉，
        // 不然巨集跑完之後編輯區還會多出一個字；字元事件有時晚一個影格才到，所以下一個影格也處理。
        if let Some((key, left)) = self.swallow_text.take() {
            ctx.input_mut(|i| drop_key_text(&mut i.events, key));
            if left > 1 {
                self.swallow_text = Some((key, left - 1));
            }
        }
        if action.is_none() {
            let hit = self.macros.iter().position(|m| {
                m.shortcut
                    .as_ref()
                    .is_some_and(|sc| ctx.input_mut(|i| i.consume_shortcut(sc)))
            });
            if let Some(i) = hit {
                if let Some(sc) = self.macros[i].shortcut {
                    ctx.input_mut(|ev| drop_key_text(&mut ev.events, sc.logical_key));
                    self.swallow_text = Some((sc.logical_key, 1));
                }
                action = Some(Action::RunMacroEntry(i));
            }
        }
        // Ctrl＋滾輪（或觸控板捏合）：egui 以縮放比例表示，換算成字級的加減
        if action.is_none() {
            let steps = zoom_steps(&mut self.zoom_accum, zoom_delta);
            if steps != 0 {
                action = Some(zoom_action(adjust_font_size(zoom_base, steps as f32)));
            }
        }

        egui::Panel::top("toolbar").show_separator_line(false).show(ui, |ui| {
            if let Some(a) = self.toolbar(ui) {
                action = Some(a);
            }
        });
        if self.find.open {
            egui::Panel::top("find").show(ui, |ui| {
                if let Some(a) = self.find_bar(ui) {
                    action = Some(a);
                }
            });
        }
        // 下方的面板（狀態列、輸出面板）左右兩端對齊窗格卡片的外緣，不要比圓角框更寬
        let bottom_frame = aligned_panel_frame(ui.style());
        egui::Panel::bottom("status").frame(bottom_frame.clone()).show(ui, |ui| {
            if let Some(a) = self.status_bar(ui) {
                action = Some(a);
            }
        });
        if self.output_expanded() {
            egui::Panel::bottom("output")
                .frame(bottom_frame.clone())
                .resizable(true)
                .default_size(110.0)
                .show(ui, |ui| {
                    if let Some(a) = self.output_panel(ui) {
                        action = Some(a);
                    }
                });
        } else {
            // 收合：只剩一條標題列。用另一個 id，這樣展開時會回到使用者調整過的高度
            egui::Panel::bottom("output_bar").frame(bottom_frame.clone()).show(ui, |ui| {
                if let Some(a) = self.output_header(ui, false) {
                    action = Some(a);
                }
            });
        }
        if self.folder.open {
            egui::Panel::bottom("folder_search")
                .resizable(true)
                .default_size(220.0)
                .show(ui, |ui| {
                    if let Some(a) = self.folder_search_panel(ui) {
                        action = Some(a);
                    }
                });
        }
        // 窗格是有框線的卡片，不需要工具列下面的分隔線；上方也不多留邊，離工具列近一點（左右與下方維持原本的空隙）
        let central = egui::Frame::central_panel(ui.style())
            .inner_margin(egui::Margin { left: CENTRAL_SIDE_MARGIN, right: CENTRAL_SIDE_MARGIN, top: 0, bottom: 8 });
        egui::CentralPanel::default().frame(central).show(ui, |ui| {
            if let Some(a) = self.panes_ui(ui) {
                action = Some(a);
            }
        });
        if self.settings_open {
            self.show_settings_area(&ctx, &mut action);
        }
        if self.macro_open {
            self.show_macro_area(&ctx, &mut action);
        }
        self.scheme_editor(&ctx);
        if let Some(mut p) = self.modal.take() {
            match modal::show(&ctx, &p.spec, p.input.as_mut()) {
                Some(answer) => self.resolve_modal(p.then, answer, p.input.map(|i| i.text), &ctx),
                None => self.modal = Some(p),
            }
        }

        if let Some(a) = action {
            self.handle(a, &ctx);
            if self.pending_editor_focus {
                ctx.request_repaint(); // 下一個畫面就把鍵盤焦點移進編輯區
            }
        }

        // 設定有變更：停下來 0.4 秒後才寫入磁碟（字級拖曳時會連續變動，不要每次都寫）
        if let Some(since) = self.settings_dirty_since {
            if since.elapsed() >= Duration::from_millis(400) {
                self.flush_settings();
            } else {
                ctx.request_repaint_after(Duration::from_millis(450));
            }
        }

        let title = format!(
            "{}{} - FEditor",
            self.doc.name(),
            if self.doc.dirty { " ●" } else { "" }
        );
        if title != self.last_title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.last_title = title;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use feditor_core::Buffer;

    fn app_with(text: &str) -> App {
        let mut app = App::new(None, Settings::default());
        app.doc.buffer = Buffer::from_text(text);
        app.doc.mark_synced();
        app
    }

    #[test]
    fn find_selects_the_match_and_continues_from_the_selection() {
        let mut app = app_with("cat dog cat");
        app.find.query = "cat".to_owned();
        app.find_step(false);
        assert_eq!(app.editor.sel.range(), 0..3);
        app.find_step(false);
        assert_eq!(app.editor.sel.range(), 8..11);
        app.find_step(false); // 繞回
        assert_eq!(app.editor.sel.range(), 0..3);
        assert!(app.find.wrapped);
        app.find_step(true); // 往回找：前面沒有了，繞到結尾
        assert_eq!(app.editor.sel.range(), 8..11);
    }

    #[test]
    fn find_continues_from_where_the_user_moved_the_caret() {
        let mut app = app_with("cat dog cat dog cat");
        app.find.query = "cat".to_owned();
        app.find_step(false);
        // 使用者自己把游標點到第二個 cat 之後
        app.editor.set_caret(&mut app.doc.buffer, 13, false);
        app.find_step(false);
        assert_eq!(app.editor.sel.range(), 16..19);
    }

    #[test]
    fn replace_one_edits_the_buffer_selects_next_and_is_undoable() {
        let mut app = app_with("cat dog cat");
        app.find.query = "cat".to_owned();
        app.find.replacement = "bird".to_owned();
        app.find_step(false);
        app.replace_one();
        assert_eq!(app.doc.text(), "bird dog cat");
        assert!(app.doc.dirty);
        assert_eq!(app.editor.sel.range(), 9..12, "下一個 cat 已被選取");
        app.replace_one();
        assert_eq!(app.doc.text(), "bird dog bird");
        app.editor.undo(&mut app.doc.buffer);
        assert_eq!(app.doc.text(), "bird dog cat");
        app.editor.undo(&mut app.doc.buffer);
        assert_eq!(app.doc.text(), "cat dog cat");
    }

    #[test]
    fn replace_all_is_one_undo_step_and_marks_dirty() {
        let mut app = app_with("a a a");
        app.find.query = "a".to_owned();
        app.find.replacement = "bb".to_owned();
        app.replace_all();
        assert_eq!(app.doc.text(), "bb bb bb");
        assert!(app.doc.dirty);
        app.editor.undo(&mut app.doc.buffer);
        assert_eq!(app.doc.text(), "a a a");
    }

    #[test]
    fn macro_marks_dirty_and_undoes_in_one_step_but_failure_does_not() {
        let dir = tempfile::tempdir().unwrap();
        let ok = dir.path().join("ok.js");
        std::fs::write(&ok, "editor.insert(0, 'X'); editor.insert(0, 'Y');").unwrap();
        let bad = dir.path().join("bad.js");
        std::fs::write(&bad, "editor.insert(0, 'Z'); throw new Error('boom');").unwrap();

        let mut app = app_with("abc");
        app.run_macro_file(bad);
        assert_eq!(app.doc.text(), "abc");
        assert!(!app.doc.dirty, "失敗並完整還原，不算修改");

        app.run_macro_file(ok);
        assert_eq!(app.doc.text(), "YXabc");
        assert!(app.doc.dirty);
        app.editor.undo(&mut app.doc.buffer);
        assert_eq!(app.doc.text(), "abc");
    }

    #[test]
    fn status_bar_shows_line_and_column_with_configurable_cjk_width() {
        let mut app = app_with("first\n中文ab");
        assert_eq!(app.caret_position_text(), "行 1，欄 1");
        // 第 2 行，游標在「文」之後
        app.editor.set_caret(&mut app.doc.buffer, 8, false);
        assert_eq!(app.caret_position_text(), "行 2，欄 3", "預設全形字算 1 欄");
        app.settings.wide_cols = 2;
        assert_eq!(app.caret_position_text(), "行 2，欄 5", "全形字算 2 欄");
        app.editor.set_caret(&mut app.doc.buffer, 5, false);
        assert_eq!(app.caret_position_text(), "行 1，欄 6", "行尾");
    }

    // ── 窗格（分割視窗） ─────────────────────────────────────

    use pane_ops::Target;
    use panes::Side;

    /// 開 a、b 兩個檔案（分頁依序是 a、b，作用中是 b）。
    fn app_with_two_files() -> (App, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        for (name, text) in [("a.txt", "aaa"), ("b.txt", "bbb"), ("c.txt", "ccc")] {
            std::fs::write(dir.path().join(name), text).unwrap();
        }
        let mut app = App::new(None, Settings::default());
        app.open_path(&dir.path().join("a.txt"));
        app.open_path(&dir.path().join("b.txt"));
        (app, dir)
    }

    fn pane_names(app: &App) -> Vec<(panes::PaneId, Vec<String>)> {
        app.layout
            .root
            .leaves()
            .into_iter()
            .map(|id| {
                let n = if id == app.layout.focus { app.tabs.len() } else { app.layout.others[&id].len() };
                let names = (0..n)
                    .map(|i| {
                        if id == app.layout.focus {
                            app.tabs.label(&app.doc, i).0
                        } else {
                            app.layout.others[&id].label_stashed(i).0
                        }
                    })
                    .collect();
                (id, names)
            })
            .collect()
    }

    #[test]
    fn dragging_a_tab_to_a_side_splits_the_pane_and_moves_the_tab() {
        let (mut app, _dir) = app_with_two_files();
        let first = app.layout.focus;
        // 把 a（不是作用中的）拖到左側：新窗格在左邊，裡面是 a，焦點跟過去
        app.move_tab(first, 0, Target::Split(first, Side::Left));
        let panes = pane_names(&app);
        assert_eq!(panes.len(), 2);
        assert_eq!(panes[0].1, ["a.txt"], "左邊是新窗格");
        assert_eq!(panes[1], (first, vec!["b.txt".to_owned()]));
        assert_eq!(app.layout.focus, panes[0].0);
        assert_eq!(app.doc.text(), "aaa");
        assert_eq!(app.tabs.len(), 1);
        // 沒有焦點的窗格，內容也完整：b 還在
        assert_eq!(app.layout.others[&first].label_stashed(0).0, "b.txt");
    }

    #[test]
    fn splitting_to_the_right_or_bottom_puts_the_new_pane_after() {
        for (side, expect_new_last) in [(Side::Right, true), (Side::Bottom, true)] {
            let (mut app, _dir) = app_with_two_files();
            let first = app.layout.focus;
            app.move_tab(first, 1, Target::Split(first, side)); // 作用中的 b
            let panes = pane_names(&app);
            assert_eq!(panes.len(), 2, "{side:?}");
            assert_eq!(expect_new_last, panes[1].0 == app.layout.focus, "{side:?}：新窗格在後面");
            assert_eq!(panes[1].1, ["b.txt"]);
            assert_eq!(panes[0].1, ["a.txt"]);
            assert_eq!(app.doc.text(), "bbb");
        }
    }

    #[test]
    fn a_lone_tab_cannot_be_split_off_its_own_pane_and_a_pane_is_not_a_drop_target_for_itself() {
        let (mut app, _dir) = app_with_two_files();
        let first = app.layout.focus;
        app.move_tab(first, 0, Target::Into(first));
        assert_eq!(app.layout.pane_count(), 1);
        app.close_tab(0); // 只剩 b
        assert_eq!(app.tabs.len(), 1);
        app.move_tab(first, 0, Target::Split(first, Side::Left));
        assert_eq!(app.layout.pane_count(), 1, "只剩一個分頁的窗格，切不出新窗格");
        assert_eq!(app.doc.text(), "bbb");
    }

    #[test]
    fn moving_the_last_tab_out_of_a_pane_closes_that_pane() {
        let (mut app, _dir) = app_with_two_files();
        let first = app.layout.focus;
        app.move_tab(first, 1, Target::Split(first, Side::Right)); // [a] | [b]
        let second = app.layout.focus;
        assert_ne!(first, second);
        // 把 b 拖回左邊那個窗格：右邊空了，窗格自動關掉
        app.move_tab(second, 0, Target::Into(first));
        assert_eq!(app.layout.pane_count(), 1);
        assert_eq!(app.layout.focus, first);
        assert_eq!(pane_names(&app), [(first, vec!["a.txt".to_owned(), "b.txt".to_owned()])]);
        assert_eq!(app.doc.text(), "bbb", "搬過去的分頁成為作用中分頁");
        assert!(app.layout.others.is_empty());
    }

    #[test]
    fn closing_a_pane_moves_its_tabs_to_the_neighbour_and_keeps_unsaved_edits() {
        let (mut app, _dir) = app_with_two_files();
        let first = app.layout.focus;
        app.move_tab(first, 1, Target::Split(first, Side::Right)); // [a] | [b]
        let second = app.layout.focus;
        // b 有未儲存的修改
        app.doc.buffer.insert(0, "x");
        assert!(app.doc.sync());
        app.close_pane(second);
        assert_eq!(app.layout.pane_count(), 1);
        assert_eq!(pane_names(&app)[0].1, ["a.txt", "b.txt"], "分頁接在相鄰窗格的後面");
        assert_eq!(app.doc.text(), "xbbb", "沒有關掉任何文件，修改還在");
        assert!(app.doc.dirty);
        assert_eq!(app.all_dirty(), vec![(first, 1)]);
        // 只剩一個窗格時關不掉
        app.close_pane(first);
        assert_eq!(app.layout.pane_count(), 1);
    }

    #[test]
    fn closing_the_last_tab_of_a_pane_closes_the_pane_instead_of_leaving_a_blank_document() {
        let (mut app, _dir) = app_with_two_files();
        let first = app.layout.focus;
        app.move_tab(first, 1, Target::Split(first, Side::Right)); // [a] | [b]
        app.close_tab(0); // 關掉右邊窗格唯一的分頁 b
        assert_eq!(app.layout.pane_count(), 1);
        assert_eq!(app.layout.focus, first);
        assert_eq!(app.doc.text(), "aaa");
        assert_eq!(app.tabs.len(), 1, "沒有多出空白文件");
    }

    #[test]
    fn opening_a_file_that_is_open_in_another_pane_switches_to_that_pane() {
        let (mut app, dir) = app_with_two_files();
        let first = app.layout.focus;
        app.move_tab(first, 0, Target::Split(first, Side::Left)); // [a] | [b]，焦點在 a
        assert_eq!(app.doc.text(), "aaa");
        app.open_path(&dir.path().join("b.txt"));
        assert_eq!(app.layout.focus, first, "焦點跳到開著 b 的窗格");
        assert_eq!(app.doc.text(), "bbb");
        assert_eq!(app.layout.pane_count(), 2, "沒有多開分頁或窗格");
        assert_eq!(pane_names(&app).iter().map(|(_, n)| n.len()).sum::<usize>(), 2);
        // 新開的檔案放進有焦點的窗格
        app.open_path(&dir.path().join("c.txt"));
        assert_eq!(app.tabs.len(), 2);
        assert_eq!(app.doc.text(), "ccc");
    }

    #[test]
    fn dirty_tabs_and_the_session_cover_every_pane() {
        let (mut app, dir) = app_with_two_files();
        let first = app.layout.focus;
        app.move_tab(first, 0, Target::Split(first, Side::Left)); // [a] | [b]
        app.doc.buffer.insert(0, "x"); // a 變髒（有焦點的窗格）
        assert!(app.doc.sync());
        app.focus_pane(first);
        app.doc.buffer.insert(0, "y"); // b 也髒（原本沒有焦點，現在有）
        assert!(app.doc.sync());
        assert_eq!(app.all_dirty().len(), 2, "兩個窗格各一個未儲存的分頁");
        let session = app.session_all();
        assert_eq!(session.panes.len(), 2);
        let names: Vec<_> = session
            .panes
            .iter()
            .flat_map(|p| p.files.iter())
            .map(|(p, _)| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["a.txt", "b.txt"], "依畫面由左到右");
        assert_eq!(session.focus, app.layout.focus, "記住有焦點的窗格");
        assert!(session.layout.as_deref().is_some_and(|l| l.starts_with('H')), "{:?}", session.layout);
        let _ = dir;
    }

    #[test]
    fn focusing_another_pane_keeps_a_recording_in_progress() {
        let (mut app, _dir) = app_with_two_files();
        let first = app.layout.focus;
        app.move_tab(first, 0, Target::Split(first, Side::Left));
        app.editor.recording = Some(Vec::new());
        app.focus_pane(first);
        assert_eq!(app.layout.focus, first);
        assert!(app.editor.recording.is_some(), "錄製跟著使用者走，不屬於某個窗格");
        assert!(app.layout.others.values().all(|s| s.len() >= 1));
    }

    #[test]
    fn dropping_a_dragged_tab_is_cancelled_when_nothing_useful_is_under_the_pointer() {
        let (mut app, _dir) = app_with_two_files();
        let first = app.layout.focus;
        app.begin_tab_drag(first, 0);
        assert_eq!(app.tab_drag.as_ref().map(|d| d.label.as_str()), Some("a.txt"));
        app.begin_tab_drag(first, 9);
        assert_eq!(app.tab_drag.as_ref().unwrap().index, 0, "不存在的分頁不能拖");
    }

    // ── 預覽窗格與版面還原 ───────────────────────────────────

    /// 資料夾裡有 a.md、b.md、c.txt；回傳沒有使用者外掛的 App。
    fn app_for_preview() -> (App, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        for (name, text) in [("a.md", "# 甲標題\n\n內文"), ("b.md", "# 乙標題"), ("c.txt", "純文字")] {
            std::fs::write(dir.path().join(name), text).unwrap();
        }
        let mut app = App::new(None, Settings::default());
        app.preview_plugins.clear(); // 不要受使用者自己的外掛影響
        (app, dir)
    }

    fn wait_for_preview(app: &mut App, id: panes::PaneId, done: impl Fn(&preview::Rendered) -> bool) -> preview::Rendered {
        let ctx = egui::Context::default();
        for _ in 0..250 {
            let _ = app.update_preview(id, &ctx);
            if let Some(r) = app.previews.get(&id).and_then(|p| p.rendered()) {
                if done(r) {
                    return r.clone();
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        panic!("預覽一直沒有出現");
    }

    #[test]
    fn the_preview_button_opens_one_pane_on_the_right_keeps_the_focus_and_toggles_off() {
        let (mut app, dir) = app_for_preview();
        app.open_path(&dir.path().join("a.md"));
        let editor_pane = app.layout.focus;
        app.toggle_preview();
        assert_eq!((app.layout.pane_count(), app.tab_pane_count()), (2, 1));
        let pv = app.preview_id().unwrap();
        assert!(app.is_preview(pv));
        assert_eq!(app.layout.root.leaves(), vec![editor_pane, pv], "預覽在右邊");
        assert_eq!(app.layout.focus, editor_pane, "焦點留在編輯的窗格");
        // 再按一次關掉（整個程式只有一個預覽窗格，不會越開越多）
        app.toggle_preview();
        assert_eq!(app.layout.pane_count(), 1);
        assert!(app.previews.is_empty() && app.preview_id().is_none());
        // 目前的文件沒有預覽（.txt、沒存過檔）也照開：預覽窗格會說明，切到有預覽的文件就顯示
        app.open_path(&dir.path().join("c.txt"));
        app.toggle_preview();
        assert_eq!(app.layout.pane_count(), 2);
        let pv = app.preview_id().unwrap();
        assert_eq!(app.update_preview(pv, &egui::Context::default()), preview_pane::Status::NoRenderer);
        app.toggle_preview();
        app.new_document();
        app.toggle_preview();
        assert_eq!(app.layout.pane_count(), 2);
    }
    #[test]
    fn a_preview_pane_renders_in_the_background_follows_edits_and_the_active_tab() {
        let (mut app, dir) = app_for_preview();
        app.open_path(&dir.path().join("a.md"));
        app.toggle_preview();
        let pv = app.preview_id().unwrap();
        let r = wait_for_preview(&mut app, pv, |_| true);
        assert!(matches!(&r, preview::Rendered::Markdown(b) if matches!(&b[0], preview::Block::Heading(1, _))), "{r:?}");
        // 改了文字：停一下之後預覽跟著更新
        let at = app.doc.buffer.len_chars();
        app.doc.buffer.insert(at, "\n\n## 新增的小標題");
        app.doc.sync();
        let r = wait_for_preview(&mut app, pv, |r| {
            matches!(r, preview::Rendered::Markdown(b) if b.iter().any(|x| matches!(x, preview::Block::Heading(2, _))))
        });
        assert!(matches!(r, preview::Rendered::Markdown(_)));
        // 換到沒有預覽的分頁：預覽窗格說沒有預覽；換回來又有
        let ctx = egui::Context::default();
        app.open_path(&dir.path().join("c.txt"));
        assert_eq!(app.update_preview(pv, &ctx), preview_pane::Status::NoRenderer);
        app.open_path(&dir.path().join("a.md"));
        assert!(matches!(app.update_preview(pv, &ctx), preview_pane::Status::Ready(_) | preview_pane::Status::Working));
    }

    #[test]
    fn dropping_a_tab_on_the_preview_pane_edge_splits_the_preview_pane_but_not_into_it() {
        let (mut app, dir) = app_for_preview();
        app.open_path(&dir.path().join("a.md"));
        app.open_path(&dir.path().join("c.txt"));
        let a = app.layout.focus;
        app.switch_tab(0);
        app.toggle_preview(); // [A][預覽]
        let pv = app.preview_id().unwrap();
        // 放進預覽窗格：不行
        app.move_tab(a, 1, Target::Into(pv));
        assert_eq!((app.layout.pane_count(), app.tabs.len()), (2, 2), "預覽窗格不收分頁");
        // 放到預覽窗格的下側：把預覽窗格切開，分頁變成預覽下面的新窗格
        app.move_tab(a, 1, Target::Split(pv, Side::Bottom));
        let b = app.layout.focus;
        assert_ne!(b, a);
        assert_eq!(app.layout.root.leaves(), vec![a, pv, b], "[A | (預覽 / B)]");
        assert_eq!(app.tab_pane_count(), 2);
        assert_eq!(app.doc.name(), "c.txt");
        // 預覽沒有因為任何窗格被關掉而消失：關掉 A，預覽還在，分頁併進 B
        app.close_pane(a);
        assert_eq!(app.tab_pane_count(), 1);
        assert!(app.preview_id().is_some(), "預覽窗格是獨立的，不跟著某個窗格");
        assert_eq!(app.tabs.len(), 2);
        assert_eq!(app.layout.pane_count(), 2);
    }

    #[test]
    fn a_preview_pane_can_be_dragged_anywhere_and_always_shows_the_focused_document() {
        let (mut app, dir) = app_for_preview();
        app.open_path(&dir.path().join("a.md"));
        app.open_path(&dir.path().join("b.md"));
        let a = app.layout.focus;
        app.toggle_preview(); // [A][預覽]
        let pv = app.preview_id().unwrap();
        // b.md 拖到下面變成第二個一般窗格：[[A / B] | 預覽]
        app.move_tab(a, 1, Target::Split(a, Side::Bottom));
        let b = app.layout.focus;
        assert_eq!(app.layout.root.leaves(), vec![a, b, pv]);
        let ctx = egui::Context::default();
        let heading = |r: &preview::Rendered| match r {
            preview::Rendered::Markdown(bl) => match bl.first() {
                Some(preview::Block::Heading(1, s)) => s[0].text.clone(),
                _ => String::new(),
            },
            _ => String::new(),
        };
        // 焦點在 B（b.md）：預覽就是 b.md
        let r = wait_for_preview(&mut app, pv, |r| heading(r) == "乙標題");
        assert_eq!(heading(&r), "乙標題");
        // 焦點換到 A：預覽跟著換成 a.md
        app.focus_pane(a);
        let _ = app.update_preview(pv, &ctx);
        assert_eq!(app.previews[&pv].source, a);
        let r = wait_for_preview(&mut app, pv, |r| heading(r) == "甲標題");
        assert_eq!(heading(&r), "甲標題");
        // 拖預覽的標題：開始拖曳，放到 B 的左側
        app.begin_preview_drag(pv);
        assert!(app.tab_drag.as_ref().is_some_and(|d| d.preview && d.pane == pv));
        app.move_preview(pv, b, Side::Left);
        assert_eq!(app.layout.root.leaves(), vec![a, pv, b], "預覽在 B 的左邊");
        assert_eq!(app.layout.focus, a, "焦點不變");
        // 再放到 A 的下側
        app.move_preview(pv, a, Side::Bottom);
        assert_eq!(app.layout.pane_count(), 3);
        // 不合法的放法什麼都不做：放到自己、來源不是預覽、放到不存在的窗格
        let before = app.layout.root.leaves();
        app.move_preview(pv, pv, Side::Right);
        app.move_preview(a, b, Side::Right);
        app.move_preview(pv, 999, Side::Right);
        assert_eq!(app.layout.root.leaves(), before);
        assert!(app.is_preview(pv), "放不下也不會讓預覽消失");
        // 版面存起來再還原：預覽還在同一個位置，而且只有一個
        let session = session::Session::parse(&app.session_all().to_text());
        let mut fresh = App::new(None, Settings::default());
        fresh.preview_plugins.clear();
        fresh.restore_from(&session);
        assert_eq!((fresh.layout.pane_count(), fresh.previews.len()), (3, 1));
        assert_eq!(fresh.layout.root.to_spec(&|id| fresh.is_preview(id)), session.layout.clone().unwrap());
    }
    #[test]
    fn closing_the_last_tab_next_to_a_preview_leaves_a_blank_document_and_keeps_the_preview_pane() {
        let (mut app, dir) = app_for_preview();
        app.open_path(&dir.path().join("a.md"));
        app.toggle_preview();
        app.close_tab(0);
        assert_eq!(app.tab_pane_count(), 1);
        assert_eq!(app.layout.pane_count(), 2, "預覽窗格還在（顯示沒有預覽）");
        assert!(app.doc.path.is_none());
        let pv = app.preview_id().unwrap();
        assert_eq!(app.update_preview(pv, &egui::Context::default()), preview_pane::Status::NoRenderer);
    }

    /// 三個檔案、兩個窗格、一個預覽窗格：[a.md, c.txt] | [b.md][預覽]，焦點在 b.md 的窗格。
    fn app_with_three_panes(dir: &std::path::Path) -> App {
        let mut app = App::new(None, Settings::default());
        app.preview_plugins.clear();
        for name in ["a.md", "b.md", "c.txt"] {
            app.open_path(&dir.join(name));
        }
        let first = app.layout.focus;
        app.move_tab(first, 1, Target::Split(first, Side::Right));
        app.toggle_preview();
        app
    }

    #[test]
    fn the_pane_layout_comes_back_after_a_restart_with_tabs_carets_focus_and_previews() {
        let (_app, dir) = app_for_preview();
        let mut app = app_with_three_panes(dir.path());
        app.editor.set_caret(&mut app.doc.buffer, 3, false);
        let before = app.session_all();
        assert_eq!(before.panes.len(), 2);
        assert_eq!(before.previews.len(), 1);
        // 存成文字再讀回來（就是關閉再重開）
        let session = session::Session::parse(&before.to_text());
        assert_eq!(session, before);

        let mut fresh = App::new(None, Settings::default());
        fresh.preview_plugins.clear();
        fresh.restore_from(&session);
        assert_eq!((fresh.layout.pane_count(), fresh.tab_pane_count()), (3, 2));
        assert_eq!(fresh.previews.len(), 1);
        assert_eq!(fresh.layout.focus, app.layout.focus, "焦點窗格");
        assert_eq!(fresh.doc.name(), "b.md");
        assert_eq!(fresh.editor.sel.head, 3, "游標位置");
        let names = |a: &App| {
            a.layout
                .root
                .leaves()
                .into_iter()
                .filter(|id| !a.is_preview(*id))
                .map(|id| {
                    let n = if id == a.layout.focus { a.tabs.len() } else { a.layout.others[&id].len() };
                    (0..n)
                        .map(|i| if id == a.layout.focus { a.tabs.label(&a.doc, i).0 } else { a.layout.others[&id].label_stashed(i).0 })
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(names(&fresh), names(&app));
        assert_eq!(names(&fresh), vec![vec!["a.md", "c.txt"], vec!["b.md"]]);
        let pv = *fresh.previews.keys().next().unwrap();
        assert_eq!(fresh.previews[&pv].source, fresh.layout.focus, "預覽跟著有焦點的窗格（b.md）");
        // 版面也一樣（比例、分割方向）
        assert_eq!(fresh.session_all().layout, before.layout);
        // 還原之後還能正常新增窗格（編號不會撞到舊的）
        let before_ids = fresh.layout.root.leaves();
        fresh.move_tab(fresh.layout.focus, 0, Target::Split(fresh.layout.focus, Side::Bottom)); // b.md 只有一個分頁：不動
        assert_eq!(fresh.layout.root.leaves(), before_ids);
    }

    #[test]
    fn restoring_skips_missing_files_and_drops_panes_that_end_up_empty_but_keeps_the_preview() {
        let (_app, dir) = app_for_preview();
        let app = app_with_three_panes(dir.path());
        let session = session::Session::parse(&app.session_all().to_text());
        // b.md 不見了：它的窗格拿掉，剩下 [a.md, c.txt]；預覽窗格是獨立的，還在
        std::fs::remove_file(dir.path().join("b.md")).unwrap();
        let mut fresh = App::new(None, Settings::default());
        fresh.restore_from(&session);
        assert_eq!((fresh.tab_pane_count(), fresh.layout.pane_count()), (1, 2));
        assert_eq!(fresh.previews.len(), 1);
        assert_eq!(fresh.tabs.len(), 2);
        assert_ne!(fresh.layout.focus, session.focus, "原本有焦點的窗格沒了，焦點移到還在的窗格");
        // c.txt 也不見了：只剩 a.md
        std::fs::remove_file(dir.path().join("c.txt")).unwrap();
        let mut fresh = App::new(None, Settings::default());
        fresh.restore_from(&session);
        assert_eq!((fresh.tabs.len(), fresh.doc.name().as_str()), (1, "a.md"));
        // 全部不見了：什麼都不開，也不會當掉
        std::fs::remove_file(dir.path().join("a.md")).unwrap();
        let mut fresh = App::new(None, Settings::default());
        fresh.restore_from(&session);
        assert_eq!((fresh.layout.pane_count(), fresh.tabs.len()), (1, 1));
        assert!(fresh.doc.path.is_none());
    }

    #[test]
    fn an_old_flat_session_still_restores_into_one_pane_and_a_broken_layout_falls_back_to_it() {
        let (_app, dir) = app_for_preview();
        let a = dir.path().join("a.md");
        let b = dir.path().join("b.md");
        let old = session::Session::parse(&format!("active = 1\nfile = {}\t0\nfile = {}\t2\n", a.display(), b.display()));
        let mut fresh = App::new(None, Settings::default());
        fresh.restore_from(&old);
        assert_eq!((fresh.layout.pane_count(), fresh.tabs.len()), (1, 2));
        assert_eq!(fresh.doc.name(), "b.md", "作用中的分頁");
        assert_eq!(fresh.editor.sel.head, 2);
        // 版面文字壞掉：退回單一窗格，檔案還是開起來
        let mut broken = session::Session::parse(&format!("layout = H0.5[P0\nfocus = 0\npane = 0\nactive = 0\nfile = {}\t0\n", a.display()));
        broken.focus = 0;
        let mut fresh = App::new(None, Settings::default());
        fresh.restore_from(&broken);
        assert_eq!((fresh.layout.pane_count(), fresh.doc.name().as_str()), (1, "a.md"));
    }

    #[test]
    fn terminal_settings_change_through_actions_are_clamped_and_leave_the_other_font_sizes_alone() {
        let mut app = App::new(None, Settings::default());
        let ctx = egui::Context::default();
        let (editor_size, preview_size) = (app.settings.font_size, app.settings.preview_font_size);
        app.handle(Action::SetTerminalFontSize(20.0), &ctx);
        assert_eq!(app.settings.terminal_font_size, 20.0);
        app.handle(Action::SetTerminalFontSize(9999.0), &ctx);
        assert_eq!(app.settings.terminal_font_size, settings::MAX_FONT_SIZE);
        app.handle(Action::SetTerminalFontSize(f32::NAN), &ctx);
        assert_eq!(app.settings.terminal_font_size, DEFAULT_TERMINAL_FONT_SIZE);
        assert_eq!((app.settings.font_size, app.settings.preview_font_size), (editor_size, preview_size), "各自獨立");
        app.handle(Action::SetTerminalMinContrast(4.5), &ctx);
        assert_eq!(app.settings.terminal_min_contrast, 4.5);
        app.handle(Action::SetTerminalMinContrast(0.0), &ctx);
        assert_eq!(app.settings.terminal_min_contrast, MIN_TERMINAL_CONTRAST, "拉回範圍內，最小是不調整");
        app.handle(Action::SetTerminalMinContrast(f32::INFINITY), &ctx);
        assert_eq!(app.settings.terminal_min_contrast, DEFAULT_TERMINAL_MIN_CONTRAST);
        // shell：邊打字邊存，所以前後的空白保留（下一個詞的開頭）；換行會被換成空白
        app.handle(Action::SetTerminalShell("pwsh ".into()), &ctx);
        assert_eq!(app.settings.terminal_shell, "pwsh ");
        app.handle(Action::SetTerminalShell("a\nb".into()), &ctx);
        assert_eq!(app.settings.terminal_shell, "a b");
    }

    #[test]
    fn the_ruler_column_is_a_view_option_changed_through_set_view() {
        let mut app = App::new(None, Settings::default());
        let ctx = egui::Context::default();
        assert_eq!(app.settings.view.ruler_column, settings::DEFAULT_RULER_COLUMN);
        let view = settings::ViewOptions { ruler_column: 72, ..app.settings.view };
        app.handle(Action::SetView(view), &ctx);
        assert_eq!(app.settings.view.ruler_column, 72);
        let off = settings::ViewOptions { ruler_column: 0, ..app.settings.view };
        app.handle(Action::SetView(off), &ctx);
        assert_eq!(app.settings.view.ruler_column, 0);
        assert!(app.settings.view.word_wrap == settings::ViewOptions::default().word_wrap, "其他的顯示選項不受影響");
    }

    #[test]
    fn the_plain_terminal_button_uses_the_default_shell_and_the_dropdown_picks_another_one_for_that_terminal_only() {
        let mut app = App::new(None, Settings::default());
        let ctx = egui::Context::default();
        let shell_of = |app: &App| app.doc.terminal.as_ref().map(|t| t.shell().to_owned());
        // 沒設定：系統預設（空字串）
        app.handle(Action::NewTerminal, &ctx);
        assert_eq!(shell_of(&app).as_deref(), Some(""));
        // 設了預設終端機：按鈕用它；下拉清單選別的只影響那一個
        app.settings.terminal_shell = "pwsh -NoLogo".to_owned();
        app.handle(Action::NewTerminal, &ctx);
        assert_eq!(shell_of(&app).as_deref(), Some("pwsh -NoLogo"));
        app.handle(Action::NewTerminalWith("cmd".to_owned()), &ctx);
        assert_eq!(shell_of(&app).as_deref(), Some("cmd"));
        assert_eq!(app.settings.terminal_shell, "pwsh -NoLogo", "預設沒有被改掉");
        app.handle(Action::NewTerminal, &ctx);
        assert_eq!(shell_of(&app).as_deref(), Some("pwsh -NoLogo"), "下一次按鈕還是用預設的");
        // 標籤上的 shell 圖示要知道這個分頁跑的是哪個 shell；文字分頁沒有
        let (pane, idx) = app.terminal_tabs()[0];
        assert_eq!(app.pane_tab_terminal_shell(pane, idx).as_deref(), Some(""));
        assert_eq!(app.pane_tab_terminal_shell(app.layout.focus, app.tabs.active()).as_deref(), Some("pwsh -NoLogo"));
        let text_pane = app.layout.root.leaves()[0];
        assert_eq!(app.pane_tab_terminal_shell(text_pane, 0), None, "文字分頁沒有 shell");
        // 開著的終端機不受之後改的設定影響
        let first = app.terminal_tabs()[0];
        app.focus_pane(first.0);
        app.switch_tab(first.1);
        app.settings.terminal_shell = "nu".to_owned();
        assert_eq!(shell_of(&app).as_deref(), Some(""), "已經開著的還是當初的 shell");
    }

    #[test]
    fn the_preview_font_size_is_adjusted_separately_from_the_editor_font_size() {
        let ctx = egui::Context::default();
        let mut app = App::new(None, Settings::default());
        let editor_size = app.settings.font_size;
        app.handle(Action::SetPreviewFontSize(20.0), &ctx);
        assert_eq!(app.settings.preview_font_size, 20.0);
        assert_eq!(app.settings.font_size, editor_size, "編輯區的字級不受影響");
        assert!(app.settings_dirty_since.is_some(), "要寫進設定");
        app.handle(Action::SetPreviewFontSize(9999.0), &ctx);
        assert_eq!(app.settings.preview_font_size, settings::MAX_FONT_SIZE, "拉回範圍內");
        app.handle(Action::SetPreviewFontSize(f32::NAN), &ctx);
        assert_eq!(app.settings.preview_font_size, DEFAULT_PREVIEW_FONT_SIZE, "不是數字就用預覽自己的預設");
    }

    #[test]
    fn clicking_the_preview_pane_gives_it_the_input_focus_and_clicking_an_editor_pane_takes_it_back() {
        let (mut app, dir) = app_for_preview();
        app.open_path(&dir.path().join("a.md"));
        app.toggle_preview();
        let pv = app.preview_id().unwrap();
        let editor_pane = app.layout.focus;
        let ctx = egui::Context::default();
        let mut raw = egui::RawInput::default();
        raw.screen_rect = Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 700.0)));
        let frame = |app: &mut App, events: Vec<egui::Event>| {
            let mut r = raw.clone();
            r.events = events;
            let mut output = ctx.run_ui(r, |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    app.panes_ui(ui);
                });
            });
            output.textures_delta.clear();
        };
        let press = |pos: egui::Pos2| {
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() },
            ]
        };
        frame(&mut app, vec![]); // 先排一次版面，才知道每個窗格在哪
        frame(&mut app, vec![]);
        let rect_of = |app: &App, id| app.pane_rects.iter().find(|(i, _)| *i == id).map(|(_, r)| *r).unwrap();
        assert!(!app.preview_focused, "一開始焦點在編輯區");
        // 點預覽窗格
        let pv_rect = rect_of(&app, pv);
        frame(&mut app, press(pv_rect.center()));
        assert!(app.preview_focused, "預覽窗格取得輸入焦點");
        assert_eq!(app.layout.focus, editor_pane, "一般窗格的焦點（預覽跟著它）不變");
        frame(&mut app, vec![egui::Event::PointerButton { pos: pv_rect.center(), button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() }]);
        // 點編輯的窗格：焦點回來
        let ed_rect = rect_of(&app, editor_pane);
        frame(&mut app, press(ed_rect.center()));
        assert!(!app.preview_focused);
        // 換分頁也把焦點交還編輯區
        app.preview_focused = true;
        app.new_document();
        frame(&mut app, vec![]);
        assert!(!app.preview_focused);
        // 關掉預覽就沒有預覽焦點這回事
        app.preview_focused = true;
        app.close_preview(pv);
        assert!(!app.preview_focused);
    }

    #[test]
    fn tab_in_the_find_bar_only_switches_between_the_search_and_replace_fields() {
        let mut app = App::new(None, Settings::default());
        app.find.open = true;
        let ctx = egui::Context::default();
        let frame = |app: &mut App, events: Vec<egui::Event>| {
            let mut raw = egui::RawInput::default();
            raw.screen_rect = Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 600.0)));
            raw.events = events;
            let mut output = ctx.run_ui(raw, |ui| {
                egui::Panel::top("find").show(ui, |ui| {
                    app.find_bar(ui);
                });
                egui::CentralPanel::default().show(ui, |_| {});
            });
            output.textures_delta.clear();
        };
        let tab = |shift: bool| {
            vec![egui::Event::Key {
                key: egui::Key::Tab,
                physical_key: Some(egui::Key::Tab),
                pressed: true,
                repeat: false,
                modifiers: if shift { egui::Modifiers::SHIFT } else { egui::Modifiers::NONE },
            }]
        };
        let focus = |ctx: &egui::Context| {
            (
                ctx.memory(|m| m.has_focus(find_input_id())),
                ctx.memory(|m| m.has_focus(replace_input_id())),
            )
        };
        ctx.memory_mut(|m| m.request_focus(find_input_id()));
        frame(&mut app, vec![]);
        frame(&mut app, vec![]);
        assert_eq!(focus(&ctx), (true, false), "一開始在搜尋框");
        assert!(!app.find.show_replace);
        // 取代列收著：Tab 把它展開，並切到取代框
        frame(&mut app, tab(false));
        frame(&mut app, vec![]);
        assert!(app.find.show_replace);
        assert_eq!(focus(&ctx), (false, true));
        // 取代框 → 搜尋框
        frame(&mut app, tab(false));
        frame(&mut app, vec![]);
        assert_eq!(focus(&ctx), (true, false));
        // Shift+Tab 也一樣只在兩個框之間切換，不會跳到別的元件
        frame(&mut app, tab(true));
        frame(&mut app, vec![]);
        assert_eq!(focus(&ctx), (false, true));
        frame(&mut app, tab(true));
        frame(&mut app, vec![]);
        assert_eq!(focus(&ctx), (true, false));
        // 連按多次，焦點始終在這兩個框其中之一
        for _ in 0..7 {
            frame(&mut app, tab(false));
            frame(&mut app, vec![]);
            let (f, r) = focus(&ctx);
            assert!(f ^ r, "焦點跑到別的元件去了");
        }
    }

    #[test]
    fn icon_buttons_drawn_in_mouse_only_mode_are_not_reachable_with_tab() {
        let ctx = egui::Context::default();
        let mut raw = egui::RawInput::default();
        raw.screen_rect = Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(300.0, 200.0)));
        let mut output = ctx.run_ui(raw, |ui| {
            let normal = icons::small_button(ui, Icon::Close, "x", true);
            let mouse = icons::mouse_only(|| icons::small_button(ui, Icon::Close, "x", true));
            assert!(normal.sense.is_focusable(), "一般的按鈕可以用 Tab 聚焦");
            assert!(!mouse.sense.is_focusable(), "mouse_only 裡的按鈕不行");
            assert!(mouse.sense.senses_click(), "但還是可以點");
            let after = icons::small_button(ui, Icon::Close, "x", true);
            assert!(after.sense.is_focusable(), "離開 mouse_only 就恢復");
        });
        output.textures_delta.clear();
    }

    #[test]
    fn changing_tabs_hands_the_keyboard_focus_back_to_the_editor() {
        let (mut app, _dir) = app_with_two_files();
        app.pending_editor_focus = false;
        app.switch_tab(0);
        assert!(app.pending_editor_focus, "切換分頁");
        app.pending_editor_focus = false;
        app.new_document();
        assert!(app.pending_editor_focus, "新增分頁");
        app.pending_editor_focus = false;
        app.close_tab(app.tabs.active());
        assert!(app.pending_editor_focus, "關閉分頁");
    }

    #[test]
    fn panes_draw_without_panicking_and_each_pane_gets_its_own_tab_bar() {
        let (mut app, _dir) = app_with_two_files();
        let first = app.layout.focus;
        app.move_tab(first, 0, Target::Split(first, Side::Left));
        let ctx = egui::Context::default();
        let mut raw = egui::RawInput::default();
        raw.screen_rect = Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 700.0)));
        for _ in 0..3 {
            let mut output = ctx.run_ui(raw.clone(), |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    assert!(app.panes_ui(ui).is_none());
                });
            });
            output.textures_delta.clear();
        }
        assert_eq!(app.layout.pane_count(), 2);
        assert!(app.tab_drag.is_none());
    }

    #[test]
    fn new_document_state_is_reset() {
        let mut app = app_with("hello");
        app.editor.set_caret(&mut app.doc.buffer, 3, false);
        app.doc = Document::new();
        app.text_replaced();
        assert_eq!(app.editor.sel.head, 0);
        assert!(app.find.current.is_none());
    }

    #[test]
    fn opening_files_uses_tabs_reuses_blank_and_switches_to_duplicates() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.txt");
        let b = dir.path().join("b.txt");
        std::fs::write(&a, "aaa").unwrap();
        std::fs::write(&b, "bbb").unwrap();
        let mut app = App::new(None, Settings::default());
        app.open_path(&a);
        assert_eq!(app.tabs.len(), 1, "空白文件直接被用掉");
        app.open_path(&b);
        assert_eq!(app.tabs.len(), 2);
        assert_eq!(app.doc.text(), "bbb");
        app.open_path(&a);
        assert_eq!(app.tabs.len(), 2, "已經開著的檔案不重複開");
        assert_eq!(app.doc.text(), "aaa");
        app.new_document();
        assert_eq!(app.tabs.len(), 3);
        app.new_document();
        assert_eq!(app.tabs.len(), 3, "已經在空白文件上就不再多開");
    }

    #[test]
    fn dropped_files_open_in_tabs_and_a_dropped_folder_asks_which_files_to_open() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.txt");
        let b = dir.path().join("b.txt");
        std::fs::write(&a, "aaa").unwrap();
        std::fs::write(&b, "bbb").unwrap();
        let mut app = App::new(None, Settings::default());
        // 交談窗以拖進來的資料夾為起點；這裡假裝使用者在那個資料夾裡選了 b.txt
        app.pick_files_in = |dir| vec![dir.join("b.txt")];
        app.open_dropped(vec![
            a.clone(),
            dir.path().to_path_buf(), // 資料夾
            b.clone(),
            b.clone(), // 同一個檔案重複
        ]);
        assert_eq!(app.tabs.len(), 2, "a、b 各一個分頁（空白文件被 a 用掉）");
        assert_eq!(app.doc.text(), "bbb", "最後一個成為作用中分頁");
        let warns = app.log.iter().filter(|(kind, _)| *kind == LogKind::Warn).count();
        assert_eq!(warns, 0, "資料夾不再被略過");
        // 再拖一次已經開著的檔案：切過去，不重複開
        app.open_dropped(vec![a.clone()]);
        assert_eq!(app.tabs.len(), 2);
        assert_eq!(app.doc.text(), "aaa");
    }

    #[test]
    fn cancelling_the_dialog_for_a_dropped_folder_opens_nothing_and_it_is_asked_once_per_folder() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "aaa").unwrap();
        let mut app = App::new(None, Settings::default());
        app.pick_files_in = |_| Vec::new(); // 使用者按了取消
        app.open_dropped(vec![dir.path().to_path_buf()]);
        assert_eq!(app.tabs.len(), 1);
        assert!(app.doc.path.is_none(), "什麼都沒開");
        // 同一個資料夾拖兩次（同一批）只問一次：第二次回傳的檔案如果被開了兩次就會多出分頁
        app.pick_files_in = |dir| vec![dir.join("a.txt")];
        app.open_dropped(vec![dir.path().to_path_buf(), dir.path().to_path_buf()]);
        assert_eq!(app.tabs.len(), 1, "只開了一次（空白文件被用掉）");
        assert!(app.doc.path.is_some());
    }

    #[test]
    fn output_panel_toggles_and_peeks_open_for_errors_and_macro_output_only() {
        let mut app = App::new(None, Settings::default());
        assert!(!app.output_expanded(), "預設收合");
        app.push(LogKind::Info, "一般訊息");
        app.push(LogKind::Warn, "警告");
        assert!(!app.output_expanded(), "一般訊息與警告不會把面板彈開");
        app.push(LogKind::Error, "出錯了");
        assert!(app.output_expanded(), "錯誤讓面板暫時展開");
        assert!(!app.settings.show_output, "暫時展開不改設定");
        assert!(app.settings_dirty_since.is_none(), "也不用寫檔");
        // 暫時展開中按切換 = 收合，而且不留下設定變更
        app.toggle_output();
        assert!(!app.output_expanded());
        assert!(app.settings_dirty_since.is_none());
        // 使用者自己展開：寫進設定；之後來新訊息不影響
        app.toggle_output();
        assert!(app.output_expanded() && app.settings.show_output);
        assert!(app.settings_dirty_since.is_some());
        app.push(LogKind::Output, "巨集輸出");
        app.toggle_output();
        assert!(!app.output_expanded() && !app.settings.show_output, "再按一次收合，連暫時展開也一併清掉");
        app.push(LogKind::Output, "console.log");
        assert!(app.output_expanded(), "巨集輸出也會暫時展開");
    }

    #[test]
    fn shortcut_character_is_dropped_but_other_text_is_kept() {
        let mut ev = vec![
            egui::Event::Text("m".into()),
            egui::Event::Text("x".into()),
            egui::Event::Text("M".into()),
            egui::Event::Text("ab".into()),
        ];
        assert_eq!(drop_key_text(&mut ev, egui::Key::M), 2, "大小寫都要丟");
        assert_eq!(ev.len(), 2);
        let mut ev = vec![egui::Event::Text("1".into()), egui::Event::Text("-".into())];
        assert_eq!(drop_key_text(&mut ev, egui::Key::Num1), 1);
        assert_eq!(drop_key_text(&mut ev, egui::Key::Minus), 1);
        assert!(ev.is_empty());
        let mut ev = vec![egui::Event::Text("f6".into())];
        assert_eq!(drop_key_text(&mut ev, egui::Key::F6), 1, "名稱也比對");
    }

    #[test]
    fn recording_asks_for_a_name_and_empty_names_are_refused_without_saving() {
        let mut app = App::new(None, Settings::default());
        app.macros.clear(); // App::new 會讀這台機器真實的巨集資料夾，測試不能受它影響
        app.ask_recording_name("editor.type('x');".into(), None, None);
        let p = app.modal.as_ref().expect("要先問名稱");
        assert!(p.input.is_some(), "有文字框");
        assert_eq!(p.input.as_ref().unwrap().text, "錄製的巨集", "預設名稱");
        assert_eq!(p.spec.dismiss, None, "Esc 不能關掉，免得錄好的東西不小心丟掉");
        let ctx = egui::Context::default();
        let Pending { then, .. } = app.modal.take().unwrap();
        // 空白名稱：不存檔，換一個帶錯誤訊息的新對話框，來源碼還在
        app.resolve_modal(then, modal::Answer::Yes, Some("   ".into()), &ctx);
        let p = app.modal.as_ref().expect("重新詢問");
        assert!(p.spec.error.as_deref().unwrap().contains("空"));
        assert!(matches!(&p.then, Then::SaveRecording(s) if s == "editor.type('x');"));
        // 放棄：沒有對話框，也不存檔
        let Pending { then, .. } = app.modal.take().unwrap();
        app.resolve_modal(then, modal::Answer::No, None, &ctx);
        assert!(app.modal.is_none());
        assert!(app.log.iter().any(|(_, t)| t.contains("放棄")));
    }

    #[test]
    fn default_recording_name_skips_names_already_in_use() {
        let mut app = App::new(None, Settings::default());
        let entry = |name: &str| macro_lib::MacroEntry {
            path: PathBuf::from(format!("{name}.js")),
            name: name.to_owned(),
            shortcut: None,
        };
        app.macros.clear(); // 不受這台機器真實的巨集資料夾影響
        assert_eq!(app.unused_macro_name(), "錄製的巨集");
        app.macros = vec![entry("錄製的巨集"), entry("錄製的巨集 2")];
        assert_eq!(app.unused_macro_name(), "錄製的巨集 3");
    }

    #[test]
    fn renaming_a_macro_rewrites_its_name_and_empty_names_ask_again() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("m.js");
        std::fs::write(&path, "// @shortcut F6\nrun();\n").unwrap();
        let mut app = App::new(None, Settings::default());
        app.macros = vec![macro_lib::MacroEntry { path: path.clone(), name: "m".into(), shortcut: None }];
        let ctx = egui::Context::default();
        app.resolve_modal(Then::RenameMacro(0), modal::Answer::Yes, Some("  ".into()), &ctx);
        assert!(app.modal.as_ref().unwrap().spec.error.is_some(), "空名稱要重新詢問");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "// @shortcut F6\nrun();\n", "沒有動檔案");
        app.modal = None;
        app.resolve_modal(Then::RenameMacro(0), modal::Answer::Yes, Some(" 新名稱 ".into()), &ctx);
        assert!(app.modal.is_none());
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "// @name 新名稱\n// @shortcut F6\nrun();\n"
        );
        // 取消：什麼都不做
        app.resolve_modal(Then::RenameMacro(0), modal::Answer::Cancel, Some("別的".into()), &ctx);
        assert!(std::fs::read_to_string(&path).unwrap().contains("新名稱"));
    }

    #[test]
    fn renaming_a_tab_file_renames_it_on_disk_and_refuses_bad_or_taken_names() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.txt");
        let b = dir.path().join("b.txt");
        std::fs::write(&a, "aaa").unwrap();
        std::fs::write(&b, "bbb").unwrap();
        let mut app = App::new(None, Settings::default());
        app.open_path(&a);
        app.open_path(&b); // 作用中是 b
        let ctx = egui::Context::default();
        // 詢問時：預設是原檔名，只選主檔名
        app.ask_rename_file(None, None);
        let p = app.modal.as_ref().unwrap();
        assert_eq!(p.input.as_ref().unwrap().text, "b.txt");
        app.modal = None;
        // 別的分頁已經開著 a.txt：拒絕並重新詢問，帶著使用者打的字
        app.resolve_modal(Then::RenameFile, modal::Answer::Yes, Some("a.txt".into()), &ctx);
        let p = app.modal.as_ref().expect("重新詢問");
        assert!(p.spec.error.as_deref().unwrap().contains("a.txt"));
        assert_eq!(p.input.as_ref().unwrap().text, "a.txt");
        assert!(b.exists());
        app.modal = None;
        // 不合法的檔名
        app.resolve_modal(Then::RenameFile, modal::Answer::Yes, Some("x/y.txt".into()), &ctx);
        assert!(app.modal.as_ref().unwrap().spec.error.is_some());
        app.modal = None;
        // 成功
        app.resolve_modal(Then::RenameFile, modal::Answer::Yes, Some("c.md".into()), &ctx);
        assert!(app.modal.is_none());
        let c = dir.path().join("c.md");
        assert!(c.exists() && !b.exists());
        assert_eq!(app.doc.path.as_deref(), Some(c.as_path()));
        assert_eq!(app.doc.text(), "bbb");
        assert_eq!(app.tabs.len(), 2);
        // 取消：不動
        app.resolve_modal(Then::RenameFile, modal::Answer::Cancel, Some("zzz.txt".into()), &ctx);
        assert!(c.exists());
    }

    /// 測試用：App 的配色清單只用內建的，不受這台機器 themes 資料夾裡的檔案影響。
    fn app_with_builtin_schemes() -> App {
        let mut app = App::new(None, Settings::default());
        app.schemes = scheme::builtins();
        app
    }

    #[test]
    fn light_and_dark_use_their_own_scheme_and_unknown_names_fall_back() {
        use scheme::Role;
        let ctx = egui::Context::default();
        let mut app = app_with_builtin_schemes();
        app.settings.set_scheme_name(true, "Nord");
        app.settings.set_scheme_name(false, "GitHub Light");
        app.apply_schemes(&ctx);
        let by_name = |n: &str| scheme::builtins().into_iter().find(|s| s.name == n).unwrap();
        assert_eq!(ctx.style_of(egui::Theme::Dark).visuals.extreme_bg_color, by_name("Nord").get(Role::EditorBg));
        assert_eq!(ctx.style_of(egui::Theme::Light).visuals.panel_fill, by_name("GitHub Light").get(Role::PanelBg));
        // 只改深色：淺色不動
        let light_before = ctx.style_of(egui::Theme::Light).visuals.panel_fill;
        app.handle(Action::SetScheme(true, "Dracula".into()), &ctx);
        assert_eq!(app.settings.scheme_name(true), "Dracula");
        assert_eq!(ctx.style_of(egui::Theme::Dark).visuals.extreme_bg_color, by_name("Dracula").get(Role::EditorBg));
        assert_eq!(ctx.style_of(egui::Theme::Light).visuals.panel_fill, light_before);
        assert!(app.settings_dirty_since.is_some(), "選擇要寫進設定");
        // 設定檔裡的名稱不存在（例如自訂配色檔被刪了）：退回預設，不會壞掉
        app.settings.set_scheme_name(false, "已經被刪掉的配色");
        app.apply_schemes(&ctx);
        assert_eq!(ctx.style_of(egui::Theme::Light).visuals.panel_fill, by_name(scheme::DEFAULT_LIGHT).get(Role::PanelBg));
    }

    #[test]
    fn editing_a_scheme_previews_live_and_closing_restores_the_chosen_one() {
        use scheme::Role;
        let ctx = egui::Context::default();
        let mut app = app_with_builtin_schemes();
        app.settings.set_scheme_name(true, "Dracula");
        let mut edit = scheme::pick(&app.schemes, true, "Dracula").clone();
        edit.set(Role::EditorBg, egui::Color32::from_rgb(9, 8, 7));
        app.scheme_edit = Some(edit);
        app.apply_schemes(&ctx);
        assert_eq!(ctx.style_of(egui::Theme::Dark).visuals.extreme_bg_color, egui::Color32::from_rgb(9, 8, 7), "編輯中的顏色立刻預覽");
        // 沒按儲存就放掉：回到清單裡選的那一套
        app.scheme_edit = None;
        app.apply_schemes(&ctx);
        let dracula = scheme::builtins().into_iter().find(|s| s.name == "Dracula").unwrap();
        assert_eq!(ctx.style_of(egui::Theme::Dark).visuals.extreme_bg_color, dracula.get(Role::EditorBg));
        // 編輯中的是深色配色，淺色那一邊不受影響
        let light = ctx.style_of(egui::Theme::Light).visuals.panel_fill;
        let mut edit = dracula.clone();
        edit.set(Role::PanelBg, egui::Color32::from_rgb(1, 1, 1));
        app.scheme_edit = Some(edit);
        app.apply_schemes(&ctx);
        assert_eq!(ctx.style_of(egui::Theme::Light).visuals.panel_fill, light);
    }

    #[test]
    fn saving_a_scheme_as_a_custom_one_validates_the_name_and_selects_it() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with_builtin_schemes();
        // 沒有正在編輯的配色
        assert!(app.save_scheme_as(dir.path(), "X").is_err());
        let mut edit = scheme::pick(&app.schemes, true, "Dracula").clone();
        edit.set(scheme::Role::SynKeyword, egui::Color32::from_rgb(1, 2, 3));
        app.scheme_edit = Some(edit);
        // 空的、與內建同名：拒絕，編輯中的還在
        assert!(app.save_scheme_as(dir.path(), "  ").unwrap_err().contains("空"));
        assert!(app.save_scheme_as(dir.path(), "Nord").unwrap_err().contains("內建"));
        assert!(app.scheme_edit.is_some());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0, "拒絕時不寫檔");
        // 成功：寫出檔案、設為深色主題的配色、結束編輯
        app.save_scheme_as(dir.path(), "我的 Dracula").unwrap();
        assert!(app.scheme_edit.is_none());
        assert_eq!(app.settings.scheme_name(true), "我的 Dracula");
        assert_eq!(app.settings.scheme_name(false), "", "淺色不受影響");
        let (custom, messages) = scheme::load_custom(dir.path());
        assert!(messages.is_empty());
        assert_eq!(custom.len(), 1);
        assert_eq!(custom[0].name, "我的 Dracula");
        assert!(custom[0].dark);
    }

    #[test]
    fn save_as_never_overwrites_another_custom_scheme() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with_builtin_schemes();
        let mut mine = scheme::default_for(true);
        mine.name = "甲".into();
        mine.builtin = false;
        let path = scheme::save_custom(dir.path(), &mine).unwrap();
        mine.path = Some(path.clone());
        app.schemes = scheme::combined(&[mine.clone()]);
        // 正在編輯別的配色，想另存成「甲」：已經有了，拒絕
        app.scheme_edit = Some(scheme::pick(&app.schemes, true, "Dracula").clone());
        assert!(app.save_scheme_as(dir.path(), "甲").unwrap_err().contains("已經有"));
        // 正在編輯「甲」本身再存成「甲」：寫回原檔
        app.scheme_edit = Some(mine.clone());
        let before = std::fs::read_dir(dir.path()).unwrap().count();
        app.save_scheme_as(dir.path(), "甲").unwrap();
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), before, "沒有多出新檔案");
    }

    #[test]
    fn dropped_files_are_ignored_while_a_dialog_is_waiting() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.txt");
        std::fs::write(&a, "aaa").unwrap();
        let mut app = App::new(None, Settings::default());
        app.modal = Some(Pending::new(
            modal::Spec::new("t", "m").button("好", modal::Answer::Yes),
            Then::External,
        ));
        app.open_dropped(vec![a]);
        assert_eq!(app.tabs.len(), 1);
        assert_eq!(app.doc.text(), "", "對話框還沒回答，不開檔");
    }

    #[test]
    fn switching_tabs_keeps_edits_and_clears_search_state() {
        let mut app = app_with("cat dog cat");
        app.find.query = "cat".to_owned();
        app.find_step(false);
        assert!(app.find.current.is_some());
        app.doc.buffer.insert(0, "x");
        app.doc.sync();
        app.new_document();
        assert!(app.find.current.is_none(), "搜尋結果屬於原本的分頁");
        assert_eq!(app.doc.text(), "");
        app.switch_tab(0);
        assert_eq!(app.doc.text(), "xcat dog cat");
        assert!(app.doc.dirty, "未儲存狀態跟著分頁");
        assert_eq!(app.tabs.dirty_tabs(&app.doc), vec![0]);
    }

    fn later(path: &std::path::Path) {
        let t = std::time::SystemTime::now() + std::time::Duration::from_secs(5);
        std::fs::File::options().write(true).open(path).unwrap().set_modified(t).unwrap();
    }

    #[test]
    fn external_change_reloads_a_clean_document_and_keeps_the_caret() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.txt");
        std::fs::write(&p, "hello world").unwrap();
        let mut app = App::new(None, Settings::default());
        app.open_path(&p);
        app.editor.set_caret(&mut app.doc.buffer, 5, false);
        std::fs::write(&p, "hello brave new world").unwrap();
        later(&p);
        app.handle_external_change(|_| panic!("沒有未儲存的修改，不該詢問"));
        assert_eq!(app.doc.text(), "hello brave new world");
        assert!(!app.doc.dirty);
        assert_eq!(app.editor.sel.head, 5);
        // 已經是最新版：再檢查一次什麼都不做
        app.handle_external_change(|_| panic!("不該詢問"));
    }

    #[test]
    fn external_change_with_unsaved_edits_asks_and_remembers_the_answer() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.txt");
        std::fs::write(&p, "disk v1").unwrap();
        let mut app = App::new(None, Settings::default());
        app.open_path(&p);
        app.doc.buffer.insert(0, "mine ");
        app.doc.sync();
        std::fs::write(&p, "disk v2").unwrap();
        later(&p);
        let mut asked = 0;
        app.handle_external_change(|_| {
            asked += 1;
            false
        });
        assert_eq!(asked, 1);
        assert_eq!(app.doc.text(), "mine disk v1", "選擇保留就不動編輯器的內容");
        assert!(app.doc.dirty);
        app.handle_external_change(|_| panic!("同一個版本不要一直問"));
        // 之後又變更一次，這次選擇重新載入
        std::fs::write(&p, "disk v3").unwrap();
        std::fs::File::options()
            .write(true)
            .open(&p)
            .unwrap()
            .set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(10))
            .unwrap();
        app.handle_external_change(|_| true);
        assert_eq!(app.doc.text(), "disk v3");
        assert!(!app.doc.dirty);
    }

    #[test]
    fn background_tabs_in_every_pane_follow_external_changes_and_deletions() {
        use crate::document::DiskState;
        let dir = tempfile::tempdir().unwrap();
        let [a, b, c] = ["a.txt", "b.txt", "c.txt"].map(|n| {
            let p = dir.path().join(n);
            std::fs::write(&p, format!("{n} v1")).unwrap();
            p
        });
        let mut app = App::new(None, Settings::default());
        app.open_path(&a);
        app.open_path(&b);
        app.open_path(&c); // 作用中是 c；a、b 是同一個窗格的背景分頁
        // 把 a 搬到新窗格，讓「其他窗格」也有一個背景分頁
        let focus = app.layout.focus;
        app.move_tab(focus, 0, pane_ops::Target::Split(focus, panes::Side::Right));
        let other = *app.layout.others.keys().next().expect("應該有另一個窗格");
        app.focus_pane(other);
        app.focus_pane(focus);
        let text_of = |app: &mut App, name: &str| -> Option<(String, DiskState)> {
            let mut found = None;
            let sets = std::iter::once(&mut app.tabs).chain(app.layout.others.values_mut());
            for (n, set) in sets.enumerate() {
                for tab in set.background_mut(n == 0) {
                    if tab.doc.name() == name {
                        found = Some((tab.doc.text().to_owned(), tab.doc.disk_state));
                    }
                }
            }
            found
        };
        let bump = |p: &std::path::Path, text: &str, secs: u64| {
            std::fs::write(p, text).unwrap();
            let t = std::time::SystemTime::now() + std::time::Duration::from_secs(secs);
            std::fs::File::options().write(true).open(p).unwrap().set_modified(t).unwrap();
        };
        // 乾淨的背景分頁：自動重新載入（兩個窗格都要）
        bump(&a, "a.txt v2", 5);
        bump(&b, "b.txt v2", 5);
        app.check_background_docs();
        assert_eq!(text_of(&mut app, "a.txt").unwrap().0, "a.txt v2", "其他窗格的背景分頁");
        assert_eq!(text_of(&mut app, "b.txt").unwrap().0, "b.txt v2", "同窗格的背景分頁");
        // 有未儲存修改的背景分頁：不動內容，只標記，而且只提醒一次
        {
            let tab = app.layout.others.values_mut().flat_map(|s| s.background_mut(false)).find(|t| t.doc.name() == "a.txt").unwrap();
            tab.doc.buffer.insert(0, "mine ");
            tab.doc.sync();
        }
        let logs_before = app.log.len();
        bump(&a, "a.txt v3", 10);
        app.check_background_docs();
        app.check_background_docs();
        let (text, state) = text_of(&mut app, "a.txt").unwrap();
        assert_eq!(text, "mine a.txt v2");
        assert_eq!(state, DiskState::Stale);
        assert_eq!(app.log.len(), logs_before + 1, "只提醒一次");
        // 被刪除：標記，內容還在
        std::fs::remove_file(&b).unwrap();
        app.check_background_docs();
        assert_eq!(text_of(&mut app, "b.txt").unwrap(), ("b.txt v2".to_owned(), DiskState::Missing));
        // 作用中的分頁被刪除也要標記
        std::fs::remove_file(&c).unwrap();
        app.check_external_changes();
        assert_eq!(app.doc.disk_state, DiskState::Missing);
        // 標籤上看得到
        let id = app.layout.focus;
        let active = app.tabs.active();
        assert!(app.pane_tab_text(id, active).contains(t(" （已刪除）")));
    }

    #[test]
    fn terminals_are_tabs_that_open_in_their_own_pane_and_can_join_any_tab_bar() {
        let mut app = App::new(None, Settings::default());
        app.new_terminal();
        assert_eq!(app.layout.pane_count(), 2, "新終端機在自己的窗格");
        assert!(app.doc.terminal.is_some(), "焦點在新的終端機分頁");
        assert_eq!(app.doc.name(), "終端機 1");
        let term_pane = app.layout.focus;
        let editor_pane = app.layout.root.leaves()[0];
        assert_ne!(term_pane, editor_pane);
        assert_eq!(app.terminal_tabs(), vec![(term_pane, 0)]);
        // 終端機分頁不是「空白文件」：開檔案不會把它吃掉
        assert!(!app.is_pristine());
        // 把終端機分頁拖進編輯窗格的標籤列：兩個分頁在同一個窗格，終端機窗格消失
        app.move_tab(term_pane, 0, Target::Into(editor_pane));
        assert_eq!(app.layout.pane_count(), 1);
        assert_eq!(app.tabs.len(), 2);
        assert!(app.doc.terminal.is_some(), "搬過去之後它是作用中的分頁");
        // 切回文字分頁、再切回終端機：shell 還活著（同一個 TerminalPane）
        app.switch_tab(0);
        assert!(app.doc.terminal.is_none());
        app.switch_tab(1);
        assert_eq!(app.doc.terminal.as_ref().map(|t| t.number()), Some(1));
        // 再開一個：編號 2，在新窗格；兩個終端機各自獨立
        app.new_terminal();
        assert_eq!(app.doc.name(), "終端機 2");
        assert_eq!(app.terminal_tabs().len(), 2);
        // 拖到側邊：切成新窗格，關掉終端機分頁就結束那個 shell
        let focus = app.layout.focus;
        app.move_tab(focus, 0, Target::Split(editor_pane, Side::Right));
        assert!(app.layout.pane_count() >= 2);
        // 工作階段裡只有文字文件，沒有終端機
        assert_eq!(app.session_all().file_count(), 0);
        // 關掉兩個終端機分頁
        for _ in 0..4 {
            if let Some((pane, i)) = app.terminal_tabs().first().copied() {
                app.focus_pane(pane);
                app.switch_tab(i);
                app.close_tab(i);
            }
        }
        assert!(app.terminal_tabs().is_empty());
    }
    #[test]
    fn double_clicking_a_terminal_tab_renames_it_and_an_empty_name_restores_the_default() {
        let mut app = App::new(None, Settings::default());
        let ctx = egui::Context::default();
        app.new_terminal();
        assert_eq!(app.doc.name(), "終端機 1");
        app.start_rename_tab(app.tabs.active());
        assert!(matches!(app.modal.as_ref().map(|m| &m.then), Some(&Then::RenameTerminal)), "要問新名字，不是另存新檔");
        app.modal = None;
        app.resolve_modal(Then::RenameTerminal, modal::Answer::Yes, Some("  伺服器  ".into()), &ctx);
        assert_eq!(app.doc.name(), "伺服器", "前後空白去掉");
        assert_eq!(app.pane_tab_text(app.layout.focus, app.tabs.active()), "伺服器");
        app.resolve_modal(Then::RenameTerminal, modal::Answer::Cancel, Some("別的".into()), &ctx);
        assert_eq!(app.doc.name(), "伺服器", "取消不改");
        app.resolve_modal(Then::RenameTerminal, modal::Answer::Yes, Some("   ".into()), &ctx);
        assert_eq!(app.doc.name(), "終端機 1", "留空改回預設");
        // 取了名字之後，新的終端機仍然拿到最小的空號（編號和名字無關）
        app.resolve_modal(Then::RenameTerminal, modal::Answer::Yes, Some("終端機 1".into()), &ctx);
        app.new_terminal();
        assert_eq!(app.doc.name(), "終端機 2");
    }

    #[test]
    fn terminal_tabs_are_remembered_with_their_names_and_positions_and_come_back_as_new_shells() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (dir.path().join("a.txt"), dir.path().join("b.txt"));
        std::fs::write(&a, "aaa").unwrap();
        std::fs::write(&b, "bbb").unwrap();
        let mut app = App::new(None, Settings::default());
        app.open_path(&a);
        app.open_path(&b);
        let editor_pane = app.layout.focus;
        // 兩個終端機：「終端機 1」，和一個自己取名的「伺服器」，都拖進編輯窗格；位置是 a、終端機 1、b、伺服器
        app.new_terminal();
        app.new_terminal();
        let ctx = egui::Context::default();
        app.resolve_modal(Then::RenameTerminal, modal::Answer::Yes, Some("伺服器".into()), &ctx);
        let second = app.layout.focus;
        app.move_tab(second, 0, Target::Into(editor_pane));
        let first = app.terminal_tabs().into_iter().find(|(p, _)| *p != editor_pane).expect("還有另一個終端機");
        app.move_tab(first.0, first.1, Target::Into(editor_pane));
        let names: Vec<String> = (0..app.tabs.len()).map(|i| app.pane_label(editor_pane, i).0).collect();
        assert_eq!(app.tabs.len(), 4, "{names:?}");

        let saved = session::Session::parse(&app.session_all().to_text());
        assert_eq!(saved.panes.len(), 1);
        assert_eq!(saved.panes[0].files.len(), 2);
        assert_eq!(saved.panes[0].terminals.len(), 2);

        // 重新執行：同樣的檔案與終端機分頁、同樣的名字與順序，每個終端機是新開的 shell
        let mut fresh = App::new(None, Settings::default());
        fresh.restore_from(&saved);
        assert_eq!(fresh.tabs.len(), 4);
        let order = |app: &App| -> Vec<String> {
            let pane = app.layout.focus;
            (0..app.tabs.len()).map(|i| app.pane_label(pane, i).0).collect()
        };
        assert_eq!(order(&fresh), order(&app));
        assert_eq!(fresh.terminal_tabs().len(), 2);
        assert_eq!(fresh.tabs.active(), app.tabs.active(), "作用中的分頁也一樣");
        // 存檔再讀回來是同一份（還原沒有改變記錄的內容）
        assert_eq!(session::Session::parse(&fresh.session_all().to_text()), saved);
    }

    #[test]
    fn a_pane_with_only_a_terminal_comes_back_and_a_missing_file_does_not_block_the_terminal() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.txt");
        std::fs::write(&a, "aaa").unwrap();
        let mut app = App::new(None, Settings::default());
        app.open_path(&a);
        app.new_terminal();
        assert_eq!(app.layout.pane_count(), 2);
        let saved = session::Session::parse(&app.session_all().to_text());
        // 檔案被刪掉之後：終端機窗格還是會回來
        std::fs::remove_file(&a).unwrap();
        let mut fresh = App::new(None, Settings::default());
        fresh.restore_from(&saved);
        assert_eq!(fresh.terminal_tabs().len(), 1, "終端機還原了");
        assert_eq!(fresh.doc.name(), "終端機 1");
    }

    #[test]
    fn a_preview_link_to_a_local_file_opens_it_in_a_new_tab_and_a_missing_one_is_reported() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (dir.path().join("a.md"), dir.path().join("b.md"));
        std::fs::write(&a, "[b](b.md)").unwrap();
        std::fs::write(&b, "# bee").unwrap();
        let mut app = App::new(None, Settings::default());
        let ctx = egui::Context::default();
        app.open_path(&a);
        assert_eq!(app.tabs.len(), 1);
        app.handle(Action::OpenLocalFile(b.clone()), &ctx);
        assert_eq!(app.tabs.len(), 2, "新分頁");
        assert_eq!(app.doc.name(), "b.md");
        // 再點一次：不重複開，切到它
        app.switch_tab(0);
        app.handle(Action::OpenLocalFile(b.clone()), &ctx);
        assert_eq!((app.tabs.len(), app.doc.name().as_str()), (2, "b.md"));
        // 不存在：不開分頁，輸出面板有警告
        let logs = app.log.len();
        app.handle(Action::OpenLocalFile(dir.path().join("missing.md")), &ctx);
        assert_eq!(app.tabs.len(), 2);
        assert_eq!(app.log.len(), logs + 1);
    }

    #[test]
    fn closing_a_clean_tab_removes_it() {
        let mut app = app_with("one");
        app.new_document();
        app.doc.buffer.insert(0, "two");
        app.doc.mark_synced();
        assert_eq!(app.tabs.len(), 2);
        app.close_tab(1);
        assert_eq!(app.tabs.len(), 1);
        assert_eq!(app.doc.text(), "one");
    }
}
fn main() -> eframe::Result {
    // 這台機器的 Vulkan 後端會崩潰，Windows 上預設改用 DX12（使用者仍可自行覆寫）
    #[cfg(windows)]
    if std::env::var_os("WGPU_BACKEND").is_none() {
        // SAFETY: 在啟動任何執行緒之前、main 開頭設定環境變數
        unsafe { std::env::set_var("WGPU_BACKEND", "dx12") };
    }

    let settings = Settings::load();
    feditor_core::i18n::apply(settings.language); // 之後產生的所有介面字串都用這個語言
    let icon = eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png"))
        .expect("內建圖示 assets/icon.png 損毀");
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_icon(icon),
        ..Default::default()
    };

    eframe::run_native(
        "FEditor",
        options,
        Box::new(move |cc| {
            install_fonts(&cc.egui_ctx, settings.font_family.as_deref());
            egui_extras::install_image_loaders(&cc.egui_ctx); // 預覽 SVG 與 Markdown 圖片用
            cc.egui_ctx.set_theme(settings.theme);
            // 關掉 egui 內建的「Ctrl ＋／－ 縮放整個介面」：我們的 Ctrl ＋／－ 是調整編輯區字級
            cc.egui_ctx.options_mut(|o| o.zoom_with_keyboard = false);
            apply_font_size(&cc.egui_ctx, settings.font_size);
            // 第一個命令列參數視為要開啟的檔案
            let initial = std::env::args_os().nth(1).map(PathBuf::from);
            let restore = initial.is_none();
            // 先載入自訂語言，這樣還原的分頁一開始就有顏色
            let syntax_msgs = syntax::load_user_languages(syntax::syntax_dir().as_deref());
            let mut app = App::new(initial, settings);
            for m in syntax_msgs {
                app.push(LogKind::Warn, m);
            }
            app.apply_schemes(&cc.egui_ctx); // 淺色、深色各套上自己選的配色
            // 不帶檔名啟動：把上次開著的檔案開回來。之後才開始記錄，免得還原到一半就覆蓋掉
            if restore {
                app.restore_session();
            }
            app.session_enabled = true;
            app.save_session();
            app.start_font_scan(cc.egui_ctx.clone());
            Ok(Box::new(app))
        }),
    )
}
