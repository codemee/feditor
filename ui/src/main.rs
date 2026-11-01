// FEditor UI：以 egui 自繪。編輯區是自繪的（見 editor 模組），不用 egui 的 TextEdit。
// 操作以工具列按鈕為主，不使用功能表。
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
mod session;
mod settings;
mod syntax;
mod tabs;
mod theme;

use document::{Document, SaveDocError};
use eframe::egui;
use egui::text::CCursor;
use egui::text_selection::CCursorRange;
use encoding_rs::{Encoding, UTF_8, UTF_16BE, UTF_16LE};
use editor::model::EditorState;
use editor::view::EditorView;
use feditor_core::fileio::{LineEnding, common_encodings};
use find::{FindState, Summary};
use icons::Icon;
use settings::{
    DEFAULT_FONT_SIZE, MAX_FONT_SIZE, MIN_FONT_SIZE, Settings, ThemeChoice, adjust_font_size,
    clamp_font_size, zoom_steps,
};
use std::ops::Range;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn editor_id() -> egui::Id {
    egui::Id::unique("editor")
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

/// 配色狀態鈕目前要顯示的圖示。
fn theme_icon(choice: ThemeChoice) -> Icon {
    match choice {
        ThemeChoice::System => Icon::ThemeSystem,
        ThemeChoice::Light => Icon::ThemeLight,
        ThemeChoice::Dark => Icon::ThemeDark,
    }
}

fn theme_label(choice: ThemeChoice) -> &'static str {
    match choice {
        ThemeChoice::System => "跟隨系統",
        ThemeChoice::Light => "淺色",
        ThemeChoice::Dark => "深色",
    }
}

/// 確認對話框回答之後要接著做的事。
enum Then {
    CloseTab,
    CloseWindow,
    Reload(&'static Encoding),
    DeleteMacro(usize),
    External,
}

struct Pending {
    spec: modal::Spec,
    then: Then,
}

/// 圖示按鈕之間的間距（像素）。
const BUTTON_GAP: f32 = 4.0;

/// 把這一列的元件間距縮成 [`BUTTON_GAP`]。放圖示按鈕的橫列開頭呼叫。
fn tight_buttons(ui: &mut egui::Ui) {
    ui.spacing_mut().item_spacing.x = BUTTON_GAP;
}

/// 這一區的元件 hover／按下／展開時不畫外框，只保留底色。
fn no_hover_outline(ui: &mut egui::Ui) {
    let w = &mut ui.visuals_mut().widgets;
    for s in [&mut w.hovered, &mut w.active, &mut w.open] {
        s.bg_stroke = egui::Stroke::NONE;
        s.expansion = 0.0; // hover 時底色不要比原本的範圍大一圈
    }
}

/// 下拉清單裡面（選項）的樣式：選項 hover 時不畫外框，並在右邊留出捲軸的位置，
/// 這樣捲軸不會壓在選項（特別是被選取的那一項）的底色上。放在 `show_ui` 的開頭呼叫。
fn flat_list(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    no_hover_outline(ui);
    egui::Frame::NONE
        .inner_margin(egui::Margin { right: 14, ..Default::default() })
        .show(ui, add);
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
    ui.strong("規則表達式（Rust regex 語法）");
    ui.add_space(2.0);
    egui::Grid::new("regex_help")
        .num_columns(2)
        .spacing([14.0, 2.0])
        .show(ui, |ui| {
            for (syntax, meaning) in [
                (".", "任意字元（不含換行）"),
                ("\\d  \\w  \\s", "數字、字母數字底線、空白"),
                ("^  $", "行首、行尾（以「行」為單位）"),
                ("[abc]  [^abc]", "其中之一、排除這些"),
                ("*  +  ?", "重複 0 次以上、1 次以上、0 或 1 次"),
                ("{2,5}", "重複 2 到 5 次（後面加 ? 為非貪婪）"),
                ("(…)  |", "群組（取代時用 $1）、或"),
                ("\\.  \\(", "跳脫特殊字元"),
                ("(?i)", "不分大小寫"),
            ] {
                ui.label(egui::RichText::new(syntax).monospace());
                ui.label(meaning);
                ui.end_row();
            }
        });
    ui.add_space(4.0);
    ui.label("取代字串：$1 代入群組，\\n 換行，\\t 跳位，$$ 為字面的 $");
    ui.add_space(4.0);
    ui.hyperlink_to(
        "完整語法說明（docs.rs/regex）",
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
    /// 設定編輯區的字級（點）。設定面板的字級欄與快捷鍵都走這一條，所以兩邊永遠同步。
    SetFontSize(f32),
    /// 設定編輯區的字型家族；`None` 表示作業系統預設的等寬字型。
    SetFontFamily(Option<String>),
    /// 編輯區的顯示選項（空白字元、行號、目前行、自動換行）。
    SetView(settings::ViewOptions),
    SwitchTab(usize),
    CloseTab(usize),
    NextTab,
    PrevTab,
    /// 展開或收起巨集面板。
    ToggleMacroMenu,
    RunMacroEntry(usize),
    EditMacro(usize),
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
    /// 目前作用中分頁的文件與編輯狀態；其他分頁放在 	abs 裡（見 	abs.rs）。
    doc: Document,
    tabs: tabs::TabSet,
    last_macro: Option<PathBuf>,
    log: Vec<(LogKind, String)>,
    last_title: String,
    allow_close: bool,
    /// 目前顯示中的確認對話框，以及使用者回答之後要接著做的事。
    modal: Option<Pending>,
    /// 關閉視窗時，使用者已經選了「不儲存」的分頁（索引）。
    close_skipped: Vec<usize>,
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
}

impl App {
    fn new(initial: Option<PathBuf>, settings: Settings) -> Self {
        let mut app = Self {
            doc: Document::new(),
            tabs: tabs::TabSet::new(),
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
            settings_anchor: None,
            macros: Vec::new(),
            macro_open: false,
            macro_anchor: None,
            capturing_shortcut: None,
            folder: folder_search::FolderSearch::default(),
            session_enabled: false,
            last_disk_check: Instant::now(),
            was_focused: true,
        };
        app.refresh_macros();
        if let Some(path) = initial {
            app.open_path(&path);
        }
        app
    }

    fn push(&mut self, kind: LogKind, text: impl Into<String>) {
        self.log.push((kind, text.into()));
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
                self.push(LogKind::Warn, format!("無法儲存設定：{e}"));
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
        let editor_focused = ctx.memory(|m| m.has_focus(editor_id()));
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
        ctx.memory_mut(|m| m.request_focus(editor_id()));
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
            Ok(0) => self.push(LogKind::Info, "沒有可取代的內容"),
            Ok(n) => {
                self.doc.sync();
                // 整份文字都換過了，原本的選取範圍已經沒有意義：收成游標
                let head = self.editor.sel.head;
                self.editor.set_caret(&mut self.doc.buffer, head, false);
                self.push(LogKind::Info, format!("已取代 {n} 筆"));
            }
            // 搜尋字串是空的或規則有誤：交給搜尋流程去顯示錯誤
            Err(_) => self.find_changed(),
        }
    }
    // ── 檔案 ────────────────────────────────────────────────

    /// 對作用中的文件顯示「要儲存嗎？」對話框；答案由 [`App::resolve_modal`] 處理。
    fn ask_save(&mut self, then: Then, before: &str) {
        let spec = modal::Spec::new(
            "尚未儲存",
            format!("「{}」有尚未儲存的修改。要在{}之前儲存嗎？", self.doc.name(), before),
        )
        .button("儲存", modal::Answer::Yes)
        .button("不儲存", modal::Answer::No)
        .button("取消", modal::Answer::Cancel);
        self.modal = Some(Pending { spec, then });
    }

    /// 使用者回答了確認對話框：接著做原本被擋住的事。
    fn resolve_modal(&mut self, then: Then, answer: modal::Answer, ctx: &egui::Context) {
        use modal::Answer::{No, Yes};
        match then {
            Then::CloseTab => match answer {
                Yes if self.save() => self.close_active_tab(),
                No => self.close_active_tab(),
                _ => {}
            },
            Then::CloseWindow => {
                match answer {
                    Yes if self.save() => {}
                    No => self.close_skipped.push(self.tabs.active()),
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
        let next = self
            .tabs
            .dirty_tabs(&self.doc)
            .into_iter()
            .find(|i| !self.close_skipped.contains(i));
        match next {
            Some(i) => {
                self.switch_tab(i);
                self.ask_save(Then::CloseWindow, "關閉");
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
        self.doc.path.is_none() && !self.doc.dirty && self.doc.buffer.is_empty()
    }

    /// 分頁換了：搜尋結果屬於上一個分頁，作廢。
    fn tab_changed(&mut self) {
        self.find.invalidate();
        self.find.reveal = false;
        self.save_session();
    }

    // ── 工作階段與外部變更 ──────────────────────────────────

    fn save_session(&self) {
        if self.session_enabled {
            self.tabs.session(&self.doc, &self.editor).save();
        }
    }

    /// 把上次開著的檔案開回來（游標也放回原位）。
    fn restore_session(&mut self) {
        let session = session::Session::load();
        let mut active_tab = None;
        for (i, (path, caret)) in session.files.iter().enumerate() {
            if !path.exists() {
                continue;
            }
            self.open_path(path);
            let n = self.doc.buffer.len_chars();
            self.editor.sel = editor::model::Selection::collapsed((*caret).min(n));
            self.editor.scroll_to_caret = true;
            if i == session.active {
                active_tab = Some(self.tabs.active());
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
            self.push(LogKind::Warn, format!("{name} 已被其他程式修改，目前保留編輯器裡的版本"));
            return;
        }
        let sel = self.editor.sel;
        match self.doc.reload_with(self.doc.format.encoding) {
            Ok(warnings) => {
                self.find.invalidate();
                self.editor.sel = sel;
                self.editor.extra.clear();
                self.editor.clamp(&self.doc.buffer);
                self.push(LogKind::Info, format!("{name} 已被其他程式修改，已自動重新載入"));
                for w in warnings {
                    self.push(LogKind::Warn, w);
                }
            }
            Err(e) => {
                self.doc.accept_disk_version();
                self.push(LogKind::Error, format!("無法重新載入 {name}：{e}"));
            }
        }
    }

    fn check_external_changes(&mut self) {
        if self.modal.is_some() {
            return;
        }
        if self.doc.dirty && self.doc.changed_on_disk() {
            let spec = modal::Spec::new(
                "檔案已被修改",
                format!(
                    "「{}」已被其他程式修改，而且你在編輯器裡也有尚未儲存的修改。要重新載入嗎？（會捨棄你的修改）",
                    self.doc.name()
                ),
            )
            .button("重新載入", modal::Answer::Yes)
            .button("保留我的版本", modal::Answer::No);
            self.modal = Some(Pending { spec, then: Then::External });
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
            self.ask_save(Then::CloseTab, "關閉分頁");
        } else {
            self.close_active_tab();
        }
    }

    fn close_active_tab(&mut self) {
        let i = self.tabs.active();
        self.tabs.close(&mut self.doc, &mut self.editor, i);
        self.tab_changed();
    }

    fn open(&mut self) {
        if let Some(paths) = rfd::FileDialog::new().set_title("開啟檔案").pick_files() {
            for path in paths {
                self.open_path(&path);
            }
        }
    }

    /// 開啟指定路徑：已經開著就切過去，否則開在新分頁（作用中是空白文件時直接用它）。
    fn open_path(&mut self, path: &std::path::Path) {
        if let Some(i) = self.tabs.find_path(&self.doc, path) {
            self.switch_tab(i);
            self.push(LogKind::Info, format!("{} 已經開啟，切換到該分頁", self.doc.name()));
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
                    format!(
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
            Err(e) => self.push(LogKind::Error, format!("無法開啟 {}：{e}", path.display())),
        }
    }
    /// 回傳是否成功儲存。
    fn save(&mut self) -> bool {
        match self.doc.save() {
            Ok(()) => {
                self.push(LogKind::Info, format!("已儲存 {}", self.doc.name()));
                true
            }
            Err(SaveDocError::NoPath) => self.save_as(),
            Err(e) => {
                self.push(LogKind::Error, format!("儲存失敗：{e}"));
                false
            }
        }
    }

    fn save_as(&mut self) -> bool {
        let Some(path) = rfd::FileDialog::new()
            .set_title("另存新檔")
            .set_file_name(self.doc.name())
            .save_file()
        else {
            return false;
        };
        match self.doc.save_as(path) {
            Ok(()) => {
                self.push(LogKind::Info, format!("已儲存 {}", self.doc.name()));
                true
            }
            Err(e) => {
                self.push(LogKind::Error, format!("儲存失敗：{e}"));
                false
            }
        }
    }

    fn reload(&mut self, encoding: &'static Encoding) {
        if self.doc.dirty {
            let spec = modal::Spec::new("重新載入", "重新載入會捨棄尚未儲存的修改，確定嗎？")
                .button("重新載入", modal::Answer::Yes)
                .button("取消", modal::Answer::Cancel);
            self.modal = Some(Pending { spec, then: Then::Reload(encoding) });
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
                    format!("已以 {} 重新載入", self.doc.format.encoding_label()),
                );
                for w in warnings {
                    self.push(LogKind::Warn, w);
                }
            }
            Err(e) => self.push(LogKind::Error, format!("重新載入失敗：{e}")),
        }
    }

    // ── 巨集 ────────────────────────────────────────────────

    fn pick_and_run_macro(&mut self) {
        let picked = rfd::FileDialog::new()
            .set_title("選擇要執行的 JavaScript 巨集")
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
                self.push(LogKind::Error, format!("無法讀取 {name}：{e}"));
                return;
            }
        };

        self.push(LogKind::Info, format!("執行 {name}"));
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
                self.push(LogKind::Info, "完成");
            }
            Some(e) => {
                self.push(LogKind::Error, e);
                if report.rolled_back {
                    // 修改已經全部退回，文字與執行前相同，不算有未儲存的修改
                    self.doc.mark_synced();
                    self.push(LogKind::Info, "巨集失敗，已還原所有修改");
                }
            }
        }
    }
    // ── 動作分派 ────────────────────────────────────────────

    fn handle(&mut self, action: Action, ctx: &egui::Context) {
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
            Action::Reload(enc) => self.reload(enc),
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
            Action::DeleteMacro(i) => self.delete_macro(i),
            Action::CaptureShortcut(i) => {
                self.capturing_shortcut = if self.capturing_shortcut == Some(i) { None } else { Some(i) };
            }
            Action::ToggleRecord => self.toggle_record(),
            Action::OpenMacroDir => self.open_macro_dir(),
            Action::SwitchTab(i) => self.switch_tab(i),
            Action::CloseTab(i) => self.close_tab(if i == usize::MAX { self.tabs.active() } else { i }),
            Action::NextTab => self.switch_tab(self.tabs.neighbor(true)),
            Action::PrevTab => self.switch_tab(self.tabs.neighbor(false)),
            Action::SetView(view) => {
                if view != self.settings.view {
                    self.settings.view = view;
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
            if icons::button(ui, Icon::New, "新增空白文件 (Ctrl+N)", true).clicked() {
                action = Some(Action::New);
            }
            if icons::button(ui, Icon::Open, "開啟檔案 (Ctrl+O)", true).clicked() {
                action = Some(Action::Open);
            }
            if icons::button(ui, Icon::Save, "儲存 (Ctrl+S)", true).clicked() {
                action = Some(Action::Save);
            }
            if icons::button(ui, Icon::SaveAs, "另存新檔 (Ctrl+Shift+S)", true).clicked() {
                action = Some(Action::SaveAs);
            }

            group_separator(ui);

            let find_tip = "尋找與取代 (Ctrl+F，取代 Ctrl+H)";
            if icons::toggle(ui, Icon::Search, find_tip, self.find.open).clicked() {
                action = Some(if self.find.open {
                    Action::CloseFind
                } else {
                    Action::Find { replace: false }
                });
            }

            let grep_tip = "在資料夾的所有檔案中尋找 (Ctrl+Shift+F)";
            if icons::toggle(ui, Icon::FindInFiles, grep_tip, self.folder.open).clicked() {
                action = Some(Action::ToggleFolderSearch);
            }

            group_separator(ui);

            let run = icons::toggle(ui, Icon::Run, "巨集：執行、設快捷鍵、編輯", self.macro_open);
            self.macro_anchor = Some(run.rect);
            if run.clicked() {
                action = Some(Action::ToggleMacroMenu);
            }
            let can_rerun = self.last_macro.is_some();
            if icons::button(ui, Icon::Rerun, "重新執行上一個巨集 (F5)", can_rerun).clicked() {
                action = Some(Action::RerunMacro);
            }
            let recording = self.editor.is_recording();
            let (icon, tip) = if recording {
                (Icon::Stop, "停止錄製，並把錄下的操作存成巨集")
            } else {
                (Icon::Record, "錄製巨集：記下之後的按鍵與輸入，可重複播放（滑鼠操作不會錄）")
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
                    "設定（配色、字型、字級）",
                    self.settings_open,
                );
                group_separator(ui);
                self.settings_anchor = Some(more.rect);
                if more.clicked() {
                    self.settings_open = !self.settings_open;
                }
            });
        });
        action
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
                "開始錄製巨集：之後的按鍵與輸入都會記下來（滑鼠操作不會）。再按一次錄製鈕結束",
            );
            return;
        }
        let Some(source) = self.editor.stop_recording() else {
            self.push(LogKind::Info, "沒有錄到任何操作");
            return;
        };
        let Some(dir) = macro_lib::macros_dir() else {
            // 沒有設定資料夾可存：把原始碼開在新分頁，讓使用者自己決定存哪裡
            let mut doc = Document::new();
            doc.buffer = feditor_core::Buffer::from_text(&source);
            self.tabs.push(&mut self.doc, &mut self.editor, doc);
            self.tab_changed();
            self.push(LogKind::Warn, "找不到設定資料夾，錄好的巨集開在新分頁，請自行另存");
            return;
        };
        match macro_lib::save_new(&dir, &source) {
            Ok(path) => {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                self.last_macro = Some(path);
                self.refresh_macros();
                self.push(
                    LogKind::Info,
                    format!("已儲存錄好的巨集 {name}；按 F5 重新執行，或到巨集面板重新命名、設快捷鍵、編輯"),
                );
            }
            Err(e) => self.push(LogKind::Error, format!("無法儲存錄好的巨集：{e}")),
        }
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
            "刪除巨集",
            format!("確定要刪除巨集「{}」嗎？檔案會被刪除。", entry.name),
        )
        .button("刪除", modal::Answer::Yes)
        .button("取消", modal::Answer::Cancel);
        self.modal = Some(Pending { spec, then: Then::DeleteMacro(i) });
    }

    fn do_delete_macro(&mut self, i: usize) {
        let Some(entry) = self.macros.get(i).cloned() else {
            return;
        };
        match std::fs::remove_file(&entry.path) {
            Ok(()) => self.push(LogKind::Info, format!("已刪除巨集 {}", entry.name)),
            Err(e) => self.push(LogKind::Error, format!("無法刪除 {}：{e}", entry.path.display())),
        }
        self.refresh_macros();
    }

    fn open_macro_dir(&mut self) {
        let Some(dir) = macro_lib::macros_dir() else {
            self.push(LogKind::Warn, "找不到設定資料夾");
            return;
        };
        if let Err(e) = std::fs::create_dir_all(&dir) {
            self.push(LogKind::Error, format!("無法建立 {}：{e}", dir.display()));
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
            self.push(LogKind::Error, format!("無法開啟 {}：{e}", dir.display()));
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
                    format!("{} 是內建快捷鍵，請換一個", macro_lib::format_shortcut(&sc)),
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
                    format!(
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
                    Some(t) => format!("巨集「{}」的快捷鍵設為 {t}", entry.name),
                    None => format!("已清除巨集「{}」的快捷鍵", entry.name),
                };
                self.push(LogKind::Info, msg);
            }
            Err(e) => self.push(LogKind::Error, format!("無法更新 {}：{e}", entry.path.display())),
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
                        "快捷鍵要有 Ctrl 或 Alt（功能鍵 F1–F12 除外），單獨的字母鍵是打字用的",
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
                egui::RichText::new("還沒有巨集。按工具列的錄製鈕錄一個，或把 .js 檔放進巨集資料夾。")
                    .color(ui.visuals().weak_text_color()),
            );
        }
        for (i, entry) in self.macros.iter().enumerate() {
            ui.horizontal(|ui| {
                tight_buttons(ui);
                if icons::small_button(ui, Icon::Run, "執行這支巨集", true).clicked() {
                    action = Some(Action::RunMacroEntry(i));
                }
                ui.add_space(4.0);
                ui.label(&entry.name);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if icons::small_button(ui, Icon::Clear, "刪除這支巨集", true).clicked() {
                        action = Some(Action::DeleteMacro(i));
                    }
                    if icons::small_button(ui, Icon::Edit, "編輯原始碼（開在新分頁）", true).clicked() {
                        action = Some(Action::EditMacro(i));
                    }
                    let capturing = self.capturing_shortcut == Some(i);
                    let tip = "設定快捷鍵：點一下再按下組合鍵（Esc 取消、Delete 清除）";
                    if icons::small_toggle(ui, Icon::Keyboard, tip, capturing, true).clicked() {
                        action = Some(Action::CaptureShortcut(i));
                    }
                    let label = if capturing {
                        "請按下快捷鍵…".to_owned()
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
            if ui.button("從檔案執行…").clicked() {
                action = Some(Action::RunMacro);
            }
            if ui.button("開啟巨集資料夾").clicked() {
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
        let mut dialog = rfd::FileDialog::new().set_title("選擇要搜尋的資料夾");
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
            ui.label("在檔案中尋找");
            let response = ui.add(
                egui::TextEdit::singleline(&mut self.folder.query)
                    .id(folder_query_id())
                    .hint_text("搜尋文字")
                    .desired_width(240.0),
            );
            if std::mem::take(&mut self.folder.focus_query) {
                response.request_focus();
            }
            let enter = response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if response.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                action = Some(Action::ToggleFolderSearch);
            }
            option_toggle(ui, Icon::CaseSensitive, "區分大小寫", &mut self.folder.case_sensitive);
            option_toggle(ui, Icon::WholeWord, "完整單字", &mut self.folder.whole_word);
            option_toggle(ui, Icon::Regex, "規則表達式", &mut self.folder.regex);
            if running {
                if icons::button(ui, Icon::Stop, "停止搜尋", true).clicked() {
                    action = Some(Action::StopFolderSearch);
                }
            } else if icons::button(ui, Icon::Search, "開始搜尋 (Enter)", true).clicked() || enter {
                action = Some(Action::RunFolderSearch);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if icons::small_button(ui, Icon::Close, "關閉 (Esc)", true).clicked() {
                    action = Some(Action::ToggleFolderSearch);
                }
            });
        });
        ui.horizontal(|ui| {
            ui.label("資料夾");
            ui.add(
                egui::TextEdit::singleline(&mut self.folder.dir)
                    .desired_width(ui.available_width() - 330.0),
            );
            if icons::small_button(ui, Icon::Open, "選擇資料夾…", true).clicked() {
                action = Some(Action::PickSearchFolder);
            }
            ui.label("檔名");
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

    /// 設定面板：配色、字型、字級。
    fn settings_panel(&self, ui: &mut egui::Ui) -> Option<Action> {
        let mut action = None;
        ui.set_min_width(340.0);
        // 掃描結果複製一份出來，避免整個繪製過程一直握著鎖
        let scan = self.font_scan.lock().ok().and_then(|guard| {
            guard
                .as_ref()
                .map(|s| (s.families.clone(), s.default_name.clone()))
        });

        egui::Grid::new("settings_grid")
            .num_columns(2)
            .spacing([14.0, 10.0])
            .show(ui, |ui| {
                // 配色：三態狀態鈕（從工具列搬進來），旁邊寫出目前的狀態
                ui.label("配色");
                ui.horizontal(|ui| {
                    let current = self.settings.theme;
                    let next = current.next();
                    let tip = format!(
                        "配色：{}（點擊切換為「{}」）",
                        theme_label(current),
                        theme_label(next)
                    );
                    if icons::button(ui, theme_icon(current), &tip, true).clicked() {
                        action = Some(Action::SetTheme(next));
                    }
                    ui.label(theme_label(current));
                });
                ui.end_row();

                // 字型：只列等寬字型
                ui.label("字型");
                match &scan {
                    None => {
                        ui.label(
                            egui::RichText::new("正在找出系統上的等寬字型…")
                                .color(ui.visuals().weak_text_color()),
                        );
                        ui.ctx().request_repaint_after(Duration::from_millis(150));
                    }
                    Some((families, default_name)) => {
                        let default_label = match default_name {
                            Some(name) => format!("系統預設（{name}）"),
                            None => "系統預設".to_owned(),
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
                                if ui.selectable_label(current.is_none(), &default_label).clicked()
                                {
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
                }
                ui.end_row();

                // 字級：與快捷鍵共用同一個設定值，所以兩邊永遠同步
                ui.label("字級");
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
                ui.end_row();

                // 顯示：四個開關，圖示加 tooltip
                ui.label("顯示");
                ui.horizontal(|ui| {
                    tight_buttons(ui);
                    let v = self.settings.view;
                    let mut next = v;
                    if icons::toggle(ui, Icon::LineNumbers, "顯示行號", v.show_line_numbers).clicked() {
                        next.show_line_numbers = !v.show_line_numbers;
                    }
                    if icons::toggle(ui, Icon::CurrentLine, "標示目前這一行", v.highlight_line).clicked() {
                        next.highlight_line = !v.highlight_line;
                    }
                    if icons::toggle(ui, Icon::Whitespace, "顯示空白、Tab 與換行符號", v.show_whitespace).clicked() {
                        next.show_whitespace = !v.show_whitespace;
                    }
                    if icons::toggle(ui, Icon::Syntax, "語法上色（依副檔名）", v.syntax).clicked() {
                        next.syntax = !v.syntax;
                    }
                    let too_big = self.doc.buffer.line_count() > editor::wrap::MAX_LINES;
                    let tip = if too_big {
                        format!("自動換行（超過 {} 行的檔案不支援）", editor::wrap::MAX_LINES)
                    } else {
                        "自動換行".to_owned()
                    };
                    let on = v.word_wrap && !too_big;
                    if icons::toggle(ui, Icon::WordWrap, &tip, on).clicked() && !too_big {
                        next.word_wrap = !v.word_wrap;
                    }
                    if next != v {
                        action = Some(Action::SetView(next));
                    }
                });
                ui.end_row();
            });

        ui.add_space(6.0);
        ui.label(
            egui::RichText::new("調整字級：Ctrl ＋／－ 或 Ctrl＋滾輪；Ctrl＋0 還原預設")
                .small()
                .color(ui.visuals().weak_text_color()),
        );
        action
    }

    /// 分頁列（有兩個以上的分頁才顯示）：點擊切換、中鍵或 × 關閉。未儲存的分頁在名稱後面加「●」。
    fn tab_bar(&self, ui: &mut egui::Ui) -> Option<Action> {
        let mut action = None;
        egui::ScrollArea::horizontal()
            .id_salt("tab_scroll")
            .auto_shrink([false, true])
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    tight_buttons(ui);
                    no_hover_outline(ui);
                    for i in 0..self.tabs.len() {
                        let (name, full, dirty) = self.tabs.label(&self.doc, i);
                        let text = if dirty { format!("{name} ●") } else { name };
                        let selected = i == self.tabs.active();
                        // 整個分頁是一個元件：名稱在左，× 在右側內部，滑鼠移到分頁上才畫出 ×
                        // （× 的位置一直保留，顯示與隱藏時分頁寬度不會跳動）
                        let font = egui::TextStyle::Button.resolve(ui.style());
                        let galley = ui.painter().layout_no_wrap(text, font, egui::Color32::WHITE);
                        let (pad, close_w) = (8.0, 14.0);
                        let size = egui::vec2(
                            pad + galley.size().x + 6.0 + close_w + 4.0,
                            ui.spacing().interact_size.y,
                        );
                        let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
                        let close = egui::Rect::from_center_size(
                            egui::pos2(rect.right() - 4.0 - close_w / 2.0, rect.center().y),
                            egui::vec2(close_w, close_w),
                        );
                        let close_resp =
                            ui.interact(close, ui.make_persistent_id(("tab_close", i)), egui::Sense::click());
                        let over = response.hovered() || close_resp.hovered();
                        let visuals = ui.style().interact_selectable(&response, selected);
                        if selected || over {
                            ui.painter().rect_filled(
                                rect,
                                egui::CornerRadius::same(4),
                                visuals.weak_bg_fill,
                            );
                        }
                        let text_pos = egui::pos2(
                            rect.left() + pad,
                            rect.center().y - galley.size().y / 2.0,
                        );
                        ui.painter()
                            .galley_with_override_text_color(text_pos, galley, visuals.fg_stroke.color);
                        if over {
                            if close_resp.hovered() {
                                ui.painter().rect_filled(
                                    close,
                                    egui::CornerRadius::same(3),
                                    ui.visuals().widgets.hovered.bg_fill,
                                );
                            }
                            let c = close.center();
                            let d = 3.0;
                            let stroke = egui::Stroke::new(1.0, visuals.fg_stroke.color);
                            ui.painter().line_segment([c + egui::vec2(-d, -d), c + egui::vec2(d, d)], stroke);
                            ui.painter().line_segment([c + egui::vec2(-d, d), c + egui::vec2(d, -d)], stroke);
                        }
                        if close_resp.clicked() {
                            action = Some(Action::CloseTab(i));
                        } else if response.clicked() {
                            action = Some(Action::SwitchTab(i));
                        }
                        if response.middle_clicked() {
                            action = Some(Action::CloseTab(i));
                        }
                        if close_resp.hovered() {
                            close_resp.on_hover_text("關閉分頁");
                        } else {
                            response.on_hover_text(full);
                        }
                    }
                });
            });
        action
    }

    /// 尋找列：搜尋框、三個選項（區分大小寫、完整單字、規則表達式）、上一筆／下一筆，
    /// 以及可展開的取代欄。
    fn find_bar(&mut self, ui: &mut egui::Ui) -> Option<Action> {
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
            if icons::small_button(ui, chevron, "顯示或隱藏取代 (Ctrl+H)", true).clicked() {
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
                    .hint_text("尋找")
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
                "區分大小寫",
                &mut self.find.opts.case_sensitive,
            );
            changed |= option_toggle(
                ui,
                Icon::WholeWord,
                "完整單字（前後不可緊鄰字母、數字或底線）",
                &mut self.find.opts.whole_word,
            );
            // 規則表達式：tooltip 是基本語法速查加完整說明的連結（滑鼠移到 tooltip 上也不會消失）
            let regex_on = self.find.opts.regex;
            if icons::small_toggle_rich(ui, Icon::Regex, "規則表達式", regex_on, regex_help)
                .clicked()
            {
                self.find.opts.regex = !regex_on;
                changed = true;
            }
            if changed {
                action = Some(Action::FindChanged);
            }

            if icons::small_button(ui, Icon::ChevronUp, "上一筆 (Shift+Enter 或 Shift+F3)", !query_empty)
                .clicked()
            {
                action = Some(Action::FindPrev);
            }
            if icons::small_button(ui, Icon::ChevronDown, "下一筆 (Enter 或 F3)", !query_empty)
                .clicked()
            {
                action = Some(Action::FindNext);
            }

            // 結果摘要
            match self.find.summary() {
                Summary::Idle => {}
                Summary::Error(msg) => {
                    ui.label(egui::RichText::new("規則有誤").color(ui.visuals().error_fg_color))
                        .on_hover_text(msg);
                }
                Summary::NoMatch => {
                    ui.label(egui::RichText::new("找不到").color(ui.visuals().error_fg_color));
                }
                Summary::Count {
                    index,
                    total,
                    capped,
                } => {
                    let plus = if capped { "+" } else { "" };
                    let text = match index {
                        Some(i) => format!("{}/{total}{plus}", i + 1),
                        None => format!("{total}{plus} 筆"),
                    };
                    ui.label(egui::RichText::new(text).color(ui.visuals().weak_text_color()));
                }
                Summary::TooLarge => {
                    ui.label(egui::RichText::new("—").color(ui.visuals().weak_text_color()))
                        .on_hover_text("檔案很大，不統計筆數");
                }
            }
            if self.find.wrapped {
                ui.label(egui::RichText::new("已繞回").color(ui.visuals().weak_text_color()));
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if icons::small_button(ui, Icon::Close, "關閉 (Esc)", true).clicked() {
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
                        .hint_text("取代為")
                        .desired_width(220.0),
                );
                let input = if self.find.opts.regex {
                    input.on_hover_text(
                        "規則表達式模式：可用 $1、${名稱} 代入群組，\\n、\\t 代表換行與跳位，字面的 $ 請寫成 $$",
                    )
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
                if icons::small_button(ui, Icon::ReplaceOne, "取代這一筆 (Enter)", !query_empty)
                    .clicked()
                {
                    action = Some(Action::ReplaceOne);
                }
                if icons::small_button(ui, Icon::ReplaceAll, "全部取代", !query_empty).clicked() {
                    action = Some(Action::ReplaceAll);
                }
            });
        }
        action
    }

    /// 狀態列：左邊是檔案狀態，右邊是這個檔案的格式設定（編碼、BOM、換行、重新載入）。
    /// 這些控制項不放標題，說明都在 tooltip。
    fn status_bar(&mut self, ui: &mut egui::Ui) -> Option<Action> {
        let mut action = None;
        ui.horizontal(|ui| {
            tight_buttons(ui);
            let path = self
                .doc
                .path
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "未命名".to_owned());
            // 整列由右往左排：先放右側的格式控制項，最後讓路徑吃掉剩下的空間，放不下時截斷。
            // 這樣路徑很長時，也不會和右邊的控制項重疊。（未儲存的標示在標題列，這裡不重複。）
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // 由右往左加入，所以畫面上由左到右的順序是：編碼、BOM、換行、重新載入

                // 重新載入：點擊後彈出編碼清單
                let can_reload = self.doc.path.is_some();
                let reload = icons::small_button(
                    ui,
                    Icon::Reload,
                    "以指定編碼重新讀取目前的檔案（自動偵測錯誤時使用）",
                    can_reload,
                );
                egui::Popup::menu(&reload).show(|ui| {
                    no_hover_outline(ui);
                    ui.label("重新載入為：");
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
                    .on_hover_text("換行格式（存檔時使用）");

                let bom_applies = [UTF_8, UTF_16LE, UTF_16BE].contains(&self.doc.format.encoding);
                let bom_tip = if bom_applies {
                    "BOM：在檔案開頭寫入位元組順序標記"
                } else {
                    "BOM：此編碼不適用"
                };
                let bom = self.doc.format.bom;
                if icons::small_toggle(ui, Icon::Bom, bom_tip, bom, bom_applies).clicked() {
                    self.doc.set_bom(!bom);
                }

                let mut chosen = None;
                // 寬度以「最長的可能顯示文字」為準：所有編碼名稱，加上 "UTF-8 BOM"
                let enc_width = combo_width(
                    ui,
                    common_encodings()
                        .iter()
                        .map(|e| e.name())
                        .chain(std::iter::once("UTF-8 BOM")),
                );
                egui::ComboBox::from_id_salt("encoding")
                    .icon(icons::combo_chevron)
                    .width(enc_width)
                    .selected_text(self.doc.format.encoding_label())
                    .show_ui(ui, |ui| flat_list(ui, |ui| {
                        for enc in common_encodings() {
                            let selected = self.doc.format.encoding == enc;
                            if ui.selectable_label(selected, enc.name()).clicked() {
                                chosen = Some(enc);
                            }
                        }
                    }))
                    .response
                    .on_hover_text("編碼（存檔時使用）。無法表示的字元會讓存檔失敗，不會被替換");
                if let Some(enc) = chosen {
                    self.doc.set_encoding(enc);
                }

                // 語言（語法上色）：「自動」依副檔名，沒有檔名時看內容猜
                let current = self.doc.lang;
                let resolved = syntax::resolve(current, self.doc.path.as_deref(), &self.doc.buffer);
                let shown = resolved.map_or("純文字", |l| l.name);
                let langs = syntax::all_languages();
                let lang_width = combo_width(ui, langs.iter().map(|l| l.name).chain(["純文字", "自動"]));
                let mut picked = None;
                egui::ComboBox::from_id_salt("language")
                    .icon(icons::combo_chevron)
                    .width(lang_width)
                    .selected_text(shown)
                    .show_ui(ui, |ui| flat_list(ui, |ui| {
                        if ui.selectable_label(current == syntax::LangChoice::Auto, "自動").clicked() {
                            picked = Some(syntax::LangChoice::Auto);
                        }
                        if ui.selectable_label(current == syntax::LangChoice::Plain, "純文字").clicked() {
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
                    .on_hover_text("語言（語法上色）。自動：依副檔名，沒有檔名時看內容判斷；也可以手動指定");
                if let Some(choice) = picked {
                    self.doc.lang = choice;
                }

                // 左側：路徑（剩下的空間全給它，放不下就截斷，完整路徑看 tooltip）
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.add(egui::Label::new(&path).truncate())
                        .on_hover_text(&path);
                });
            });
        });
        action
    }

    fn output_panel(&self, ui: &mut egui::Ui) -> Option<Action> {
        let mut action = None;
        // 標題列：左邊是標題，右邊是清除按鈕
        ui.horizontal(|ui| {
            ui.label("輸出");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let has_log = !self.log.is_empty();
                if icons::button(ui, Icon::Clear, "清除輸出", has_log).clicked() {
                    action = Some(Action::ClearLog);
                }
            });
        });
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

        // 要關閉視窗了：先把還沒寫入的設定存起來，並記住開著的分頁
        if ctx.input(|i| i.viewport().close_requested()) {
            self.flush_settings();
            self.save_session();
        }

        // 檔案被別的程式改了？視窗拿回焦點時、以及每秒檢查一次
        let focused = ctx.input(|i| i.focused);
        if focused && (!self.was_focused || self.last_disk_check.elapsed() >= Duration::from_secs(1)) {
            self.last_disk_check = Instant::now();
            self.check_external_changes();
        }
        self.was_focused = focused;
        ctx.request_repaint_after(Duration::from_secs(1));

        // 關閉視窗前，若有未儲存的修改先詢問
        if ctx.input(|i| i.viewport().close_requested())
            && !self.tabs.dirty_tabs(&self.doc).is_empty()
            && !self.allow_close
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            if self.modal.is_none() {
                self.close_skipped.clear();
                self.next_close_prompt(&ctx);
            }
        }

        self.capture_shortcut(&ctx);
        if self.folder.poll() {
            ctx.request_repaint();
        }

        // 快捷鍵（Ctrl+Shift+S 要先於 Ctrl+S 檢查，因為 egui 比對修飾鍵時 Ctrl+S 也會吃到它）
        let font_size = self.settings.font_size;
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
                Some(Action::SetFontSize(adjust_font_size(font_size, 1.0)))
            } else if i.consume_shortcut(&egui::gui_zoom::kb_shortcuts::ZOOM_OUT) {
                Some(Action::SetFontSize(adjust_font_size(font_size, -1.0)))
            } else if i.consume_shortcut(&egui::gui_zoom::kb_shortcuts::ZOOM_RESET) {
                Some(Action::SetFontSize(DEFAULT_FONT_SIZE))
            } else {
                None
            }
        });
        // 使用者替巨集設定的快捷鍵
        if action.is_none() {
            let hit = self.macros.iter().position(|m| {
                m.shortcut
                    .as_ref()
                    .is_some_and(|sc| ctx.input_mut(|i| i.consume_shortcut(sc)))
            });
            if let Some(i) = hit {
                action = Some(Action::RunMacroEntry(i));
            }
        }
        // Ctrl＋滾輪（或觸控板捏合）：egui 以縮放比例表示，換算成字級的加減
        if action.is_none() {
            let steps = zoom_steps(&mut self.zoom_accum, zoom_delta);
            if steps != 0 {
                action = Some(Action::SetFontSize(adjust_font_size(font_size, steps as f32)));
            }
        }

        egui::Panel::top("toolbar").show(ui, |ui| {
            if let Some(a) = self.toolbar(ui) {
                action = Some(a);
            }
        });
        if self.tabs.len() > 1 {
            egui::Panel::top("tabs").show(ui, |ui| {
                if let Some(a) = self.tab_bar(ui) {
                    action = Some(a);
                }
            });
        }
        if self.find.open {
            egui::Panel::top("find").show(ui, |ui| {
                if let Some(a) = self.find_bar(ui) {
                    action = Some(a);
                }
            });
        }
        egui::Panel::bottom("status").show(ui, |ui| {
            if let Some(a) = self.status_bar(ui) {
                action = Some(a);
            }
        });
        egui::Panel::bottom("output")
            .resizable(true)
            .default_size(110.0)
            .show(ui, |ui| {
                if let Some(a) = self.output_panel(ui) {
                    action = Some(a);
                }
            });
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
        egui::CentralPanel::default().show(ui, |ui| {
            let App {
                doc,
                find,
                editor,
                settings,
                ..
            } = &mut *self;
            let reveal = std::mem::take(&mut find.reveal);
            let options = settings.view;
            let resolved = syntax::resolve(doc.lang, doc.path.as_deref(), &doc.buffer);
            // 行註解符號：語言有就用語言的（手動選了語言、或沒有檔名時才有意義），否則看副檔名
            editor.comment_prefix = resolved
                .and_then(|l| l.line_comment())
                .unwrap_or_else(|| language::comment_prefix(doc.path.as_deref()));
            let language = if options.syntax { resolved } else { None };
            EditorView {
                id: editor_id(),
                buffer: &mut doc.buffer,
                state: editor,
                highlight: find.current_chars.clone(),
                reveal,
                options,
                language,
            }
            .show(ui);
            // 使用者在編輯區改了文字：標成未儲存，之前找到的位置與筆數作廢
            if doc.sync() {
                find.invalidate();
            }
        });
        if self.settings_open {
            self.show_settings_area(&ctx, &mut action);
        }
        if self.macro_open {
            self.show_macro_area(&ctx, &mut action);
        }
        if let Some(p) = self.modal.take() {
            match modal::show(&ctx, &p.spec) {
                Some(answer) => self.resolve_modal(p.then, answer, &ctx),
                None => self.modal = Some(p),
            }
        }

        if let Some(a) = action {
            self.handle(a, &ctx);
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
            theme::apply(&cc.egui_ctx); // 深色配色採用 EVA 色票
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
