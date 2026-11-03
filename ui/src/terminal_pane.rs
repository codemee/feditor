//! 終端機窗格：App 這一側的管理（開關、焦點、輸入）與畫面。shell 與跳脫序列的處理在 `terminal.rs`。
//!
//! 終端機窗格和預覽窗格一樣不放分頁（[`App::is_preview`] 對兩者都回傳 `true`），可以拖到版面的任何位置。
//! 鍵盤焦點用 egui 的焦點機制：點一下終端機畫面，它的元件拿到焦點，這一幀開頭把所有按鍵、文字、
//! 複製貼上事件從輸入裡取走、送給 shell，編輯區與全域快捷鍵就不會收到。

use alacritty_terminal::grid::Scroll;
use alacritty_terminal::index::{Column, Line, Point, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::term::TermMode;
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::vte::ansi::{Color, NamedColor};
use eframe::egui::{self, Color32};
use feditor_core::i18n::t;
use feditor_core::tf;

use crate::panes::{PaneId, Side as PaneSide};
use crate::terminal::{self, GridSize, Terminal};
use crate::{App, LogKind};

/// 開關終端機。不用 Ctrl+C／V／D 等 shell 要用的鍵。
pub const TOGGLE_TERMINAL: egui::KeyboardShortcut = egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::Backtick);

pub struct TerminalPane {
    /// 還沒開 shell（第一次畫的時候才知道有幾格）。開失敗時是錯誤訊息。
    term: Option<Terminal>,
    error: Option<String>,
    /// shell 的起始資料夾。
    dir: Option<std::path::PathBuf>,
    /// 下一個畫面要把鍵盤焦點給它。
    want_focus: bool,
    /// 滑鼠滾輪累積的捲動量（行）。
    scroll_acc: f32,
    /// 輸入法正在組的字（還沒確定，不送給 shell；畫在游標的位置）。
    preedit: String,
}

impl TerminalPane {
    pub fn new(dir: Option<std::path::PathBuf>) -> Self {
        Self { term: None, error: None, dir, want_focus: true, scroll_acc: 0.0, preedit: String::new() }
    }

    /// 下一個畫面把鍵盤焦點給這個終端機。
    pub fn request_focus(&mut self) {
        self.want_focus = true;
    }

    fn title(&self) -> String {
        self.term.as_ref().and_then(Terminal::title).filter(|s| !s.trim().is_empty()).unwrap_or_else(|| t("終端機").to_owned())
    }
}

pub(crate) fn widget_id(id: PaneId) -> egui::Id {
    egui::Id::unique(("terminal_widget", id))
}

impl App {
    /// 終端機窗格的編號（目前只開一個）。
    pub(crate) fn terminal_id(&self) -> Option<PaneId> {
        self.terminals.keys().min().copied()
    }

    /// 在有焦點的窗格下方開一個終端機。窗格太小放不下就提示。
    fn open_terminal(&mut self) {
        let focus = self.layout.focus;
        let rect = self.pane_rects.iter().find(|(id, _)| *id == focus).map(|(_, r)| *r);
        if rect.is_some_and(|r| !crate::panes::can_split(r, PaneSide::Bottom)) {
            self.push(LogKind::Warn, t("窗格太小，放不下終端機"));
            return;
        }
        let id = self.layout.alloc_id();
        if self.layout.root.split(focus, PaneSide::Bottom, id) {
            let dir = self.doc.path.as_ref().and_then(|p| p.parent().map(|d| d.to_path_buf()));
            self.terminals.insert(id, TerminalPane::new(dir.or_else(|| std::env::current_dir().ok())));
        }
    }

    pub(crate) fn close_terminal(&mut self, id: PaneId) {
        if self.terminals.remove(&id).is_some() {
            self.layout.root.remove(id);
            self.pending_editor_focus = true;
        }
    }

    /// 工具列的終端機鈕：已經有就關掉（shell 跟著結束），沒有就開一個。
    pub(crate) fn toggle_terminal(&mut self) {
        match self.terminal_id() {
            Some(id) => self.close_terminal(id),
            None => self.open_terminal(),
        }
    }

    /// 快捷鍵：沒有終端機就開；有的話在編輯區與終端機之間切換焦點。
    pub(crate) fn focus_terminal_shortcut(&mut self, ctx: &egui::Context) {
        let Some(id) = self.terminal_id() else {
            self.open_terminal();
            return;
        };
        if ctx.memory(|m| m.has_focus(widget_id(id))) {
            self.pending_editor_focus = true;
        } else if let Some(p) = self.terminals.get_mut(&id) {
            p.want_focus = true;
        }
    }

    /// shell 結束了（使用者輸入 exit）：關掉窗格。
    pub(crate) fn reap_terminals(&mut self) {
        let done: Vec<PaneId> = self.terminals.iter().filter(|(_, p)| p.term.as_ref().is_some_and(Terminal::exited)).map(|(id, _)| *id).collect();
        for id in done {
            self.close_terminal(id);
        }
    }

    /// 終端機有鍵盤焦點時，把這一幀的按鍵等輸入事件從 egui 取走，送給 shell。要在其他快捷鍵處理之前呼叫。
    pub(crate) fn terminal_input(&mut self, ctx: &egui::Context) {
        let Some(id) = self.terminals.keys().copied().find(|id| ctx.memory(|m| m.has_focus(widget_id(*id)))) else {
            return;
        };
        let Some(term) = self.terminals.get(&id).and_then(|p| p.term.as_ref()) else { return };
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
        if let (Some(p), Some(pane)) = (preedit, self.terminals.get_mut(&id)) {
            pane.preedit = p;
        }
    }

    /// 終端機窗格：標題列（可拖曳、關閉鈕）加上終端機畫面。
    pub(crate) fn show_terminal_pane(&mut self, ui: &mut egui::Ui, id: PaneId, rect: egui::Rect) -> Option<crate::Action> {
        let ctx = ui.ctx().clone();
        let title = self.terminals.get(&id)?.title();
        let mut action = None;
        let framed = egui::Frame::new().inner_margin(egui::Margin { left: 12, right: 8, top: 5, bottom: 0 }).show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.set_min_height(crate::pane_ops::TAB_HEIGHT);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if crate::icons::small_button(ui, crate::Icon::ClosePane, t("關閉終端機"), true).clicked() {
                        action = Some(crate::Action::CloseTerminal(id));
                    }
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        let r = ui.add(egui::Label::new(egui::RichText::new(&title).weak()).truncate().selectable(false).sense(egui::Sense::click_and_drag()));
                        let r = r.on_hover_cursor(egui::CursorIcon::Grab).on_hover_text(t("拖曳標題，可以把終端機移到別的窗格旁邊"));
                        if r.drag_started() {
                            action = Some(crate::Action::BeginPreviewDrag(id));
                        }
                    });
                });
            });
        });
        let bar_h = framed.response.rect.height();
        let inset = crate::pane_ops::CARD_INSET;
        let body = egui::Rect::from_min_max(egui::pos2(rect.min.x + inset, rect.min.y + bar_h), egui::pos2(rect.max.x - inset, rect.max.y - inset));
        let dark = ui.visuals().dark_mode;
        let bg = ui.visuals().extreme_bg_color;
        ui.painter().rect_filled(body, egui::CornerRadius::same(4), bg);
        let content = body.shrink(6.0);
        ui.set_clip_rect(content.intersect(ui.clip_rect()));

        // 一格的大小：用編輯區的等寬字
        let font = egui::TextStyle::Monospace.resolve(&ctx.global_style());
        let (cw, ch) = ctx.fonts_mut(|f| (f.glyph_width(&font, '0').max(1.0), f.row_height(&font).max(1.0)));
        let size = GridSize { cols: ((content.width() / cw).floor() as usize).max(2), lines: ((content.height() / ch).floor() as usize).max(1) };

        let pane = self.terminals.get_mut(&id)?;
        if pane.term.is_none() && pane.error.is_none() {
            match Terminal::spawn(&ctx, size, (cw as u16, ch as u16), pane.dir.clone()) {
                Ok(t) => pane.term = Some(t),
                Err(e) => pane.error = Some(tf!("無法開啟 shell：{e}", e = e)),
            }
        }
        if let Some(e) = &pane.error {
            ui.scope_builder(egui::UiBuilder::new().max_rect(content), |ui| {
                ui.label(egui::RichText::new(e).color(ui.visuals().error_fg_color));
            });
            return action;
        }
        let preedit = pane.preedit.clone();
        let Some(term) = pane.term.as_mut() else { return action };
        term.resize(size, (cw as u16, ch as u16));

        // 互動：點一下取得焦點、拖曳選取、滾輪捲動
        let wid = widget_id(id);
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
        let display_offset = term.term.lock().grid().display_offset() as i32;
        let point_at = |pos: egui::Pos2| -> (Point, Side) {
            let x = (pos.x - content.min.x) / cw;
            let col = (x.floor().max(0.0) as usize).min(size.cols - 1);
            let row = (((pos.y - content.min.y) / ch).floor().max(0.0) as i32).min(size.lines as i32 - 1);
            let side = if x.fract() < 0.5 { Side::Left } else { Side::Right };
            (Point::new(Line(row - display_offset), Column(col)), side)
        };
        if let Some(pos) = resp.interact_pointer_pos() {
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
        if resp.clicked() {
            term.term.lock().selection = None;
        }
        if resp.hovered() {
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

        // 畫格子
        let painter = ui.painter_at(content);
        let palette = Palette::new(ui.visuals().text_color(), ui.visuals().extreme_bg_color, crate::theme::terminal_colors(&ctx, dark));
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
            let at = cell_rect(cur.point.column.0.min(size.cols - 1), cur_row.clamp(0, size.lines as i32 - 1), 1.0);
            if !preedit.is_empty() {
                let galley = painter.layout_no_wrap(preedit.clone(), font.clone(), ui.visuals().text_color());
                let box_ = egui::Rect::from_min_size(at.min, egui::vec2(galley.size().x.max(cw), ch));
                painter.rect_filled(box_, 0.0, ui.visuals().extreme_bg_color);
                painter.galley(at.min, galley.clone(), ui.visuals().text_color());
                let y = box_.max.y - 1.0;
                painter.line_segment([egui::pos2(box_.min.x, y), egui::pos2(box_.min.x + galley.size().x, y)], egui::Stroke::new(1.0, ui.visuals().text_color()));
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
        action
    }
}

/// 文字和底色的最低對比度（WCAG 比值）。程式輸出的顏色低於它就會被調整到看得清楚為止。
/// 3:1 是 WCAG 對大字與圖形的下限，比 4.5 寬鬆，不會把程式刻意用的淡色（例如註解的灰）全部拉黑。
const MIN_CONTRAST: f32 = 3.0;

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
}

impl Palette {
    fn new(default_fg: Color32, default_bg: Color32, ansi: [Color32; 16]) -> Self {
        Self { default_fg, default_bg, ansi }
    }

    fn ansi(&self, i: usize) -> Color32 {
        self.ansi[i]
    }

    /// 最低對比度保護：前景和它所在的底色對比不到 [`MIN_CONTRAST`] 時，把前景往能拉開差距的方向（底色亮就往黑、
    /// 底色暗就往白）一步步調整，直到夠為止；調不到（例如前景和底色一樣）就直接用黑或白。
    /// 對比夠的顏色原封不動，所以多數輸出不受影響。
    fn readable(&self, fg: Color32, bg: Color32) -> Color32 {
        if contrast(fg, bg) >= MIN_CONTRAST {
            return fg;
        }
        let target = if luminance(bg) > 0.5 { Color32::BLACK } else { Color32::WHITE };
        for step in 1..=10 {
            let t = step as f32 / 10.0;
            let mix = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round() as u8;
            let c = Color32::from_rgb(mix(fg.r(), target.r()), mix(fg.g(), target.g()), mix(fg.b(), target.b()));
            if contrast(c, bg) >= MIN_CONTRAST {
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

    #[test]
    fn palette_maps_named_indexed_and_true_colors() {
        let p = Palette::new(Color32::WHITE, Color32::BLACK, crate::scheme::default_for(true).terminal_colors());
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
        let p = Palette::new(Color32::BLACK, Color32::WHITE, crate::scheme::default_for(false).terminal_colors());
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
        let dark = Palette::new(Color32::WHITE, dark_bg, crate::scheme::default_for(true).terminal_colors());
        let light = Palette::new(Color32::BLACK, light_bg, crate::scheme::default_for(false).terminal_colors());
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
    fn light_and_dark_palettes_differ() {
        let dark = Palette::new(Color32::WHITE, Color32::BLACK, crate::scheme::default_for(true).terminal_colors());
        let light = Palette::new(Color32::BLACK, Color32::WHITE, crate::scheme::default_for(false).terminal_colors());
        assert_ne!(dark.ansi(7), light.ansi(7));
    }
}
