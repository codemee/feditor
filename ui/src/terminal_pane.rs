//! 終端機：一種特別的分頁。shell 與跳脫序列的處理在 `terminal.rs`，這裡負責畫面、輸入與 App 這一側的管理。
//!
//! 終端機分頁的內容放在 [`crate::document::Document::terminal`]：它和文字分頁一樣跟著 `Document` 搬來搬去，
//! 所以可以拖進任何窗格的標籤列、也可以拖到窗格側邊切成新窗格；分頁不在前景時，shell 照常在背景執行。
//! 鍵盤焦點用 egui 的焦點機制：點一下終端機畫面，它的元件拿到焦點，這一幀開頭把所有按鍵、文字、
//! 複製貼上事件從輸入裡取走、送給 shell，編輯區與全域快捷鍵就不會收到。

use std::sync::atomic::{AtomicU64, Ordering};

use alacritty_terminal::grid::{Dimensions, Grid, Scroll};
use alacritty_terminal::index::{Column, Line, Point, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::term::TermMode;
use alacritty_terminal::term::cell::{Cell, Flags};
use alacritty_terminal::vte::ansi::{Color, NamedColor};
use eframe::egui::{self, Color32};
use feditor_core::i18n::t;
use feditor_core::tf;

use crate::document::Document;
use crate::editor::model::EditorState;
use crate::panes::{PaneId, Side as PaneSide};
use crate::tabs::Tab;
use crate::terminal::{self, GridSize, Terminal};
use crate::terminal_text::{self, Glyph, Segment};
use crate::{App, LogKind};

/// 在編輯區與各個終端機之間切換焦點（沒有終端機就開一個）。不用 Ctrl+C／V／D 等 shell 要用的鍵。
pub const TOGGLE_TERMINAL: egui::KeyboardShortcut = egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::Backtick);
/// 新增一個終端機。
pub const NEW_TERMINAL: egui::KeyboardShortcut = egui::KeyboardShortcut::new(
    egui::Modifiers { shift: true, ..egui::Modifiers::COMMAND },
    egui::Key::Backtick,
);

/// 每個終端機一個編號，畫面元件的 id 用它（分頁搬到別的窗格，焦點與 id 都不變）。
static NEXT_UID: AtomicU64 = AtomicU64::new(1);

/// 畫終端機時用到的設定（來自設定檔）。
pub struct TerminalOptions {
    pub font_size: f32,
    pub min_contrast: f32,
}

impl TerminalOptions {
    pub fn from_settings(s: &crate::settings::Settings) -> Self {
        Self { font_size: s.terminal_font_size, min_contrast: s.terminal_min_contrast }
    }
}

pub struct TerminalPane {
    uid: u64,
    /// 顯示在標題上的編號（目前沒人用的最小正整數）。
    number: usize,
    /// 使用者自己取的名字；沒有就用「終端機 N」。
    custom_name: Option<String>,
    /// 還沒開 shell（第一次畫的時候才知道有幾格）。開失敗時是錯誤訊息。
    term: Option<Terminal>,
    error: Option<String>,
    /// shell 的起始資料夾。
    dir: Option<std::path::PathBuf>,
    /// 開 shell 時要加上的環境變數（執行環境：虛擬環境的 `PATH` 等，見 `runtime_env::terminal_env`）。
    env: Vec<(String, String)>,
    /// shell 還沒開起來（第一次畫面才開）時先收下的輸入，開好之後馬上送進去（「執行目前檔案」用）。
    pending_input: Vec<u8>,
    /// 這個終端機要執行的 shell（程式加參數）；空字串是系統預設。開的時候就決定了，之後改設定不影響已經開著的。
    shell: String,
    /// 下一個畫面要把鍵盤焦點給它。
    want_focus: bool,
    /// 滑鼠滾輪累積的捲動量（行）。
    scroll_acc: f32,
    /// 輸入法正在組的字（還沒確定，不送給 shell；畫在游標的位置）。
    preedit: String,
    /// 程式要求滑鼠回報時，目前按著哪個鍵（0 左、1 中、2 右），以及最後回報的格子（避免同一格重複回報移動）。
    mouse_button: Option<u8>,
    last_mouse_cell: Option<(usize, usize)>,
    /// 終端機內的搜尋列（Ctrl+Shift+F）；`None` 是沒開。
    search: Option<Search>,
}

/// 終端機內搜尋的狀態。
struct Search {
    query: String,
    /// 每個符合項目是一串欄位範圍（跨越折行時不只一段），依畫面由上到下排。
    matches: Vec<Vec<Segment>>,
    /// 目前選的是第幾筆。
    current: Option<usize>,
    /// 上次掃描時的搜尋字串與時間：字串改了、或過了一小段時間（輸出會一直變），才重新掃描。
    scanned_query: String,
    scanned_at: Option<std::time::Instant>,
    /// 下一個畫面把鍵盤焦點給搜尋框。
    focus_input: bool,
    /// 下一個畫面把畫面捲到目前這一筆。
    jump: bool,
}

impl Default for Search {
    fn default() -> Self {
        Self { query: String::new(), matches: Vec::new(), current: None, scanned_query: String::new(), scanned_at: None, focus_input: true, jump: false }
    }
}

impl Search {
    /// 往較早（`older`）或較新的符合項目移動，繞完一圈接回另一頭。
    fn step(&mut self, older: bool) {
        let n = self.matches.len();
        if n == 0 {
            return;
        }
        self.current = Some(match (self.current, older) {
            (None, true) => n - 1,
            (None, false) => 0,
            (Some(0), true) => n - 1,
            (Some(c), true) => c - 1,
            (Some(c), false) => (c + 1) % n,
        });
        self.jump = true;
    }
}

const SEARCH_RESCAN: std::time::Duration = std::time::Duration::from_millis(400);

/// 格線上一列的字元。
fn push_row(grid: &Grid<Cell>, line: i32, out: &mut Vec<Glyph>) {
    let row = &grid[Line(line)];
    for col in 0..grid.columns() {
        let cell = &row[Column(col)];
        if cell.flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER) {
            continue;
        }
        out.push(Glyph { c: if cell.c == '\0' { ' ' } else { cell.c }, line, col, wide: cell.flags.contains(Flags::WIDE_CHAR) });
    }
}

/// 這一列的行尾是不是折行（下一列接著它）。
fn row_wraps(grid: &Grid<Cell>, line: i32) -> bool {
    grid.columns() > 0 && grid[Line(line)][Column(grid.columns() - 1)].flags.contains(Flags::WRAPLINE)
}

/// 從第 `first` 列開始的邏輯行（沿著折行往下接），以及它之後下一列的列號。
fn logical_from(grid: &Grid<Cell>, first: i32) -> (Vec<Glyph>, i32) {
    let bottom = grid.bottommost_line().0;
    let mut out = Vec::new();
    let mut line = first;
    loop {
        push_row(grid, line, &mut out);
        if line < bottom && row_wraps(grid, line) {
            line += 1;
        } else {
            return (out, line + 1);
        }
    }
}

/// 包含第 `line` 列的邏輯行（先沿著折行往上找到開頭）。
fn logical_containing(grid: &Grid<Cell>, line: i32) -> Vec<Glyph> {
    let top = grid.topmost_line().0;
    let mut start = line;
    while start > top && row_wraps(grid, start - 1) {
        start -= 1;
    }
    logical_from(grid, start).0
}

/// 掃描整個終端機（含往上捲出去的歷史），找出所有符合的項目。
fn scan_matches(grid: &Grid<Cell>, query: &str) -> Vec<Vec<Segment>> {
    let mut out = Vec::new();
    if query.is_empty() {
        return out;
    }
    let (top, bottom) = (grid.topmost_line().0, grid.bottommost_line().0);
    let mut line = top;
    while line <= bottom {
        let (glyphs, next) = logical_from(grid, line);
        for (start, end) in terminal_text::find_matches(&glyphs, query) {
            out.push(terminal_text::segments(&glyphs, start, end));
        }
        line = next;
    }
    out
}

fn search_input_id(uid: u64) -> egui::Id {
    egui::Id::unique(("terminal_search_input", uid))
}

impl std::fmt::Debug for TerminalPane {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TerminalPane").field("number", &self.number).finish_non_exhaustive()
    }
}

impl TerminalPane {
    /// 這個終端機的識別編號（每個終端機各不相同，關掉也不會重複使用）。
    pub fn uid(&self) -> u64 {
        self.uid
    }

    /// 測試用：還沒送出的輸入。
    #[cfg(test)]
    pub fn pending_text(&self) -> String {
        String::from_utf8_lossy(&self.pending_input).into_owned()
    }

    /// 把一段文字當成使用者輸入送給 shell。shell 還沒開起來就先收著，開好之後送出。
    pub fn queue_input(&mut self, text: &str) {
        match &self.term {
            Some(term) => term.write(text.as_bytes().to_vec()),
            None => self.pending_input.extend_from_slice(text.as_bytes()),
        }
    }

    /// 設定開 shell 時要加上的環境變數（要在 shell 還沒開之前呼叫）。
    pub fn set_env(&mut self, env: Vec<(String, String)>) {
        self.env = env;
    }

    pub fn new(dir: Option<std::path::PathBuf>, number: usize, shell: String) -> Self {
        Self {
            uid: NEXT_UID.fetch_add(1, Ordering::Relaxed),
            number,
            custom_name: None,
            term: None,
            error: None,
            dir,
            env: Vec::new(),
            pending_input: Vec::new(),
            shell,
            want_focus: true,
            scroll_acc: 0.0,
            preedit: String::new(),
            mouse_button: None,
            last_mouse_cell: None,
            search: None,
        }
    }

    pub fn number(&self) -> usize {
        self.number
    }

    /// 這個終端機要執行的 shell（空字串是系統預設）。
    pub fn shell(&self) -> &str {
        &self.shell
    }

    /// 分頁上的名稱：使用者取的名字，沒有就是「終端機 2」。
    pub fn title(&self) -> String {
        self.custom_name.clone().unwrap_or_else(|| self.default_title())
    }

    /// 使用者自己取的名字（沒有就是 `None`）。
    pub fn custom_name(&self) -> Option<&str> {
        self.custom_name.as_deref()
    }

    /// 沒有自訂名字時的名稱：「終端機 2」。
    pub fn default_title(&self) -> String {
        tf!("終端機 {n}", n = self.number)
    }

    /// 自己取名；前後空白去掉，空的就改回預設名稱。
    pub fn set_custom_name(&mut self, name: &str) {
        let name = name.trim();
        self.custom_name = (!name.is_empty()).then(|| name.to_owned());
    }

    /// shell 已經結束。
    pub fn exited(&self) -> bool {
        self.term.as_ref().is_some_and(Terminal::exited)
    }

    /// 下一個畫面把鍵盤焦點給這個終端機。
    pub fn request_focus(&mut self) {
        self.want_focus = true;
    }

    /// 開搜尋列（已經開著就把鍵盤焦點拉回搜尋框）。
    pub fn open_search(&mut self) {
        self.search.get_or_insert_with(Search::default).focus_input = true;
    }

    pub(crate) fn has_focus(&self, ctx: &egui::Context) -> bool {
        ctx.memory(|m| m.has_focus(widget_id(self.uid)))
    }
}

fn widget_id(uid: u64) -> egui::Id {
    egui::Id::unique(("terminal_widget", uid))
}

/// 目前沒人用的最小正整數（編號跟著關掉的終端機回收）。
fn lowest_free_number(used: impl IntoIterator<Item = usize>) -> usize {
    let used: std::collections::HashSet<usize> = used.into_iter().collect();
    (1..).find(|n| !used.contains(n)).expect("正整數用不完")
}

/// 在 `order` 裡，`current` 的下一個；`current` 是 `None`（焦點不在任何終端機）就是第一個。
/// 回傳 `None` 表示繞完一圈、該回到原來的地方。
fn next_in_cycle<T: PartialEq + Copy>(order: &[T], current: Option<T>) -> Option<T> {
    match current {
        None => order.first().copied(),
        Some(c) => order.iter().position(|x| *x == c).and_then(|i| order.get(i + 1).copied()),
    }
}

impl App {
    /// 所有終端機分頁：（窗格, 分頁編號），依畫面順序（窗格由左到右、由上到下，窗格內由左到右）。
    pub(crate) fn terminal_tabs(&self) -> Vec<(PaneId, usize)> {
        let mut out = Vec::new();
        for id in self.layout.root.leaves() {
            let live = (id == self.layout.focus).then_some(&self.doc);
            if let Some(set) = self.set_ref(id) {
                out.extend(set.terminal_tabs(live).into_iter().map(|i| (id, i)));
            }
        }
        out
    }

    /// 識別編號為 `uid` 的終端機在哪個窗格的第幾個分頁（已經關掉了就是 `None`）。
    fn find_terminal_by_uid(&self, uid: u64) -> Option<(PaneId, usize)> {
        for id in self.layout.root.leaves() {
            let live = (id == self.layout.focus).then_some(&self.doc);
            if let Some((i, _)) = self.set_ref(id).and_then(|s| s.terminal_uids(live).into_iter().find(|(_, u)| *u == uid)) {
                return Some((id, i));
            }
        }
        None
    }

    /// 在終端機執行 `program args… file extra_args`（`extra_args` 是使用者寫的命令列參數，原樣交給 shell）：用上次「執行」用的那個終端機（還在的話），沒有就開一個新的。
    /// 重用時先送 Ctrl+C 中斷上次還沒跑完的程式、再切到檔案所在的資料夾。
    pub(crate) fn run_in_terminal(&mut self, program: &str, args: &[String], file: &std::path::Path, extra_args: &str) {
        let reused = self.run_terminal.and_then(|uid| self.find_terminal_by_uid(uid));
        match reused {
            Some((pane, idx)) => self.show_terminal_tab(pane, idx),
            None => {
                self.new_terminal();
                if self.doc.terminal.is_none() {
                    // 新終端機沒有變成作用中的分頁：找最後一個終端機
                    if let Some((pane, idx)) = self.terminal_tabs().last().copied() {
                        self.show_terminal_tab(pane, idx);
                    }
                }
                self.pending_editor_focus = true;
            }
        }
        let Some(terminal) = self.doc.terminal.as_mut() else {
            self.push(LogKind::Warn, t("開不了終端機，沒辦法執行"));
            return;
        };
        let kind = crate::terminal::shell_kind(terminal.shell());
        let mut text = String::new();
        if reused.is_some() {
            text.push('\u{3}');
            if let Some(dir) = file.parent() {
                text.push_str(&kind.cd_line(&dir.to_string_lossy()));
                text.push('\r');
            }
        }
        text.push_str(&kind.run_line(program, args, &file.to_string_lossy(), extra_args));
        text.push('\r');
        terminal.queue_input(&text);
        self.run_terminal = Some(terminal.uid());
    }

    /// 新增一個終端機：在有焦點的窗格下方切出新窗格放它；那裡放不下就依序試其他窗格（大的先）。
    /// 全都放不下才提示。
    pub(crate) fn new_terminal(&mut self) {
        self.new_terminal_with(None);
    }

    /// 新增一個終端機，指定要執行的 shell（程式加參數）；`None` 用設定裡的預設終端機。
    pub(crate) fn new_terminal_with(&mut self, shell: Option<String>) {
        let rect_of = |app: &App, id: PaneId| app.pane_rects.iter().find(|(p, _)| *p == id).map(|(_, r)| *r);
        let focus = self.layout.focus;
        let mut others: Vec<PaneId> = self.layout.root.leaves().into_iter().filter(|id| *id != focus && !self.is_preview(*id)).collect();
        others.sort_by(|a, b| {
            let area = |id: &PaneId| rect_of(self, *id).map_or(0.0, |r| r.width() * r.height());
            area(b).total_cmp(&area(a))
        });
        let mut candidates = vec![focus];
        candidates.extend(others);
        // 還沒畫過（沒有範圍資料）時照樣允許，之後畫面會依最小窗格大小調整
        let target = candidates.into_iter().find(|id| rect_of(self, *id).is_none_or(|r| crate::panes::can_split(r, PaneSide::Bottom)));
        let Some(target) = target else {
            self.push(LogKind::Warn, t("窗格太小，放不下終端機"));
            return;
        };
        let mut numbers = Vec::new();
        for id in self.layout.root.leaves() {
            let live = (id == self.layout.focus).then_some(&self.doc);
            if let Some(set) = self.set_ref(id) {
                numbers.extend(set.terminal_numbers(live));
            }
        }
        let dir = self.doc.path.as_ref().and_then(|p| p.parent().map(|d| d.to_path_buf()));
        let shell = shell.unwrap_or_else(|| self.settings.terminal_shell.clone());
        let dir = dir.or_else(|| std::env::current_dir().ok());
        let env = self.terminal_env(dir.as_deref());
        let mut doc = Document::new_terminal(lowest_free_number(numbers), dir, shell);
        if let Some(terminal) = doc.terminal.as_mut() {
            terminal.set_env(env);
        }
        self.open_tab_in_new_pane(target, PaneSide::Bottom, Tab { doc, editor: EditorState::default() });
    }

    /// 切到第 `idx` 個分頁（在窗格 `pane`）並把鍵盤焦點給它。
    fn show_terminal_tab(&mut self, pane: PaneId, idx: usize) {
        self.focus_pane(pane);
        self.switch_tab(idx);
        self.pending_editor_focus = true; // 作用中的是終端機，所以焦點會給終端機
    }

    /// 繞完終端機之後，回到按 Ctrl+` 之前所在的分頁。
    fn return_from_terminals(&mut self) {
        let Some((pane, idx)) = self.terminal_return.take() else { return };
        if !self.layout.root.contains(pane) || self.is_preview(pane) {
            return;
        }
        self.focus_pane(pane);
        if idx < self.tabs.len() {
            self.switch_tab(idx);
        }
        self.pending_editor_focus = true;
    }

    /// 快捷鍵：沒有終端機就開一個；有的話焦點依序在各個終端機（畫面順序）之間循環，
    /// 繞完一圈回到原來的分頁。
    pub(crate) fn focus_terminal_shortcut(&mut self, ctx: &egui::Context) {
        let tabs = self.terminal_tabs();
        if tabs.is_empty() {
            self.new_terminal();
            return;
        }
        let here = self.doc.terminal.as_ref().filter(|t| t.has_focus(ctx)).map(|_| (self.layout.focus, self.tabs.active()));
        match next_in_cycle(&tabs, here) {
            Some((pane, idx)) => {
                if here.is_none() {
                    self.terminal_return = Some((self.layout.focus, self.tabs.active()));
                }
                self.show_terminal_tab(pane, idx);
            }
            None => self.return_from_terminals(),
        }
    }

    /// shell 結束了（使用者輸入 exit）：關掉那個分頁（一個畫面處理一個）。
    pub(crate) fn reap_terminals(&mut self, ctx: &egui::Context) {
        let mut done = None;
        for id in self.layout.root.leaves() {
            let live = (id == self.layout.focus).then_some(&self.doc);
            if let Some(i) = self.set_ref(id).and_then(|s| s.exited_terminal_tabs(live).first().copied()) {
                done = Some((id, i));
                break;
            }
        }
        if let Some((pane, i)) = done {
            self.handle(crate::Action::PaneTab(pane, crate::pane_ops::TabCmd::Close(i)), ctx);
        }
    }

    /// 作用中的分頁是終端機而且有鍵盤焦點時，把這一幀的按鍵等輸入事件從 egui 取走，送給 shell。
    /// 要在其他快捷鍵處理之前呼叫。
    pub(crate) fn terminal_input(&mut self, ctx: &egui::Context) {
        // 游標在搜尋框裡：Ctrl+F 回到搜尋框（不要變成編輯區的尋找）
        if let Some(pane) = self.doc.terminal.as_mut() {
            if pane.search.is_some() && ctx.memory(|m| m.has_focus(search_input_id(pane.uid))) && ctx.input_mut(|i| i.consume_shortcut(&crate::FIND)) {
                pane.open_search();
            }
        }
        let Some(pane) = self.doc.terminal.as_mut().filter(|t| t.has_focus(ctx)) else { return };
        let Some(term) = pane.term.as_ref() else { return };
        let mut taken: Vec<egui::Event> = Vec::new();
        ctx.input_mut(|i| {
            i.events.retain(|e| {
                let mine = match e {
                    // 開關終端機的快捷鍵留給全域處理
                    egui::Event::Key { key, modifiers, .. } if *key == TOGGLE_TERMINAL.logical_key && modifiers.command => false,
                    egui::Event::Key { .. } | egui::Event::Text(_) | egui::Event::Copy | egui::Event::Cut | egui::Event::Paste(_) | egui::Event::Ime(_) => true,
                    _ => false,
                };
                if mine {
                    taken.push(e.clone());
                }
                !mine
            });
        });
        let mut preedit: Option<String> = None;
        let mut open_search = false;
        let alt = ctx.input(|i| i.modifiers.alt && !i.modifiers.ctrl);
        let mode = term.mode();
        let mut out: Vec<u8> = Vec::new();
        for event in taken {
            match event {
                egui::Event::Text(text) => {
                    if text.chars().all(|c| !c.is_control()) {
                        if alt {
                            out.extend(terminal::alt_text(&text));
                        } else {
                            out.extend(text.as_bytes());
                        }
                    }
                }
                egui::Event::Ime(egui::ImeEvent::Commit(text)) => {
                    out.extend(text.as_bytes());
                    preedit = Some(String::new());
                }
                egui::Event::Ime(egui::ImeEvent::Preedit { text, .. }) => preedit = Some(text),
                // Ctrl+F：和編輯區一樣開搜尋列。全螢幕程式（vim、less 這類）在執行時，Ctrl+F 還是交給程式
                egui::Event::Key { key: egui::Key::F, pressed: true, modifiers, .. }
                    if (modifiers.ctrl || modifiers.command) && !modifiers.shift && !modifiers.alt && !mode.contains(TermMode::ALT_SCREEN) =>
                {
                    open_search = true;
                }
                egui::Event::Key { key, pressed: true, modifiers, .. } => {
                    let clipboard_key = (modifiers.command || modifiers.ctrl) && matches!(key, egui::Key::C | egui::Key::X | egui::Key::V);
                    let alt_gr = modifiers.ctrl && modifiers.alt;
                    if !clipboard_key && !alt_gr {
                        if let Some(bytes) = terminal::encode_key(key, modifiers, mode) {
                            out.extend(bytes);
                        }
                    }
                }
                egui::Event::Copy => {
                    let selected = term.term.lock().selection_to_string().filter(|s| !s.is_empty());
                    match selected {
                        Some(s) => ctx.copy_text(s),
                        None => out.push(0x03), // 沒有選取：就是 Ctrl+C，中斷執行中的程式
                    }
                }
                egui::Event::Cut => out.push(0x18),
                egui::Event::Paste(text) => out.extend(terminal::paste_bytes(&text, mode)),
                _ => {}
            }
        }
        if !out.is_empty() {
            term.term.lock().scroll_display(Scroll::Bottom);
            term.write(out);
        }
        if let Some(p) = preedit {
            pane.preedit = p;
        }
        if open_search {
            pane.open_search();
        }
    }
}

/// 畫終端機的畫面（分頁列下面那一塊）；第一次畫的時候才開 shell。
pub(crate) fn draw_body(ui: &mut egui::Ui, pane: &mut TerminalPane, body: egui::Rect, opts: &TerminalOptions) {
    let ctx = ui.ctx().clone();
    let dark = ui.visuals().dark_mode;
    let bg = ui.visuals().extreme_bg_color;
    ui.painter().rect_filled(body, egui::CornerRadius::same(4), bg);
    let content = body.shrink(6.0);
    ui.set_clip_rect(content.intersect(ui.clip_rect()));
        // 一格的大小：用終端機自己的字級（設定裡調整）
        let font = egui::FontId::monospace(opts.font_size);
        let (cw, ch) = ctx.fonts_mut(|f| (f.glyph_width(&font, '0').max(1.0), f.row_height(&font).max(1.0)));
        let size = GridSize { cols: ((content.width() / cw).floor() as usize).max(2), lines: ((content.height() / ch).floor() as usize).max(1) };

        if pane.term.is_none() && pane.error.is_none() {
            match Terminal::spawn(&ctx, size, (cw as u16, ch as u16), pane.dir.clone(), &pane.shell, &pane.env) {
                Ok(t) => {
                    if !pane.pending_input.is_empty() {
                        t.write(std::mem::take(&mut pane.pending_input));
                    }
                    pane.term = Some(t);
                }
                Err(e) => pane.error = Some(tf!("無法開啟 shell：{e}", e = e)),
            }
        }
        if let Some(e) = &pane.error {
            ui.scope_builder(egui::UiBuilder::new().max_rect(content), |ui| {
                ui.label(egui::RichText::new(e).color(ui.visuals().error_fg_color));
            });
            return;
        }
        let preedit = pane.preedit.clone();
        let Some(term) = pane.term.as_mut() else { return };
        term.resize(size, (cw as u16, ch as u16));

        // 互動：點一下取得焦點、拖曳選取、滾輪捲動
        let wid = widget_id(pane.uid);
        let resp = ui.interact(content, wid, egui::Sense::click_and_drag());
        if resp.hovered() && ctx.input(|i| i.pointer.primary_pressed()) {
            pane.want_focus = true;
        }
        if std::mem::take(&mut pane.want_focus) {
            ctx.memory_mut(|m| m.request_focus(wid));
        }
        let focused = ctx.memory(|m| m.has_focus(wid));
        if focused {
            // Tab、方向鍵、Esc 都是給 shell 的，不要讓 egui 拿去切換焦點
            ctx.memory_mut(|m| m.set_focus_lock_filter(wid, egui::EventFilter { tab: true, horizontal_arrows: true, vertical_arrows: true, escape: true }));
        }
        let mode = term.mode();
        // 程式要求滑鼠回報（vim、htop 這類）時，滑鼠事件送給程式；按住 Shift 則照一般終端機的做法，仍然可以選取文字
        let report = mode.intersects(TermMode::MOUSE_MODE) && !ctx.input(|i| i.modifiers.shift);
        let display_offset = term.term.lock().grid().display_offset() as i32;
        let point_at = |pos: egui::Pos2| -> (Point, Side) {
            let x = (pos.x - content.min.x) / cw;
            let col = (x.floor().max(0.0) as usize).min(size.cols - 1);
            let row = (((pos.y - content.min.y) / ch).floor().max(0.0) as i32).min(size.lines as i32 - 1);
            let side = if x.fract() < 0.5 { Side::Left } else { Side::Right };
            (Point::new(Line(row - display_offset), Column(col)), side)
        };
        if let Some(pos) = resp.interact_pointer_pos().filter(|_| !report) {
            if resp.drag_started_by(egui::PointerButton::Primary) {
                let (p, side) = point_at(pos);
                term.term.lock().selection = Some(Selection::new(SelectionType::Simple, p, side));
            } else if resp.dragged_by(egui::PointerButton::Primary) {
                let (p, side) = point_at(pos);
                if let Some(sel) = term.term.lock().selection.as_mut() {
                    sel.update(p, side);
                }
            }
        }
        if resp.clicked() && !report {
            term.term.lock().selection = None;
        }
        if resp.hovered() && !report {
            let dy = ctx.input(|i| i.smooth_scroll_delta.y);
            pane.scroll_acc += dy / ch;
            let lines = pane.scroll_acc.trunc() as i32;
            if lines != 0 {
                pane.scroll_acc -= lines as f32;
                if mode.contains(TermMode::ALT_SCREEN) {
                    // vim、less 這類全螢幕程式：滾輪改成方向鍵
                    let key: &[u8] = if lines > 0 { b"\x1b[A" } else { b"\x1b[B" };
                    for _ in 0..lines.abs() * 3 {
                        term.write(key.to_vec());
                    }
                } else {
                    term.term.lock().scroll_display(Scroll::Delta(lines));
                }
            }
        }

        // 滑鼠回報：按下、放開、拖曳（依程式要求的模式）與滾輪，換算成格子座標送給程式
        if report {
            let (events, mods, hover) = ctx.input(|i| (i.events.clone(), i.modifiers, i.pointer.hover_pos()));
            let cell_at = |pos: egui::Pos2| {
                let col = ((pos.x - content.min.x) / cw).floor().max(0.0) as usize;
                let row = ((pos.y - content.min.y) / ch).floor().max(0.0) as usize;
                (col.min(size.cols - 1), row.min(size.lines - 1))
            };
            let mut out: Vec<u8> = Vec::new();
            for e in events {
                match e {
                    egui::Event::PointerButton { pos, button, pressed, modifiers } => {
                        let code = match button {
                            egui::PointerButton::Primary => 0,
                            egui::PointerButton::Middle => 1,
                            egui::PointerButton::Secondary => 2,
                            _ => continue,
                        };
                        if pressed {
                            if !content.contains(pos) {
                                continue;
                            }
                            pane.mouse_button = Some(code);
                        } else if pane.mouse_button == Some(code) {
                            pane.mouse_button = None;
                        } else {
                            continue;
                        }
                        let (col, row) = cell_at(pos);
                        pane.last_mouse_cell = Some((col, row));
                        if let Some(bytes) = terminal::encode_mouse(code, col, row, !pressed, modifiers, false, mode) {
                            out.extend(bytes);
                        }
                    }
                    egui::Event::PointerMoved(pos) => {
                        let wants = mode.contains(TermMode::MOUSE_MOTION) || (mode.contains(TermMode::MOUSE_DRAG) && pane.mouse_button.is_some());
                        if !wants || !(content.contains(pos) || pane.mouse_button.is_some()) {
                            continue;
                        }
                        let cell = cell_at(pos);
                        if pane.last_mouse_cell == Some(cell) {
                            continue;
                        }
                        pane.last_mouse_cell = Some(cell);
                        if let Some(bytes) = terminal::encode_mouse(pane.mouse_button.unwrap_or(3), cell.0, cell.1, false, mods, true, mode) {
                            out.extend(bytes);
                        }
                    }
                    _ => {}
                }
            }
            if resp.hovered() {
                pane.scroll_acc += ctx.input(|i| i.smooth_scroll_delta.y) / ch;
                let lines = pane.scroll_acc.trunc() as i32;
                if lines != 0 {
                    pane.scroll_acc -= lines as f32;
                    let (col, row) = hover.map_or((0, 0), cell_at);
                    for _ in 0..lines.abs().min(10) {
                        if let Some(bytes) = terminal::encode_mouse(if lines > 0 { 64 } else { 65 }, col, row, false, mods, false, mode) {
                            out.extend(bytes);
                        }
                    }
                }
            }
            if !out.is_empty() {
                term.write(out);
            }
        } else {
            pane.mouse_button = None;
        }

        // 搜尋：搜尋字串改了、或過了一小段時間就重新掃描整個終端機（含歷史）；要跳到目前這一筆時把畫面捲過去
        if let Some(search) = pane.search.as_mut() {
            let changed = search.query != search.scanned_query;
            if changed || search.scanned_at.is_none_or(|t| t.elapsed() >= SEARCH_RESCAN) {
                search.matches = scan_matches(term.term.lock().grid(), &search.query);
                search.scanned_query = search.query.clone();
                search.scanned_at = Some(std::time::Instant::now());
                search.current = match (changed, search.current) {
                    // 新的搜尋字串：從最新（最下面）的一筆開始
                    (true, _) => search.matches.len().checked_sub(1),
                    (false, Some(c)) => (!search.matches.is_empty()).then(|| c.min(search.matches.len() - 1)),
                    (false, None) => None,
                };
                search.jump |= changed;
            }
            if std::mem::take(&mut search.jump) {
                if let Some(seg) = search.current.and_then(|c| search.matches.get(c)).and_then(|m| m.first()) {
                    term.term.lock().scroll_to_point(Point::new(Line(seg.line), Column(seg.start_col)));
                }
            }
            ctx.request_repaint_after(SEARCH_RESCAN);
        }
        // 畫面上看得到的符合項目：列號 → （起欄, 迄欄, 是不是目前這一筆）
        let display_offset = term.term.lock().grid().display_offset() as i32;
        let mut highlights: std::collections::HashMap<i32, Vec<(usize, usize, bool)>> = std::collections::HashMap::new();
        if let Some(search) = pane.search.as_ref() {
            let (first, last) = (-display_offset, -display_offset + size.lines as i32 - 1);
            for (i, segs) in search.matches.iter().enumerate() {
                for s in segs.iter().filter(|s| s.line >= first && s.line <= last) {
                    highlights.entry(s.line).or_default().push((s.start_col, s.end_col, search.current == Some(i)));
                }
            }
        }
        // 網址：按住 Ctrl 把滑鼠移到網址上，整段網址加底線、游標變成手指，按一下就用預設瀏覽器開啟。
        // （程式要求滑鼠回報時，滑鼠事件歸程式；按住 Shift 取消回報之後，Ctrl＋Shift＋點擊一樣可以開網址）
        let mut link_cells: std::collections::HashSet<(i32, usize)> = std::collections::HashSet::new();
        let ctrl_held = ctx.input(|i| i.modifiers.ctrl || i.modifiers.command);
        if ctrl_held && !report {
            if let Some(pos) = ctx.input(|i| i.pointer.hover_pos()).filter(|p| content.contains(*p)) {
                let (p, _) = point_at(pos);
                let (line, col) = (p.line.0, p.column.0);
                let glyphs = logical_containing(term.term.lock().grid(), line);
                let hovered = glyphs.iter().position(|g| g.line == line && col >= g.col && col < g.col + g.width());
                if let Some((start, end, url)) = hovered.and_then(|i| terminal_text::url_at(&glyphs, i)) {
                    for g in &glyphs[start..end] {
                        for k in 0..g.width() {
                            link_cells.insert((g.line, g.col + k));
                        }
                    }
                    ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
                    if resp.clicked() {
                        ctx.open_url(egui::OpenUrl::new_tab(url));
                    }
                }
            }
        }

        // 畫格子
        let painter = ui.painter_at(content);
        let palette = Palette::new(ui.visuals().text_color(), ui.visuals().extreme_bg_color, crate::theme::terminal_colors(&ctx, dark), opts.min_contrast);
        let accent = ui.visuals().selection.bg_fill;
        let guard = term.term.lock();
        let rc = guard.renderable_content();
        let offset = rc.display_offset as i32;
        let cell_rect = |col: usize, row: i32, cells: f32| {
            egui::Rect::from_min_size(egui::pos2(content.min.x + col as f32 * cw, content.min.y + row as f32 * ch), egui::vec2(cw * cells, ch))
        };
        for cell in rc.display_iter {
            let flags = cell.cell.flags;
            if flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER) {
                continue;
            }
            let row = cell.point.line.0 + offset;
            if row < 0 || row >= size.lines as i32 {
                continue;
            }
            let col = cell.point.column.0;
            let cells = if flags.contains(Flags::WIDE_CHAR) { 2.0 } else { 1.0 };
            let (mut fg, mut bgc) = (cell.cell.fg, cell.cell.bg);
            if flags.contains(Flags::INVERSE) {
                std::mem::swap(&mut fg, &mut bgc);
            }
            let selected = rc.selection.is_some_and(|s| s.contains(cell.point));
            let r = cell_rect(col, row, cells);
            // 這一格實際的底色：選取、程式指定的底色，或窗格自己的底色
            let cell_bg = if selected { accent } else { palette.background(bgc).unwrap_or(palette.default_bg) };
            if selected {
                painter.rect_filled(r, 0.0, accent);
            } else if let Some(c) = palette.background(bgc) {
                painter.rect_filled(r, 0.0, c);
            }
            if let Some(spans) = highlights.get(&cell.point.line.0) {
                for &(start, end, current) in spans {
                    if col >= start && col < end {
                        let warn = ui.visuals().warn_fg_color;
                        painter.rect_filled(r, 0.0, warn.gamma_multiply(if current { 0.6 } else { 0.28 }));
                    }
                }
            }
            let ch_ = cell.cell.c;
            if ch_ != ' ' && ch_ != '\0' && !flags.contains(Flags::HIDDEN) {
                let mut color = palette.foreground(fg, flags.contains(Flags::BOLD));
                if !is_graphic_glyph(ch_) {
                    color = palette.readable(color, cell_bg);
                }
                if flags.contains(Flags::DIM) {
                    color = color.gamma_multiply(0.6);
                }
                painter.text(r.min, egui::Align2::LEFT_TOP, ch_.to_string(), font.clone(), color);
            }
            if flags.intersects(Flags::ALL_UNDERLINES) {
                let y = r.max.y - 1.0;
                painter.line_segment([egui::pos2(r.min.x, y), egui::pos2(r.max.x, y)], egui::Stroke::new(1.0, palette.foreground(fg, false)));
            }
            if flags.contains(Flags::STRIKEOUT) {
                let y = r.center().y;
                painter.line_segment([egui::pos2(r.min.x, y), egui::pos2(r.max.x, y)], egui::Stroke::new(1.0, palette.foreground(fg, false)));
            }
            if link_cells.contains(&(cell.point.line.0, col)) {
                let y = r.max.y - 1.0;
                painter.line_segment([egui::pos2(r.min.x, y), egui::pos2(r.max.x, y)], egui::Stroke::new(1.5, ui.visuals().hyperlink_color));
            }
        }
        // 游標
        let cur = rc.cursor;
        let cur_row = cur.point.line.0 + offset;
        if rc.mode.contains(TermMode::SHOW_CURSOR) && cur_row >= 0 && cur_row < size.lines as i32 {
            let r = cell_rect(cur.point.column.0, cur_row, 1.0);
            let color = ui.visuals().text_color();
            if focused {
                match cur.shape {
                    alacritty_terminal::vte::ansi::CursorShape::Beam => {
                        {
                            painter.rect_filled(egui::Rect::from_min_size(r.min, egui::vec2(2.0, ch)), 0.0, color);
                        }
                    }
                    alacritty_terminal::vte::ansi::CursorShape::Underline => {
                        {
                            painter.rect_filled(egui::Rect::from_min_size(egui::pos2(r.min.x, r.max.y - 2.0), egui::vec2(cw, 2.0)), 0.0, color);
                        }
                    }
                    _ => {
                        painter.rect_filled(r, 0.0, color.gamma_multiply(0.55));
                    }
                }
            } else {
                painter.rect_stroke(r, 0.0, egui::Stroke::new(1.0, color), egui::StrokeKind::Inside);
            }
        }
        // 輸入法：告訴系統候選字視窗要放在游標旁，組字中的字畫在游標位置（底線、蓋住底下的格子）
        if focused {
            let start_col = cur.point.column.0.min(size.cols - 1);
            let start_row = cur_row.clamp(0, size.lines as i32 - 1);
            let mut at = cell_rect(start_col, start_row, 1.0);
            if !preedit.is_empty() {
                let text_color = ui.visuals().text_color();
                // 組字的字照格子排：寬字佔兩格，超過右邊界就換到下一列；排不下時整段往上靠，讓最後一列留在畫面內
                let mut rows: Vec<(usize, String)> = vec![(start_col, String::new())];
                let mut col = start_col;
                for c in preedit.chars() {
                    let w = if ctx.fonts_mut(|f| f.glyph_width(&font, c)) > cw * 1.5 { 2 } else { 1 };
                    if col + w > size.cols && col > 0 {
                        rows.push((0, String::new()));
                        col = 0;
                    }
                    rows.last_mut().unwrap().1.push(c);
                    col += w;
                }
                let shift = (start_row + rows.len() as i32 - size.lines as i32).max(0);
                let stroke = egui::Stroke::new(1.0, text_color);
                for (i, (from, line)) in rows.iter().enumerate() {
                    let row = start_row + i as i32 - shift;
                    let galley = painter.layout_no_wrap(line.clone(), font.clone(), text_color);
                    let origin = cell_rect(*from, row, 1.0).min;
                    let box_ = egui::Rect::from_min_size(origin, egui::vec2(galley.size().x.max(cw), ch));
                    painter.rect_filled(box_, 0.0, ui.visuals().extreme_bg_color);
                    painter.galley(origin, galley.clone(), text_color);
                    let y = box_.max.y - 1.0;
                    painter.line_segment([egui::pos2(box_.min.x, y), egui::pos2(box_.min.x + galley.size().x, y)], stroke);
                    if i + 1 == rows.len() {
                        // 組字中的插入點：畫在組字文字的最後面，輸入法的候選視窗也跟著它走
                        let x = box_.min.x + galley.size().x;
                        at = egui::Rect::from_min_size(egui::pos2(x, origin.y), egui::vec2(cw, ch));
                        painter.rect_filled(egui::Rect::from_min_size(egui::pos2(x, origin.y), egui::vec2(2.0, ch)), 0.0, text_color);
                    }
                }
            }
            ui.output_mut(|o| {
                o.ime = Some(egui::output::IMEOutput {
                    purpose: egui::IMEPurpose::Terminal,
                    rect: content,
                    cursor_rect: at,
                    should_interrupt_composition: false,
                });
            });
        }
        // 搜尋列
        let uid = pane.uid;
        let mut close_search = false;
        if let Some(search) = pane.search.as_mut() {
            let bar = egui::Rect::from_min_size(egui::pos2(content.max.x - 360.0, content.min.y + 2.0), egui::vec2(356.0, 30.0));
            let bar = bar.intersect(content.expand(-2.0));
            ui.scope_builder(egui::UiBuilder::new().max_rect(bar), |ui| {
                egui::Frame::popup(ui.style()).inner_margin(egui::Margin::symmetric(6, 2)).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let input = ui.add(
                            egui::TextEdit::singleline(&mut search.query)
                                .id(search_input_id(uid))
                                .desired_width(150.0)
                                .hint_text(t("在終端機中搜尋")),
                        );
                        if std::mem::take(&mut search.focus_input) {
                            input.request_focus();
                        }
                        let count = match (search.current, search.matches.len()) {
                            (_, 0) if search.query.is_empty() => String::new(),
                            (_, 0) => t("沒有符合").to_owned(),
                            (Some(c), n) => format!("{}/{}", c + 1, n),
                            (None, n) => format!("0/{n}"),
                        };
                        ui.label(egui::RichText::new(count).weak());
                        if crate::icons::small_button(ui, crate::Icon::ChevronUp, t("較早的符合項目（Enter）"), true).clicked() {
                            search.step(true);
                        }
                        if crate::icons::small_button(ui, crate::Icon::ChevronDown, t("較新的符合項目（Shift+Enter）"), true).clicked() {
                            search.step(false);
                        }
                        if crate::icons::small_button(ui, crate::Icon::Close, t("關閉搜尋（Esc）"), true).clicked() {
                            close_search = true;
                        }
                        // Enter 找下一筆（Shift+Enter 反方向），Esc 關閉；輸入框按 Enter 或 Esc 會失去焦點，要再要回來
                        if input.lost_focus() {
                            let (enter, esc, shift) = ui.input(|i| (i.key_pressed(egui::Key::Enter), i.key_pressed(egui::Key::Escape), i.modifiers.shift));
                            if esc {
                                close_search = true;
                            } else if enter {
                                search.step(!shift);
                                input.request_focus();
                            }
                        }
                    });
                });
            });
        }
        if close_search {
            pane.search = None;
            pane.want_focus = true;
        }
}


/// WCAG 相對亮度。
fn luminance(c: Color32) -> f32 {
    let channel = |v: u8| {
        let s = v as f32 / 255.0;
        if s <= 0.03928 { s / 12.92 } else { ((s + 0.055) / 1.055).powf(2.4) }
    };
    0.2126 * channel(c.r()) + 0.7152 * channel(c.g()) + 0.0722 * channel(c.b())
}

/// WCAG 對比度（1～21）。
fn contrast(a: Color32, b: Color32) -> f32 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// 方塊、表格線、點字、Powerline 符號這類「圖形字元」：程式常刻意讓前景色和底色接近來拼出圖案
///（進度條、狀態列的三角形接縫），調整它們的顏色會破壞圖案，所以不做對比度保護。
fn is_graphic_glyph(c: char) -> bool {
    matches!(c as u32, 0x2500..=0x259F | 0x2800..=0x28FF | 0xE0B0..=0xE0BF)
}

/// 終端機的顏色：16 色來自配色方案，256 色的其餘部分（色立方與灰階）是標準色，真彩色原樣畫。
struct Palette {
    default_fg: Color32,
    /// 預設背景（窗格的底色）。反相的格子，前景要用它。
    default_bg: Color32,
    /// ANSI 0–15，來自目前的配色方案（見 scheme::Scheme::terminal_colors）。
    ansi: [Color32; 16],
    /// 文字和底色的最低對比度；1 以下表示不調整。
    min_contrast: f32,
}

impl Palette {
    fn new(default_fg: Color32, default_bg: Color32, ansi: [Color32; 16], min_contrast: f32) -> Self {
        Self { default_fg, default_bg, ansi, min_contrast }
    }

    fn ansi(&self, i: usize) -> Color32 {
        self.ansi[i]
    }

    /// 最低對比度保護：前景和它所在的底色對比不到 [`MIN_CONTRAST`] 時，把前景往能拉開差距的方向（底色亮就往黑、
    /// 底色暗就往白）一步步調整，直到夠為止；調不到（例如前景和底色一樣）就直接用黑或白。
    /// 對比夠的顏色原封不動，所以多數輸出不受影響。
    fn readable(&self, fg: Color32, bg: Color32) -> Color32 {
        if self.min_contrast <= 1.0 || contrast(fg, bg) >= self.min_contrast {
            return fg;
        }
        let target = if luminance(bg) > 0.5 { Color32::BLACK } else { Color32::WHITE };
        for step in 1..=10 {
            let t = step as f32 / 10.0;
            let mix = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round() as u8;
            let c = Color32::from_rgb(mix(fg.r(), target.r()), mix(fg.g(), target.g()), mix(fg.b(), target.b()));
            if contrast(c, bg) >= self.min_contrast {
                return c;
            }
        }
        target
    }
    fn indexed(&self, i: usize) -> Color32 {
        match i {
            0..=15 => self.ansi(i),
            16..=231 => {
                let n = i - 16;
                let level = |v: usize| [0u8, 95, 135, 175, 215, 255][v];
                Color32::from_rgb(level(n / 36), level(n / 6 % 6), level(n % 6))
            }
            _ => {
                let v = (8 + 10 * (i.min(255) - 232)) as u8;
                Color32::from_gray(v)
            }
        }
    }

    /// 前景色。粗體的 8 色用亮色版本（和多數終端機一樣）。
    fn foreground(&self, c: Color, bold: bool) -> Color32 {
        match c {
            Color::Spec(rgb) => Color32::from_rgb(rgb.r, rgb.g, rgb.b),
            Color::Indexed(i) => self.indexed(if bold && i < 8 { i as usize + 8 } else { i as usize }),
            Color::Named(n) => match n {
                NamedColor::Foreground | NamedColor::BrightForeground | NamedColor::Cursor => self.default_fg,
                NamedColor::DimForeground => self.default_fg.gamma_multiply(0.6),
                NamedColor::Background => self.default_bg,
                other => {
                    let i = other as usize;
                    match i {
                        0..=7 => self.ansi(if bold { i + 8 } else { i }),
                        8..=15 => self.ansi(i),
                        // DimBlack…DimWhite（259～266）：對應的暗色
                        259..=266 => self.ansi(i - 259).gamma_multiply(0.6),
                        _ => self.default_fg,
                    }
                }
            },
        }
    }

    /// 背景色；預設背景回傳 `None`（用窗格自己的底色，不另外畫）。
    fn background(&self, c: Color) -> Option<Color32> {
        match c {
            Color::Spec(rgb) => Some(Color32::from_rgb(rgb.r, rgb.g, rgb.b)),
            Color::Indexed(i) => Some(self.indexed(i as usize)),
            Color::Named(NamedColor::Background) => None,
            Color::Named(n) => {
                let i = n as usize;
                match i {
                    0..=15 => Some(self.ansi(i)),
                    259..=266 => Some(self.ansi(i - 259).gamma_multiply(0.6)),
                    // 前景色當背景（反相之後的預設前景）
                    _ => Some(self.default_fg),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alacritty_terminal::vte::ansi::Rgb;
    use crate::settings::DEFAULT_TERMINAL_MIN_CONTRAST as MIN_CONTRAST;

    #[test]
    fn palette_maps_named_indexed_and_true_colors() {
        let p = Palette::new(Color32::WHITE, Color32::BLACK, crate::scheme::default_for(true).terminal_colors(), MIN_CONTRAST);
        assert_eq!(p.foreground(Color::Spec(Rgb { r: 1, g: 2, b: 3 }), false), Color32::from_rgb(1, 2, 3));
        assert_eq!(p.foreground(Color::Named(NamedColor::Foreground), false), Color32::WHITE);
        assert_eq!(p.foreground(Color::Named(NamedColor::Background), false), Color32::BLACK, "反相後的前景是窗格底色");
        assert_eq!(p.foreground(Color::Named(NamedColor::Red), false), p.ansi(1));
        assert_eq!(p.foreground(Color::Named(NamedColor::Red), true), p.ansi(9), "粗體用亮色");
        assert_eq!(p.foreground(Color::Indexed(2), false), p.ansi(2));
        // 6×6×6 色立方與灰階
        assert_eq!(p.indexed(16), Color32::from_rgb(0, 0, 0));
        assert_eq!(p.indexed(231), Color32::from_rgb(255, 255, 255));
        assert_eq!(p.indexed(196), Color32::from_rgb(255, 0, 0));
        assert_eq!(p.indexed(232), Color32::from_gray(8));
        assert_eq!(p.indexed(255), Color32::from_gray(238));
        // 最低對比度可以關掉（1 以下）：低對比的文字就原樣畫
        let off = Palette::new(Color32::WHITE, Color32::BLACK, crate::scheme::default_for(true).terminal_colors(), 1.0);
        let dim = Color32::from_rgb(0x20, 0x20, 0x20);
        assert_eq!(off.readable(dim, Color32::BLACK), dim);
        // 預設背景不畫
        assert_eq!(p.background(Color::Named(NamedColor::Background)), None);
        assert!(p.background(Color::Named(NamedColor::Blue)).is_some());
    }

    #[test]
    fn sgr_sequences_reach_the_grid_as_colors_the_palette_can_draw() {
        use alacritty_terminal::event::VoidListener;
        use alacritty_terminal::term::Config;
        use alacritty_terminal::vte::ansi::{Processor, StdSyncHandler};
        let size = GridSize { cols: 20, lines: 3 };
        let mut term = alacritty_terminal::Term::new(Config::default(), &size, VoidListener);
        let mut parser: Processor<StdSyncHandler> = Processor::new();
        parser.advance(&mut term, b"\x1b[31mR\x1b[0m\x1b[44;97mB\x1b[0m\x1b[38;2;255;128;0mT\x1b[0m\x1b[38;5;196mI\x1b[0m\x1b[1;32mG\x1b[0mN");
        let p = Palette::new(Color32::BLACK, Color32::WHITE, crate::scheme::default_for(false).terminal_colors(), MIN_CONTRAST);
        let cell = |c: usize| term.grid()[Line(0)][Column(c)].clone();
        let (r, b, t, i, g, n) = (cell(0), cell(1), cell(2), cell(3), cell(4), cell(5));
        assert_ne!(p.foreground(r.fg, false), Color32::BLACK, "紅字 {:?}", r.fg);
        assert!(p.background(b.bg).is_some(), "藍底 {:?}", b.bg);
        assert_eq!(p.foreground(b.fg, false), p.ansi(15), "亮白字 {:?}", b.fg);
        assert_eq!(p.foreground(t.fg, false), Color32::from_rgb(255, 128, 0), "真彩色 {:?}", t.fg);
        assert_eq!(p.foreground(i.fg, false), Color32::from_rgb(255, 0, 0), "256 色 {:?}", i.fg);
        assert_ne!(p.foreground(g.fg, g.flags.contains(Flags::BOLD)), Color32::BLACK, "粗體綠 {:?}", g.fg);
        assert_eq!(p.foreground(n.fg, false), Color32::BLACK, "沒設顏色的字用預設前景");
    }
    #[test]
    fn low_contrast_text_is_pushed_toward_readable_and_good_text_is_untouched() {
        let dark_bg = Color32::from_rgb(0x1e, 0x1e, 0x1e);
        let light_bg = Color32::from_rgb(0xfa, 0xfa, 0xfa);
        let dark = Palette::new(Color32::WHITE, dark_bg, crate::scheme::default_for(true).terminal_colors(), MIN_CONTRAST);
        let light = Palette::new(Color32::BLACK, light_bg, crate::scheme::default_for(false).terminal_colors(), MIN_CONTRAST);
        // 對比夠的原封不動
        let fine = Color32::from_rgb(0xdd, 0xdd, 0xdd);
        assert_eq!(dark.readable(fine, dark_bg), fine);
        // 深色底上的深灰字：變亮到夠為止
        let dim = Color32::from_rgb(0x30, 0x30, 0x30);
        let fixed = dark.readable(dim, dark_bg);
        assert!(contrast(fixed, dark_bg) >= MIN_CONTRAST && luminance(fixed) > luminance(dim), "{fixed:?}");
        // 淺色底上的淺黃字：變暗，而且盡量保留色相（不是直接變黑）
        let yellow = Color32::from_rgb(0xf5, 0xf5, 0x43);
        let fixed = light.readable(yellow, light_bg);
        assert!(contrast(fixed, light_bg) >= MIN_CONTRAST && luminance(fixed) < luminance(yellow), "{fixed:?}");
        assert!(fixed.r() > fixed.b() && fixed.g() > fixed.b(), "黃色調暗之後還是偏黃 {fixed:?}");
        // 前景和底色一模一樣：往對比的方向拉到剛好夠（淡灰），不會是整片消失
        assert!(contrast(dark.readable(dark_bg, dark_bg), dark_bg) >= MIN_CONTRAST);
        assert!(contrast(light.readable(light_bg, light_bg), light_bg) >= MIN_CONTRAST);
    }

    #[test]
    fn graphic_glyphs_are_exempt_from_the_contrast_guard() {
        for c in ['█', '▀', '─', '│', '┌', '⣿', '\u{e0b0}', '\u{e0b2}'] {
            assert!(is_graphic_glyph(c), "{c}");
        }
        for c in ['a', '中', '>', ' ', '→'] {
            assert!(!is_graphic_glyph(c), "{c}");
        }
    }

    #[test]
    fn terminal_numbers_reuse_the_lowest_free_one_and_focus_cycles_through_editor_and_terminals() {
        assert_eq!(lowest_free_number([]), 1);
        assert_eq!(lowest_free_number([1, 2, 3]), 4);
        assert_eq!(lowest_free_number([1, 3]), 2, "關掉 2 之後，下一個又是 2");
        assert_eq!(lowest_free_number([2]), 1);
        let order = [7, 9, 4];
        assert_eq!(next_in_cycle(&order, None), Some(7), "從編輯區到第一個終端機");
        assert_eq!(next_in_cycle(&order, Some(7)), Some(9));
        assert_eq!(next_in_cycle(&order, Some(9)), Some(4));
        assert_eq!(next_in_cycle(&order, Some(4)), None, "最後一個之後回到編輯區");
        assert_eq!(next_in_cycle::<u32>(&[], None), None);
        assert_eq!(next_in_cycle(&order, Some(99)), None, "不在清單裡的當作繞完");
    }

    /// 在寬 `cols`、高 `lines` 的終端機裡「顯示」一段輸出（`\r\n` 換行，太長的會折行）。
    fn term_with(cols: usize, lines: usize, output: &str) -> alacritty_terminal::Term<alacritty_terminal::event::VoidListener> {
        use alacritty_terminal::event::VoidListener;
        use alacritty_terminal::term::Config;
        use alacritty_terminal::vte::ansi::{Processor, StdSyncHandler};
        let size = GridSize { cols, lines };
        let mut term = alacritty_terminal::Term::new(Config::default(), &size, VoidListener);
        let mut parser: Processor<StdSyncHandler> = Processor::new();
        parser.advance(&mut term, output.as_bytes());
        term
    }

    #[test]
    fn search_finds_matches_in_the_scrollback_and_across_wrapped_rows() {
        // 10 欄寬：第一行折成兩列，歷史（往上捲出去）裡也有符合的字
        let mut out = String::new();
        for i in 0..8 {
            out.push_str(&format!("line {i} Foo\r\n"));
        }
        out.push_str("hello wor");
        out.push_str("ld HELLO\r\n");
        let term = term_with(10, 5, &out);
        assert!(term.grid().history_size() > 0, "有些輸出已經捲進歷史");
        let found = scan_matches(term.grid(), "foo");
        assert_eq!(found.len(), 8, "歷史裡的也找得到，不分大小寫");
        let world = scan_matches(term.grid(), "world");
        assert_eq!(world.len(), 1);
        assert_eq!(world[0].len(), 2, "跨越折行：兩列各一段");
        assert_eq!(scan_matches(term.grid(), "hello").len(), 2);
        assert!(scan_matches(term.grid(), "").is_empty());
        assert!(scan_matches(term.grid(), "nothing like this").is_empty());
    }

    #[test]
    fn a_url_wrapped_over_two_rows_is_found_from_either_row() {
        let term = term_with(20, 4, "go https://example.com/a/very/long/path ok\r\n");
        let grid = term.grid();
        let top = grid.topmost_line().0;
        for line in [top, top + 1] {
            let glyphs = logical_containing(grid, line);
            let at = glyphs.iter().position(|g| g.line == line && g.c != ' ').expect("這一列有字");
            let url = terminal_text::url_at(&glyphs, at).map(|x| x.2);
            if line == top {
                // 第一列最前面的 "go" 不是網址的一部分，找網址中間的字
                let mid = glyphs.iter().position(|g| g.c == 'x').expect("example 的 x");
                assert_eq!(terminal_text::url_at(&glyphs, mid).map(|x| x.2).as_deref(), Some("https://example.com/a/very/long/path"));
            } else {
                assert_eq!(url.as_deref(), Some("https://example.com/a/very/long/path"), "第二列也屬於同一個網址");
            }
        }
    }

    #[test]
    fn search_steps_wrap_around_in_both_directions_and_ask_for_a_jump() {
        let seg = |l: i32| vec![Segment { line: l, start_col: 0, end_col: 1 }];
        let mut s = Search { matches: vec![seg(0), seg(1), seg(2)], ..Search::default() };
        s.step(true); // 還沒選：往較早的從最後一筆開始
        assert_eq!((s.current, s.jump), (Some(2), true));
        s.jump = false;
        s.step(true);
        s.step(true);
        assert_eq!(s.current, Some(0));
        s.step(true);
        assert_eq!(s.current, Some(2), "最早的再往前，繞到最新的");
        s.step(false);
        assert_eq!(s.current, Some(0), "最新的再往後，繞回最早的");
        let mut empty = Search::default();
        empty.step(true);
        assert_eq!((empty.current, empty.jump), (None, false), "沒有符合項目就什麼都不做");
    }

    #[test]
    fn light_and_dark_palettes_differ() {
        let dark = Palette::new(Color32::WHITE, Color32::BLACK, crate::scheme::default_for(true).terminal_colors(), MIN_CONTRAST);
        let light = Palette::new(Color32::BLACK, Color32::WHITE, crate::scheme::default_for(false).terminal_colors(), MIN_CONTRAST);
        assert_ne!(dark.ansi(7), light.ansi(7));
    }
}
