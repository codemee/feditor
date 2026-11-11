//! 使用者設定：目前只有配色。存成簡單的 `key = value` 文字檔，放在各平台的使用者設定目錄。
//!
//! 讀取時盡量容錯：檔案不存在、內容壞掉、值不認得，都退回預設值，不會讓程式起不來。
//! 寫入沿用核心的安全存檔（先寫暫存檔再改名），不會留下寫到一半的設定檔。

use eframe::egui;
use feditor_core::fileio::{self, FileFormat, LineEnding};
use feditor_core::i18n::t;
use feditor_core::tf;
use std::path::{Path, PathBuf};

/// 配色偏好。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ThemeChoice {
    /// 跟隨作業系統的淺色／深色設定。
    #[default]
    System,
    Light,
    Dark,
}

impl ThemeChoice {
    /// 按一下狀態鈕後切換到的下一個：跟隨系統 → 淺色 → 深色 → 跟隨系統。
    pub fn next(self) -> Self {
        match self {
            ThemeChoice::System => ThemeChoice::Light,
            ThemeChoice::Light => ThemeChoice::Dark,
            ThemeChoice::Dark => ThemeChoice::System,
        }
    }

    fn key(self) -> &'static str {
        match self {
            ThemeChoice::System => "system",
            ThemeChoice::Light => "light",
            ThemeChoice::Dark => "dark",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value.trim().trim_matches('"').to_ascii_lowercase().as_str() {
            "system" => Some(ThemeChoice::System),
            "light" => Some(ThemeChoice::Light),
            "dark" => Some(ThemeChoice::Dark),
            _ => None,
        }
    }
}

impl From<ThemeChoice> for egui::ThemePreference {
    fn from(choice: ThemeChoice) -> Self {
        match choice {
            ThemeChoice::System => egui::ThemePreference::System,
            ThemeChoice::Light => egui::ThemePreference::Light,
            ThemeChoice::Dark => egui::ThemePreference::Dark,
        }
    }
}

/// 字級（點）的範圍與預設值。預設值與 egui 預設的等寬字級一致。
pub const MIN_FONT_SIZE: f32 = 8.0;
pub const MAX_FONT_SIZE: f32 = 48.0;
pub const DEFAULT_FONT_SIZE: f32 = 13.0;
/// 預覽窗格內文的預設字級（預覽的字級和編輯區分開調整）。
pub const DEFAULT_PREVIEW_FONT_SIZE: f32 = 14.0;
/// 終端機的字級（點）。
pub const DEFAULT_TERMINAL_FONT_SIZE: f32 = DEFAULT_FONT_SIZE;
/// 終端機文字和底色的最低對比度（WCAG 比值）；低於它的文字會被調整到看得清楚。
pub const DEFAULT_TERMINAL_MIN_CONTRAST: f32 = 3.0;
/// 對比度的範圍：1 是不調整（關閉），7 是 WCAG AAA 的一般文字標準。
pub const MIN_TERMINAL_CONTRAST: f32 = 1.0;
pub const MAX_TERMINAL_CONTRAST: f32 = 7.0;

/// 把字級限制在合理範圍內；不是有限數字（NaN、無限大）時退回預設值。
pub fn clamp_font_size(size: f32) -> f32 {
    if size.is_finite() {
        size.clamp(MIN_FONT_SIZE, MAX_FONT_SIZE)
    } else {
        DEFAULT_FONT_SIZE
    }
}

/// 字級加上 `delta`（可為負），並限制在範圍內。
pub fn adjust_font_size(size: f32, delta: f32) -> f32 {
    clamp_font_size(size + delta)
}

/// 縮放量累積到這個值（自然對數）就換算成字級加減 1 點。
pub const ZOOM_STEP_LN: f32 = 0.1;

/// 把 Ctrl＋滾輪（或觸控板捏合）的縮放量換算成「字級要加減幾點」。
///
/// egui 把滾輪與捏合都表示成縮放比例（1.0 表示沒變）。這裡取對數累加，每累積到
/// [`ZOOM_STEP_LN`] 就換成 1 點，餘數留在 `accum` 裡下次再用，所以觸控板連續的小量也不會漏掉。
/// 回傳值為正表示放大，為負表示縮小。
pub fn zoom_steps(accum: &mut f32, zoom_delta: f32) -> i32 {
    if !zoom_delta.is_finite() || zoom_delta <= 0.0 {
        return 0;
    }
    *accum += zoom_delta.ln();
    let steps = (*accum / ZOOM_STEP_LN).trunc();
    *accum -= steps * ZOOM_STEP_LN;
    steps as i32
}

/// 編輯區的顯示選項。
fn parse_bool(value: &str) -> Option<bool> {
    match value.trim_matches('"').to_ascii_lowercase().as_str() {
        "true" | "on" | "yes" | "1" => Some(true),
        "false" | "off" | "no" | "0" => Some(false),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ViewOptions {
    /// 用符號標出空白、Tab 與換行。
    pub show_whitespace: bool,
    pub show_line_numbers: bool,
    /// 目前游標所在那一行畫上底色。
    pub highlight_line: bool,
    /// 太長的行折到下一行顯示（不改變文字內容）。
    pub word_wrap: bool,
    /// 依檔案類型做語法上色。
    pub syntax: bool,
    /// 是否啟用安全界線：從 `ruler_column` 欄的右緣起，右邊的區域換成淡淡的顏色，提醒一行不要寫過這個寬度。
    pub ruler: bool,
    /// 安全界線的欄數（1 起算）。欄數和畫面一致（中日韓全形字佔兩欄）；沒啟用時也會記著。
    pub ruler_column: u16,
}

/// Tab 欄數的範圍與預設（設定面板用；實際換算在 `feditor_core::motion`）。
pub const MIN_TAB_COLUMNS: u8 = 1;
pub const MAX_TAB_COLUMNS: u8 = 16;
pub const DEFAULT_TAB_COLUMNS: u8 = 4;

/// 安全界線欄數的範圍。
pub const MIN_RULER_COLUMN: u16 = 1;
pub const MAX_RULER_COLUMN: u16 = 500;
/// 安全界線的預設欄數（常見的程式碼寬度）。
pub const DEFAULT_RULER_COLUMN: u16 = 80;

impl Default for ViewOptions {
    fn default() -> Self {
        Self {
            show_whitespace: false,
            show_line_numbers: true,
            highlight_line: true,
            word_wrap: false,
            syntax: true,
            ruler: true,
            ruler_column: DEFAULT_RULER_COLUMN,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub theme: ThemeChoice,
    /// 編輯區字型的家族名稱；`None` 表示使用作業系統預設的等寬字型。
    pub font_family: Option<String>,
    /// 編輯區字級（點）。
    pub font_size: f32,
    /// 預覽窗格內文的字級（點在預覽窗格上時，縮放快捷鍵調的是它）。
    pub preview_font_size: f32,
    /// 一個 Tab 佔幾欄（沒有個別設定的檔案類型都用這個）。
    pub tab_width: u8,
    /// 依檔案類型（語法上色的語言名稱，例如 `Rust`、`Python`）個別設定的 Tab 欄數。
    pub tab_widths: std::collections::BTreeMap<String, u8>,
    /// 依檔案類型（語言名稱，純文字是 [`PLAIN_TEXT_KEY`]）個別設定「自動換行」開或關；沒有的用 `view.word_wrap`（預設值）。
    pub wrap_langs: std::collections::BTreeMap<String, bool>,
    /// 依檔案類型指定括號內斷行的風格（排列格式用）。鍵是語言名稱；沒有的用語言本身的預設。
    pub wrap_styles: std::collections::BTreeMap<String, crate::syntax::WrapStyle>,
    /// 手動選的執行環境：鍵是專案資料夾（[`crate::runtime_env::project_root`]），值是直譯器（[`crate::runtime_env::Candidate::key`]）。
    /// 沒有的就自動搜尋。
    pub runtime_choices: std::collections::BTreeMap<String, String>,
    /// 「執行目前檔案」的命令列參數：鍵是檔案的完整路徑，值是原樣接在檔案後面的文字。沒有的就沒有參數。
    pub run_args: std::collections::BTreeMap<String, String>,
    /// 最近開啟的檔案（完整路徑，最近的在最前面，最多 `recent::MAX_RECENT` 個）。設定檔裡一行一個 `recent = 路徑`。
    pub recent_files: Vec<String>,
    /// 輸入 Tab 時改成輸入空白（縮排也用空白）。所有檔案類型共用。
    pub tab_spaces: bool,
    /// 按 Enter 時依語言自動加深／減淺縮排（預設開）。
    pub auto_indent: bool,
    /// 在檔案總管（Windows）、Finder（macOS）的右鍵選單加入 FEditor。
    pub shell_menu: bool,
    /// 在 `~/.local/bin` 加入 `feditor` 指令（macOS、Linux）。
    pub cli_command: bool,
    /// 雙按檔案時，讓選到的類別（[`Settings::file_assoc_groups`]）預設用 FEditor 開啟。
    pub file_assoc: bool,
    /// 要讓 FEditor 接手的檔案類別（`file_assoc::GROUPS` 的 `key`）；預設是程式碼與設定檔。
    pub file_assoc_groups: std::collections::BTreeSet<String>,
    /// 尋找面板停靠的邊、左右邊停靠時的寬度、上下邊停靠時的高度，以及兩個結果表格的分隔比例（None 是預設）。
    pub search_dock_edge: crate::search_dock::DockEdge,
    pub search_dock_width: f32,
    pub search_dock_height: f32,
    pub search_split: Option<f32>,
    /// 終端機的字級（點在終端機上時，縮放快捷鍵調的是它）。
    pub terminal_font_size: f32,
    /// 終端機文字和底色的最低對比度；1 表示不調整。
    pub terminal_min_contrast: f32,
    /// 新開的終端機要執行的 shell（程式加參數，例如 `pwsh -NoLogo`）；空字串表示系統預設的 shell。
    pub terminal_shell: String,
    pub view: ViewOptions,
    /// 輸出面板展開（`true`）還是收合成一條（`false`，預設）。輸出主要是巨集與開發時看的。
    pub show_output: bool,
    /// 淺色主題用的配色名稱；空字串表示該主題的預設配色。
    pub light_scheme: String,
    /// 深色主題用的配色名稱；空字串表示該主題的預設配色。
    pub dark_scheme: String,
    /// 狀態列的欄號裡，全形字（中日韓文字等）算幾欄：1（一個字一欄）或 2（和畫面寬度一致）。
    pub wide_cols: u8,
    /// 介面語言：跟隨系統、中文、英文。
    pub language: feditor_core::i18n::LangChoice,
}

/// 設定檔裡代表「純文字（沒有語言）」的檔案類型名稱（不翻譯，設定檔要在各種介面語言下通用）。
pub const PLAIN_TEXT_KEY: &str = "Plain text";

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: ThemeChoice::default(),
            font_family: None,
            font_size: DEFAULT_FONT_SIZE,
            preview_font_size: DEFAULT_PREVIEW_FONT_SIZE,
            tab_width: DEFAULT_TAB_COLUMNS,
            tab_widths: Default::default(),
            wrap_langs: Default::default(),
            wrap_styles: Default::default(),
            runtime_choices: Default::default(),
            run_args: Default::default(),
            recent_files: Vec::new(),
            tab_spaces: false,
            auto_indent: true,
            shell_menu: false,
            cli_command: false,
            file_assoc: false,
            file_assoc_groups: crate::file_assoc::default_groups(),
            search_dock_edge: crate::search_dock::DockEdge::Right,
            search_dock_width: crate::search_dock::DEFAULT_WIDTH,
            search_dock_height: crate::search_dock::DEFAULT_HEIGHT,
            search_split: None,
            terminal_font_size: DEFAULT_TERMINAL_FONT_SIZE,
            terminal_min_contrast: DEFAULT_TERMINAL_MIN_CONTRAST,
            terminal_shell: String::new(),
            view: ViewOptions::default(),
            show_output: false,
            light_scheme: String::new(),
            dark_scheme: String::new(),
            wide_cols: 1,
            language: Default::default(),
        }
    }
}

impl Settings {
    /// 這個主題目前選的配色名稱（空字串表示預設）。
    pub fn scheme_name(&self, dark: bool) -> &str {
        if dark { &self.dark_scheme } else { &self.light_scheme }
    }

    /// 這個檔案類型（語法上色的語言名稱；純文字等沒有語言的是 `None`）的 Tab 欄數：有個別設定就用它，否則用預設的。
    pub fn tab_width_for(&self, language: Option<&str>) -> usize {
        language.and_then(|name| self.tab_widths.get(name)).copied().unwrap_or(self.tab_width) as usize
    }

    /// 這個檔案類型（`None` 是純文字或認不出來的）要不要自動換行：有個別設定就用它，否則用預設值 `view.word_wrap`。
    pub fn word_wrap_for(&self, language: Option<&str>) -> bool {
        self.wrap_langs.get(language.unwrap_or(PLAIN_TEXT_KEY)).copied().unwrap_or(self.view.word_wrap)
    }

    /// 設定這個檔案類型的自動換行；`None` 移除個別設定（改用預設值）。
    pub fn set_word_wrap_for(&mut self, language: Option<&str>, on: Option<bool>) {
        let key = language.unwrap_or(PLAIN_TEXT_KEY).to_owned();
        match on {
            Some(on) => self.wrap_langs.insert(key, on),
            None => self.wrap_langs.remove(&key),
        };
    }

    /// 使用者替這個檔案類型指定的括號內斷行風格；沒指定回傳 `None`（用語言本身的）。
    pub fn wrap_style_for(&self, language: &str) -> Option<crate::syntax::WrapStyle> {
        self.wrap_styles.get(language).copied()
    }

    pub fn set_scheme_name(&mut self, dark: bool, name: &str) {
        let slot = if dark { &mut self.dark_scheme } else { &mut self.light_scheme };
        *slot = name.to_owned();
    }
}

impl Settings {
    /// 解析設定檔內容。空行與以 `#` 開頭的行會被略過；不認得的鍵或值一律忽略。
    pub fn parse(text: &str) -> Self {
        let mut settings = Self::default();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let value = value.trim();
            match key.trim() {
                "theme" => {
                    if let Some(theme) = ThemeChoice::parse(value) {
                        settings.theme = theme;
                    }
                }
                "font_family" => {
                    let name = value.trim_matches('"').trim();
                    settings.font_family = (!name.is_empty()).then(|| name.to_owned());
                }
                "font_size" => {
                    // 讀不懂或不是有限數字就忽略；超出範圍的拉回範圍內
                    if let Ok(size) = value.parse::<f32>() {
                        if size.is_finite() {
                            settings.font_size = clamp_font_size(size);
                        }
                    }
                }
                "preview_font_size" => {
                    if let Ok(size) = value.parse::<f32>() {
                        settings.preview_font_size = if size.is_finite() {
                            size.clamp(MIN_FONT_SIZE, MAX_FONT_SIZE)
                        } else {
                            DEFAULT_PREVIEW_FONT_SIZE
                        };
                    }
                }
                "tab_width" => {
                    if let Ok(n) = value.parse::<u32>() {
                        settings.tab_width = n.clamp(MIN_TAB_COLUMNS as u32, MAX_TAB_COLUMNS as u32) as u8;
                    }
                }
                // tab_width.語言名稱 = 欄數：這個檔案類型自己的 Tab 欄數
                key if key.starts_with("tab_width.") => {
                    let name = key["tab_width.".len()..].trim();
                    if let (false, Ok(n)) = (name.is_empty(), value.parse::<u32>()) {
                        settings.tab_widths.insert(name.to_owned(), n.clamp(MIN_TAB_COLUMNS as u32, MAX_TAB_COLUMNS as u32) as u8);
                    }
                }
                // run_args.檔案路徑 = 參數：這個檔案「執行」時的命令列參數
                key if key.starts_with("run_args.") => {
                    let file = key["run_args.".len()..].trim();
                    if !file.is_empty() && !value.is_empty() {
                        settings.run_args.insert(file.to_owned(), value.to_owned());
                    }
                }
                // recent = 路徑：最近開啟的檔案（一行一個，最近的在前面）
                "recent" => {
                    if !value.is_empty() {
                        settings.recent_files.push(value.to_owned());
                    }
                }
                // runtime.專案資料夾 = 直譯器：這個專案手動選的執行環境
                key if key.starts_with("runtime.") => {
                    let project = key["runtime.".len()..].trim();
                    let value = value.trim_matches('"').trim();
                    if !project.is_empty() && !value.is_empty() {
                        settings.runtime_choices.insert(project.to_owned(), value.to_owned());
                    }
                }
                // word_wrap.語言名稱 = on|off：這個檔案類型自己的「自動換行」
                key if key.starts_with("word_wrap.") => {
                    let name = key["word_wrap.".len()..].trim();
                    if let (false, Some(on)) = (name.is_empty(), parse_bool(value)) {
                        settings.wrap_langs.insert(name.to_owned(), on);
                    }
                }
                // wrap_style.語言名稱 = align|hang：這個檔案類型括號內斷行的風格
                key if key.starts_with("wrap_style.") => {
                    let name = key["wrap_style.".len()..].trim();
                    if let (false, Some(style)) = (name.is_empty(), crate::syntax::WrapStyle::parse(value)) {
                        settings.wrap_styles.insert(name.to_owned(), style);
                    }
                }
                "shell_menu" => {
                    if let Some(on) = parse_bool(value) {
                        settings.shell_menu = on;
                    }
                }
                "cli_command" => {
                    if let Some(on) = parse_bool(value) {
                        settings.cli_command = on;
                    }
                }
                "file_assoc" => {
                    if let Some(on) = parse_bool(value) {
                        settings.file_assoc = on;
                    }
                }
                // 逗號分隔的類別名稱；寫了就以它為準（可以是空的：一類都不接手），不認得的名稱略過
                "file_assoc_groups" => {
                    settings.file_assoc_groups = value
                        .split(',')
                        .map(str::trim)
                        .filter(|k| crate::file_assoc::GROUPS.iter().any(|g| g.key == *k))
                        .map(str::to_owned)
                        .collect();
                }
                "search_dock" => {
                    if let Some(edge) = crate::search_dock::DockEdge::parse(value) {
                        settings.search_dock_edge = edge;
                    }
                }
                "search_dock_width" => {
                    if let Ok(v) = value.parse::<f32>() {
                        settings.search_dock_width = v.clamp(crate::search_dock::MIN_WIDTH, 4000.0);
                    }
                }
                "search_dock_height" => {
                    if let Ok(v) = value.parse::<f32>() {
                        settings.search_dock_height = v.clamp(crate::search_dock::MIN_HEIGHT, 4000.0);
                    }
                }
                "search_split" => {
                    if let Ok(v) = value.parse::<f32>() {
                        settings.search_split = v.is_finite().then(|| v.clamp(0.1, 0.9));
                    }
                }
                "tab_spaces" => {
                    if let Some(on) = parse_bool(value) {
                        settings.tab_spaces = on;
                    }
                }
                "auto_indent" => {
                    if let Some(on) = parse_bool(value) {
                        settings.auto_indent = on;
                    }
                }
                "terminal_font_size" => {
                    if let Ok(size) = value.parse::<f32>() {
                        settings.terminal_font_size =
                            if size.is_finite() { size.clamp(MIN_FONT_SIZE, MAX_FONT_SIZE) } else { DEFAULT_TERMINAL_FONT_SIZE };
                    }
                }
                "terminal_min_contrast" => {
                    if let Ok(ratio) = value.parse::<f32>() {
                        settings.terminal_min_contrast = if ratio.is_finite() {
                            ratio.clamp(MIN_TERMINAL_CONTRAST, MAX_TERMINAL_CONTRAST)
                        } else {
                            DEFAULT_TERMINAL_MIN_CONTRAST
                        };
                    }
                }
                "terminal_shell" => settings.terminal_shell = value.trim_matches('"').trim().to_owned(),
                "ruler_column" => {
                    // 讀不懂就忽略；超出範圍的拉回範圍內。0 是舊版「不畫」的寫法：視為沒啟用，欄數留預設
                    if let Ok(n) = value.parse::<u32>() {
                        if n == 0 {
                            settings.view.ruler = false;
                        } else {
                            settings.view.ruler_column = n.clamp(MIN_RULER_COLUMN as u32, MAX_RULER_COLUMN as u32) as u16;
                        }
                    }
                }
                "light_scheme" => settings.light_scheme = value.trim_matches('"').trim().to_owned(),
                "dark_scheme" => settings.dark_scheme = value.trim_matches('"').trim().to_owned(),
                "language" => {
                    if let Some(l) = feditor_core::i18n::LangChoice::parse(value) {
                        settings.language = l;
                    }
                }
                "wide_cols" => match value {
                    "1" => settings.wide_cols = 1,
                    "2" => settings.wide_cols = 2,
                    _ => {}
                },
                "show_output" => {
                    if let Some(on) = parse_bool(value) {
                        settings.show_output = on;
                    }
                }
                key @ ("show_whitespace" | "show_line_numbers" | "highlight_line" | "word_wrap" | "syntax" | "ruler") => {
                    let Some(on) = parse_bool(value) else { continue };
                    match key {
                        "show_whitespace" => settings.view.show_whitespace = on,
                        "show_line_numbers" => settings.view.show_line_numbers = on,
                        "highlight_line" => settings.view.highlight_line = on,
                        "syntax" => settings.view.syntax = on,
                        "ruler" => settings.view.ruler = on,
                        _ => settings.view.word_wrap = on,
                    }
                }
                _ => {}
            }
        }
        // 最近開啟的檔案：去掉重複與不能用的，最多十個
        settings.recent_files = crate::recent::clean(std::mem::take(&mut settings.recent_files));
        settings
    }

    pub fn to_text(&self) -> String {
        let mut text = format!("theme = {}\n", self.theme.key());
        if let Some(name) = &self.font_family {
            text.push_str(&format!("font_family = {name}\n"));
        }
        text.push_str(&format!("font_size = {}\n", self.font_size));
        text.push_str(&format!("preview_font_size = {}\n", self.preview_font_size));
        text.push_str(&format!("tab_width = {}\ntab_spaces = {}\n", self.tab_width, self.tab_spaces));
        text.push_str(&format!("auto_indent = {}\n", self.auto_indent));
        text.push_str(&format!("shell_menu = {}\n", self.shell_menu));
        text.push_str(&format!("cli_command = {}\n", self.cli_command));
        text.push_str(&format!("file_assoc = {}\n", self.file_assoc));
        let groups: Vec<&str> = self.file_assoc_groups.iter().map(String::as_str).collect();
        text.push_str(&format!("file_assoc_groups = {}\n", groups.join(",")));
        text.push_str(&format!("search_dock = {}\nsearch_dock_width = {}\nsearch_dock_height = {}\n", self.search_dock_edge.name(), self.search_dock_width.round(), self.search_dock_height.round()));
        if let Some(split) = self.search_split {
            text.push_str(&format!("search_split = {split:.3}\n"));
        }
        for (name, width) in &self.tab_widths {
            // 名稱是單行文字，也不能含等號（會被當成鍵值的分隔）；正常的語言名稱不會有
            if !name.is_empty() && !name.contains(['\r', '\n', '=']) {
                text.push_str(&format!("tab_width.{name} = {width}\n"));
            }
        }
        for (project, program) in &self.runtime_choices {
            // 專案資料夾與直譯器都是單行文字；資料夾含等號會被當成鍵值的分隔，那種就不記
            if !project.is_empty() && !project.contains(['\r', '\n', '=']) && !program.contains(['\r', '\n']) {
                text.push_str(&format!("runtime.{project} = {program}\n"));
            }
        }
        for path in crate::recent::clean(self.recent_files.clone()) {
            text.push_str(&format!("recent = {path}\n"));
        }
        for (file, args) in &self.run_args {
            if !file.is_empty() && !file.contains(['\r', '\n', '=']) && !args.contains(['\r', '\n']) && !args.trim().is_empty() {
                text.push_str(&format!("run_args.{file} = {}\n", args.trim()));
            }
        }
        for (name, on) in &self.wrap_langs {
            if !name.is_empty() && !name.contains(['\r', '\n', '=']) {
                text.push_str(&format!("word_wrap.{name} = {on}\n"));
            }
        }
        for (name, style) in &self.wrap_styles {
            if !name.is_empty() && !name.contains(['\r', '\n', '=']) {
                text.push_str(&format!("wrap_style.{name} = {}\n", style.name()));
            }
        }
        text.push_str(&format!("terminal_font_size = {}\n", self.terminal_font_size));
        text.push_str(&format!("terminal_min_contrast = {}\n", self.terminal_min_contrast));
        if !self.terminal_shell.is_empty() {
            text.push_str(&format!("terminal_shell = {}\n", self.terminal_shell.replace(['\r', '\n'], " ")));
        }
        let v = self.view;
        text.push_str(&format!(
            "show_whitespace = {}\nshow_line_numbers = {}\nhighlight_line = {}\nword_wrap = {}\nsyntax = {}\n",
            v.show_whitespace, v.show_line_numbers, v.highlight_line, v.word_wrap, v.syntax
        ));
        text.push_str(&format!("ruler = {}\nruler_column = {}\n", v.ruler, v.ruler_column));
        text.push_str(&format!("show_output = {}\n", self.show_output));
        text.push_str(&format!("wide_cols = {}\n", self.wide_cols));
        text.push_str(&format!("language = {}\n", self.language.key()));
        // 名稱是單行文字；有換行的話會破壞設定檔，所以換成空白（正常情況不會有）
        for (key, name) in [("light_scheme", &self.light_scheme), ("dark_scheme", &self.dark_scheme)] {
            if !name.is_empty() {
                text.push_str(&format!("{key} = {}\n", name.replace(['\r', '\n'], " ")));
            }
        }
        text
    }

    /// 從指定路徑讀取；檔案不存在或無法讀取時回傳預設值。
    pub fn load_from(path: &Path) -> Self {
        std::fs::read(path)
            .map(|bytes| Self::parse(&String::from_utf8_lossy(&bytes)))
            .unwrap_or_default()
    }

    /// 寫到指定路徑（會建立不存在的資料夾）。
    pub fn save_to(&self, path: &Path) -> Result<(), String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| tf!("無法建立 {}：{}", dir.display(), e))?;
        }
        let format = FileFormat {
            line_ending: LineEnding::Lf,
            ..FileFormat::default()
        };
        fileio::write_file(path, &self.to_text(), &format).map_err(|e| e.to_string())
    }

    /// 設定檔的預設位置；取不到使用者設定目錄時回傳 `None`（此時設定只在本次執行有效）。
    pub fn default_path() -> Option<PathBuf> {
        config_dir().map(|dir| dir.join("feditor").join("settings.conf"))
    }

    pub fn load() -> Self {
        Self::default_path()
            .map(|p| Self::load_from(&p))
            .unwrap_or_default()
    }

    pub fn save(&self) -> Result<(), String> {
        let path = Self::default_path().ok_or(t("找不到使用者設定目錄，設定不會被保存"))?;
        self.save_to(&path)
    }
}

#[cfg(windows)]
fn config_dir() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(PathBuf::from)
}

#[cfg(target_os = "macos")]
fn config_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support"))
}

#[cfg(all(not(windows), not(target_os = "macos")))]
fn config_dir() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_follows_the_system() {
        assert_eq!(Settings::default().theme, ThemeChoice::System);
    }

    #[test]
    fn next_cycles_through_all_three_and_comes_back() {
        let mut theme = ThemeChoice::System;
        let mut seen = Vec::new();
        for _ in 0..3 {
            theme = theme.next();
            seen.push(theme);
        }
        assert_eq!(
            seen,
            [ThemeChoice::Light, ThemeChoice::Dark, ThemeChoice::System]
        );
    }

    #[test]
    fn roundtrip_through_text() {
        for theme in [ThemeChoice::System, ThemeChoice::Light, ThemeChoice::Dark] {
            let s = Settings {
                theme,
                ..Settings::default()
            };
            assert_eq!(Settings::parse(&s.to_text()), s);
        }
    }


    #[test]
    fn font_settings_roundtrip_including_spaces_and_cjk() {
        for family in ["Courier New", "細明體", "Cascadia Mono", "Source Code Pro"] {
            let s = Settings {
                theme: ThemeChoice::Dark,
                font_family: Some(family.to_owned()),
                font_size: 15.5,
                ..Settings::default()
            };
            assert_eq!(Settings::parse(&s.to_text()), s, "{family}");
        }
    }

    #[test]
    fn default_family_is_not_written_and_reads_back_as_none() {
        let s = Settings::default();
        assert!(!s.to_text().contains("font_family"));
        assert_eq!(Settings::parse(&s.to_text()).font_family, None);
        assert_eq!(Settings::parse("font_family =   ").font_family, None);
        assert_eq!(
            Settings::parse("font_family = \"Consolas\"").font_family.as_deref(),
            Some("Consolas")
        );
    }

    #[test]
    fn font_size_is_validated_and_clamped() {
        assert_eq!(Settings::parse("font_size = 20").font_size, 20.0);
        assert_eq!(Settings::parse("font_size = 1").font_size, MIN_FONT_SIZE);
        assert_eq!(Settings::parse("font_size = 9999").font_size, MAX_FONT_SIZE);
        // 讀不懂、不是數字、無限大：維持預設，不會把先前有效的值洗掉
        assert_eq!(Settings::parse("font_size = big").font_size, DEFAULT_FONT_SIZE);
        assert_eq!(Settings::parse("font_size = NaN").font_size, DEFAULT_FONT_SIZE);
        assert_eq!(Settings::parse("font_size = inf").font_size, DEFAULT_FONT_SIZE);
        assert_eq!(
            Settings::parse("font_size = 18\nfont_size = oops").font_size,
            18.0
        );
    }

    #[test]
    fn zoom_steps_converts_wheel_zoom_to_points() {
        let mut acc = 0.0;
        // 沒有縮放：不動
        assert_eq!(zoom_steps(&mut acc, 1.0), 0);
        // 一格滾輪放大（約 e^0.25）：兩點，餘數留著
        assert_eq!(zoom_steps(&mut acc, 0.25_f32.exp()), 2);
        assert!((acc - 0.05).abs() < 1e-5);
        // 縮小對稱：e^-0.25 先抵銷餘數，再退 2 點
        let mut acc = 0.0;
        assert_eq!(zoom_steps(&mut acc, (-0.25_f32).exp()), -2);
    }

    #[test]
    fn zoom_steps_accumulates_small_touchpad_deltas() {
        let mut acc = 0.0;
        let mut total = 0;
        // 觸控板捏合是連續的小量：每次 0.01，累積 10 次才該動 1 點，而且不能漏
        for _ in 0..100 {
            total += zoom_steps(&mut acc, 0.01_f32.exp());
        }
        assert_eq!(total, 10);
    }

    #[test]
    fn zoom_steps_ignores_garbage() {
        let mut acc = 0.0;
        assert_eq!(zoom_steps(&mut acc, f32::NAN), 0);
        assert_eq!(zoom_steps(&mut acc, f32::INFINITY), 0);
        assert_eq!(zoom_steps(&mut acc, 0.0), 0);
        assert_eq!(zoom_steps(&mut acc, -1.0), 0);
        assert_eq!(acc, 0.0, "無效的輸入不可汙染累積量");
    }

    #[test]
    fn the_margin_guide_is_on_by_default_with_80_columns_the_column_is_clamped_and_both_round_trip() {
        let d = Settings::default().view;
        assert_eq!((d.ruler, d.ruler_column), (true, DEFAULT_RULER_COLUMN));
        let s = Settings::parse("ruler = false\nruler_column = 100\n").view;
        assert_eq!((s.ruler, s.ruler_column), (false, 100), "沒啟用時欄數也記著");
        assert_eq!(Settings::parse("ruler_column = 99999").view.ruler_column, MAX_RULER_COLUMN);
        assert_eq!(Settings::parse("ruler_column = -3").view.ruler_column, DEFAULT_RULER_COLUMN, "讀不懂就忽略");
        assert_eq!(Settings::parse("ruler_column = wide").view.ruler_column, DEFAULT_RULER_COLUMN);
        assert_eq!(Settings::parse("ruler = maybe").view.ruler, true, "讀不懂就用預設");
        // 舊版用 0 表示不畫：視為沒啟用，欄數留預設
        let legacy = Settings::parse("ruler_column = 0").view;
        assert_eq!((legacy.ruler, legacy.ruler_column), (false, DEFAULT_RULER_COLUMN));
        let mut s = Settings::default();
        s.view.ruler = false;
        s.view.ruler_column = 72;
        let back = Settings::parse(&s.to_text()).view;
        assert_eq!((back.ruler, back.ruler_column), (false, 72), "關閉也會記住，欄數也留著");
        s.view.ruler = true;
        assert_eq!(Settings::parse(&s.to_text()).view.ruler, true);
    }
    #[test]
    fn the_shell_menu_switch_is_off_by_default_and_round_trips() {
        assert!(!Settings::default().shell_menu);
        assert!(Settings::parse("shell_menu = true").shell_menu);
        assert!(!Settings::parse("shell_menu = maybe").shell_menu);
        let mut s = Settings::default();
        s.shell_menu = true;
        assert!(Settings::parse(&s.to_text()).shell_menu);
    }

    #[test]
    fn word_wrap_follows_the_file_type_and_falls_back_to_the_default() {
        // 舊的設定檔只有一個全域的 word_wrap：它成為所有類型的預設值
        let old = Settings::parse("word_wrap = true\n");
        assert!(old.word_wrap_for(Some("Rust")) && old.word_wrap_for(None));
        let s = Settings::parse("word_wrap = false\nword_wrap.Markdown = on\nword_wrap.Plain text = true\nword_wrap.Rust = off\nword_wrap. = on\nword_wrap.Go = maybe\n");
        assert!(s.word_wrap_for(Some("Markdown")));
        assert!(s.word_wrap_for(None), "純文字有自己的設定");
        assert!(!s.word_wrap_for(Some("Rust")));
        assert!(!s.word_wrap_for(Some("Go")), "讀不懂的值忽略，用預設");
        assert_eq!(s.wrap_langs.len(), 3, "空名稱與讀不懂的都不收");
        // 寫出再讀回來
        let mut custom = Settings::default();
        custom.set_word_wrap_for(Some("Python"), Some(true));
        custom.set_word_wrap_for(None, Some(true));
        custom.set_word_wrap_for(Some("bad=name"), Some(true));
        let back = Settings::parse(&custom.to_text());
        assert!(back.word_wrap_for(Some("Python")) && back.word_wrap_for(None));
        assert!(!back.wrap_langs.contains_key("bad=name"), "含等號的名稱不寫進設定檔");
        // 移除個別設定：回到預設
        custom.set_word_wrap_for(Some("Python"), None);
        assert!(!custom.word_wrap_for(Some("Python")));
    }

    #[test]
    fn tab_settings_have_a_default_per_file_type_overrides_and_a_global_spaces_switch() {
        let d = Settings::default();
        assert_eq!((d.tab_width, d.tab_spaces, d.tab_widths.len()), (4, false, 0));
        let s = Settings::parse("tab_width = 8\ntab_spaces = true\ntab_width.Python = 4\ntab_width.C/C++ = 2\n");
        assert_eq!((s.tab_width, s.tab_spaces), (8, true));
        assert_eq!(s.tab_width_for(Some("Python")), 4);
        assert_eq!(s.tab_width_for(Some("C/C++")), 2);
        assert_eq!(s.tab_width_for(Some("Go")), 8, "沒有個別設定的用預設的");
        assert_eq!(s.tab_width_for(None), 8, "純文字也用預設的");
        // 範圍與壞值
        assert_eq!(Settings::parse("tab_width = 0").tab_width, MIN_TAB_COLUMNS);
        assert_eq!(Settings::parse("tab_width = 99").tab_width, MAX_TAB_COLUMNS);
        assert_eq!(Settings::parse("tab_width = wide").tab_width, DEFAULT_TAB_COLUMNS);
        assert_eq!(Settings::parse("tab_width.Rust = 99").tab_widths["Rust"], MAX_TAB_COLUMNS);
        assert!(Settings::parse("tab_width.Rust = x\ntab_width. = 3").tab_widths.is_empty());
        // 存檔再讀回來（含名稱裡有空白、符號的語言）
        let mut custom = Settings::default();
        custom.tab_width = 2;
        custom.tab_spaces = true;
        custom.tab_widths.insert("Shell".to_owned(), 8);
        custom.tab_widths.insert("My Lang+".to_owned(), 3);
        custom.tab_widths.insert("bad=name".to_owned(), 5);
        let back = Settings::parse(&custom.to_text());
        assert_eq!((back.tab_width, back.tab_spaces), (2, true));
        assert_eq!(back.tab_widths.get("Shell"), Some(&8));
        assert_eq!(back.tab_widths.get("My Lang+"), Some(&3));
        assert!(!back.tab_widths.contains_key("bad=name") && !back.tab_widths.contains_key("bad"), "含等號的名稱不寫進設定檔");
    }

    #[test]
    fn file_association_is_off_by_default_picks_code_and_config_and_round_trips() {
        let d = Settings::default();
        assert!(!d.file_assoc);
        assert_eq!(d.file_assoc_groups.iter().map(String::as_str).collect::<Vec<_>>(), ["code", "config"]);
        let s = Settings::parse("file_assoc = true\nfile_assoc_groups = text, markup , nonsense\n");
        assert!(s.file_assoc);
        assert_eq!(s.file_assoc_groups.iter().map(String::as_str).collect::<Vec<_>>(), ["markup", "text"], "不認得的名稱略過");
        assert!(Settings::parse("file_assoc_groups =\n").file_assoc_groups.is_empty(), "寫了空的就是一類都不接手");
        assert_eq!(Settings::parse("file_assoc = maybe").file_assoc_groups, d.file_assoc_groups, "沒寫就用預設");
        let mut custom = Settings::default();
        custom.file_assoc = true;
        custom.file_assoc_groups = ["text".to_owned()].into();
        let back = Settings::parse(&custom.to_text());
        assert!(back.file_assoc);
        assert_eq!(back.file_assoc_groups, custom.file_assoc_groups);
        custom.file_assoc_groups.clear();
        assert!(Settings::parse(&custom.to_text()).file_assoc_groups.is_empty(), "全部取消勾選也會記住");
    }

    #[test]
    fn the_search_panel_layout_is_remembered() {
        use crate::search_dock::DockEdge;
        let d = Settings::default();
        assert_eq!((d.search_dock_edge, d.search_split), (DockEdge::Right, None));
        let mut s = Settings::default();
        s.search_dock_edge = DockEdge::Bottom;
        s.search_dock_width = 412.0;
        s.search_dock_height = 301.0;
        s.search_split = Some(0.35);
        let back = Settings::parse(&s.to_text());
        assert_eq!(back.search_dock_edge, DockEdge::Bottom);
        assert_eq!((back.search_dock_width, back.search_dock_height), (412.0, 301.0));
        assert_eq!(back.search_split, Some(0.35));
        let odd = Settings::parse("search_dock = middle\nsearch_dock_width = 5\nsearch_dock_height = abc\nsearch_split = 7\n");
        assert_eq!(odd.search_dock_edge, DockEdge::Right, "不認得的邊用預設");
        assert_eq!(odd.search_dock_width, crate::search_dock::MIN_WIDTH, "太小的寬度調回最小值");
        assert_eq!(odd.search_dock_height, crate::search_dock::DEFAULT_HEIGHT, "不是數字就用預設");
        assert_eq!(odd.search_split, Some(0.9));
    }

    #[test]
    fn run_arguments_are_remembered_per_file() {
        assert!(Settings::default().run_args.is_empty());
        let s = Settings::parse("run_args.C:\\code\\app.py = --port=8080 \"a b\"\nrun_args. = x\nrun_args.empty.py =\n");
        assert_eq!(s.run_args.len(), 1);
        assert_eq!(s.run_args["C:\\code\\app.py"], "--port=8080 \"a b\"", "值裡面的等號與引號原樣保留");
        let mut t = Settings::default();
        t.run_args.insert("/home/me/a.py".to_owned(), "  -v  ".to_owned());
        t.run_args.insert("blank.py".to_owned(), "   ".to_owned());
        t.run_args.insert("bad=name.py".to_owned(), "x".to_owned());
        let back = Settings::parse(&t.to_text());
        assert_eq!(back.run_args.get("/home/me/a.py").map(String::as_str), Some("-v"));
        assert_eq!(back.run_args.len(), 1, "空白與含等號的路徑不寫進設定檔");
    }

    #[test]
    fn recent_files_round_trip_in_order_and_are_capped_and_deduplicated() {
        assert!(Settings::default().recent_files.is_empty());
        let s = Settings::parse("recent = C:\\a b\\x=1.txt\nrecent = /home/me/y.md\nrecent =\nrecent = /home/me/y.md\n");
        assert_eq!(s.recent_files, ["C:\\a b\\x=1.txt", "/home/me/y.md"], "順序保留（最近的在前面），路徑裡的等號與空白原樣，空的、重複的去掉");
        let text = s.to_text();
        assert!(text.contains("recent = C:\\a b\\x=1.txt\nrecent = /home/me/y.md\n"));
        assert_eq!(Settings::parse(&text).recent_files, s.recent_files);
        let many: String = (0..25).map(|i| format!("recent = /f/{i}.txt\n")).collect();
        assert_eq!(Settings::parse(&many).recent_files.len(), crate::recent::MAX_RECENT);
        let mut t = Settings::default();
        t.recent_files = vec!["/ok.txt".to_owned(), "bad\nname".to_owned()];
        assert_eq!(Settings::parse(&t.to_text()).recent_files, ["/ok.txt"], "含換行的路徑不寫進設定檔");
    }

    #[test]
    fn manual_runtime_choices_are_remembered_per_project() {
        assert!(Settings::default().runtime_choices.is_empty());
        let s = Settings::parse("runtime.C:\\code\\app = C:\\code\\app\\.venv\\Scripts\\python.exe\nruntime. = x\nruntime.empty =\n");
        assert_eq!(s.runtime_choices.len(), 1);
        assert_eq!(s.runtime_choices["C:\\code\\app"], "C:\\code\\app\\.venv\\Scripts\\python.exe");
        let mut t = Settings::default();
        t.runtime_choices.insert("/home/me/app".to_owned(), "/usr/bin/python3".to_owned());
        t.runtime_choices.insert("bad=name".to_owned(), "x".to_owned());
        let back = Settings::parse(&t.to_text());
        assert_eq!(back.runtime_choices.get("/home/me/app").map(String::as_str), Some("/usr/bin/python3"));
        assert!(back.runtime_choices.len() == 1, "含等號的資料夾不寫進設定檔");
    }

    #[test]
    fn the_cli_command_switch_is_off_by_default_and_round_trips() {
        assert!(!Settings::default().cli_command);
        assert!(Settings::parse("cli_command = true").cli_command);
        assert!(!Settings::parse("cli_command = maybe").cli_command);
        let mut s = Settings::default();
        s.cli_command = true;
        assert!(Settings::parse(&s.to_text()).cli_command);
    }

    #[test]
    fn wrap_styles_are_per_language_and_round_trip() {
        use crate::syntax::WrapStyle;
        assert!(Settings::default().wrap_styles.is_empty());
        let s = Settings::parse("wrap_style.Rust = align\nwrap_style.Python = HANG\nwrap_style.Go = zigzag\nwrap_style. = hang\n");
        assert_eq!(s.wrap_style_for("Rust"), Some(WrapStyle::Align));
        assert_eq!(s.wrap_style_for("Python"), Some(WrapStyle::Hang));
        assert_eq!(s.wrap_style_for("Go"), None, "看不懂的值忽略");
        assert_eq!(s.wrap_styles.len(), 2);
        let back = Settings::parse(&s.to_text());
        assert_eq!(back.wrap_styles, s.wrap_styles);
    }

    #[test]
    fn auto_indent_is_on_by_default_and_round_trips() {
        assert!(Settings::default().auto_indent);
        assert!(!Settings::parse("auto_indent = false").auto_indent);
        assert!(Settings::parse("auto_indent = maybe").auto_indent, "看不懂就維持預設");
        let mut s = Settings::default();
        s.auto_indent = false;
        assert!(!Settings::parse(&s.to_text()).auto_indent);
    }

    #[test]
    fn terminal_settings_are_validated_and_round_trip() {
        let d = Settings::default();
        assert_eq!((d.terminal_font_size, d.terminal_min_contrast, d.terminal_shell.as_str()), (DEFAULT_TERMINAL_FONT_SIZE, 3.0, ""));
        let s = Settings::parse("terminal_font_size = 18\nterminal_min_contrast = 4.5\nterminal_shell = \"pwsh -NoLogo\"\n");
        assert_eq!((s.terminal_font_size, s.terminal_min_contrast, s.terminal_shell.as_str()), (18.0, 4.5, "pwsh -NoLogo"));
        // 超出範圍的拉回範圍；不是數字就用預設；對比度 1 是關閉
        assert_eq!(Settings::parse("terminal_font_size = 999").terminal_font_size, MAX_FONT_SIZE);
        assert_eq!(Settings::parse("terminal_font_size = nan").terminal_font_size, DEFAULT_TERMINAL_FONT_SIZE);
        assert_eq!(Settings::parse("terminal_min_contrast = 0").terminal_min_contrast, MIN_TERMINAL_CONTRAST);
        assert_eq!(Settings::parse("terminal_min_contrast = 99").terminal_min_contrast, MAX_TERMINAL_CONTRAST);
        assert_eq!(Settings::parse("terminal_min_contrast = inf").terminal_min_contrast, DEFAULT_TERMINAL_MIN_CONTRAST);
        assert_eq!(Settings::parse("terminal_min_contrast = 1").terminal_min_contrast, 1.0);
        // 存檔再讀回來（shell 是空的就不寫）
        let mut custom = Settings::default();
        custom.terminal_font_size = 11.5;
        custom.terminal_min_contrast = 1.0;
        custom.terminal_shell = "bash\n--login".to_owned();
        let text = custom.to_text();
        assert_eq!(text.lines().filter(|l| l.starts_with("terminal_shell")).count(), 1, "換行不會破壞設定檔");
        let back = Settings::parse(&text);
        assert_eq!((back.terminal_font_size, back.terminal_min_contrast, back.terminal_shell.as_str()), (11.5, 1.0, "bash --login"));
        assert!(!Settings::default().to_text().contains("terminal_shell"));
    }

    #[test]
    fn preview_font_size_is_separate_validated_and_round_trips() {
        assert_eq!(Settings::default().preview_font_size, DEFAULT_PREVIEW_FONT_SIZE);
        assert_eq!(Settings::parse("preview_font_size = 19.5").preview_font_size, 19.5);
        assert_eq!(Settings::parse("preview_font_size = 1").preview_font_size, MIN_FONT_SIZE);
        assert_eq!(Settings::parse("preview_font_size = 999").preview_font_size, MAX_FONT_SIZE);
        assert_eq!(Settings::parse("preview_font_size = inf").preview_font_size, DEFAULT_PREVIEW_FONT_SIZE);
        assert_eq!(Settings::parse("preview_font_size = big").preview_font_size, DEFAULT_PREVIEW_FONT_SIZE);
        let mut s = Settings::default();
        s.preview_font_size = 17.0;
        s.font_size = 12.0;
        let back = Settings::parse(&s.to_text());
        assert_eq!((back.preview_font_size, back.font_size), (17.0, 12.0), "和編輯區的字級各自獨立");
    }

    #[test]
    fn adjust_font_size_steps_and_stays_in_range() {
        assert_eq!(adjust_font_size(13.0, 1.0), 14.0);
        assert_eq!(adjust_font_size(13.0, -1.0), 12.0);
        assert_eq!(adjust_font_size(MIN_FONT_SIZE, -1.0), MIN_FONT_SIZE);
        assert_eq!(adjust_font_size(MAX_FONT_SIZE, 1.0), MAX_FONT_SIZE);
        assert_eq!(clamp_font_size(f32::NAN), DEFAULT_FONT_SIZE);
        assert_eq!(clamp_font_size(f32::INFINITY), DEFAULT_FONT_SIZE);
    }
    #[test]
    fn parser_is_forgiving() {
        let text = "# 註解\n\n  unknown = 1  \nno equals sign\n  theme =  \"Dark\"  \n";
        assert_eq!(Settings::parse(text).theme, ThemeChoice::Dark);
        // CRLF 也能讀
        assert_eq!(Settings::parse("theme = light\r\n").theme, ThemeChoice::Light);
        // 值不認得就維持預設
        assert_eq!(Settings::parse("theme = purple").theme, ThemeChoice::System);
        // 後面的有效設定覆蓋前面的；無效的不會洗掉先前有效的
        assert_eq!(
            Settings::parse("theme = dark\ntheme = nonsense").theme,
            ThemeChoice::Dark
        );
        assert_eq!(Settings::parse("").theme, ThemeChoice::System);
    }

    #[test]
    fn missing_or_garbage_file_falls_back_to_default() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            Settings::load_from(&dir.path().join("nope.conf")),
            Settings::default()
        );
        let bad = dir.path().join("bad.conf");
        std::fs::write(&bad, [0xFF, 0xFE, 0x00, 0x80, 0xC3]).unwrap();
        assert_eq!(Settings::load_from(&bad), Settings::default());
    }

    #[test]
    fn save_creates_missing_folders_and_reads_back() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a").join("b").join("settings.conf");
        let s = Settings {
            theme: ThemeChoice::Dark,
            ..Settings::default()
        };
        s.save_to(&path).unwrap();
        assert_eq!(Settings::load_from(&path), s);
        // 改寫後覆蓋舊值，且不留暫存檔
        let s2 = Settings {
            theme: ThemeChoice::Light,
            ..Settings::default()
        };
        s2.save_to(&path).unwrap();
        assert_eq!(Settings::load_from(&path), s2);
        assert_eq!(std::fs::read_dir(path.parent().unwrap()).unwrap().count(), 1);
    }

    #[test]
    fn converts_to_egui_preference() {
        assert_eq!(
            egui::ThemePreference::from(ThemeChoice::Dark),
            egui::ThemePreference::Dark
        );
        assert_eq!(
            egui::ThemePreference::from(ThemeChoice::System),
            egui::ThemePreference::System
        );
    }

    #[test]
    fn view_options_round_trip_and_ignore_bad_values() {
        let mut st = Settings::default();
        st.view.show_whitespace = true;
        st.view.word_wrap = true;
        st.view.highlight_line = false;
        st.view.syntax = false;
        assert_eq!(Settings::parse(&st.to_text()), st);
        let parsed = Settings::parse("show_whitespace = maybe\nshow_line_numbers = off\n");
        assert!(!parsed.view.show_whitespace, "不認得的值忽略");
        assert!(!parsed.view.show_line_numbers);
        assert!(Settings::default().view.show_line_numbers);
    }

    #[test]
    fn language_setting_round_trips_and_defaults_to_system() {
        use feditor_core::i18n::LangChoice;
        assert_eq!(Settings::default().language, LangChoice::System);
        for l in LangChoice::ALL {
            let mut s = Settings::default();
            s.language = l;
            assert_eq!(Settings::parse(&s.to_text()).language, l);
        }
        assert_eq!(Settings::parse("language = klingon\n").language, LangChoice::System);
    }

    #[test]
    fn wide_cols_setting_round_trips_and_rejects_other_values() {
        assert_eq!(Settings::default().wide_cols, 1);
        let mut s = Settings::default();
        s.wide_cols = 2;
        assert_eq!(Settings::parse(&s.to_text()).wide_cols, 2);
        assert_eq!(Settings::parse("wide_cols = 3\n").wide_cols, 1, "只接受 1 或 2");
        assert_eq!(Settings::parse("wide_cols = abc\n").wide_cols, 1);
    }

    #[test]
    fn color_scheme_names_are_chosen_per_theme_and_round_trip() {
        let s = Settings::default();
        assert_eq!((s.scheme_name(true), s.scheme_name(false)), ("", ""), "預設是空字串＝該主題的預設配色");
        assert!(!s.to_text().contains("_scheme"), "預設值不寫進檔案");
        let mut s = Settings::default();
        s.set_scheme_name(true, "Dracula");
        s.set_scheme_name(false, "我的 淺色 配色");
        assert_eq!(Settings::parse(&s.to_text()), s, "名稱含空白與中文也要讀得回來");
        assert_eq!(s.scheme_name(true), "Dracula");
        assert_eq!(s.scheme_name(false), "我的 淺色 配色");
        // 只設一邊，另一邊維持預設
        let parsed = Settings::parse("dark_scheme = Nord\n");
        assert_eq!(parsed.scheme_name(true), "Nord");
        assert_eq!(parsed.scheme_name(false), "");
    }

    #[test]
    fn output_panel_is_collapsed_by_default_and_round_trips() {
        assert!(!Settings::default().show_output);
        let mut st = Settings::default();
        st.show_output = true;
        assert_eq!(Settings::parse(&st.to_text()), st);
        assert!(Settings::parse("show_output = on\n").show_output);
        assert!(!Settings::parse("show_output = maybe\n").show_output, "不認得的值忽略");
    }
}
