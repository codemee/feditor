//! 配色方案：淺色、深色各自可以選一套，也可以自己做（存成 `themes/*.conf`）。
//!
//! 一套配色就是 [`Role`] 列出的幾十種顏色（背景、文字、控制項、強調色、語法上色）。
//! [`visuals_for`] 把它轉成 egui 的 `Visuals`，語法上色的顏色則由 [`Scheme::syntax_colors`] 取得。
//! 與畫面無關（只用到 `Color32` 與 `Visuals`），方便單元測試。
//!
//! 自訂配色檔的格式（`key = value`，`#` 開頭的整行是註解）：
//!
//! ```text
//! name = 我的配色
//! mode = dark            # dark 或 light：這套配色用在深色還是淺色主題
//! base = Dracula         # 可省略：沒寫到的顏色從這套現成的配色繼承
//! editor_bg = #282A36
//! syn_keyword = #FF79C6
//! ```

use eframe::egui::{Color32, Visuals};
use feditor_core::i18n::t;
use feditor_core::tf;
use std::path::{Path, PathBuf};

/// 內建配色手寫的顏色數（背景、文字、控制項、強調、語法）；後面接終端機的 16 色，由 [ill_terminal] 補上。
const BASE_COUNT: usize = 25;
/// 終端機的 16 色：黑、紅、綠、黃、藍、洋紅、青、白，再加上它們的亮色版本。
pub const TERMINAL_COUNT: usize = 16;

/// 一種顏色的用途。順序固定：內建配色的色票照這個順序排。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    EditorBg,
    PanelBg,
    PopupBg,
    FaintBg,
    CodeBg,
    Text,
    TextStrong,
    TextWeak,
    Button,
    ButtonHover,
    ButtonActive,
    Selection,
    Line,
    LineHover,
    Accent,
    Cursor,
    Link,
    Warning,
    Error,
    SynComment,
    SynString,
    SynNumber,
    SynKeyword,
    SynType,
    SynFunction,
    TermBlack,
    TermRed,
    TermGreen,
    TermYellow,
    TermBlue,
    TermMagenta,
    TermCyan,
    TermWhite,
    TermBrightBlack,
    TermBrightRed,
    TermBrightGreen,
    TermBrightYellow,
    TermBrightBlue,
    TermBrightMagenta,
    TermBrightCyan,
    TermBrightWhite,
}

/// 編輯畫面上顏色分組的標題。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    Background,
    Text,
    Controls,
    Accent,
    Syntax,
    Terminal,
}

impl Group {
    pub const ALL: [Group; 6] = [Group::Background, Group::Text, Group::Controls, Group::Accent, Group::Syntax, Group::Terminal];

    pub fn label(self) -> &'static str {
        match self {
            Group::Background => t("背景"),
            Group::Text => t("文字"),
            Group::Controls => t("按鈕與邊線"),
            Group::Accent => t("強調與狀態"),
            Group::Syntax => t("語法上色"),
            Group::Terminal => t("終端機"),
        }
    }
}

impl Role {
    pub const COUNT: usize = BASE_COUNT + TERMINAL_COUNT;
    pub const ALL: [Role; Role::COUNT] = [
        Role::EditorBg,
        Role::PanelBg,
        Role::PopupBg,
        Role::FaintBg,
        Role::CodeBg,
        Role::Text,
        Role::TextStrong,
        Role::TextWeak,
        Role::Button,
        Role::ButtonHover,
        Role::ButtonActive,
        Role::Selection,
        Role::Line,
        Role::LineHover,
        Role::Accent,
        Role::Cursor,
        Role::Link,
        Role::Warning,
        Role::Error,
        Role::SynComment,
        Role::SynString,
        Role::SynNumber,
        Role::SynKeyword,
        Role::SynType,
        Role::SynFunction,
        Role::TermBlack,
        Role::TermRed,
        Role::TermGreen,
        Role::TermYellow,
        Role::TermBlue,
        Role::TermMagenta,
        Role::TermCyan,
        Role::TermWhite,
        Role::TermBrightBlack,
        Role::TermBrightRed,
        Role::TermBrightGreen,
        Role::TermBrightYellow,
        Role::TermBrightBlue,
        Role::TermBrightMagenta,
        Role::TermBrightCyan,
        Role::TermBrightWhite,
    ];

    /// 設定檔裡的鍵。
    pub fn key(self) -> &'static str {
        match self {
            Role::EditorBg => "editor_bg",
            Role::PanelBg => "panel_bg",
            Role::PopupBg => "popup_bg",
            Role::FaintBg => "faint_bg",
            Role::CodeBg => "code_bg",
            Role::Text => "text",
            Role::TextStrong => "text_strong",
            Role::TextWeak => "text_weak",
            Role::Button => "button",
            Role::ButtonHover => "button_hover",
            Role::ButtonActive => "button_active",
            Role::Selection => "selection",
            Role::Line => "line",
            Role::LineHover => "line_hover",
            Role::Accent => "accent",
            Role::Cursor => "cursor",
            Role::Link => "link",
            Role::Warning => "warning",
            Role::Error => "error",
            Role::SynComment => "syn_comment",
            Role::SynString => "syn_string",
            Role::SynNumber => "syn_number",
            Role::SynKeyword => "syn_keyword",
            Role::SynType => "syn_type",
            Role::SynFunction => "syn_function",
            Role::TermBlack => "term_black",
            Role::TermRed => "term_red",
            Role::TermGreen => "term_green",
            Role::TermYellow => "term_yellow",
            Role::TermBlue => "term_blue",
            Role::TermMagenta => "term_magenta",
            Role::TermCyan => "term_cyan",
            Role::TermWhite => "term_white",
            Role::TermBrightBlack => "term_bright_black",
            Role::TermBrightRed => "term_bright_red",
            Role::TermBrightGreen => "term_bright_green",
            Role::TermBrightYellow => "term_bright_yellow",
            Role::TermBrightBlue => "term_bright_blue",
            Role::TermBrightMagenta => "term_bright_magenta",
            Role::TermBrightCyan => "term_bright_cyan",
            Role::TermBrightWhite => "term_bright_white",
        }
    }

    /// 編輯畫面上顯示的名稱。
    pub fn label(self) -> &'static str {
        match self {
            Role::EditorBg => t("編輯區背景"),
            Role::PanelBg => t("面板背景"),
            Role::PopupBg => t("浮動視窗背景"),
            Role::FaintBg => t("淡色背景"),
            Role::CodeBg => t("程式碼底色"),
            Role::Text => t("一般文字"),
            Role::TextStrong => t("強調文字"),
            Role::TextWeak => t("次要文字"),
            Role::Button => t("按鈕"),
            Role::ButtonHover => t("按鈕（滑鼠移過）"),
            Role::ButtonActive => t("按鈕（按下）"),
            Role::Selection => t("選取範圍"),
            Role::Line => t("邊線"),
            Role::LineHover => t("邊線（滑鼠移過）"),
            Role::Accent => t("強調色"),
            Role::Cursor => t("游標"),
            Role::Link => t("連結"),
            Role::Warning => t("警告"),
            Role::Error => t("錯誤"),
            Role::SynComment => t("註解"),
            Role::SynString => t("字串"),
            Role::SynNumber => t("數字"),
            Role::SynKeyword => t("關鍵字"),
            Role::SynType => t("型別"),
            Role::SynFunction => t("函式"),
            Role::TermBlack => t("終端機：黑"),
            Role::TermRed => t("終端機：紅"),
            Role::TermGreen => t("終端機：綠"),
            Role::TermYellow => t("終端機：黃"),
            Role::TermBlue => t("終端機：藍"),
            Role::TermMagenta => t("終端機：洋紅"),
            Role::TermCyan => t("終端機：青"),
            Role::TermWhite => t("終端機：白"),
            Role::TermBrightBlack => t("終端機：亮黑（灰）"),
            Role::TermBrightRed => t("終端機：亮紅"),
            Role::TermBrightGreen => t("終端機：亮綠"),
            Role::TermBrightYellow => t("終端機：亮黃"),
            Role::TermBrightBlue => t("終端機：亮藍"),
            Role::TermBrightMagenta => t("終端機：亮洋紅"),
            Role::TermBrightCyan => t("終端機：亮青"),
            Role::TermBrightWhite => t("終端機：亮白"),
        }
    }

    pub fn group(self) -> Group {
        use Role::*;
        match self {
            EditorBg | PanelBg | PopupBg | FaintBg | CodeBg => Group::Background,
            Text | TextStrong | TextWeak => Group::Text,
            Button | ButtonHover | ButtonActive | Selection | Line | LineHover => Group::Controls,
            Accent | Cursor | Link | Warning | Error => Group::Accent,
            SynComment | SynString | SynNumber | SynKeyword | SynType | SynFunction => Group::Syntax,
            TermBlack | TermRed | TermGreen | TermYellow | TermBlue | TermMagenta | TermCyan | TermWhite | TermBrightBlack
            | TermBrightRed | TermBrightGreen | TermBrightYellow | TermBrightBlue | TermBrightMagenta | TermBrightCyan
            | TermBrightWhite => Group::Terminal,
        }
    }

    pub fn from_key(key: &str) -> Option<Role> {
        Role::ALL.into_iter().find(|r| r.key() == key)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Scheme {
    pub name: String,
    /// 用在深色主題（`true`）還是淺色主題（`false`）。
    pub dark: bool,
    pub colors: [Color32; Role::COUNT],
    /// 內建的（不能直接覆寫，只能另存）。
    pub builtin: bool,
    /// 自訂配色讀自哪個檔案；儲存時會寫回這裡。
    pub path: Option<PathBuf>,
}

impl Scheme {
    pub fn get(&self, role: Role) -> Color32 {
        self.colors[role as usize]
    }

    pub fn set(&mut self, role: Role, color: Color32) {
        self.colors[role as usize] = color;
    }

    /// 語法上色的顏色，依 [`crate::syntax::Kind`] 的順序：
    /// 一般（用一般文字的顏色）、註解、字串、數字、關鍵字、型別、函式。
    pub fn syntax_colors(&self) -> [Color32; 7] {
        [
            self.get(Role::Text),
            self.get(Role::SynComment),
            self.get(Role::SynString),
            self.get(Role::SynNumber),
            self.get(Role::SynKeyword),
            self.get(Role::SynType),
            self.get(Role::SynFunction),
        ]
    }

    /// 終端機的 16 色（黑、紅、綠、黃、藍、洋紅、青、白，再來是亮色版本），順序同 ANSI 色號 0–15。
    pub fn terminal_colors(&self) -> [Color32; TERMINAL_COUNT] {
        std::array::from_fn(|i| self.colors[BASE_COUNT + i])
    }

    /// 存成自訂配色檔的內容。
    pub fn to_text(&self) -> String {
        let mut out = format!("name = {}\nmode = {}\n", self.name, if self.dark { "dark" } else { "light" });
        for group in Group::ALL {
            out.push_str(&format!("\n# {}\n", group.label()));
            for role in Role::ALL.into_iter().filter(|r| r.group() == group) {
                out.push_str(&format!("{} = {}  # {}\n", role.key(), to_hex(self.get(role)), role.label()));
            }
        }
        out
    }
}

pub fn to_hex(c: Color32) -> String {
    format!("#{:02X}{:02X}{:02X}", c.r(), c.g(), c.b())
}

/// `#RRGGBB`、`RRGGBB`、`#RGB` 都可以。
pub fn parse_hex(text: &str) -> Option<Color32> {
    let t = text.trim().trim_start_matches('#');
    if t.is_empty() || !t.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let v = u32::from_str_radix(t, 16).ok()?;
    match t.len() {
        6 => Some(Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)),
        3 => {
            let (r, g, b) = ((v >> 8) as u8 & 0xF, (v >> 4) as u8 & 0xF, v as u8 & 0xF);
            Some(Color32::from_rgb(r * 17, g * 17, b * 17))
        }
        _ => None,
    }
}

/// 解析自訂配色檔。`find_base` 依名稱找 `base` 指定的現成配色。
pub fn parse_scheme(text: &str, find_base: impl Fn(&str) -> Option<Scheme>) -> Result<Scheme, String> {
    let mut name = None;
    let mut dark = true;
    let mut base_name: Option<String> = None;
    let mut overrides: Vec<(Role, Color32)> = Vec::new();
    for (n, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err(tf!("第 {} 行：需要「鍵 = 值」", n + 1));
        };
        let (key, mut value) = (key.trim(), value.trim());
        // 名稱可以有空白；其他的值只取第一段，後面的空白加 # 是行尾說明（顏色值本身以 # 開頭）
        if key != "name" && key != "base" {
            value = value.split_whitespace().next().unwrap_or("");
        }
        match key {
            "name" => name = Some(value.to_owned()),
            "mode" => {
                dark = match value.to_ascii_lowercase().as_str() {
                    "dark" => true,
                    "light" => false,
                    _ => return Err(tf!("第 {} 行：mode 只能是 dark 或 light", n + 1)),
                }
            }
            "base" => base_name = Some(value.to_owned()),
            _ => {
                let role = Role::from_key(key).ok_or_else(|| tf!("第 {} 行：不認得的設定「{}」", n + 1, key))?;
                let color = parse_hex(value).ok_or_else(|| tf!("第 {} 行：「{}」不是顏色，要寫成 #RRGGBB", n + 1, value))?;
                overrides.push((role, color));
            }
        }
    }
    let name = name.filter(|n| !n.is_empty()).ok_or(t("缺少 name"))?;
    let mut scheme = match base_name {
        Some(b) => find_base(&b).ok_or_else(|| tf!("找不到 base 指定的配色「{b}」", b = b))?,
        None => default_for(dark),
    };
    scheme.name = name;
    scheme.dark = dark;
    scheme.builtin = false;
    scheme.path = None;
    for (role, color) in overrides {
        scheme.set(role, color);
    }
    Ok(scheme)
}

/// 自訂配色的資料夾：設定資料夾下的 `themes`。
pub fn themes_dir() -> Option<PathBuf> {
    crate::settings::Settings::default_path().and_then(|p| p.parent().map(|d| d.join("themes")))
}

/// 讀取資料夾裡所有 `*.conf` 自訂配色。壞掉的檔案略過，原因收在第二個回傳值。
pub fn load_custom(dir: &Path) -> (Vec<Scheme>, Vec<String>) {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("conf")))
        .collect();
    files.sort();
    let (mut found, mut messages): (Vec<Scheme>, Vec<String>) = (Vec::new(), Vec::new());
    let builtin = builtins();
    for path in files {
        let label = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let Ok(bytes) = std::fs::read(&path) else {
            messages.push(tf!("無法讀取配色檔 {label}", label = label));
            continue;
        };
        // base 可以是內建配色，也可以是排在前面的自訂配色
        let result = parse_scheme(&String::from_utf8_lossy(&bytes), |b| {
            found.iter().chain(builtin.iter()).find(|s| s.name == b).cloned()
        });
        match result {
            Ok(mut s) => {
                s.path = Some(path);
                found.push(s);
            }
            Err(e) => messages.push(tf!("配色檔 {label} 有誤，已略過：{e}", label = label, e = e)),
        }
    }
    (found, messages)
}

/// 把自訂配色存成檔案：原本就是從檔案讀來的就寫回去，否則在資料夾裡新建一個不重複的檔名。
pub fn save_custom(dir: &Path, scheme: &Scheme) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let path = match &scheme.path {
        Some(p) => p.clone(),
        None => {
            let stem = crate::macro_lib::file_stem_for(&scheme.name);
            let first = dir.join(format!("{stem}.conf"));
            if !first.exists() {
                first
            } else {
                (2..)
                    .map(|n| dir.join(format!("{stem}-{n}.conf")))
                    .find(|p| !p.exists())
                    .expect("檔名一定找得到")
            }
        }
    };
    std::fs::write(&path, scheme.to_text())?;
    Ok(path)
}

/// 內建配色加上自訂配色。名稱重複時自訂的在前，所以會蓋過同名的內建配色。
pub fn combined(custom: &[Scheme]) -> Vec<Scheme> {
    let mut all: Vec<Scheme> = custom.to_vec();
    all.extend(builtins());
    all
}

/// 在 `all` 裡找這個主題（深色或淺色）的配色；找不到就用該主題的預設。
pub fn pick<'a>(all: &'a [Scheme], dark: bool, name: &str) -> &'a Scheme {
    all.iter()
        .find(|s| s.dark == dark && s.name == name)
        .or_else(|| all.iter().find(|s| s.dark == dark && s.builtin))
        .expect("內建配色一定有深色與淺色的預設")
}

/// 這個主題可以選的配色名稱（自訂的在前），名稱重複的只列一次。
pub fn names_for(all: &[Scheme], dark: bool) -> Vec<&Scheme> {
    let mut out: Vec<&Scheme> = Vec::new();
    for s in all.iter().filter(|s| s.dark == dark) {
        if !out.iter().any(|o| o.name == s.name) {
            out.push(s);
        }
    }
    out
}

#[cfg(test)]
pub const DEFAULT_DARK: &str = "EVA";
pub const DEFAULT_LIGHT: &str = "預設淺色";

/// 配色在畫面上顯示的名稱。內建配色的名稱就是設定檔裡的 ID，所以只有「預設淺色」這種中文名稱
/// 需要照介面語言翻譯，其餘（EVA、Dracula…）和自訂配色原樣顯示。
pub fn display_name(name: &str) -> String {
    if name == DEFAULT_LIGHT { t("預設淺色").to_owned() } else { name.to_owned() }
}

pub fn default_for(dark: bool) -> Scheme {
    let all = builtins();
    all.into_iter()
        .find(|s| s.dark == dark)
        .expect("內建配色一定有深色與淺色的預設")
}

/// 配色轉成 egui 的 `Visuals`：以 egui 預設的深色／淺色為底，只換顏色，筆畫粗細與圓角不動。
pub fn visuals_for(s: &Scheme) -> Visuals {
    use Role::*;
    let mut v = if s.dark { Visuals::dark() } else { Visuals::light() };
    v.panel_fill = s.get(PanelBg);
    v.window_fill = s.get(PopupBg);
    v.window_stroke.color = s.get(Line);
    v.extreme_bg_color = s.get(EditorBg); // 文字框與捲動區的底色：編輯區
    v.faint_bg_color = s.get(FaintBg);
    v.code_bg_color = s.get(CodeBg);
    v.hyperlink_color = s.get(Link);
    v.warn_fg_color = s.get(Warning);
    v.error_fg_color = s.get(Error);
    v.weak_text_color = Some(s.get(TextWeak));
    v.selection.bg_fill = s.get(Selection);
    v.selection.stroke.color = s.get(Accent);
    v.text_cursor.stroke.color = s.get(Cursor);

    let w = &mut v.widgets;
    w.noninteractive.bg_fill = s.get(PanelBg);
    w.noninteractive.weak_bg_fill = s.get(PanelBg);
    w.noninteractive.bg_stroke.color = s.get(Line);
    w.noninteractive.fg_stroke.color = s.get(Text);

    w.inactive.bg_fill = s.get(Button);
    w.inactive.weak_bg_fill = s.get(Button);
    w.inactive.fg_stroke.color = s.get(Text);

    w.hovered.bg_fill = s.get(ButtonHover);
    w.hovered.weak_bg_fill = s.get(ButtonHover);
    w.hovered.bg_stroke.color = s.get(LineHover);
    w.hovered.fg_stroke.color = s.get(TextStrong);

    w.active.bg_fill = s.get(ButtonActive);
    w.active.weak_bg_fill = s.get(ButtonActive);
    w.active.bg_stroke.color = s.get(Accent);
    w.active.fg_stroke.color = s.get(TextStrong);

    w.open.bg_fill = s.get(ButtonHover);
    w.open.weak_bg_fill = s.get(ButtonHover);
    w.open.bg_stroke.color = s.get(LineHover);
    w.open.fg_stroke.color = s.get(TextStrong);
    v
}

const fn hex(v: u32) -> Color32 {
    Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

fn make(name: &str, dark: bool, h: [u32; BASE_COUNT]) -> Scheme {
    let mut colors = [Color32::BLACK; Role::COUNT];
    for (c, v) in colors.iter_mut().zip(h) {
        *c = hex(v);
    }
    fill_terminal(&mut colors, name, dark);
    Scheme { name: name.to_owned(), dark, colors, builtin: true, path: None }
}

/// 各內建配色的終端機 8 色（黑、紅、綠、黃、藍、洋紅、青、白）。多數取自各配色社群通用的終端機色票；
/// 淺色配色的顏色比官方版略深，在淺色底上才有 3:1 的對比（測試會檢查）。沒列在這裡的名稱
/// （自訂配色從 base 繼承，不會走到這裡）依深淺用 EVA／預設淺色的。
fn terminal_normals(name: &str, dark: bool) -> [u32; 8] {
    match name {
        "Dracula" => [0x21222C, 0xFF5555, 0x50FA7B, 0xF1FA8C, 0xBD93F9, 0xFF79C6, 0x8BE9FD, 0xF8F8F2],
        "Monokai" => [0x272822, 0xF92672, 0xA6E22E, 0xF4BF75, 0x66D9EF, 0xAE81FF, 0xA1EFE4, 0xF8F8F2],
        "Solarized Dark" => [0x073642, 0xDC322F, 0x859900, 0xB58900, 0x268BD2, 0xD33682, 0x2AA198, 0xEEE8D5],
        "One Dark" => [0x282C34, 0xE06C75, 0x98C379, 0xE5C07B, 0x61AFEF, 0xC678DD, 0x56B6C2, 0xABB2BF],
        "Nord" => [0x3B4252, 0xBF616A, 0xA3BE8C, 0xEBCB8B, 0x81A1C1, 0xB48EAD, 0x88C0D0, 0xE5E9F0],
        "Gruvbox Dark" => [0x282828, 0xFB4934, 0xB8BB26, 0xFABD2F, 0x83A598, 0xD3869B, 0x8EC07C, 0xEBDBB2],
        "Tokyo Night" => [0x15161E, 0xF7768E, 0x9ECE6A, 0xE0AF68, 0x7AA2F7, 0xBB9AF7, 0x7DCFFF, 0xA9B1D6],
        "Catppuccin Mocha" => [0x45475A, 0xF38BA8, 0xA6E3A1, 0xF9E2AF, 0x89B4FA, 0xF5C2E7, 0x94E2D5, 0xBAC2DE],
        "Catppuccin Macchiato" => [0x494D64, 0xED8796, 0xA6DA95, 0xEED49F, 0x8AADF4, 0xF5BDE6, 0x8BD5CA, 0xB8C0E0],
        "Catppuccin Frappé" => [0x51576D, 0xE78284, 0xA6D189, 0xE5C890, 0x8CAAEE, 0xF4B8E4, 0x81C8BE, 0xB5BFE2],
        "Solarized Light" => [0x073642, 0xDC322F, 0x7A8C00, 0xA67C00, 0x268BD2, 0xD33682, 0x1F8A82, 0x586E75],
        "GitHub Light" => [0x24292F, 0xCF222E, 0x116329, 0x9A6700, 0x0969DA, 0x8250DF, 0x1B7C83, 0x57606A],
        "One Light" => [0x383A42, 0xE45649, 0x3E8E3D, 0xA66E00, 0x4078F2, 0xA626A4, 0x0184BC, 0x696C77],
        "Gruvbox Light" => [0x3C3836, 0x9D0006, 0x79740E, 0xB57614, 0x076678, 0x8F3F71, 0x427B58, 0x7C6F64],
        "VS Code Light+" => [0x000000, 0xCD3131, 0x008000, 0x795E26, 0x0451A5, 0xBC05BC, 0x0598BC, 0x555555],
        "Catppuccin Latte" => [0x5C5F77, 0xD20F39, 0x3C9A28, 0xB8710F, 0x1E66F5, 0x8839EF, 0x179299, 0x6C6F85],
        _ if dark => [0x150F24, 0xFF4D5E, 0x8FEA00, 0xFFCB6B, 0x82AAFF, 0xC792EA, 0x4DD0E1, 0xD9D3EA],
        _ => [0x24292F, 0xCD3131, 0x0F7B0F, 0x8A6D00, 0x0451A5, 0xBC05BC, 0x0598BC, 0x57606A],
    }
}

/// 把 c 往白色混 amount（0～1）。
fn lighten(c: Color32, amount: f32) -> Color32 {
    let mix = |v: u8| (v as f32 + (255.0 - v as f32) * amount).round() as u8;
    Color32::from_rgb(mix(c.r()), mix(c.g()), mix(c.b()))
}

/// 補上終端機的 16 色：前 8 色查表；亮黑用次要文字色（保證看得見）、亮白用強調文字色；
/// 其餘的亮色在深色配色是正常色提亮一點，淺色配色維持原色（淺色底上再提亮會看不清楚）。
fn fill_terminal(colors: &mut [Color32; Role::COUNT], name: &str, dark: bool) {
    let normal = terminal_normals(name, dark).map(hex);
    colors[BASE_COUNT..BASE_COUNT + 8].copy_from_slice(&normal);
    colors[BASE_COUNT + 8] = colors[Role::TextWeak as usize];
    for i in 1..7 {
        colors[BASE_COUNT + 8 + i] = if dark { lighten(normal[i], 0.25) } else { normal[i] };
    }
    colors[BASE_COUNT + 15] = colors[Role::TextStrong as usize];
}

/// 把（可能半透明的）顏色疊在`bg` 上，得到不透明的等效色。egui 的顏色是預乘 alpha：結果 = 顏色 + 背景 × (1 − alpha)。
fn flatten(c: Color32, bg: Color32) -> Color32 {
    let keep = 255 - c.a() as u32;
    let mix = |top: u8, under: u8| (top as u32 + under as u32 * keep / 255).min(255) as u8;
    Color32::from_rgb(mix(c.r(), bg.r()), mix(c.g(), bg.g()), mix(c.b(), bg.b()))
}

/// egui 預設的淺色（淺色主題一直以來的外觀），搭配原本的語法上色顏色。
/// 其中半透明的顏色（淡色背景、次要文字）換成疊在面板上的不透明等效色，才存得成 `#RRGGBB`。
fn default_light() -> Scheme {
    let v = Visuals::light();
    let w = &v.widgets;
    let base = [
        v.extreme_bg_color,
        v.panel_fill,
        v.window_fill,
        flatten(v.faint_bg_color, v.panel_fill),
        v.code_bg_color,
        w.noninteractive.fg_stroke.color,
        w.hovered.fg_stroke.color,
        flatten(v.weak_text_color(), v.panel_fill),
        w.inactive.weak_bg_fill,
        w.hovered.weak_bg_fill,
        w.active.weak_bg_fill,
        v.selection.bg_fill,
        w.noninteractive.bg_stroke.color,
        w.hovered.bg_stroke.color,
        v.selection.stroke.color,
        v.text_cursor.stroke.color,
        v.hyperlink_color,
        v.warn_fg_color,
        v.error_fg_color,
        hex(0x6E7781),
        hex(0x0A7B34),
        hex(0xB5530E),
        hex(0x8250DF),
        hex(0x953800),
        hex(0x0550AE),
    ];
    let mut colors = [Color32::BLACK; Role::COUNT];
    colors[..BASE_COUNT].copy_from_slice(&base);
    fill_terminal(&mut colors, DEFAULT_LIGHT, false);
    Scheme { name: DEFAULT_LIGHT.to_owned(), dark: false, colors, builtin: true, path: None }
}

/// 內建配色：深色與淺色各一組。每個深色／淺色的第一套是預設（EVA、預設淺色）。
pub fn builtins() -> Vec<Scheme> {
    vec![
        // ── 深色 ──
        // 《新世紀福音戰士》初號機與 NERV：紫、螢光綠、橘、紅
        make("EVA", true, [
            0x0B0814, 0x150F24, 0x1A1230, 0x1D1530, 0x241A3B,
            0xD9D3EA, 0xF2EEFF, 0x948BB2,
            0x2A1E45, 0x4A2F7A, 0x5B2A9E, 0x5B2A9E, 0x3A2A5C, 0x9B6DE0,
            0x8FEA00, 0x8FEA00, 0x8FEA00, 0xFF8A00, 0xFF4D5E,
            0x6F809C, 0xA5E075, 0xF78C6C, 0xC792EA, 0xFFCB6B, 0x82AAFF,
        ]),
        make("Dracula", true, [
            0x282A36, 0x21222C, 0x2D2F3C, 0x2F3140, 0x343746,
            0xF8F8F2, 0xFFFFFF, 0x9AA3C7,
            0x3A3D4F, 0x44475A, 0x565A75, 0x44475A, 0x44475A, 0xBD93F9,
            0xBD93F9, 0xF8F8F2, 0x8BE9FD, 0xFFB86C, 0xFF5555,
            0x6272A4, 0xF1FA8C, 0xBD93F9, 0xFF79C6, 0x8BE9FD, 0x50FA7B,
        ]),
        make("Monokai", true, [
            0x272822, 0x1E1F1C, 0x2D2E27, 0x2E2F28, 0x3E3D32,
            0xF8F8F2, 0xFFFFFF, 0xA59F85,
            0x3A3B33, 0x46463B, 0x55554A, 0x49483E, 0x3E3D32, 0x75715E,
            0xA6E22E, 0xF8F8F0, 0x66D9EF, 0xFD971F, 0xF92672,
            0x75715E, 0xE6DB74, 0xAE81FF, 0xF92672, 0x66D9EF, 0xA6E22E,
        ]),
        make("Solarized Dark", true, [
            0x002B36, 0x073642, 0x073B48, 0x04323E, 0x0A4150,
            0x93A1A1, 0xEEE8D5, 0x839496,
            0x0E4A59, 0x14586A, 0x1D6A7D, 0x0F4C5A, 0x0F4C5A, 0x586E75,
            0x2AA198, 0x93A1A1, 0x268BD2, 0xB58900, 0xDC322F,
            0x586E75, 0x2AA198, 0xD33682, 0x859900, 0xB58900, 0x268BD2,
        ]),
        make("One Dark", true, [
            0x282C34, 0x21252B, 0x262B33, 0x2A2F38, 0x2C313A,
            0xABB2BF, 0xE6E9EF, 0x7F848E,
            0x343A45, 0x3E4451, 0x4B5263, 0x3E4451, 0x3A3F4B, 0x528BFF,
            0x528BFF, 0x528BFF, 0x61AFEF, 0xD19A66, 0xE06C75,
            0x5C6370, 0x98C379, 0xD19A66, 0xC678DD, 0xE5C07B, 0x61AFEF,
        ]),
        make("Nord", true, [
            0x2E3440, 0x272C36, 0x353B49, 0x323946, 0x3B4252,
            0xD8DEE9, 0xECEFF4, 0x8F9AAF,
            0x434C5E, 0x4C566A, 0x5A657D, 0x434C5E, 0x434C5E, 0x81A1C1,
            0x88C0D0, 0xD8DEE9, 0x88C0D0, 0xEBCB8B, 0xBF616A,
            0x616E88, 0xA3BE8C, 0xB48EAD, 0x81A1C1, 0x8FBCBB, 0x88C0D0,
        ]),
        make("Gruvbox Dark", true, [
            0x282828, 0x1D2021, 0x32302F, 0x32302F, 0x3C3836,
            0xEBDBB2, 0xFBF1C7, 0xA89984,
            0x3C3836, 0x504945, 0x665C54, 0x504945, 0x504945, 0x928374,
            0xFABD2F, 0xEBDBB2, 0x83A598, 0xFE8019, 0xFB4934,
            0x928374, 0xB8BB26, 0xD3869B, 0xFB4934, 0xFABD2F, 0x8EC07C,
        ]),
        make("Tokyo Night", true, [
            0x1A1B26, 0x16161E, 0x1F2335, 0x1F2335, 0x292E42,
            0xC0CAF5, 0xE0E6FF, 0x7982B3,
            0x292E42, 0x33395A, 0x3D4466, 0x283457, 0x292E42, 0x7AA2F7,
            0x7AA2F7, 0xC0CAF5, 0x7DCFFF, 0xE0AF68, 0xF7768E,
            0x565F89, 0x9ECE6A, 0xFF9E64, 0xBB9AF7, 0x7DCFFF, 0x7AA2F7,
        ]),
        // Catppuccin 的三種深色風味（Mocha 最深，Frappé 最淡）：base 當編輯區、mantle 當面板、
        // surface0–2 當按鈕與邊線，強調色用 mauve、游標用 rosewater
        make("Catppuccin Mocha", true, [
            0x1E1E2E, 0x181825, 0x232334, 0x262637, 0x313244,
            0xCDD6F4, 0xE8ECFF, 0xA6ADC8,
            0x313244, 0x45475A, 0x585B70, 0x45475A, 0x45475A, 0x7F849C,
            0xCBA6F7, 0xF5E0DC, 0x89B4FA, 0xFAB387, 0xF38BA8,
            0x9399B2, 0xA6E3A1, 0xFAB387, 0xCBA6F7, 0xF9E2AF, 0x89B4FA,
        ]),
        make("Catppuccin Macchiato", true, [
            0x24273A, 0x1E2030, 0x292C40, 0x2B2E42, 0x363A4F,
            0xCAD3F5, 0xE4E9FF, 0xA5ADCB,
            0x363A4F, 0x494D64, 0x5B6078, 0x494D64, 0x494D64, 0x8087A2,
            0xC6A0F6, 0xF4DBD6, 0x8AADF4, 0xF5A97F, 0xED8796,
            0x939AB7, 0xA6DA95, 0xF5A97F, 0xC6A0F6, 0xEED49F, 0x8AADF4,
        ]),
        make("Catppuccin Frappé", true, [
            0x303446, 0x292C3C, 0x353A4D, 0x373B4E, 0x414559,
            0xC6D0F5, 0xE0E6FF, 0xA5ADCE,
            0x414559, 0x51576D, 0x626880, 0x51576D, 0x51576D, 0x838BA7,
            0xCA9EE6, 0xF2D5CF, 0x8CAAEE, 0xEF9F76, 0xE78284,
            0x949CBB, 0xA6D189, 0xEF9F76, 0xCA9EE6, 0xE5C890, 0x8CAAEE,
        ]),
        // ── 淺色 ──
        default_light(),
        make("Solarized Light", false, [
            0xFDF6E3, 0xEEE8D5, 0xF5EFDC, 0xF7F0DE, 0xEEE8D5,
            0x536870, 0x073642, 0x6F8388,
            0xE2DBC3, 0xD6CEB3, 0xC7BE9F, 0xC9DDD9, 0xDDD6C1, 0x93A1A1,
            0x268BD2, 0x586E75, 0x268BD2, 0xCB4B16, 0xDC322F,
            0x93A1A1, 0x1F8A82, 0xD33682, 0x7A8C00, 0xA67C00, 0x268BD2,
        ]),
        make("GitHub Light", false, [
            0xFFFFFF, 0xF6F8FA, 0xFFFFFF, 0xF6F8FA, 0xEFF1F3,
            0x24292F, 0x1F2328, 0x57606A,
            0xE8EBEF, 0xD5DAE1, 0xC2C9D2, 0xB6E3FF, 0xD0D7DE, 0x8C959F,
            0x0969DA, 0x0969DA, 0x0969DA, 0x9A6700, 0xCF222E,
            0x6E7781, 0x0A3069, 0x0550AE, 0xCF222E, 0x953800, 0x8250DF,
        ]),
        make("One Light", false, [
            0xFAFAFA, 0xEAEAEB, 0xF0F0F1, 0xF0F0F1, 0xE5E5E6,
            0x383A42, 0x202227, 0x696C77,
            0xDADADC, 0xCECED1, 0xBFBFC3, 0xCFDCF2, 0xDBDBDC, 0xA0A1A7,
            0x4078F2, 0x526FFF, 0x4078F2, 0xAD6F00, 0xE45649,
            0xA0A1A7, 0x50A14F, 0x986801, 0xA626A4, 0xA66E00, 0x4078F2,
        ]),
        make("Gruvbox Light", false, [
            0xFBF1C7, 0xEBDBB2, 0xF2E5BC, 0xF2E5BC, 0xEBDBB2,
            0x3C3836, 0x282828, 0x7C6F64,
            0xD5C4A1, 0xC5B48F, 0xB3A27E, 0xD5C4A1, 0xD5C4A1, 0x928374,
            0x076678, 0x3C3836, 0x076678, 0xAF3A03, 0x9D0006,
            0x928374, 0x79740E, 0x8F3F71, 0x9D0006, 0xB57614, 0x427B58,
        ]),
        make("VS Code Light+", false, [
            0xFFFFFF, 0xF3F3F3, 0xF8F8F8, 0xF3F3F3, 0xEDEDED,
            0x333333, 0x000000, 0x6E6E6E,
            0xE4E4E4, 0xD6D6D6, 0xC4C4C4, 0xADD6FF, 0xE5E5E5, 0xA0A0A0,
            0x0066B8, 0x000000, 0x006AB1, 0x946A00, 0xE51400,
            0x008000, 0xA31515, 0x098658, 0x0000FF, 0x267F99, 0x795E26,
        ]),
        // Catppuccin Latte：官方的綠、橘、黃在淺色底上對比不到 3:1，字串、數字、型別與警告略微加深
        make("Catppuccin Latte", false, [
            0xEFF1F5, 0xE6E9EF, 0xEFF1F5, 0xE9ECF2, 0xDCE0E8,
            0x4C4F69, 0x2E3048, 0x6C6F85,
            0xCCD0DA, 0xBCC0CC, 0xACB0BE, 0xD2D8F4, 0xBCC0CC, 0x8C8FA1,
            0x8839EF, 0x8839EF, 0x1E66F5, 0xE5560A, 0xD20F39,
            0x7C7F93, 0x3C9A28, 0xE5560A, 0x8839EF, 0xB8710F, 0x1E66F5,
        ]),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// WCAG 相對亮度。
    fn luminance(c: Color32) -> f64 {
        fn channel(v: u8) -> f64 {
            let s = v as f64 / 255.0;
            if s <= 0.03928 { s / 12.92 } else { ((s + 0.055) / 1.055).powf(2.4) }
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
    fn there_are_dark_and_light_defaults_and_names_are_unique_per_mode() {
        let all = builtins();
        for dark in [true, false] {
            let names: Vec<&str> = all.iter().filter(|s| s.dark == dark).map(|s| s.name.as_str()).collect();
            assert!(names.len() >= 6, "每種主題至少六套：{names:?}");
            let mut sorted = names.clone();
            sorted.sort();
            sorted.dedup();
            assert_eq!(sorted.len(), names.len(), "名稱不能重複");
            assert_eq!(names[0], if dark { DEFAULT_DARK } else { DEFAULT_LIGHT }, "第一套是預設");
        }
        assert!(all.iter().all(|s| s.builtin));
        assert_eq!(Role::ALL.len(), Role::COUNT);
        for (i, r) in Role::ALL.iter().enumerate() {
            assert_eq!(*r as usize, i, "Role 的順序要和 ALL 一致");
            assert_eq!(Role::from_key(r.key()), Some(*r));
        }
    }

    #[test]
    fn every_builtin_scheme_is_readable() {
        let mut failures: Vec<String> = Vec::new();
        for s in builtins() {
            let g = |r| s.get(r);
            let n = &s.name;
            let mut check = |ok: bool, what: String, value: f64| {
                if !ok {
                    failures.push(format!("{n}：{what} 只有 {value:.2}:1"));
                }
            };
            for bg in [Role::EditorBg, Role::PanelBg, Role::PopupBg] {
                for fg in [Role::Text, Role::TextStrong] {
                    let c = contrast(g(fg), g(bg));
                    check(c >= 4.5, format!("{fg:?} 在 {bg:?} 上（要 4.5）"), c);
                }
                let weak = contrast(g(Role::TextWeak), g(bg));
                // 2.8：預設淺色是 egui 原本的外觀，次要文字剛好 2.89，不為了門檻去改它
                check(weak >= 2.8, format!("次要文字在 {bg:?} 上（要 2.8）"), weak);
            }
            for bg in [Role::Button, Role::ButtonHover, Role::ButtonActive, Role::Selection] {
                let c = contrast(g(Role::TextStrong), g(bg));
                check(c >= 3.0, format!("強調文字在 {bg:?} 上（要 3）"), c);
            }
            // 按鈕要和它所在的底色分得出來（設定面板在浮動視窗上，工具列在面板上）；滑過、按下要再明顯一點
            for bg in [Role::PanelBg, Role::PopupBg] {
                let c = contrast(g(Role::Button), g(bg));
                check(c >= 1.1, format!("按鈕在 {bg:?} 上看不出來（要 1.1）"), c);
            }
            // 1.07：預設淺色是 egui 原本的外觀，兩者剛好 1.10
            let c = contrast(g(Role::ButtonHover), g(Role::Button));
            check(c >= 1.07, "滑過的按鈕和一般按鈕（要 1.07）".to_owned(), c);
            let sel = contrast(g(Role::Text), g(Role::Selection));
            check(sel >= 3.0, "選取範圍裡的文字（要 3）".to_owned(), sel);
            // 強調色、連結、警告、錯誤要在編輯區底色上看得到
            for fg in [Role::Accent, Role::Link, Role::Warning, Role::Error, Role::Cursor] {
                let c = contrast(g(fg), g(Role::EditorBg));
                check(c >= 2.8, format!("{fg:?} 在編輯區上（要 2.8）"), c);
            }
            // 語法顏色：註解可以淡一點（2:1），其他至少 3:1
            for fg in [Role::SynString, Role::SynNumber, Role::SynKeyword, Role::SynType, Role::SynFunction] {
                let c = contrast(g(fg), g(Role::EditorBg));
                check(c >= 3.0, format!("{fg:?} 在編輯區上（要 3）"), c);
            }
            let c = contrast(g(Role::SynComment), g(Role::EditorBg));
            check(c >= 2.0, "註解（要 2）".to_owned(), c);
        }
        assert!(failures.is_empty(), "\n{}", failures.join("\n"));
    }

    #[test]
    fn terminal_colors_are_visible_on_the_editor_background_in_every_builtin_scheme() {
        let mut failures: Vec<String> = Vec::new();
        for s in builtins() {
            let bg = s.get(Role::EditorBg);
            for (i, c) in s.terminal_colors().into_iter().enumerate() {
                // 黑（0）在深色配色常拿來當底色，和編輯區底色接近是正常的；其餘都要看得見
                if i == 0 && s.dark {
                    continue;
                }
                let ratio = contrast(c, bg);
                if ratio < 3.0 {
                    failures.push(format!("{}：終端機色 {i} 只有 {ratio:.2}:1", s.name));
                }
            }
        }
        assert!(failures.is_empty(), "\n{}", failures.join("\n"));
    }

    #[test]
    fn terminal_colors_come_from_the_scheme_inherit_from_a_base_and_survive_a_file_round_trip() {
        let all = builtins();
        let dracula = all.iter().find(|s| s.name == "Dracula").unwrap();
        assert_eq!(dracula.get(Role::TermRed), hex(0xFF5555));
        assert_eq!(dracula.terminal_colors()[1], hex(0xFF5555));
        assert_ne!(dracula.terminal_colors(), default_for(true).terminal_colors());
        // 檔案裡可以只改其中一個，沒寫的從 base 繼承
        let text = "name = T\nmode = dark\nbase = Dracula\nterm_red = #123456\n";
        let custom = parse_scheme(text, |n| all.iter().find(|s| s.name == n).cloned()).unwrap();
        assert_eq!(custom.get(Role::TermRed), hex(0x123456));
        assert_eq!(custom.get(Role::TermGreen), dracula.get(Role::TermGreen));
        let again = parse_scheme(&custom.to_text(), |_| None).unwrap();
        assert_eq!(again.terminal_colors(), custom.terminal_colors());
    }

    #[test]
    fn dark_schemes_are_dark_and_light_schemes_are_light() {
        for s in builtins() {
            let editor = luminance(s.get(Role::EditorBg));
            let text = luminance(s.get(Role::Text));
            if s.dark {
                assert!(editor < 0.12 && text > editor, "{} 該是深色", s.name);
            } else {
                assert!(editor > 0.6 && text < editor, "{} 該是淺色", s.name);
            }
        }
    }

    #[test]
    fn the_default_light_scheme_reproduces_the_stock_egui_light_look() {
        let v = visuals_for(&default_light());
        let stock = Visuals::light();
        assert_eq!(v.panel_fill, stock.panel_fill);
        assert_eq!(v.extreme_bg_color, stock.extreme_bg_color);
        assert_eq!(v.window_fill, stock.window_fill);
        assert_eq!(v.selection.bg_fill, stock.selection.bg_fill);
        assert_eq!(v.hyperlink_color, stock.hyperlink_color);
        assert_eq!(v.widgets.inactive.weak_bg_fill, stock.widgets.inactive.weak_bg_fill);
        assert_eq!(v.widgets.hovered.weak_bg_fill, stock.widgets.hovered.weak_bg_fill);
        assert_eq!(v.widgets.active.weak_bg_fill, stock.widgets.active.weak_bg_fill);
        assert_eq!(v.widgets.noninteractive.fg_stroke.color, stock.widgets.noninteractive.fg_stroke.color);
        assert!(!v.dark_mode);
    }

    #[test]
    fn eva_reproduces_the_original_dark_look() {
        let v = visuals_for(&default_for(true));
        assert!(v.dark_mode);
        assert_eq!(v.panel_fill, hex(0x150F24));
        assert_eq!(v.extreme_bg_color, hex(0x0B0814));
        assert_eq!(v.hyperlink_color, hex(0x8FEA00));
        assert_eq!(v.warn_fg_color, hex(0xFF8A00));
        assert_eq!(v.selection.bg_fill, hex(0x5B2A9E));
        assert_eq!(v.widgets.active.bg_stroke.color, hex(0x8FEA00));
        assert_eq!(v.widgets.hovered.fg_stroke.width, Visuals::dark().widgets.hovered.fg_stroke.width, "筆畫粗細不動");
    }

    #[test]
    fn hex_colors_parse_and_format() {
        assert_eq!(parse_hex("#FF8000"), Some(Color32::from_rgb(255, 128, 0)));
        assert_eq!(parse_hex("ff8000"), Some(Color32::from_rgb(255, 128, 0)));
        assert_eq!(parse_hex(" #f80 "), Some(Color32::from_rgb(255, 136, 0)));
        for bad in ["", "#", "#12", "#1234567", "#GG0000", "red", "#ＦＦ００００"] {
            assert_eq!(parse_hex(bad), None, "{bad:?}");
        }
        assert_eq!(to_hex(Color32::from_rgb(1, 2, 255)), "#0102FF");
    }

    #[test]
    fn a_scheme_survives_a_round_trip_through_text() {
        for original in builtins() {
            let find = |_: &str| None;
            let parsed = parse_scheme(&original.to_text(), find).unwrap();
            assert_eq!(parsed.name, original.name);
            assert_eq!(parsed.dark, original.dark);
            assert_eq!(parsed.colors, original.colors, "{}", original.name);
            assert!(!parsed.builtin);
        }
    }

    #[test]
    fn custom_files_inherit_from_a_base_and_report_errors() {
        let dracula = builtins().into_iter().find(|s| s.name == "Dracula").unwrap();
        let find = |n: &str| (n == "Dracula").then(|| dracula.clone());
        let s = parse_scheme(
            "# 我的\nname = 我的 Dracula\nmode = dark\nbase = Dracula\nsyn_keyword = #00FF00  # 改關鍵字\n",
            find,
        )
        .unwrap();
        assert_eq!(s.get(Role::SynKeyword), Color32::from_rgb(0, 255, 0));
        assert_eq!(s.get(Role::EditorBg), dracula.get(Role::EditorBg), "沒寫的從 base 繼承");
        // 沒有 base：用該主題的預設補齊
        let s = parse_scheme("name = X\nmode = light\ntext = #123456\n", |_| None).unwrap();
        assert!(!s.dark);
        assert_eq!(s.get(Role::Text), Color32::from_rgb(0x12, 0x34, 0x56));
        assert_eq!(s.get(Role::EditorBg), default_for(false).get(Role::EditorBg));
        let err = |t: &str| parse_scheme(t, |_| None).unwrap_err();
        assert!(err("mode = dark").contains("name"));
        assert!(err("name = A\nmode = blue").contains("mode"));
        assert!(err("name = A\nbogus = #000000").contains("bogus"));
        assert!(err("name = A\ntext = red").contains("顏色"));
        assert!(err("name = A\nbase = 不存在").contains("不存在"));
        assert!(err("name = A\njust words").contains("第 2 行"));
    }

    #[test]
    fn custom_schemes_load_save_and_override_builtins_of_the_same_name() {
        let dir = tempfile::tempdir().unwrap();
        let mut mine = default_for(true);
        mine.name = "Dracula".to_owned(); // 與內建同名
        mine.builtin = false;
        mine.set(Role::EditorBg, Color32::from_rgb(1, 2, 3));
        let path = save_custom(dir.path(), &mine).unwrap();
        assert_eq!(path.extension().unwrap(), "conf");
        std::fs::write(dir.path().join("broken.conf"), "name = 壞的\ntext = nope\n").unwrap();
        std::fs::write(dir.path().join("note.txt"), "ignored").unwrap();
        let (custom, messages) = load_custom(dir.path());
        assert_eq!(custom.len(), 1);
        assert_eq!(messages.len(), 1);
        assert!(messages[0].contains("broken.conf"));
        assert_eq!(custom[0].path.as_deref(), Some(path.as_path()));
        let all = combined(&custom);
        // 同名：自訂的蓋過內建的，而且清單裡只列一次
        assert_eq!(pick(&all, true, "Dracula").get(Role::EditorBg), Color32::from_rgb(1, 2, 3));
        let names = names_for(&all, true);
        assert_eq!(names.iter().filter(|s| s.name == "Dracula").count(), 1);
        assert!(!names[0].builtin, "自訂的排在前面");
        // 再存一次：寫回原檔，不會多一個檔案
        let again = save_custom(dir.path(), &custom[0]).unwrap();
        assert_eq!(again, path);
        // 另一個同名的新配色：自動換檔名
        let mut twin = mine.clone();
        twin.path = None;
        let other = save_custom(dir.path(), &twin).unwrap();
        assert_ne!(other, path);
    }

    #[test]
    fn picking_an_unknown_name_falls_back_to_the_default_of_that_mode() {
        let all = builtins();
        assert_eq!(pick(&all, true, "不存在").name, DEFAULT_DARK);
        assert_eq!(pick(&all, false, "不存在").name, DEFAULT_LIGHT);
        // 名字對但主題不對：不能把淺色配色套到深色
        assert_eq!(pick(&all, true, "GitHub Light").name, DEFAULT_DARK);
        assert_eq!(pick(&all, false, "Dracula").name, DEFAULT_LIGHT);
    }

    #[test]
    fn syntax_colors_follow_the_scheme_in_kind_order() {
        let s = default_for(true);
        let c = s.syntax_colors();
        assert_eq!(c[0], s.get(Role::Text));
        assert_eq!(c[1], s.get(Role::SynComment));
        assert_eq!(c[6], s.get(Role::SynFunction));
    }
}
