//! 編輯區的畫面與輸入處理（egui）。
//!
//! 只排版、繪製「看得到的那幾列」，所以不管檔案多大，每一幀的成本都只和視窗高度有關。
//! 游標與選取、文字修改都交給 [`EditorState`]（見 `model.rs`）；這裡只負責：
//! 把 egui 的事件翻譯成對模型的操作、把模型的狀態畫出來、以及告訴輸入法候選窗要出現在哪裡。
//!
//! 「列」是畫面上的一橫排，「行」是文字裡以換行字元分隔的一段。沒有自動換行時列與行一一對應；
//! 有自動換行時，一行可能佔好幾列（對應表見 [`wrap`]）。

use super::model::{EditorState, Selection};
use super::wrap::{self, WrapMap};
use crate::settings::ViewOptions;
use crate::syntax::{self, Kind, Language, StateCache};
use eframe::egui::{
    self, Align, Align2, Color32, CursorIcon, Event, EventFilter, FontId, IMEPurpose, Id, ImeEvent,
    Key, Modifiers, Painter, Pos2, Rect, Response, Sense, Stroke, TextStyle, Ui, Vec2, pos2, vec2,
};
use feditor_core::{Buffer, motion};
use std::cell::RefCell;
use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

/// 文字區左邊與行號區之間的留白。
const PAD_X: f32 = 8.0;

/// 畫面排版用的尺寸。
#[allow(dead_code)] // 目前只有測試讀它們
#[derive(Clone, Copy, Debug)]
pub struct Metrics {
    /// 一列的高度。
    pub row_h: f32,
    /// 一個半形字元的寬度（等寬字型）。
    pub char_w: f32,
    /// 行號區的寬度（不顯示行號時為 0）。
    pub gutter_w: f32,
}

pub struct EditorView<'a> {
    pub id: Id,
    pub buffer: &'a mut Buffer,
    pub state: &'a mut EditorState,
    /// 搜尋結果（字元範圍），畫成高亮。
    pub highlight: Option<Range<usize>>,
    /// 這個畫面要把 `highlight` 捲到可見處。
    pub reveal: bool,
    pub options: ViewOptions,
    /// 要用哪種語言上色；`None` 表示不上色。
    pub language: Option<&'static Language>,
}

#[allow(dead_code)] // 目前只有測試讀它們
pub struct EditorOutput {
    pub response: Response,
    pub focused: bool,
    /// 使用者操作了編輯區（點擊、移動游標、輸入）。
    pub touched: bool,
    pub metrics: Metrics,
    /// 第 0 列頂端在螢幕上的位置（會隨捲動改變）。
    pub content_origin: Pos2,
    /// 實際使用的折行寬度（欄數）；沒有自動換行時為 `None`。
    pub wrap_cols: Option<usize>,
}

impl Metrics {
    /// 文字區（第 0 欄）左緣相對於內容區左緣的位移。
    pub fn text_left_offset(&self) -> f32 {
        self.gutter_w + PAD_X
    }
}

/// 一列排版好的結果。
///
/// 等寬字型只保證拉丁字母等寬；中文字常常來自另一個字型，字寬不一定剛好是半形的兩倍，
/// 如果整行交給 egui 一次排，中文字就會和上下行的英文對不齊。所以這裡**自己依欄位排版**：
/// 每個字元固定佔「欄寬 × 半形字寬」，半形英數字整串一起排（它們本來就等寬），
/// 其他字元（中文、全形、emoji…）一個一個排，再置中放進自己的格子裡。
struct LineLayout {
    /// 畫面列的編號（從文件最上面算起）。
    row: usize,
    line: usize,
    /// 這一列第一個字元在整行（已含組字文字）裡的位置。
    start: usize,
    /// 這一列有幾個字元。
    len: usize,
    /// 是這一行的最後一列：換行字元的標記與「選到換行」的延伸畫在這裡。
    last: bool,
    /// 整行排版用的文字（組字中的那一行，已經把組字文字插在游標處），用來判斷排版是否過期。
    full: Arc<str>,
    /// 這一列的文字。還沒展開 Tab。
    text: String,
    /// 半形字元的寬度。
    char_w: f32,
    /// 排好的片段：相對於文字區左緣的 x 座標、垂直位移、排版結果。
    segments: Vec<Segment>,
}

struct Segment {
    x: f32,
    y: f32,
    galley: Arc<egui::Galley>,
}

impl LineLayout {
    /// 整行裡第 `k` 個字元左緣的 x 座標，相對於文字區左緣。不在這一列裡的位置會夾到列首或列尾。
    fn x_at(&self, k: usize) -> f32 {
        let local = k.saturating_sub(self.start).min(self.len);
        motion::display_col(&self.text, local) as f32 * self.char_w
    }

    /// 文字區裡的 x 座標（相對於文字區左緣）最接近哪個字元邊界（整行裡的位置）。
    fn index_at_x(&self, x: f32) -> usize {
        self.start + motion::char_index_at_col(&self.text, x / self.char_w).min(self.len)
    }

    /// 在換行處的列尾（不是這一行的最後一列）：這個位置同時也是下一列的開頭。
    fn is_wrap_end(&self, idx: usize) -> bool {
        !self.last && idx == self.start + self.len
    }
}

/// 把組字文字插在第 `col` 個字元之前。
fn compose(line: &str, col: usize, preedit: &str) -> String {
    let byte = line.char_indices().nth(col).map_or(line.len(), |(b, _)| b);
    let mut out = String::with_capacity(line.len() + preedit.len());
    out.push_str(&line[..byte]);
    out.push_str(preedit);
    out.push_str(&line[byte..]);
    out
}

/// 語法上色用的語言與跨行狀態快取。
#[derive(Clone)]
struct SyntaxCtx {
    lang: &'static Language,
    cache: Rc<RefCell<StateCache>>,
}

/// 排版時需要的字型與尺寸。
#[derive(Clone)]
struct TextSpec {
    font_id: FontId,
    color: Color32,
    /// 各種語法元素的顏色，依 [`Kind`] 的順序。
    kind_colors: [Color32; 7],
    syntax: Option<SyntaxCtx>,
    char_w: f32,
    row_h: f32,
}

impl TextSpec {
    fn color_of(&self, kind: Kind) -> Color32 {
        match kind {
            Kind::Plain => self.color,
            k => self.kind_colors[k as usize],
        }
    }
}

/// 畫面列與文字行的對應：有自動換行時查累計表，否則一列就是一行。
struct Rows<'m> {
    map: Option<&'m WrapMap>,
    cols: Option<usize>,
    lines: usize,
}

impl Rows<'_> {
    fn total(&self) -> usize {
        self.map.map_or(self.lines, WrapMap::total_rows)
    }

    /// 畫面列屬於哪一行、是那一行的第幾列。
    fn locate(&self, row: usize) -> (usize, usize) {
        self.map.map_or((row, 0), |m| m.locate(row))
    }

    fn first_row(&self, line: usize) -> usize {
        self.map.map_or(line, |m| m.first_row(line))
    }

    /// 這一行最後一列的下一列（不含）。
    fn end_row(&self, line: usize) -> usize {
        self.first_row(line + 1)
    }
}

/// 組字中的那一行要把組字文字插進去；其他行就是原文。
fn composed_text(buffer: &Buffer, state: &EditorState, line: usize) -> String {
    let mut text = buffer.line_text(line);
    if let Some(preedit) = &state.preedit {
        let (caret_line, col) = caret_line_col(buffer, state);
        if caret_line == line {
            text = compose(&text, col, &preedit.text);
        }
    }
    text
}

/// 排版第 `line` 行裡落在畫面列 `want` 範圍內的那幾列。`first_row` 是這一行第一列的編號。
fn layout_rows(
    ui: &Ui,
    spec: &TextSpec,
    buffer: &Buffer,
    state: &EditorState,
    line: usize,
    first_row: usize,
    cols: Option<usize>,
    want: Range<usize>,
) -> Vec<LineLayout> {
    let text = composed_text(buffer, state, line);
    let full: Arc<str> = Arc::from(text.as_str());
    // 語法上色：每個字元的種類（組字中的那一行用插入組字文字之後的內容分詞）
    let kinds: Option<Vec<Kind>> = spec.syntax.as_ref().map(|s| {
        let start = s.cache.borrow_mut().state_at(s.lang, buffer, line);
        syntax::kinds_of(s.lang, &text, start)
    });
    let (ranges, chars): (Vec<Range<usize>>, Option<Vec<char>>) = match cols {
        Some(c) => (motion::wrap_rows(&text, c), Some(text.chars().collect())),
        None => (vec![0..text.chars().count()], None),
    };
    let n = ranges.len();
    let mut out = Vec::new();
    for (i, r) in ranges.into_iter().enumerate() {
        let row = first_row + i;
        if !want.contains(&row) {
            continue;
        }
        let row_text = match &chars {
            Some(chars) => chars[r.clone()].iter().collect(),
            None => text.clone(),
        };
        let row_kinds = kinds.as_ref().map(|k| &k[r.start.min(k.len())..r.end.min(k.len())]);
        let segments = layout_segments(ui, spec, &row_text, row_kinds);
        out.push(LineLayout {
            row,
            line,
            start: r.start,
            len: r.len(),
            last: i + 1 == n,
            full: full.clone(),
            text: row_text,
            char_w: spec.char_w,
            segments,
        });
    }
    out
}

/// 依欄位把一列文字切成片段並排版（見 [`LineLayout`]）。
fn layout_segments(ui: &Ui, spec: &TextSpec, text: &str, kinds: Option<&[Kind]>) -> Vec<Segment> {
    let painter = ui.painter();
    let mut out = Vec::new();
    // 目前累積中的片段：起始欄、文字、佔幾欄、是否為半形英數字串
    let mut run = String::new();
    let mut run_col = 0usize;
    let mut run_cells = 0usize;
    let mut run_is_ascii = true;
    let mut run_kind = Kind::Plain;
    let mut col = 0usize;

    let flush = |out: &mut Vec<Segment>,
                 run: &mut String,
                 run_col: usize,
                 cells: usize,
                 ascii: bool,
                 kind: Kind| {
        if run.is_empty() {
            return;
        }
        let galley = painter.layout_no_wrap(std::mem::take(run), spec.font_id.clone(), spec.color_of(kind));
        let cell_w = cells as f32 * spec.char_w;
        // 水平：全形字在自己的格子裡置中；半形字串從格子的左緣開始排（它們本來就等寬，不能位移）
        let pad = if ascii { 0.0 } else { ((cell_w - galley.size().x) / 2.0).max(0.0) };
        let x = run_col as f32 * spec.char_w + pad;
        // 垂直：在行高裡置中。不同字型的行高不同，各自置中才會落在同一條水平線上
        let y = (spec.row_h - galley.size().y) / 2.0;
        out.push(Segment { x, y, galley });
    };

    for (idx, c) in text.chars().enumerate() {
        let kind = kinds.and_then(|k| k.get(idx)).copied().unwrap_or(Kind::Plain);
        if c == '\t' {
            flush(&mut out, &mut run, run_col, run_cells, run_is_ascii, run_kind);
            col = (col / motion::tab_width() + 1) * motion::tab_width();
            continue;
        }
        let width = motion::char_width(c);
        if width == 0 {
            // 組合字元：跟著前一個字元一起排
            if !run.is_empty() {
                run.push(c);
            }
            continue;
        }
        let ascii = c.is_ascii();
        if ascii && run_is_ascii && !run.is_empty() && kind == run_kind {
            run.push(c);
            run_cells += 1;
            col += 1;
            continue;
        }
        flush(&mut out, &mut run, run_col, run_cells, run_is_ascii, run_kind);
        run.push(c);
        run_col = col;
        run_cells = width;
        run_is_ascii = ascii;
        run_kind = kind;
        col += width;
    }
    flush(&mut out, &mut run, run_col, run_cells, run_is_ascii, run_kind);
    out
}

fn caret_line_col(buffer: &Buffer, state: &EditorState) -> (usize, usize) {
    let caret = state.sel.head.min(buffer.len_chars());
    let line = buffer.line_of_char(caret);
    (line, caret - buffer.line_start(line))
}

/// 取得（必要時現場排版）畫面列 `row` 的排版，回傳它在 `layouts` 裡的位置。
/// 看得到的列已經排好；游標或搜尋結果在畫面外時才需要現場排。
fn ensure_row(
    layouts: &mut Vec<LineLayout>,
    rows: &Rows,
    ui: &Ui,
    spec: &TextSpec,
    buffer: &Buffer,
    state: &EditorState,
    row: usize,
) -> usize {
    if let Some(i) = layouts.iter().position(|l| l.row == row) {
        return i;
    }
    let (line, _) = rows.locate(row);
    let mut made = layout_rows(
        ui,
        spec,
        buffer,
        state,
        line,
        rows.first_row(line),
        rows.cols,
        row..row + 1,
    );
    layouts.append(&mut made);
    layouts.len() - 1
}

/// 游標（整行裡的位置 `k`，已含組字文字）落在這一行的第幾列。
fn row_of_caret(state: &EditorState, buffer: &Buffer, line: usize, col: usize, k: usize) -> usize {
    if state.wrap_cols.is_none() {
        return 0;
    }
    if state.preedit.is_some() {
        let ranges = motion::wrap_rows(&composed_text(buffer, state, line), state.wrap_cols.unwrap());
        return ranges.iter().position(|r| k < r.end).unwrap_or(ranges.len() - 1);
    }
    let rows = state.rows_for(buffer, line);
    state.row_index(buffer, &rows, line, col)
}

/// 空白字元的標記：空格畫小點、Tab 畫箭頭、全形空白畫小方框，行尾畫換行符號。
/// 安全界線的水平位置（換色區域的左緣）：`column` 欄（至少 1）文字的右緣，文字區的左邊是 `text_left`。
fn ruler_x(column: u16, text_left: f32, char_w: f32) -> Option<f32> {
    (column > 0).then(|| text_left + column as f32 * char_w)
}

fn paint_whitespace(
    painter: &Painter,
    layout: &LineLayout,
    text_left: f32,
    y: f32,
    row_h: f32,
    color: Color32,
    show_eol: bool,
) {
    let cw = layout.char_w;
    let mid = y + row_h / 2.0;
    let stroke = Stroke::new(1.0, color);
    let mut col = 0usize;
    for c in layout.text.chars() {
        let x0 = text_left + col as f32 * cw;
        match c {
            ' ' => {
                painter.circle_filled(pos2(x0 + cw / 2.0, mid), (cw * 0.09).max(1.0), color);
                col += 1;
            }
            '\u{3000}' => {
                let r = Rect::from_center_size(pos2(x0 + cw, mid), vec2(cw * 1.2, row_h * 0.45));
                painter.rect_stroke(r, 0.0, stroke, egui::StrokeKind::Inside);
                col += 2;
            }
            '\t' => {
                let next = (col / motion::tab_width() + 1) * motion::tab_width();
                let x1 = text_left + next as f32 * cw;
                let (a, b) = (x0 + cw * 0.2, x1 - cw * 0.2);
                painter.line_segment([pos2(a, mid), pos2(b, mid)], stroke);
                let h = row_h * 0.16;
                painter.line_segment([pos2(b - h, mid - h), pos2(b, mid)], stroke);
                painter.line_segment([pos2(b - h, mid + h), pos2(b, mid)], stroke);
                col = next;
            }
            _ => col += motion::char_width(c),
        }
    }
    if show_eol {
        // ↵：先往下再往左，箭頭指向左邊
        let x = text_left + col as f32 * cw + cw * 0.7;
        let (top, bottom) = (mid - row_h * 0.2, mid + row_h * 0.1);
        let left = x - cw * 0.55;
        painter.line_segment([pos2(x, top), pos2(x, bottom)], stroke);
        painter.line_segment([pos2(x, bottom), pos2(left, bottom)], stroke);
        let h = row_h * 0.12;
        painter.line_segment([pos2(left + h, bottom - h), pos2(left, bottom)], stroke);
        painter.line_segment([pos2(left + h, bottom + h), pos2(left, bottom)], stroke);
    }
}

impl EditorView<'_> {
    pub fn show(self, ui: &mut Ui) -> EditorOutput {
        let EditorView {
            id,
            buffer,
            state,
            highlight,
            reveal,
            options,
            language,
        } = self;

        let font_id = TextStyle::Monospace.resolve(ui.style());
        let (row_h, char_w) = ui
            .ctx()
            .fonts_mut(|f| (f.row_height(&font_id), f.glyph_width(&font_id, '0')));
        // 行高取整到實際像素，列與列的間距才會一致（小數行高會讓文字被各自對齊到不同的像素）
        let ppp = ui.ctx().pixels_per_point();
        let row_h = (row_h * ppp).ceil() / ppp;
        // 行號區只留剛好放下最大行號的寬度，左右各留 3/4 個字元寬
        let digits = buffer.line_count().to_string().len();
        let metrics = Metrics {
            row_h,
            char_w,
            gutter_w: if options.show_line_numbers {
                (digits as f32 + 1.5) * char_w
            } else {
                0.0
            },
        };
        state.clamp(buffer);
        // Tab 欄寬變了（設定改了、或這份文件的檔案類型不同）：橫向捲動的範圍要重算（它只會往大長）
        if state.tab_seen != motion::tab_width() {
            state.tab_seen = motion::tab_width();
            state.max_cols = 0;
        }

        // 自動換行：折行寬度由目前的可用寬度決定。太大的文件不換行（見 wrap::MAX_LINES）
        let wrap_cols = (options.word_wrap && buffer.line_count() <= wrap::MAX_LINES).then(|| {
            let bar = ui.spacing().scroll.allocated_width();
            let w = ui.available_width() - bar - metrics.text_left_offset() - char_w;
            ((w / char_w).floor().max(4.0)) as usize
        });
        state.wrap_cols = wrap_cols;

        // 組字中點到視窗裡其他「不能輸入」的地方（空白處、按鈕等），egui 會把焦點收走，輸入法的
        // 組字就被中斷了。這時沒有其他輸入框接手焦點，就把焦點拿回來，組字才能繼續。
        // 點到別的輸入框時，焦點在它手上，我們不搶；點到編輯區自己則由下面的滑鼠處理負責中斷。
        if state.preedit.is_some()
            && ui.memory(|m| m.focused().is_none())
            && ui.input(|i| i.focused)
        {
            ui.memory_mut(|m| m.request_focus(id));
        }
        let mut focused = ui.memory(|m| m.has_focus(id));
        // 組字中，焦點被別的輸入框取走了：把組字的內容直接輸入到編輯區，而不是讓它留在半空中
        if state.preedit.is_some() && !focused && ui.input(|i| i.focused) {
            if let Some(text) = state.preedit.as_ref().map(|p| p.text.clone()) {
                state.ime_commit(buffer, &text);
                state.swallow_ime = Some((text, 3));
                focused = false;
            }
        }
        let mut touched = false;
        if focused {
            ui.memory_mut(|m| {
                m.set_focus_lock_filter(
                    id,
                    EventFilter {
                        tab: true,
                        horizontal_arrows: true,
                        vertical_arrows: true,
                        // 有多個游標時，Esc 用來回到單一游標，不能讓它把焦點收走
                        escape: state.has_multi(),
                    },
                );
            });
            touched = process_events(ui, buffer, state);
        }

        // 列與行的對應表（文字在上面可能被改了，所以放在處理完事件之後）
        let mut base_map = std::mem::take(&mut state.wrap_map);
        if let Some(cols) = wrap_cols {
            base_map.ensure(buffer, cols);
        }
        // 組字文字讓游標所在那一行變長時，那一行的列數可能增加；對應表要跟著調整
        let adjusted = match (wrap_cols, &state.preedit) {
            (Some(cols), Some(_)) => {
                let (caret_line, _) = caret_line_col(buffer, state);
                let composed = motion::wrap_rows(&composed_text(buffer, state, caret_line), cols).len();
                let plain = base_map.first_row(caret_line + 1) - base_map.first_row(caret_line);
                (composed > plain).then(|| base_map.with_extra_rows(caret_line, composed - plain))
            }
            _ => None,
        };
        let rows = Rows {
            map: wrap_cols.map(|_| adjusted.as_ref().unwrap_or(&base_map)),
            cols: wrap_cols,
            lines: buffer.line_count(),
        };

        let rows_total = rows.total();
        let content_h = rows_total as f32 * row_h + row_h * 2.0;
        let content_w = metrics.text_left_offset() + (state.max_cols as f32 + 4.0) * char_w;

        let text_color = ui.visuals().text_color();
        let syntax_cache = Rc::new(RefCell::new(std::mem::take(&mut state.syntax_cache)));
        let spec = TextSpec {
            font_id: font_id.clone(),
            color: text_color,
            kind_colors: crate::theme::syntax_colors(ui.ctx(), ui.visuals().dark_mode),
            syntax: language.map(|lang| SyntaxCtx {
                lang,
                cache: syntax_cache.clone(),
            }),
            char_w,
            row_h,
        };
        let scroll = if wrap_cols.is_some() {
            // 自動換行時沒有橫向捲動
            egui::ScrollArea::vertical().horizontal_scroll_offset(0.0)
        } else {
            egui::ScrollArea::both()
        };
        let inner = scroll
            .id_salt(id.with("scroll"))
            .auto_shrink([false, false])
            .show_viewport(ui, |ui, viewport| {
                let content_w = if wrap_cols.is_some() { viewport.width() } else { content_w };
                let (content_rect, _) =
                    ui.allocate_exact_size(vec2(content_w, content_h), Sense::hover());
                let origin = content_rect.min;
                let visible = ui.clip_rect();
                let text_left = origin.x + metrics.text_left_offset();

                // 這一幀看得到哪幾列
                let first = (viewport.min.y / row_h).floor().max(0.0) as usize;
                let last = ((viewport.max.y / row_h).ceil() as usize + 1).min(rows_total);
                state.view_rows = ((visible.height() / row_h).floor() as usize).max(1);

                let mut layouts: Vec<LineLayout> = Vec::new();
                let mut row = first;
                while row < last {
                    let (line, _) = rows.locate(row);
                    let hi = last.min(rows.end_row(line)).max(row + 1);
                    layouts.extend(layout_rows(
                        ui,
                        &spec,
                        buffer,
                        state,
                        line,
                        rows.first_row(line),
                        wrap_cols,
                        row..hi,
                    ));
                    row = hi;
                }
                if wrap_cols.is_none() {
                    for layout in &layouts {
                        let cols = motion::display_col(
                            &buffer.line_text(layout.line),
                            buffer.line_len_chars(layout.line),
                        );
                        state.max_cols = state.max_cols.max(cols);
                    }
                }

                // ── 滑鼠 ──
                let response = ui
                    .interact(visible, id, Sense::click_and_drag())
                    .on_hover_cursor(CursorIcon::Text);
                // 螢幕位置 → （整份文字的字元位置, 是不是停在換行處的列尾）
                let char_at = |layouts: &mut Vec<LineLayout>,
                               ui: &Ui,
                               state: &EditorState,
                               buffer: &Buffer,
                               pos: Pos2|
                 -> (usize, bool) {
                    let rel_y = pos.y - origin.y;
                    let row = ((rel_y / row_h).floor().max(0.0) as usize)
                        .min(rows_total.saturating_sub(1));
                    let i = ensure_row(layouts, &rows, ui, &spec, buffer, state, row);
                    let layout = &layouts[i];
                    let col = layout.index_at_x(pos.x - text_left);
                    let line_len = buffer.line_len_chars(layout.line);
                    let end = layout.is_wrap_end(col);
                    (buffer.line_start(layout.line) + col.min(line_len), end)
                };
                let (shift, ctrl, alt) = ui.input(|i| (i.modifiers.shift, i.modifiers.command, i.modifiers.alt));
                // 螢幕位置 → （行, 顯示欄）：矩形選取用
                let line_dcol_at = |pos: Pos2| -> (usize, usize) {
                    let row = (((pos.y - origin.y) / row_h).floor().max(0.0) as usize)
                        .min(rows_total.saturating_sub(1));
                    let (line, _) = rows.locate(row);
                    let dcol = ((pos.x - text_left) / char_w).round().max(0.0) as usize;
                    (line, dcol)
                };
                // interact 偵測到「點在別處」時會把焦點收走。組字中要是沒有別的輸入框接手，
                // 就立刻拿回來（趕在下面回報輸入法位置之前），輸入法才不會被關掉而中斷組字。
                if state.preedit.is_some()
                    && ui.memory(|m| m.focused().is_none())
                    && ui.input(|i| i.focused)
                {
                    ui.memory_mut(|m| m.request_focus(id));
                }
                if response.clicked() || response.drag_started() || response.dragged() {
                    response.request_focus();
                }
                if let Some(pos) = response.interact_pointer_pos() {
                    if response.triple_clicked() {
                        let (idx, _) = char_at(&mut layouts, ui, state, buffer, pos);
                        state.select_line_at(buffer, idx);
                        touched = true;
                    } else if response.double_clicked() {
                        let (idx, _) = char_at(&mut layouts, ui, state, buffer, pos);
                        state.select_word_at(buffer, idx);
                        touched = true;
                    } else if response.clicked() && ctrl && !shift {
                        // Ctrl＋點擊：加上（或移除）一個游標
                        let (idx, _) = char_at(&mut layouts, ui, state, buffer, pos);
                        state.toggle_caret(buffer, idx);
                        touched = true;
                    } else if response.clicked() {
                        let (idx, end) = char_at(&mut layouts, ui, state, buffer, pos);
                        state.set_caret(buffer, idx, shift);
                        if end {
                            state.mark_row_end(buffer);
                        }
                        touched = true;
                    } else if response.dragged() && (alt || state.rect_anchor.is_some()) {
                        // Alt＋拖曳：矩形選取
                        if response.drag_started() || state.rect_anchor.is_none() {
                            let origin_pos = ui.input(|i| i.pointer.press_origin()).unwrap_or(pos);
                            state.rect_anchor = Some(line_dcol_at(origin_pos));
                        }
                        if let Some(anchor) = state.rect_anchor {
                            state.rect_select(buffer, anchor, line_dcol_at(pos));
                        }
                        touched = true;
                    } else if response.dragged() {
                        // 拖曳一開始要先把起點放在「按下去」的位置，而不是 egui 判定為拖曳時
                        // （已經移動了一小段）的位置。
                        if response.drag_started() {
                            let origin_pos = ui.input(|i| i.pointer.press_origin()).unwrap_or(pos);
                            let (idx, _) = char_at(&mut layouts, ui, state, buffer, origin_pos);
                            state.set_caret(buffer, idx, shift);
                        }
                        let (idx, _) = char_at(&mut layouts, ui, state, buffer, pos);
                        state.set_caret(buffer, idx, true);
                        touched = true;
                        // 拖曳到視窗外面時，往那個方向捲動
                        let dy = if pos.y < visible.top() {
                            visible.top() - pos.y
                        } else if pos.y > visible.bottom() {
                            visible.bottom() - pos.y
                        } else {
                            0.0
                        };
                        let dx = if wrap_cols.is_some() {
                            0.0
                        } else if pos.x < visible.left() + metrics.gutter_w {
                            visible.left() + metrics.gutter_w - pos.x
                        } else if pos.x > visible.right() {
                            visible.right() - pos.x
                        } else {
                            0.0
                        };
                        if dx != 0.0 || dy != 0.0 {
                            ui.scroll_with_delta(vec2(dx, dy) * 0.3);
                            ui.ctx().request_repaint();
                        }
                    }
                }

                if response.drag_stopped() {
                    state.rect_anchor = None;
                }

                // 滑鼠操作可能打斷了組字，這一列的排版要跟上
                if state.preedit.is_none() {
                    for i in 0..layouts.len() {
                        let l = &layouts[i];
                        if *l.full != *buffer.line_text(l.line) {
                            let (line, row) = (l.line, l.row);
                            let mut fresh = layout_rows(
                                ui,
                                &spec,
                                buffer,
                                state,
                                line,
                                rows.first_row(line),
                                wrap_cols,
                                row..row + 1,
                            );
                            if let Some(new) = fresh.pop() {
                                layouts[i] = new;
                            }
                        }
                    }
                }

                // ── 繪製 ──
                let painter = ui.painter().with_clip_rect(visible);
                // 小圓角：窗格是圓角卡片，編輯區放在裡面，角落不能是方的
                painter.rect_filled(visible, eframe::egui::CornerRadius::same(4), ui.visuals().extreme_bg_color);

                let (caret_line, caret_col) = caret_line_col(buffer, state);
                let preedit_len = state
                    .preedit
                    .as_ref()
                    .map_or(0, |p| p.text.chars().count());
                let all_sels = if state.has_multi() {
                    state.all_selections().0
                } else {
                    vec![state.sel]
                };
                let has_selection = all_sels.iter().any(|s| !s.is_empty());
                let sel_color = ui.visuals().selection.bg_fill;
                let find_color = ui.visuals().warn_fg_color.gamma_multiply(0.35);
                let faint = ui.visuals().weak_text_color().gamma_multiply(0.6);

                // 安全界線：第 N 欄右緣的右邊整塊換一個淡淡的顏色（畫在文字底下，不擋字），
                // 一行寫過界線，那一段字就落在換色的區域裡。
                // 只畫在文字區裡：界線被捲到左邊看不到、或被行號區蓋住時，換色的範圍從文字區的左緣算起
                if let Some(x) = options.ruler.then(|| ruler_x(options.ruler_column, text_left, char_w)).flatten() {
                    let left = x.max(visible.left() + metrics.gutter_w);
                    if left < visible.right() {
                        painter.rect_filled(
                            Rect::from_min_max(pos2(left, visible.top()), pos2(visible.right(), visible.bottom())),
                            0.0,
                            ui.visuals().weak_text_color().gamma_multiply(0.12),
                        );
                    }
                }
                for layout in &layouts {
                    let y = origin.y + layout.row as f32 * row_h;
                    if y + row_h < visible.top() || y > visible.bottom() {
                        continue; // 為了游標或搜尋結果現場排的、但不在畫面內的列
                    }
                    let line_start = buffer.line_start(layout.line);
                    let composing = state.preedit.is_some() && layout.line == caret_line;

                    // 目前這一行的底色
                    // （不要求編輯區有焦點：在設定面板按開關時焦點不在編輯區，仍要看得到效果）
                    if options.highlight_line && !has_selection && layout.line == caret_line {
                        painter.rect_filled(
                            Rect::from_min_size(
                                pos2(visible.left(), y),
                                vec2(visible.width(), row_h),
                            ),
                            0.0,
                            ui.visuals().selection.bg_fill.gamma_multiply(0.3),
                        );
                    }

                    // 搜尋結果與選取
                    let paint_span = |range: &Range<usize>, color: Color32, radius: f32| {
                        if let Some(r) = span_rect(layout, range, line_start, y, text_left, row_h) {
                            painter.rect_filled(r, radius, color);
                        }
                    };
                    // 搜尋命中的那一筆（包含已經被選取的）用圓角色框，其他選取維持方角
                    const HIT_RADIUS: f32 = 3.0;
                    if let Some(h) = &highlight {
                        // 已經被選取的那一筆由選取色負責，不要疊兩層
                        if !(has_selection && state.sel.range() == *h) {
                            paint_span(h, find_color, HIT_RADIUS);
                        }
                    }
                    for s in all_sels.iter().filter(|s| !s.is_empty()) {
                        let is_hit = highlight.as_ref() == Some(&s.range());
                        paint_span(&s.range(), sel_color, if is_hit { HIT_RADIUS } else { 0.0 });
                    }

                    for seg in &layout.segments {
                        painter.galley(pos2(text_left + seg.x, y + seg.y), seg.galley.clone(), text_color);
                    }

                    if options.show_whitespace {
                        let show_eol = layout.last && layout.line + 1 < buffer.line_count();
                        paint_whitespace(&painter, layout, text_left, y, row_h, faint, show_eol);
                    }

                    // 組字中的文字：底線；輸入法正在選字的那一段畫得粗一點
                    if composing {
                        if let Some(pre) = &state.preedit {
                            let row_lo = layout.start;
                            let row_hi = layout.start + layout.len;
                            let base = y + row_h - 1.5;
                            let underline = |from: usize, to: usize, stroke: Stroke| {
                                let (a, b) = (from.max(row_lo), to.min(row_hi));
                                if a < b || (a == b && from == to) {
                                    painter.line_segment(
                                        [
                                            pos2(text_left + layout.x_at(a), base),
                                            pos2(text_left + layout.x_at(b), base),
                                        ],
                                        stroke,
                                    );
                                }
                            };
                            underline(caret_col, caret_col + preedit_len, Stroke::new(1.0, text_color));
                            if let Some(active) = &pre.active {
                                underline(
                                    caret_col + active.start,
                                    caret_col + active.end,
                                    Stroke::new(2.5, ui.visuals().selection.stroke.color),
                                );
                            }
                        }
                    }
                }

                // 游標：組字中時畫在組字文字的後面（或選字範圍的後面）
                let (caret_rect, ime_rect);
                {
                    let caret_k = match &state.preedit {
                        Some(p) => caret_col + p.cursor.min(preedit_len),
                        None => caret_col,
                    };
                    let sub = row_of_caret(state, buffer, caret_line, caret_col, caret_k);
                    let row = rows.first_row(caret_line) + sub;
                    let i = ensure_row(&mut layouts, &rows, ui, &spec, buffer, state, row);
                    let layout = &layouts[i];
                    let y = origin.y + row as f32 * row_h;
                    let x = text_left + layout.x_at(caret_k);
                    caret_rect = Rect::from_min_size(pos2(x, y), vec2(2.0, row_h));
                    // 輸入法的候選窗貼著「組字開始處到結束處」，沒組字時就是游標
                    let x_start = text_left + layout.x_at(caret_col);
                    let x_end = text_left
                        + layout.x_at(caret_col + preedit_len).max(layout.x_at(caret_col));
                    ime_rect = Rect::from_min_max(pos2(x_start, y), pos2(x_end.max(x_start + 2.0), y + row_h));

                    let blink = (ui.input(|i| i.time) * 2.0) as u64 % 2 == 0;
                    if focused && (blink || touched) {
                        let stroke = ui.visuals().text_cursor.stroke;
                        let stroke = Stroke::new(stroke.width.max(1.5), stroke.color);
                        painter.line_segment([pos2(x, y), pos2(x, y + row_h)], stroke);
                        // 其他游標：只畫看得到的
                        for s in &state.extra {
                            let line = buffer.line_of_char(s.head.min(buffer.len_chars()));
                            let col = s.head - buffer.line_start(line);
                            let rs = state.rows_for(buffer, line);
                            let row = rows.first_row(line) + state.row_index(buffer, &rs, line, col);
                            if row < first || row >= last {
                                continue;
                            }
                            if let Some(l) = layouts.iter().find(|l| l.row == row) {
                                let x = text_left + l.x_at(col);
                                let y = origin.y + row as f32 * row_h;
                                painter.line_segment([pos2(x, y), pos2(x, y + row_h)], stroke);
                            }
                        }
                    }
                }
                if focused {
                    ui.ctx().request_repaint_after(Duration::from_millis(500));
                }

                // 行號區（固定在左邊，不跟著橫向捲動）。畫在文字之後，所以會蓋住捲到左邊去的文字。
                if options.show_line_numbers {
                    let gutter = Rect::from_min_max(
                        pos2(visible.left(), visible.top()),
                        pos2(visible.left() + metrics.gutter_w, visible.bottom()),
                    );
                    painter.rect_filled(gutter, 0.0, ui.visuals().panel_fill);
                    painter.line_segment(
                        [gutter.right_top(), gutter.right_bottom()],
                        Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color),
                    );
                    for row in first..last {
                        let (line, sub) = rows.locate(row);
                        if sub != 0 {
                            continue; // 折行後的第二列以後不編號
                        }
                        let y = origin.y + row as f32 * row_h;
                        let color = if line == caret_line {
                            text_color
                        } else {
                            ui.visuals().weak_text_color()
                        };
                        painter.text(
                            pos2(gutter.right() - char_w * 0.75, y + row_h / 2.0),
                            Align2::RIGHT_CENTER,
                            (line + 1).to_string(),
                            font_id.clone(),
                            color,
                        );
                    }
                }

                // ── 捲動 ──
                if state.scroll_to_caret {
                    state.scroll_to_caret = false;
                    let target = Rect::from_min_max(
                        pos2(
                            caret_rect.left() - metrics.gutter_w - char_w,
                            caret_rect.top() - row_h * 0.5,
                        ),
                        pos2(caret_rect.right() + char_w * 3.0, caret_rect.bottom() + row_h * 0.5),
                    );
                    ui.scroll_to_rect(target, None);
                }
                if reveal {
                    if let Some(h) = &highlight {
                        let line = buffer.line_of_char(h.start);
                        let col = (h.start - buffer.line_start(line)).min(buffer.line_len_chars(line));
                        let ranges = state.rows_for(buffer, line);
                        let sub = ranges.iter().position(|r| col < r.end).unwrap_or(ranges.len() - 1);
                        let row = rows.first_row(line) + sub;
                        let i = ensure_row(&mut layouts, &rows, ui, &spec, buffer, state, row);
                        let x = text_left + layouts[i].x_at(col);
                        let y = origin.y + row as f32 * row_h;
                        ui.scroll_to_rect(
                            Rect::from_min_size(pos2(x, y), vec2(char_w * 2.0, row_h)),
                            Some(Align::Center),
                        );
                    }
                }

                (response, origin, visible, ime_rect)
            });

        state.wrap_map = base_map;
        state.syntax_cache = std::mem::take(&mut *syntax_cache.borrow_mut());
        let (response, content_origin, visible, ime_rect) = inner.inner;

        // 告訴輸入法：編輯區在哪、候選窗要貼在哪、要不要中斷目前的組字
        if ui.memory(|m| m.owns_ime_events(id)) {
            let to_global = ui
                .ctx()
                .layer_transform_to_global(ui.layer_id())
                .unwrap_or_default();
            let interrupt = std::mem::take(&mut state.interrupt_ime);
            ui.output_mut(|o| {
                o.ime = Some(egui::output::IMEOutput {
                    purpose: IMEPurpose::Normal,
                    rect: to_global * visible,
                    cursor_rect: to_global * ime_rect,
                    should_interrupt_composition: interrupt,
                });
            });
        }

        EditorOutput {
            response,
            focused: ui.memory(|m| m.has_focus(id)),
            touched,
            metrics,
            content_origin,
            wrap_cols,
        }
    }
}

/// 一段選取（或搜尋結果）在某一列上佔的矩形；與這一列沒有交集時回傳 `None`。
/// 選取含到換行字元時（只會在這一行的最後一列），矩形向右多延伸半個字元寬，讓「選到了換行」看得出來。
fn span_rect(
    layout: &LineLayout,
    range: &Range<usize>,
    line_start: usize,
    y: f32,
    text_left: f32,
    row_h: f32,
) -> Option<Rect> {
    let row_lo = line_start + layout.start;
    let row_hi = row_lo + layout.len;
    let a = range.start.max(row_lo);
    let b = range.end.min(row_hi + usize::from(layout.last));
    if a >= b {
        return None;
    }
    let x0 = text_left + layout.x_at(a - line_start);
    let x1 = if b > row_hi {
        text_left + layout.x_at(row_hi - line_start) + layout.char_w * 0.5
    } else {
        text_left + layout.x_at(b - line_start)
    };
    Some(Rect::from_min_max(pos2(x0, y), pos2(x1.max(x0 + 1.0), y + row_h)))
}

/// 診斷用：環境變數 FEDITOR_IME_LOG 設成檔案路徑時，把輸入法相關的事件（以及組字中收到的
/// 按鍵、文字）連同當時的游標與組字狀態追加寫入該檔案，用來查輸入法的實際行為。
fn ime_log(event: &Event, buffer: &Buffer, state: &EditorState) {
    use std::io::Write;
    let Some(path) = std::env::var_os("FEDITOR_IME_LOG") else {
        return;
    };
    let relevant = matches!(event, Event::Ime(_))
        || (state.preedit.is_some() && matches!(event, Event::Key { .. } | Event::Text(_)));
    if !relevant {
        return;
    }
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let (line, col) = caret_line_col(buffer, state);
        let _ = writeln!(
            file,
            "{event:?}  | head={} (行{line} 欄{col}) preedit={:?}",
            state.sel.head, state.preedit
        );
    }
}

/// 處理這一幀的鍵盤與輸入法事件，回傳是否有任何操作。
fn process_events(ui: &Ui, buffer: &mut Buffer, state: &mut EditorState) -> bool {
    let filter = EventFilter {
        tab: true,
        horizontal_arrows: true,
        vertical_arrows: true,
        escape: state.has_multi(),
    };
    let events = ui.input(|i| i.filtered_events(&filter));
    let mut touched = false;
    for event in events {
        ime_log(&event, buffer, state);
        touched |= match event {
            Event::Text(text) => {
                let text: String = text.chars().filter(|c| !c.is_control()).collect();
                if !text.is_empty() {
                    state.record_typed(&text);
                }
                state.insert_text(buffer, &text)
            }
            Event::Copy => {
                if let Some(text) = state.selected_text(buffer) {
                    ui.ctx().copy_text(text);
                }
                false
            }
            Event::Cut => match state.cut(buffer) {
                Some(text) => {
                    ui.ctx().copy_text(text);
                    true
                }
                None => false,
            },
            Event::Paste(text) => {
                state.record_pasted(&text);
                state.paste(buffer, &text)
            }
            Event::Ime(ime) => {
                match ime {
                    ImeEvent::Preedit {
                        text,
                        active_range_chars,
                    } => state.ime_preedit(buffer, &text, active_range_chars),
                    ImeEvent::Commit(text) => {
                        if !text.is_empty() {
                            state.record_typed(&text);
                        }
                        state.ime_commit(buffer, &text)
                    }
                    ImeEvent::DeleteSurrounding {
                        before_chars,
                        after_chars,
                    } => state.ime_delete_surrounding(buffer, before_chars, after_chars),
                    #[allow(deprecated)]
                    _ => {}
                }
                true
            }
            Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } => handle_key(state, buffer, key, modifiers),
            _ => false,
        };
    }
    touched
}

/// 按鍵對應的編輯指令名稱（見 `commands.rs`）；沒有對應的回傳 `None`。
fn key_command(key: Key, m: Modifiers, multi: bool) -> Option<&'static str> {
    // 詞為單位的移動與刪除：Windows／Linux 用 Ctrl，macOS 用 Option
    let word = if cfg!(target_os = "macos") { m.alt } else { m.ctrl };
    let shift = m.shift;
    let pick = |plain: &'static str, selecting: &'static str| if shift { selecting } else { plain };
    Some(match key {
        Key::ArrowLeft => match (shift, word) {
            (false, false) => "left",
            (false, true) => "wordLeft",
            (true, false) => "selectLeft",
            (true, true) => "selectWordLeft",
        },
        Key::ArrowRight => match (shift, word) {
            (false, false) => "right",
            (false, true) => "wordRight",
            (true, false) => "selectRight",
            (true, true) => "selectWordRight",
        },
        Key::PageUp => pick("pageUp", "selectPageUp"),
        Key::PageDown => pick("pageDown", "selectPageDown"),
        Key::Home if m.command => pick("docStart", "selectDocStart"),
        Key::Home => pick("home", "selectHome"),
        Key::End if m.command => pick("docEnd", "selectDocEnd"),
        Key::End => pick("end", "selectEnd"),
        Key::Backspace if word => "deleteWordLeft",
        Key::Backspace => "backspace",
        Key::Delete if word => "deleteWordRight",
        Key::Delete => "delete",
        Key::Enter => "enter",
        Key::ArrowUp if m.command && m.alt => "addCaretUp",
        Key::ArrowDown if m.command && m.alt => "addCaretDown",
        Key::D if m.command && !shift => "selectNextOccurrence",
        Key::Escape if multi => "clearCarets",
        Key::ArrowUp if m.alt && shift => "duplicateLine",
        Key::ArrowDown if m.alt && shift => "duplicateLine",
        Key::ArrowUp if m.alt => "moveLineUp",
        Key::ArrowDown if m.alt => "moveLineDown",
        Key::K if m.command && shift => "deleteLine",
        Key::F if m.alt && shift && !m.command => "reformat",
        Key::Slash if m.command => "toggleComment",
        Key::OpenBracket if m.command => "unindent",
        Key::CloseBracket if m.command => "indent",
        Key::Tab if shift => "shiftTab",
        Key::Tab => "tab",
        Key::ArrowUp => pick("up", "selectUp"),
        Key::ArrowDown => pick("down", "selectDown"),
        Key::A if m.command => "selectAll",
        Key::Z if m.command && shift => "redo",
        Key::Z if m.command => "undo",
        Key::Y if m.command => "redo",
        _ => return None,
    })
}

fn handle_key(state: &mut EditorState, buffer: &mut Buffer, key: Key, m: Modifiers) -> bool {
    match key_command(key, m, state.has_multi()) {
        Some(name) => state.command(buffer, name).unwrap_or(false),
        None => false,
    }
}

/// 讓測試與整合程式碼拿到目前的選取（避免它們直接碰模型的內部欄位）。
#[allow(dead_code)]
pub fn selection_of(state: &EditorState) -> Selection {
    state.sel
}

#[allow(dead_code)]
fn _assert_types(_: Vec2, _: Align) {}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{Event, ImeEvent, PointerButton, RawInput};

    /// 沒有視窗的測試用 egui：每次 `frame` 跑一幀，把事件餵給編輯區。
    struct Harness {
        ctx: egui::Context,
        buffer: Buffer,
        state: EditorState,
        id: Id,
        time: f64,
        last: Option<EditorOutput>,
        ime: Option<egui::output::IMEOutput>,
        options: ViewOptions,
        size: Vec2,
    }

    impl Harness {
        fn new(text: &str) -> Self {
            let ctx = egui::Context::default();
            let mut h = Harness {
                ctx,
                buffer: Buffer::from_text(text),
                state: EditorState::default(),
                id: Id::unique("test-editor"),
                time: 0.0,
                last: None,
                ime: None,
                options: ViewOptions::default(),
                size: vec2(800.0, 400.0),
            };
            let id = h.id;
            h.ctx.all_styles_mut(|s| s.scroll_animation = egui::style::ScrollAnimation::none());
            h.ctx.memory_mut(|m| m.request_focus(id));
            h.frame(vec![]);
            h.frame(vec![]);
            h
        }

        fn frame(&mut self, events: Vec<Event>) {
            self.frame_with(events, self.size, None)
        }

        fn frame_with(&mut self, events: Vec<Event>, size: Vec2, highlight: Option<Range<usize>>) {
            self.time += 0.016;
            let input = RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, size)),
                events,
                time: Some(self.time),
                focused: true,
                ..Default::default()
            };
            let id = self.id;
            let (buffer, state) = (&mut self.buffer, &mut self.state);
            let mut last = None;
            let reveal = highlight.is_some();
            let mut output = self.ctx.run_ui(input, |ui| {
                egui::Panel::top("test-top").show(ui, |ui| {
                    ui.label("不是輸入框的區域");
                });
                egui::CentralPanel::default().show(ui, |ui| {
                    last = Some(
                        EditorView {
                            id,
                            buffer: &mut *buffer,
                            state: &mut *state,
                            highlight: highlight.clone(),
                            reveal,
                            options: self.options,
                            language: None,
                        }
                        .show(ui),
                    );
                });
            });
            self.ime = output.platform_output.ime;
            output.textures_delta.clear();
            self.last = last;
        }

        fn type_text(&mut self, s: &str) {
            self.frame(vec![Event::Text(s.to_string())]);
        }

        fn key(&mut self, key: Key, modifiers: Modifiers) {
            self.frame(vec![Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers,
            }]);
        }

        fn click_at(&mut self, pos: Pos2, modifiers: Modifiers, times: usize) {
            // 和上一次點擊隔開夠久，否則會被當成雙擊
            self.time += 2.0;
            self.frame(vec![Event::ModifiersChanged(modifiers)]);
            for _ in 0..times {
                self.frame(vec![Event::PointerMoved(pos)]);
                self.frame(vec![Event::PointerButton {
                    pos,
                    button: PointerButton::Primary,
                    pressed: true,
                    modifiers,
                }]);
                self.frame(vec![Event::PointerButton {
                    pos,
                    button: PointerButton::Primary,
                    pressed: false,
                    modifiers,
                }]);
            }
            self.frame(vec![Event::ModifiersChanged(Modifiers::NONE)]);
        }

        /// 第 `line` 行、第 `col` 個字元左緣在螢幕上的位置（取行中央的高度）。
        fn point_of(&self, line: usize, col: usize) -> Pos2 {
            let out = self.last.as_ref().unwrap();
            let m = out.metrics;
            let text = self.buffer.line_text(line);
            let d = motion::display_col(&text, col);
            pos2(
                out.content_origin.x + m.text_left_offset() + d as f32 * m.char_w + 1.0,
                out.content_origin.y + (line as f32 + 0.5) * m.row_h,
            )
        }

        fn text(&self) -> String {
            self.buffer.text()
        }
    }

    const NONE: Modifiers = Modifiers::NONE;

    #[test]
    fn typing_inserts_text_and_moves_caret() {
        let mut h = Harness::new("");
        h.type_text("abc");
        h.key(Key::Enter, NONE);
        h.type_text("中文");
        assert_eq!(h.text(), "abc\n中文");
        assert_eq!(h.state.sel.head, 6);
    }

    #[test]
    fn control_characters_in_text_events_are_ignored() {
        let mut h = Harness::new("");
        h.type_text("a\u{7f}b\r");
        assert_eq!(h.text(), "ab");
    }

    #[test]
    fn unfocused_view_ignores_keys() {
        let mut h = Harness::new("");
        let id = h.id;
        h.ctx.memory_mut(|m| m.surrender_focus(id));
        h.frame(vec![]);
        h.type_text("x");
        assert_eq!(h.text(), "");
    }

    #[test]
    fn arrows_backspace_delete_and_tab() {
        let mut h = Harness::new("ab");
        h.key(Key::End, NONE);
        h.key(Key::ArrowLeft, NONE);
        h.key(Key::Delete, NONE);
        assert_eq!(h.text(), "a");
        h.key(Key::Backspace, NONE);
        assert_eq!(h.text(), "");
        h.key(Key::Tab, NONE);
        assert_eq!(h.text(), "\t");
        h.key(Key::Tab, Modifiers::SHIFT);
        assert_eq!(h.text(), "");
    }

    #[test]
    fn select_all_copy_cut_paste_roundtrip() {
        let mut h = Harness::new("hello");
        h.key(Key::A, Modifiers::COMMAND);
        h.frame(vec![Event::Cut]);
        assert_eq!(h.text(), "");
        h.frame(vec![Event::Paste("hello".into())]);
        assert_eq!(h.text(), "hello");
        h.key(Key::A, Modifiers::COMMAND);
        h.frame(vec![Event::Copy]);
        assert_eq!(h.text(), "hello");
    }

    #[test]
    fn undo_and_redo_shortcuts() {
        let mut h = Harness::new("");
        h.type_text("abc");
        h.key(Key::Z, Modifiers::COMMAND);
        assert_eq!(h.text(), "");
        h.key(Key::Y, Modifiers::COMMAND);
        assert_eq!(h.text(), "abc");
        h.key(Key::Z, Modifiers::COMMAND);
        h.key(Key::Z, Modifiers::COMMAND | Modifiers::SHIFT);
        assert_eq!(h.text(), "abc");
    }

    #[test]
    fn vertical_keys_keep_goal_column() {
        let mut h = Harness::new("abcdef\nab\nabcdef");
        h.key(Key::ArrowRight, NONE);
        for _ in 0..5 {
            h.key(Key::ArrowRight, NONE);
        }
        h.key(Key::ArrowDown, NONE);
        assert_eq!(h.state.sel.head, 9); // 短行的行尾
        h.key(Key::ArrowDown, NONE);
        assert_eq!(h.state.sel.head, 7 + 3 + 6); // 第三行行首 10，欄 6
    }

    #[test]
    fn ime_preedit_commit_and_candidate_window_position() {
        let mut h = Harness::new("ab");
        h.key(Key::End, NONE);
        h.frame(vec![Event::Ime(ImeEvent::Preedit {
            text: "ㄓㄨㄥ".into(),
            active_range_chars: None,
        })]);
        assert_eq!(h.text(), "ab", "組字中的文字不能進入緩衝區");
        let ime = h.ime.clone().expect("組字時要回報輸入法位置");
        let metrics = h.last.as_ref().unwrap().metrics;
        let origin = h.last.as_ref().unwrap().content_origin;
        let text_left = origin.x + metrics.text_left_offset();
        // 候選窗貼在組字文字的起點（第 2 欄），且不在視窗外
        assert!((ime.cursor_rect.left() - (text_left + 2.0 * metrics.char_w)).abs() < 1.0);
        assert!(ime.rect.contains_rect(Rect::from_min_size(ime.cursor_rect.min, vec2(1.0, 1.0))));
        h.frame(vec![Event::Ime(ImeEvent::Commit("中".into()))]);
        assert_eq!(h.text(), "ab中");
        assert!(h.state.preedit.is_none());
    }

    #[test]
    fn ime_is_interrupted_when_clicking_elsewhere() {
        let mut h = Harness::new("hello\nworld");
        h.key(Key::End, NONE);
        h.frame(vec![Event::Ime(ImeEvent::Preedit {
            text: "ㄇ".into(),
            active_range_chars: None,
        })]);
        let target = h.point_of(1, 2);
        h.click_at(target, NONE, 1);
        assert!(h.state.preedit.is_none(), "點到別處會結束組字");
        assert_eq!(h.state.sel.head, 8);
    }

    #[test]
    fn click_places_caret_and_shift_click_extends() {
        let mut h = Harness::new("hello\nworld");
        let p = h.point_of(1, 3);
        h.click_at(p, NONE, 1);
        assert_eq!(h.state.sel.head, 6 + 3);
        assert_eq!(h.state.sel.anchor, 6 + 3);
        let p = h.point_of(0, 1);
        h.click_at(p, Modifiers::SHIFT, 1);
        assert_eq!(h.state.sel.anchor, 9);
        assert_eq!(h.state.sel.head, 1);
    }

    #[test]
    fn click_beyond_line_end_and_below_text() {
        let mut h = Harness::new("ab\ncd");
        let p = h.point_of(0, 2) + vec2(300.0, 0.0);
        h.click_at(p, NONE, 1);
        assert_eq!(h.state.sel.head, 2);
        let p = h.point_of(1, 0) + vec2(300.0, 200.0);
        h.click_at(p, NONE, 1);
        assert_eq!(h.state.sel.head, 5);
    }

    #[test]
    fn click_lands_correctly_on_cjk_and_tab() {
        // 每個字元固定佔「欄寬 × 半形字寬」，所以中文字的格子位置是精確的
        let mut h = Harness::new("中文\tx");
        let m = h.last.as_ref().unwrap().metrics;
        // 「中」佔第 0–1 欄、「文」第 2–3 欄、Tab 到第 8 欄、「x」第 8 欄
        for (col_f, expect) in [
            (0.2, 0),  // 「中」前半
            (0.9, 0),
            (1.1, 1),  // 「中」後半 → 在「中」後面
            (2.9, 1),
            (3.1, 2),  // 「文」後半 → 在「文」後面
            (5.0, 2),  // Tab 的範圍內
            (6.1, 3),  // Tab 後半 → 在 Tab 後面
            (8.2, 3),
            (8.8, 4),
            (30.0, 4), // 超過行尾
        ] {
            let p = h.point_of(0, 0) + vec2(col_f * m.char_w - 1.0, 0.0);
            h.click_at(p, NONE, 1);
            assert_eq!(h.state.sel.head, expect, "欄 {col_f}");
        }
    }

    #[test]
    fn wide_characters_are_laid_out_on_the_column_grid() {
        // 不論中文字的字型寬度是多少，後面的半形字都必須落在正確的欄位上，
        // 這樣上下行的英文才會對齊，一個中文字才會剛好等於兩個英文字
        let ctx = egui::Context::default();
        let mut checked = false;
        let mut output = ctx.run_ui(RawInput::default(), |ui| {
            let spec = TextSpec {
                font_id: TextStyle::Monospace.resolve(ui.style()),
                color: Color32::WHITE,
                kind_colors: [Color32::WHITE; 7],
                syntax: None,
                char_w: 8.0,
                row_h: 16.0,
            };
            // ab(0–1) 中(2–3) 文(4–5) cd(6–7)，Tab 從第 8 欄對齊到第 12 欄，e 在第 12 欄
            let segs = layout_segments(ui, &spec, "ab中文cd\te", None);
            let xs: Vec<f32> = segs.iter().map(|s| s.x).collect();
            assert_eq!(segs.len(), 5, "ab / 中 / 文 / cd / e：{xs:?}");
            assert_eq!(xs[0], 0.0);
            assert!(xs[1] >= 2.0 * 8.0 && xs[1] < 4.0 * 8.0, "中在自己的格子裡：{xs:?}");
            assert!(xs[2] >= 4.0 * 8.0 && xs[2] < 6.0 * 8.0, "文在自己的格子裡：{xs:?}");
            assert_eq!(xs[3], 6.0 * 8.0, "cd 從第 6 欄開始：{xs:?}");
            assert_eq!(xs[4], 12.0 * 8.0, "Tab 之後的 e 在第 12 欄：{xs:?}");
            // 組合字元跟著前一個字，不另外成為一個格子
            let segs = layout_segments(ui, &spec, "e\u{301}x", None);
            assert_eq!(segs.len(), 1);
            checked = true;
        });
        output.textures_delta.clear();
        assert!(checked);
    }
    #[test]
    fn double_and_triple_click_select_word_and_line() {
        let mut h = Harness::new("foo bar\nbaz");
        let p = h.point_of(0, 5);
        h.click_at(p, NONE, 2);
        assert_eq!(h.state.sel.range(), 4..7);
        h.click_at(p, NONE, 3);
        assert_eq!(h.state.sel.range(), 0..8);
    }

    #[test]
    fn dragging_selects_a_range() {
        let mut h = Harness::new("hello world");
        let a = h.point_of(0, 2);
        let b = h.point_of(0, 8);
        h.frame(vec![Event::PointerMoved(a)]);
        h.frame(vec![Event::PointerButton {
            pos: a,
            button: PointerButton::Primary,
            pressed: true,
            modifiers: NONE,
        }]);
        h.frame(vec![Event::PointerMoved(a + vec2(5.0, 0.0))]);
        h.frame(vec![Event::PointerMoved(b)]);
        h.frame(vec![Event::PointerButton {
            pos: b,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: NONE,
        }]);
        assert_eq!(h.state.sel.range(), 2..8);
    }

    #[test]
    fn caret_scrolls_into_view_when_moving_far() {
        let text = (0..500).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
        let mut h = Harness::new(&text);
        let top = h.last.as_ref().unwrap().content_origin.y;
        h.key(Key::End, Modifiers::COMMAND);
        h.frame(vec![]);
        h.frame(vec![]);
        let out = h.last.as_ref().unwrap();
        assert!(out.content_origin.y < top - 1000.0, "已經捲到檔案尾端");
        let caret_y = out.content_origin.y + 499.0 * out.metrics.row_h;
        assert!(caret_y > 0.0 && caret_y < 400.0, "游標在可見範圍內: {caret_y}");
    }

    #[test]
    fn page_down_moves_by_visible_rows() {
        let text = (0..200).map(|i| i.to_string()).collect::<Vec<_>>().join("\n");
        let mut h = Harness::new(&text);
        let rows = h.state.view_rows;
        assert!(rows > 5);
        h.key(Key::PageDown, NONE);
        let line = h.buffer.line_of_char(h.state.sel.head);
        assert_eq!(line, rows - 1);
    }

    #[test]
    fn reveal_scrolls_highlight_into_view() {
        let text = (0..500).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
        let mut h = Harness::new(&text);
        let target = h.buffer.line_start(300);
        h.frame_with(vec![], vec2(800.0, 400.0), Some(target..target + 4));
        // 捲動要求在下一幀才生效，再下一幀才量得到新位置
        h.frame_with(vec![], vec2(800.0, 400.0), None);
        h.frame_with(vec![], vec2(800.0, 400.0), None);
        let out = h.last.as_ref().unwrap();
        let y = out.content_origin.y + 300.0 * out.metrics.row_h;
        assert!(y > 0.0 && y < 400.0, "搜尋結果在可見範圍內: {y}");
    }

    #[test]
    fn gutter_widens_with_line_count() {
        let small = Harness::new("a");
        let big = Harness::new(&"x\n".repeat(100_000));
        assert!(big.last.as_ref().unwrap().metrics.gutter_w > small.last.as_ref().unwrap().metrics.gutter_w);
    }

    #[test]
    fn huge_document_frame_time_does_not_depend_on_size() {
        let text = "some text on a line\n".repeat(1_000_000);
        let mut h = Harness::new(&text);
        h.key(Key::End, Modifiers::COMMAND);
        h.frame(vec![]);
        let start = std::time::Instant::now();
        for _ in 0..10 {
            h.type_text("x");
        }
        let per_frame = start.elapsed() / 10;
        eprintln!("百萬行文件，每幀 {per_frame:?}");
        assert!(per_frame < Duration::from_millis(250), "太慢：{per_frame:?}");
    }

    #[test]
    fn composition_keeps_focus_when_clicking_a_non_input_area() {
        let mut h = Harness::new("ab");
        h.frame(vec![Event::Ime(ImeEvent::Preedit {
            text: "ㄇ".into(),
            active_range_chars: None,
        })]);
        // 模擬 egui 因為點到空白處而把焦點收走
        let id = h.id;
        h.ctx.memory_mut(|m| m.surrender_focus(id));
        h.frame(vec![]);
        assert!(h.last.as_ref().unwrap().focused, "焦點要拿回來");
        assert!(h.state.preedit.is_some(), "組字不能被中斷");
        assert!(h.ime.is_some(), "要持續回報輸入法位置，否則輸入法會被關掉");
    }

    #[test]
    fn focus_is_not_stolen_from_another_input_while_composing() {
        let mut h = Harness::new("ab");
        h.frame(vec![Event::Ime(ImeEvent::Preedit {
            text: "ㄇ".into(),
            active_range_chars: None,
        })]);
        let other = Id::unique("other-input");
        h.ctx.memory_mut(|m| m.request_focus(other));
        h.frame(vec![]);
        assert!(!h.last.as_ref().unwrap().focused);
    }

    #[test]
    fn gutter_is_only_as_wide_as_the_largest_line_number() {
        let one = Harness::new("a");
        let m1 = one.last.as_ref().unwrap().metrics;
        assert!((m1.gutter_w - 2.5 * m1.char_w).abs() < 0.01, "1 位數：{m1:?}");
        let nine = Harness::new(&"x\n".repeat(8));
        assert!((nine.last.as_ref().unwrap().metrics.gutter_w - 2.5 * m1.char_w).abs() < 0.01);
        let ten = Harness::new(&"x\n".repeat(9));
        assert!((ten.last.as_ref().unwrap().metrics.gutter_w - 3.5 * m1.char_w).abs() < 0.01);
    }

    /// 組字中，用滑鼠點視窗裡一個「不會取得焦點」的區域：組字要維持，而且整個過程中
    /// 都要持續回報輸入法的位置（只要有一幀沒回報，輸入法就會被關掉）。
    #[test]
    fn clicking_a_non_focusable_area_keeps_composing() {
        let mut h = Harness::new("ab");
        h.frame(vec![Event::Ime(ImeEvent::Preedit {
            text: "ㄇ".into(),
            active_range_chars: None,
        })]);
        let p = pos2(20.0, 8.0);
        h.frame(vec![Event::PointerMoved(p)]);
        h.frame(vec![Event::PointerButton {
            pos: p,
            button: PointerButton::Primary,
            pressed: true,
            modifiers: NONE,
        }]);
        assert!(h.ime.is_some(), "按下的這一幀就要持續回報");
        assert!(h.state.preedit.is_some());
        h.frame(vec![Event::PointerButton {
            pos: p,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: NONE,
        }]);
        assert!(h.ime.is_some(), "放開的這一幀也是");
        assert!(h.state.preedit.is_some(), "組字不能被中斷");
        assert!(h.last.as_ref().unwrap().focused);
        assert_eq!(h.text(), "ab");
        // 之後還能繼續組字、確定
        h.frame(vec![Event::Ime(ImeEvent::Commit("媽".into()))]);
        assert_eq!(h.text(), "媽ab"); // 游標在最前面
    }

    /// 焦點被別的輸入框取走：組字中的內容直接輸入到編輯區。
    #[test]
    fn composition_is_committed_when_another_input_takes_focus() {
        let mut h = Harness::new("ab");
        h.key(Key::End, NONE);
        h.frame(vec![Event::Ime(ImeEvent::Preedit {
            text: "ㄇㄚ".into(),
            active_range_chars: None,
        })]);
        let other = Id::unique("other-input");
        h.ctx.memory_mut(|m| m.request_focus(other));
        h.frame(vec![]);
        assert_eq!(h.text(), "abㄇㄚ", "組字的內容直接進了編輯區");
        assert!(h.state.preedit.is_none());
        assert_eq!(h.state.swallow_ime, Some(("ㄇㄚ".to_owned(), 3)));
        // 不會重複輸入
        h.frame(vec![]);
        assert_eq!(h.text(), "abㄇㄚ");
    }

    // ── 顯示選項與自動換行 ──────────────────────────────────

    fn wrapped(text: &str, width: f32) -> Harness {
        let mut h = Harness::new(text);
        h.options.word_wrap = true;
        h.size = vec2(width, 400.0);
        h.frame(vec![]);
        h.frame(vec![]);
        h
    }

    /// 第 `row`（0 起算）列、第 `col` 個字元左緣在螢幕上的位置（單一行、沒有橫向捲動時）。
    fn point_in_row(h: &Harness, row: usize, col: usize) -> Pos2 {
        let out = h.last.as_ref().unwrap();
        let m = out.metrics;
        pos2(
            out.content_origin.x + m.text_left_offset() + col as f32 * m.char_w + 1.0,
            out.content_origin.y + (row as f32 + 0.5) * m.row_h,
        )
    }

    const LONG: &str = "alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu nu xi omicron pi rho sigma tau";

    #[test]
    fn wrap_uses_the_available_width_and_splits_long_lines() {
        let h = wrapped(LONG, 300.0);
        let cols = h.last.as_ref().unwrap().wrap_cols.expect("開啟自動換行");
        assert!((10..60).contains(&cols), "cols={cols}");
        assert!(motion::wrap_rows(LONG, cols).len() > 1);
        // 沒有開啟時沒有折行寬度
        let h2 = Harness::new(LONG);
        assert_eq!(h2.last.as_ref().unwrap().wrap_cols, None);
    }

    #[test]
    fn clicking_a_wrapped_row_maps_to_the_right_characters() {
        let mut h = wrapped(LONG, 300.0);
        let cols = h.last.as_ref().unwrap().wrap_cols.unwrap();
        let rows = motion::wrap_rows(LONG, cols);
        let p = point_in_row(&h, 1, 2);
        h.click_at(p, NONE, 1);
        assert_eq!(h.state.sel.head, rows[1].start + 2);
        let p = point_in_row(&h, 2, 0);
        h.click_at(p, NONE, 1);
        assert_eq!(h.state.sel.head, rows[2].start);
    }

    #[test]
    fn clicking_past_the_end_of_a_wrapped_row_keeps_the_caret_on_that_row() {
        let mut h = wrapped(LONG, 300.0);
        let cols = h.last.as_ref().unwrap().wrap_cols.unwrap();
        let rows = motion::wrap_rows(LONG, cols);
        let far = point_in_row(&h, 0, rows[0].len() + 1); // 這一列文字之後的空白處
        h.click_at(far, NONE, 1);
        assert_eq!(h.state.sel.head, rows[0].end);
        let rows_of = h.state.rows_for(&h.buffer, 0);
        assert_eq!(
            h.state.row_index(&h.buffer, &rows_of, 0, rows[0].end),
            0,
            "游標要畫在上一列的尾端"
        );
        h.key(Key::ArrowRight, NONE);
        let rows_of = h.state.rows_for(&h.buffer, 0);
        assert_eq!(h.state.row_index(&h.buffer, &rows_of, 0, h.state.sel.head), 1);
    }

    #[test]
    fn down_and_up_move_by_rows_in_a_wrapped_line() {
        let mut h = wrapped(LONG, 300.0);
        let cols = h.last.as_ref().unwrap().wrap_cols.unwrap();
        let rows = motion::wrap_rows(LONG, cols);
        h.key(Key::ArrowDown, NONE);
        assert_eq!(h.state.sel.head, rows[1].start);
        h.key(Key::ArrowDown, NONE);
        assert_eq!(h.state.sel.head, rows[2].start);
        h.key(Key::ArrowUp, NONE);
        assert_eq!(h.state.sel.head, rows[1].start);
        h.key(Key::End, NONE);
        assert_eq!(h.state.sel.head, rows[1].end, "End 到這一列的尾端");
        h.key(Key::Home, NONE);
        assert_eq!(h.state.sel.head, rows[1].start, "Home 回到這一列的開頭");
    }

    #[test]
    fn typing_in_a_wrapped_line_keeps_working_and_remaps_rows() {
        let mut h = wrapped("short", 300.0);
        h.type_text(LONG);
        let cols = h.last.as_ref().unwrap().wrap_cols.unwrap();
        assert_eq!(h.text(), format!("{LONG}short"));
        assert!(motion::wrap_rows(&h.text(), cols).len() > 1);
        h.key(Key::ArrowDown, NONE);
        assert!(h.state.sel.head <= h.buffer.len_chars());
    }

    #[test]
    fn huge_documents_do_not_wrap() {
        let text = "x\n".repeat(wrap::MAX_LINES + 5);
        let h = wrapped(&text, 300.0);
        assert_eq!(h.last.as_ref().unwrap().wrap_cols, None);
    }

    #[test]
    fn hiding_line_numbers_removes_the_gutter_and_shifts_the_text() {
        let mut h = Harness::new("abc");
        let with = h.last.as_ref().unwrap().metrics.text_left_offset();
        h.options.show_line_numbers = false;
        h.frame(vec![]);
        let without = h.last.as_ref().unwrap().metrics;
        assert_eq!(without.gutter_w, 0.0);
        assert!(without.text_left_offset() < with);
        // 點在第 2 個字元：位置要跟著文字區一起位移
        let p = pos2(
            h.last.as_ref().unwrap().content_origin.x + without.text_left_offset() + 2.0 * without.char_w + 1.0,
            h.last.as_ref().unwrap().content_origin.y + 0.5 * without.row_h,
        );
        h.click_at(p, NONE, 1);
        assert_eq!(h.state.sel.head, 2);
    }

    #[test]
    fn whitespace_marks_and_current_line_toggle_do_not_break_rendering() {
        let mut h = Harness::new("a b\t中　文\nx");
        h.options.show_whitespace = true;
        h.options.highlight_line = false;
        h.frame(vec![]);
        h.type_text("z");
        assert_eq!(h.text(), "za b\t中　文\nx");
    }

    // ── 多重游標與矩形選取 ──────────────────────────────────

    const CTRL: Modifiers = Modifiers::COMMAND;

    #[test]
    fn ctrl_click_adds_a_caret_and_typing_goes_to_both() {
        let mut h = Harness::new("one\ntwo\nthree");
        let p = h.point_of(2, 0);
        h.click_at(p, CTRL, 1);
        assert!(h.state.has_multi());
        h.type_text("-");
        assert_eq!(h.text(), "-one\ntwo\n-three");
        h.key(Key::Escape, NONE);
        assert!(!h.state.has_multi(), "Esc 回到單一游標");
        h.type_text("+");
        assert_eq!(h.text(), "-one\ntwo\n-+three", "單一游標只輸入一處（主要游標在後加的那一處）");
    }

    #[test]
    fn plain_click_drops_the_extra_carets() {
        let mut h = Harness::new("one\ntwo\nthree");
        let p = h.point_of(1, 1);
        h.click_at(p, CTRL, 1);
        assert!(h.state.has_multi());
        let p = h.point_of(0, 0);
        h.click_at(p, NONE, 1);
        assert!(!h.state.has_multi());
    }

    #[test]
    fn ctrl_alt_arrows_add_carets_and_ctrl_d_selects_matches() {
        let mut h = Harness::new("a\nb\nc");
        h.key(Key::ArrowDown, Modifiers::COMMAND | Modifiers::ALT);
        h.key(Key::ArrowDown, Modifiers::COMMAND | Modifiers::ALT);
        h.type_text("x");
        assert_eq!(h.text(), "xa\nxb\nxc");
        let mut h = Harness::new("foo bar foo");
        h.key(Key::D, CTRL);
        h.key(Key::D, CTRL);
        h.type_text("z");
        assert_eq!(h.text(), "z bar z");
    }

    #[test]
    fn alt_drag_selects_a_rectangle() {
        let mut h = Harness::new("abcdef\nabcdef\nabcdef");
        let start = h.point_of(0, 1);
        let end = h.point_of(2, 4);
        let alt = Modifiers::ALT;
        h.frame(vec![Event::ModifiersChanged(alt)]);
        h.frame(vec![Event::PointerMoved(start)]);
        h.frame(vec![Event::PointerButton {
            pos: start,
            button: PointerButton::Primary,
            pressed: true,
            modifiers: alt,
        }]);
        h.frame(vec![Event::PointerMoved(pos2(start.x + 10.0, start.y + 10.0))]);
        h.frame(vec![Event::PointerMoved(end)]);
        h.frame(vec![Event::PointerMoved(end)]);
        h.frame(vec![Event::PointerButton {
            pos: end,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: alt,
        }]);
        h.frame(vec![Event::ModifiersChanged(NONE)]);
        let (all, _) = h.state.all_selections();
        assert_eq!(all.len(), 3);
        assert!(all.iter().all(|s| s.range().len() == 3), "{all:?}");
        h.type_text("-");
        assert_eq!(h.text(), "a-ef\na-ef\na-ef");
    }
}