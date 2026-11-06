//! 編輯器模型：選取、輸入、刪除、移動、復原，以及輸入法（IME）組字的狀態機。
//!
//! 這一層完全不碰畫面與 egui，只是對 [`Buffer`] 的操作，所以可以直接單元測試。
//! 所有位置都是字元索引；左右移動與刪除以「字素叢集」為單位（見 [`motion`]）。

use feditor_core::{Buffer, motion};
use std::ops::Range;

/// 選取範圍。`head` 就是游標（會跟著移動的那一端），`anchor` 是另一端。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Selection {
    pub anchor: usize,
    pub head: usize,
}

impl Selection {
    pub fn collapsed(at: usize) -> Self {
        Self { anchor: at, head: at }
    }

    pub fn is_empty(self) -> bool {
        self.anchor == self.head
    }

    /// 由小到大的範圍。
    pub fn range(self) -> Range<usize> {
        self.anchor.min(self.head)..self.anchor.max(self.head)
    }
}

/// 輸入法正在組字、尚未確定的文字。
///
/// 它**不在**緩衝區裡，只是畫面上暫時插在游標處：這樣組字過程不會弄髒復原歷史與「未儲存」狀態，
/// 取消組字也不必去復原什麼。確定時才一次寫進緩衝區。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Preedit {
    pub text: String,
    /// 輸入法目前「正在選字」的那一段（字元範圍，相對於 `text`），畫面上用比較明顯的底線標示。
    /// 注意它**不一定是游標**，游標位置看 [`cursor`](Self::cursor)。
    pub active: Option<Range<usize>>,
    /// 輸入法接下來會把新輸入的字插在組字文字裡的哪個位置（字元位置，相對於 `text`），
    /// 游標要畫在這裡。由 [`EditorState::ime_preedit`] 推算，推算方式見 [`infer_cursor`]。
    pub cursor: usize,
}

/// 兩段文字之間「有變動的區間」在新文字裡的結尾（字元位置）：去掉開頭與結尾相同的部分，
/// 剩下的就是變動的區間。第一次組字時 `old` 是空字串，結果就是整段文字的結尾。
fn changed_end(old: &str, new: &str) -> usize {
    let old: Vec<char> = old.chars().collect();
    let new: Vec<char> = new.chars().collect();
    let max = old.len().min(new.len());
    let prefix = old.iter().zip(&new).take_while(|(a, b)| a == b).count();
    let suffix = old
        .iter()
        .rev()
        .zip(new.iter().rev())
        .take(max - prefix)
        .take_while(|(a, b)| a == b)
        .count();
    new.len() - suffix
}

/// 推算輸入法接下來會把新輸入的字插在哪裡。
///
/// 輸入法回報的 `active` 不一定是游標：選字時它是「被標示的那一段」。實測 Windows 內建注音
/// （記錄見 `FEDITOR_IME_LOG`）的行為是：
///
/// - **文字有變動**（打字、轉成候選字）：游標在變動區間的後面。例如 `我要打字` 之間打 `ㄍ`
///   變成 `我要打ㄍ字`，游標在 `ㄍ` 後面；回報的範圍卻是後面的 `字`。
/// - **文字沒變、只是範圍換了**（方向鍵移動）：新字插在被標示那一段的開頭。
/// - **文字沒變、範圍也沒變**：輸入法按一次方向鍵就送一個事件，即使被標示的範圍沒變（例如只有
///   一段、游標在這段的前面或後面）。所以這種「重複」的事件代表游標在這段的開頭與結尾之間
///   切換了一次。實測 我要去玩耍（標示 4..5）：轉換完成時游標在 5，按左鍵送一個相同的事件
///   （游標到 4），再按左鍵才變成 3..5（游標到 3）；按右鍵回來時 4..5（游標 4），再按右鍵又是
///   一個相同的事件（游標回到 5）。
/// - 輸入法回報的是空範圍（一般的游標位置）：就用它回報的位置。
pub(crate) fn infer_cursor(prev: Option<&Preedit>, text: &str, active: &Option<Range<usize>>) -> usize {
    let len = text.chars().count();
    if let Some(a) = active.as_ref().filter(|a| a.is_empty()) {
        return a.start.min(len);
    }
    match prev {
        Some(p) if p.text == text => {
            if p.active == *active {
                match active {
                    // 範圍沒變：游標在這段的頭尾之間切換
                    Some(a) if p.cursor <= a.start => a.end.min(len),
                    Some(a) => a.start.min(len),
                    None => p.cursor.min(len),
                }
            } else {
                active.as_ref().map_or(p.cursor.min(len), |a| a.start.min(len))
            }
        }
        Some(p) => changed_end(&p.text, text),
        None => changed_end("", text),
    }
}
#[derive(Debug, Default)]
pub struct EditorState {
    pub sel: Selection,
    /// 上下移動游標時想停留的顯示欄位。經過短的行時游標會被迫靠左，
    /// 但回到長的行時要能回到原本的欄位，所以要記住它。
    pub(super) goal_col: Option<usize>,
    pub preedit: Option<Preedit>,
    /// 下一個畫面要把游標捲動到看得見的地方。
    pub scroll_to_caret: bool,
    /// 要求輸入法中斷目前的組字（例如使用者用滑鼠點到別處）。畫面讀走之後會清掉。
    pub interrupt_ime: bool,
    /// 畫面一次看得到幾行（Page Up／Page Down 要移動的行數）。由畫面每一幀更新。
    pub view_rows: usize,
    /// 目前看過的最長一行的顯示欄數。用來決定橫向捲動的範圍；只會增加，載入新檔案時歸零。
    pub max_cols: usize,
    /// 上次畫的時候的 Tab 欄寬：變了就要重算 `max_cols`（它只會往大長）。
    pub tab_seen: usize,
    /// 組字內容被我們直接寫進緩衝區之後（焦點被別的輸入框取走時），輸入法稍後可能還會送一個
    /// 內容相同的 Commit，這時它已經是別的輸入框有焦點，必須丟掉，不然文字會被打進那個輸入框。
    /// 值是（文字，還要留意幾個畫面）。
    pub swallow_ime: Option<(String, u8)>,
    /// 行註解符號（//、#…），空字串表示這種檔案沒有行註解。由檔案類型決定。
    pub comment_prefix: &'static str,
    /// 這份文件的語言（決定 Enter 自動縮排、排列格式的規則）；沒有語言就只沿用上一行的縮排。
    /// 由畫面每一幀依檔案類型設定。
    pub language: Option<&'static crate::syntax::Language>,
    /// 使用者替這個語言指定的括號內斷行風格（設定檔的 `wrap_style.<語言>`）；沒指定就用語言本身的。
    pub wrap_style: Option<crate::syntax::WrapStyle>,
    /// 自動換行的折行寬度（欄數）；None 表示不換行。由畫面每一幀更新，上下移動、Home、End
    /// 要依畫面上的「列」而不是文字的「行」。
    pub wrap_cols: Option<usize>,
    /// 游標在換行處時有兩種畫法：上一列的尾端、或下一列的開頭。用滑鼠點在列尾、或按 End，
    /// 要停在上一列的尾端，這裡記下（游標位置, 緩衝區修訂編號）；文字一改就失效。
    pub(super) wrap_end: Option<(usize, u64)>,
    /// 每一行佔幾個畫面列的累計表（畫面用）。
    pub(super) wrap_map: super::wrap::WrapMap,
    /// 主要選取（`sel`）以外的選取（多重游標、矩形選取）。見 `multi.rs`。
    pub extra: Vec<Selection>,
    /// 設定 `extra` 當時的緩衝區修訂編號；文字被別的方式改動就作廢。
    pub(super) multi_rev: u64,
    /// 正在對每個選取各做一次單一選取的操作（避免遞迴進多重選取的處理）。
    pub(super) multi_step: bool,
    /// Alt＋拖曳矩形選取時，按下去那一點的（行, 顯示欄）。
    pub rect_anchor: Option<(usize, usize)>,
    /// 巨集錄製中：目前錄到的動作；沒在錄製時為 None。見 commands.rs。
    pub recording: Option<Vec<super::commands::Recorded>>,
    /// 語法上色的跨行狀態快取（畫面用）。
    pub(super) syntax_cache: crate::syntax::StateCache,
}

/// 把 `\r\n`、`\r` 統一成 `\n`。緩衝區內只有 `\n`。
pub fn normalize_newlines(text: &str) -> String {
    if text.contains('\r') {
        text.replace("\r\n", "\n").replace('\r', "\n")
    } else {
        text.to_owned()
    }
}

impl EditorState {
    #[allow(dead_code)]
    pub fn caret(&self) -> usize {
        self.sel.head
    }

    /// 緩衝區被外部改動後（巨集、取代…）把選取範圍拉回有效範圍內。
    pub fn clamp(&mut self, buf: &Buffer) {
        let n = buf.len_chars();
        self.sel.anchor = self.sel.anchor.min(n);
        self.sel.head = self.sel.head.min(n);
        self.drop_stale_extra(buf);
    }

    /// 游標移到 `target`。`extend` 為 true 時只動 `head`（擴大或縮小選取），否則選取收成一點。
    /// 任何移動都會打斷打字的復原合併，並要求畫面捲到游標處。
    fn move_to(&mut self, buf: &mut Buffer, target: usize, extend: bool, keep_goal: bool) {
        let target = target.min(buf.len_chars());
        self.sel.head = target;
        if !extend {
            self.sel.anchor = target;
        }
        if !keep_goal {
            self.goal_col = None;
        }
        self.scroll_to_caret = true;
        buf.break_group();
    }

    /// 游標在哪一行、那一行的第幾個字元。
    fn line_and_col(buf: &Buffer, idx: usize) -> (usize, usize) {
        let line = buf.line_of_char(idx);
        (line, idx - buf.line_start(line))
    }

    // ── 移動 ────────────────────────────────────────────────

    pub fn set_caret(&mut self, buf: &mut Buffer, idx: usize, extend: bool) {
        self.extra.clear();
        self.interrupt_composition();
        self.move_to(buf, idx, extend, false);
    }

    pub fn move_left(&mut self, buf: &mut Buffer, extend: bool, by_word: bool) {
        if self.has_multi() && !self.multi_step {
            return self.for_each_selection(buf, |s, b| s.move_left(b, extend, by_word));
        }
        let target = if !extend && !self.sel.is_empty() && !by_word {
            self.sel.range().start // 有選取時按 ←：收到選取的左端
        } else if by_word {
            motion::word_left(buf.rope(), self.sel.head)
        } else {
            motion::prev_grapheme(buf.rope(), self.sel.head)
        };
        self.move_to(buf, target, extend, false);
    }

    pub fn move_right(&mut self, buf: &mut Buffer, extend: bool, by_word: bool) {
        if self.has_multi() && !self.multi_step {
            return self.for_each_selection(buf, |s, b| s.move_right(b, extend, by_word));
        }
        let target = if !extend && !self.sel.is_empty() && !by_word {
            self.sel.range().end // 有選取時按 →：收到選取的右端
        } else if by_word {
            motion::word_right(buf.rope(), self.sel.head)
        } else {
            motion::next_grapheme(buf.rope(), self.sel.head)
        };
        self.move_to(buf, target, extend, false);
    }

    /// 這一行折成哪幾列（字元範圍，相對於行首）；沒有自動換行時整行就是一列。
    pub(super) fn rows_for(&self, buf: &Buffer, line: usize) -> Vec<Range<usize>> {
        match self.wrap_cols {
            Some(cols) => super::wrap::rows_of(buf, line, cols),
            None => vec![0..buf.line_len_chars(line)],
        }
    }

    /// 游標在 `col`（行內字元位置）時落在第幾列。剛好在換行處時，預設算下一列的開頭，
    /// 除非記著「停在上一列的尾端」。
    pub(super) fn row_index(
        &self,
        buf: &Buffer,
        rows: &[Range<usize>],
        line: usize,
        col: usize,
    ) -> usize {
        let last = rows.len() - 1;
        let sub = rows.iter().position(|r| col < r.end).unwrap_or(last);
        let idx = buf.line_start(line) + col;
        let upstream = matches!(self.wrap_end, Some((i, rev)) if i == idx && rev == buf.revision());
        if sub > 0 && upstream && col == rows[sub - 1].end {
            sub - 1
        } else {
            sub
        }
    }

    /// 記下「游標停在這一列的尾端」（見 [`wrap_end`](Self::wrap_end)）。
    pub fn mark_row_end(&mut self, buf: &Buffer) {
        self.wrap_end = Some((self.sel.head, buf.revision()));
    }

    fn row_text(buf: &Buffer, line: usize, row: &Range<usize>) -> String {
        buf.line_text(line)
            .chars()
            .skip(row.start)
            .take(row.len())
            .collect()
    }

    /// 上下移動 `lines` 行（負數往上）；有自動換行時以畫面上的列為單位。保持「想停留的顯示欄位」。
    pub fn move_vertical(&mut self, buf: &mut Buffer, lines: isize, extend: bool) {
        if self.has_multi() && !self.multi_step {
            return self.for_each_selection(buf, |s, b| s.move_vertical(b, lines, extend));
        }
        if self.wrap_cols.is_some() {
            return self.move_vertical_rows(buf, lines, extend);
        }
        let (line, col) = Self::line_and_col(buf, self.sel.head);
        let goal = self
            .goal_col
            .unwrap_or_else(|| motion::display_col(&buf.line_text(line), col));
        let last = buf.line_count().saturating_sub(1) as isize;
        let wanted = line as isize + lines;
        let target = if wanted < 0 {
            0 // 已經在第一行還往上：到文字開頭
        } else if wanted > last {
            buf.len_chars() // 已經在最後一行還往下：到文字末端
        } else {
            let line = wanted as usize;
            buf.line_start(line) + motion::char_at_display_col(&buf.line_text(line), goal)
        };
        self.move_to(buf, target, extend, true);
        self.goal_col = Some(goal);
    }

    fn move_vertical_rows(&mut self, buf: &mut Buffer, lines: isize, extend: bool) {
        let (line, col) = Self::line_and_col(buf, self.sel.head);
        let rows = self.rows_for(buf, line);
        let sub = self.row_index(buf, &rows, line, col);
        let goal = self.goal_col.unwrap_or_else(|| {
            motion::display_col(&Self::row_text(buf, line, &rows[sub]), col - rows[sub].start)
        });
        let (mut l, mut s, mut cur) = (line, sub, rows);
        let mut edge = None;
        for _ in 0..lines.unsigned_abs() {
            if lines > 0 {
                if s + 1 < cur.len() {
                    s += 1;
                } else if l + 1 < buf.line_count() {
                    l += 1;
                    s = 0;
                    cur = self.rows_for(buf, l);
                } else {
                    edge = Some(buf.len_chars());
                    break;
                }
            } else if s > 0 {
                s -= 1;
            } else if l > 0 {
                l -= 1;
                cur = self.rows_for(buf, l);
                s = cur.len() - 1;
            } else {
                edge = Some(0);
                break;
            }
        }
        let target = edge.unwrap_or_else(|| {
            let row = &cur[s];
            let off = motion::char_at_display_col(&Self::row_text(buf, l, row), goal);
            let mut idx = row.start + off.min(row.len());
            if s + 1 < cur.len() {
                // 不要落在列尾（那是下一列的開頭）
                idx = idx.min(row.end.saturating_sub(1).max(row.start));
            }
            buf.line_start(l) + idx
        });
        self.move_to(buf, target, extend, true);
        self.goal_col = Some(goal);
    }

    /// Home：先到這一行第一個非空白字元，已經在那裡就到行首（再按一次切換）。
    pub fn home(&mut self, buf: &mut Buffer, extend: bool) {
        if self.has_multi() && !self.multi_step {
            return self.for_each_selection(buf, |s, b| s.home(b, extend));
        }
        let (line, col) = Self::line_and_col(buf, self.sel.head);
        let start = buf.line_start(line);
        if self.wrap_cols.is_some() {
            let rows = self.rows_for(buf, line);
            let sub = self.row_index(buf, &rows, line, col);
            if sub > 0 {
                // 折行後的第二列以後：回到這一列的開頭
                return self.move_to(buf, start + rows[sub].start, extend, false);
            }
        }
        let indent = buf
            .line_text(line)
            .chars()
            .take_while(|c| *c == ' ' || *c == '\t')
            .count();
        let first_non_blank = start + indent;
        let target = if self.sel.head != first_non_blank && indent < buf.line_len_chars(line) {
            first_non_blank
        } else {
            start
        };
        self.move_to(buf, target, extend, false);
    }

    pub fn end(&mut self, buf: &mut Buffer, extend: bool) {
        if self.has_multi() && !self.multi_step {
            return self.for_each_selection(buf, |s, b| s.end(b, extend));
        }
        let (line, col) = Self::line_and_col(buf, self.sel.head);
        if self.wrap_cols.is_some() {
            let rows = self.rows_for(buf, line);
            let sub = self.row_index(buf, &rows, line, col);
            let target = buf.line_start(line) + rows[sub].end;
            self.move_to(buf, target, extend, false);
            if sub + 1 < rows.len() {
                self.mark_row_end(buf);
            }
            return;
        }
        let target = buf.line_start(line) + buf.line_len_chars(line);
        self.move_to(buf, target, extend, false);
    }

    pub fn doc_start(&mut self, buf: &mut Buffer, extend: bool) {
        self.extra.clear();
        self.move_to(buf, 0, extend, false);
    }

    pub fn doc_end(&mut self, buf: &mut Buffer, extend: bool) {
        self.extra.clear();
        let n = buf.len_chars();
        self.move_to(buf, n, extend, false);
    }

    pub fn select_all(&mut self, buf: &mut Buffer) {
        self.extra.clear();
        self.interrupt_composition();
        self.sel = Selection {
            anchor: 0,
            head: buf.len_chars(),
        };
        self.goal_col = None;
        buf.break_group();
    }

    /// 雙擊：選取游標下的那個詞。
    pub fn select_word_at(&mut self, buf: &mut Buffer, idx: usize) {
        self.extra.clear();
        self.interrupt_composition();
        let range = motion::word_at(buf.rope(), idx.min(buf.len_chars()));
        self.sel = Selection {
            anchor: range.start,
            head: range.end,
        };
        self.goal_col = None;
        buf.break_group();
    }

    /// 三擊：選取整行（含換行）。
    pub fn select_line_at(&mut self, buf: &mut Buffer, idx: usize) {
        self.extra.clear();
        self.interrupt_composition();
        let line = buf.line_of_char(idx.min(buf.len_chars()));
        self.sel = Selection {
            anchor: buf.line_start(line),
            head: buf.line_start(line + 1).min(buf.len_chars()),
        };
        self.goal_col = None;
        buf.break_group();
    }

    // ── 輸入與刪除 ──────────────────────────────────────────

    /// 打字：取代選取（若有），否則插在游標處。連續打字在復原時併成一步。
    pub fn insert_text(&mut self, buf: &mut Buffer, text: &str) -> bool {
        if self.has_multi() {
            return self.multi_insert(buf, text);
        }
        let text = normalize_newlines(text);
        if text.is_empty() {
            return false;
        }
        // 在只有空白的行首輸入閉括號：先減一層縮排（取代游標前的那一層空白）
        if self.sel.is_empty()
            && self.language.is_some()
            && let Some(c) = text.chars().next().filter(|c| text.chars().count() == 1 && "})]".contains(*c))
        {
            let at = self.sel.head;
            let (line, col) = Self::line_and_col(buf, at);
            let before: String = buf.line_text(line).chars().take(col).collect();
            let state = self.line_state(buf, line);
            let remove = super::indent::closer_dedent(self.language, state, &before, c);
            if remove > 0 {
                buf.break_group();
                buf.replace(at - remove..at, &text);
                buf.break_group();
                self.sel = Selection::collapsed(at - remove + 1);
                self.goal_col = None;
                self.scroll_to_caret = true;
                return true;
            }
        }
        let start = if self.sel.is_empty() {
            let at = self.sel.head;
            buf.typed(at, &text);
            at
        } else {
            let range = self.sel.range();
            buf.break_group();
            buf.replace(range.clone(), &text);
            buf.break_group();
            range.start
        };
        self.sel = Selection::collapsed(start + text.chars().count());
        self.goal_col = None;
        self.scroll_to_caret = true;
        true
    }

    /// 貼上：換行統一成 `\n`，且自成一個復原步驟。
    pub fn paste(&mut self, buf: &mut Buffer, text: &str) -> bool {
        if self.has_multi() {
            return self.multi_paste(buf, text);
        }
        let text = normalize_newlines(text);
        if text.is_empty() {
            return false;
        }
        self.interrupt_composition();
        let range = self.sel.range();
        buf.break_group();
        buf.replace(range.clone(), &text);
        buf.break_group();
        self.sel = Selection::collapsed(range.start + text.chars().count());
        self.goal_col = None;
        self.scroll_to_caret = true;
        true
    }

    /// 第 `line` 行開頭的語法跨行狀態（沒有語言就是一般狀態）。
    pub(super) fn line_state(&mut self, buf: &Buffer, line: usize) -> crate::syntax::State {
        match self.language {
            Some(lang) => self.syntax_cache.state_at(lang, buf, line),
            None => crate::syntax::State::Normal,
        }
    }

    /// Enter：換行，並沿用目前這一行開頭的縮排（空白與 Tab）。
    /// 有語言的縮排規則時（見 `indent.rs`）：行尾是 `{ ( [`（Python 還有 `:`）就多縮排一層，
    /// 游標在一對括號之間時展開成三行。
    pub fn enter(&mut self, buf: &mut Buffer) -> bool {
        if self.has_multi() {
            return self.multi_enter(buf);
        }
        let range = self.sel.range();
        let (line, col) = Self::line_and_col(buf, range.start);
        let before: String = buf.line_text(line).chars().take(col).collect();
        let end_line = buf.line_of_char(range.end);
        let after: String = buf.line_text(end_line).chars().skip(range.end - buf.line_start(end_line)).collect();
        let state = self.line_state(buf, line);
        let plan = super::indent::enter_plan(self.language, state, &before, &after);
        if !self.insert_text(buf, &plan.text) {
            return false;
        }
        self.sel = Selection::collapsed(range.start + plan.caret);
        true
    }

    /// Tab：選取跨多行時整段縮排，否則插入一個 Tab；設定成用空白取代 Tab 時，改成插入空白補到下一個 Tab 位置
    ///（有多個游標時每個游標都插入一整個 Tab 欄寬的空白）。
    pub fn tab(&mut self, buf: &mut Buffer) -> bool {
        let (a, b) = self.selected_lines(buf);
        if a != b {
            return self.indent_lines(buf);
        }
        if !motion::tab_uses_spaces() {
            return self.insert_text(buf, "\t");
        }
        let width = motion::tab_width();
        let spaces = if self.has_multi() {
            width
        } else {
            let (line, col) = Self::line_and_col(buf, self.sel.range().start);
            width - motion::display_col(&buf.line_text(line), col) % width
        };
        self.insert_text(buf, &" ".repeat(spaces))
    }

    /// Shift＋Tab：把選到的行少縮排一層（一個 Tab，或最多 [motion::tab_width] 個空白）。
    pub fn shift_tab(&mut self, buf: &mut Buffer) -> bool {
        self.unindent_lines(buf)
    }

    fn delete_selection(&mut self, buf: &mut Buffer) {
        let range = self.sel.range();
        buf.break_group();
        buf.remove(range.clone());
        buf.break_group();
        self.sel = Selection::collapsed(range.start);
        self.goal_col = None;
        self.scroll_to_caret = true;
    }

    /// Backspace。有選取就刪選取；否則刪游標前一個字素叢集（`by_word` 為 true 時刪前一個詞）。
    /// 連續的 Backspace 在復原時併成一步。
    pub fn backspace(&mut self, buf: &mut Buffer, by_word: bool) -> bool {
        if self.has_multi() {
            return self.multi_backspace(buf, by_word);
        }
        if !self.sel.is_empty() {
            self.delete_selection(buf);
            return true;
        }
        let head = self.sel.head;
        if head == 0 {
            return false;
        }
        let from = if by_word {
            motion::word_left(buf.rope(), head)
        } else {
            motion::prev_grapheme(buf.rope(), head)
        };
        buf.deleted(from..head);
        self.sel = Selection::collapsed(from);
        self.goal_col = None;
        self.scroll_to_caret = true;
        true
    }

    /// Delete。有選取就刪選取；否則刪游標後一個字素叢集（`by_word` 為 true 時刪後一個詞）。
    pub fn delete_forward(&mut self, buf: &mut Buffer, by_word: bool) -> bool {
        if self.has_multi() {
            return self.multi_delete(buf, by_word);
        }
        if !self.sel.is_empty() {
            self.delete_selection(buf);
            return true;
        }
        let head = self.sel.head;
        if head >= buf.len_chars() {
            return false;
        }
        let to = if by_word {
            motion::word_right(buf.rope(), head)
        } else {
            motion::next_grapheme(buf.rope(), head)
        };
        buf.deleted(head..to);
        self.goal_col = None;
        self.scroll_to_caret = true;
        true
    }

    // ── 剪貼簿 ──────────────────────────────────────────────

    /// 目前選取的文字；沒有選取時回傳 `None`。
    pub fn selected_text(&self, buf: &Buffer) -> Option<String> {
        if self.has_multi() {
            return self.multi_selected_text(buf);
        }
        (!self.sel.is_empty()).then(|| buf.slice(self.sel.range()))
    }

    /// 剪下：回傳被剪下的文字（要放進剪貼簿），並從緩衝區刪除。
    pub fn cut(&mut self, buf: &mut Buffer) -> Option<String> {
        if self.has_multi() {
            return self.multi_cut(buf);
        }
        let text = self.selected_text(buf)?;
        self.delete_selection(buf);
        Some(text)
    }

    // ── 復原與重做 ──────────────────────────────────────────

    pub fn undo(&mut self, buf: &mut Buffer) -> bool {
        self.interrupt_composition();
        let Some(span) = buf.undo() else {
            return false;
        };
        self.after_history(buf, span);
        true
    }

    pub fn redo(&mut self, buf: &mut Buffer) -> bool {
        self.interrupt_composition();
        let Some(span) = buf.redo() else {
            return false;
        };
        self.after_history(buf, span);
        true
    }

    fn after_history(&mut self, buf: &Buffer, span: Range<usize>) {
        self.sel = Selection::collapsed(span.end.min(buf.len_chars()));
        self.goal_col = None;
        self.scroll_to_caret = true;
    }

    // ── 輸入法 ──────────────────────────────────────────────

    /// 輸入法開始或更新組字。
    ///
    /// - 空字串表示清掉組字；沒有組字時收到空字串要忽略（有些平台會在沒組字時送空事件）。
    /// - 只有換行的事件忽略（Enter 由按鍵處理）。
    /// - 第一次開始組字時，若有選取，選取會被取代（先刪掉）。
    pub fn ime_preedit(&mut self, buf: &mut Buffer, text: &str, active: Option<Range<usize>>) {
        self.extra.clear();
        if text.is_empty() {
            self.preedit = None;
            return;
        }
        if text == "\n" || text == "\r" {
            return;
        }
        if self.preedit.is_none() && !self.sel.is_empty() {
            self.delete_selection(buf);
        }
        // 輸入法回報「沒有游標位置」（None）時：若上一次有明確的位置，代表使用者把組字內的游標
        // 移到了最前面。egui 在 Windows 上會把「游標在最前面」的 (0, 0) 轉成 None（為了閃避
        // 韓文輸入法的錯誤），這裡要還原，否則游標會被畫到最後面，而輸入法內部其實還在最前面，
        // 造成再按向左鍵沒有反應。上一次也沒有位置（例如韓文輸入法）就維持 None，游標畫在最後。
        let active = match (active, &self.preedit) {
            (None, Some(prev)) if prev.active.is_some() => Some(0..0),
            (active, _) => active,
        };
        let cursor = infer_cursor(self.preedit.as_ref(), text, &active);
        self.preedit = Some(Preedit {
            text: text.to_owned(),
            active,
            cursor,
        });
        self.scroll_to_caret = true;
    }

    /// 輸入法確定：清掉組字，把確定的文字寫進緩衝區。
    pub fn ime_commit(&mut self, buf: &mut Buffer, text: &str) {
        let had_preedit = self.preedit.take().is_some();
        if text.is_empty() || text == "\n" || text == "\r" {
            return;
        }
        if !had_preedit && !self.sel.is_empty() {
            self.delete_selection(buf);
        }
        self.insert_text(buf, text);
    }

    /// 輸入法要求刪除游標前後各幾個字元（例如重新轉換）。
    pub fn ime_delete_surrounding(&mut self, buf: &mut Buffer, before: usize, after: usize) {
        let head = self.sel.head;
        let start = head.saturating_sub(before);
        let end = (head + after).min(buf.len_chars());
        if start == end {
            return;
        }
        buf.break_group();
        buf.remove(start..end);
        buf.break_group();
        self.sel = Selection::collapsed(start);
    }

    /// 使用者用滑鼠點到別處等「打斷組字」的操作：丟掉我們這邊的組字，並要求輸入法也中斷。
    pub(super) fn interrupt_composition(&mut self) {
        if self.preedit.take().is_some() {
            self.interrupt_ime = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 在文字裡用 `|` 標示游標位置，建立（緩衝區, 狀態）。`|` 本身不會出現在文字裡。
    /// 沒有 `|` 時游標在開頭（測試自己會另外設定選取範圍）。
    fn setup(text_with_caret: &str) -> (Buffer, EditorState) {
        let caret = text_with_caret
            .chars()
            .position(|c| c == '|')
            .unwrap_or(0);
        let text: String = text_with_caret.chars().filter(|c| *c != '|').collect();
        let mut state = EditorState::default();
        state.sel = Selection::collapsed(caret);
        (Buffer::from_text(&text), state)
    }

    /// 把游標（與選取）畫回文字裡方便比對：游標 `|`，選取範圍用 `[` `]` 括起來。
    fn show(buf: &Buffer, state: &EditorState) -> String {
        let text: Vec<char> = buf.text().chars().collect();
        let mut out = String::new();
        let r = state.sel.range();
        for i in 0..=text.len() {
            if !state.sel.is_empty() && i == r.start {
                out.push('[');
            }
            if state.sel.is_empty() && i == state.sel.head {
                out.push('|');
            }
            if !state.sel.is_empty() && i == r.end {
                out.push(']');
            }
            if i < text.len() {
                out.push(text[i]);
            }
        }
        out
    }

    #[test]
    fn typing_inserts_at_the_caret_and_moves_it() {
        let (mut b, mut s) = setup("ab|cd");
        assert!(s.insert_text(&mut b, "X"));
        assert_eq!(show(&b, &s), "abX|cd");
        s.insert_text(&mut b, "你好");
        assert_eq!(show(&b, &s), "abX你好|cd");
        assert!(!s.insert_text(&mut b, ""));
    }

    #[test]
    fn continuous_typing_is_one_undo_step() {
        let (mut b, mut s) = setup("|");
        for ch in "hello".chars() {
            s.insert_text(&mut b, &ch.to_string());
        }
        assert_eq!(b.text(), "hello");
        assert!(s.undo(&mut b));
        assert_eq!(show(&b, &s), "|");
        assert!(!s.undo(&mut b), "五個字只該是一步");
        assert!(s.redo(&mut b));
        assert_eq!(show(&b, &s), "hello|");
    }

    #[test]
    fn moving_the_caret_breaks_the_typing_group() {
        let (mut b, mut s) = setup("|");
        s.insert_text(&mut b, "a");
        s.move_left(&mut b, false, false);
        s.move_right(&mut b, false, false);
        s.insert_text(&mut b, "b");
        s.undo(&mut b);
        assert_eq!(b.text(), "a", "移動過游標，後面打的字是另一步");
        s.undo(&mut b);
        assert_eq!(b.text(), "");
    }

    #[test]
    fn typing_replaces_the_selection_as_one_step() {
        let (mut b, mut s) = setup("ab|cdef");
        s.sel = Selection { anchor: 1, head: 4 };
        s.insert_text(&mut b, "X");
        assert_eq!(show(&b, &s), "aX|ef");
        s.undo(&mut b);
        assert_eq!(b.text(), "abcdef", "取代選取是一步，一次復原全部退回");
    }

    #[test]
    fn backspace_deletes_a_whole_grapheme() {
        for (text, name) in [("x👍🏽|y", "膚色"), ("xe\u{301}|y", "組合重音"), ("x👨‍👩‍👧|y", "家庭"), ("x你|y", "中文")] {
            let (mut b, mut s) = setup(text);
            assert!(s.backspace(&mut b, false), "{name}");
            assert_eq!(show(&b, &s), "x|y", "{name}：一次刪掉一整個字");
        }
        let (mut b, mut s) = setup("|ab");
        assert!(!s.backspace(&mut b, false), "開頭按 Backspace 什麼都不做");
        assert_eq!(b.text(), "ab");
    }

    #[test]
    fn delete_forward_deletes_a_whole_grapheme() {
        let (mut b, mut s) = setup("x|👍🏽y");
        assert!(s.delete_forward(&mut b, false));
        assert_eq!(show(&b, &s), "x|y");
        let (mut b, mut s) = setup("ab|");
        assert!(!s.delete_forward(&mut b, false), "末端按 Delete 什麼都不做");
    }

    #[test]
    fn consecutive_backspaces_undo_in_one_step() {
        let (mut b, mut s) = setup("abcdef|");
        for _ in 0..3 {
            s.backspace(&mut b, false);
        }
        assert_eq!(show(&b, &s), "abc|");
        s.undo(&mut b);
        assert_eq!(show(&b, &s), "abcdef|", "復原後游標回到刪除前的位置");
    }

    #[test]
    fn delete_word_backward_and_forward() {
        let (mut b, mut s) = setup("foo bar|");
        s.backspace(&mut b, true);
        assert_eq!(show(&b, &s), "foo |");
        let (mut b, mut s) = setup("|foo bar");
        s.delete_forward(&mut b, true);
        assert_eq!(show(&b, &s), "|bar", "Ctrl+Delete 連同後面的空白一起刪");
    }

    #[test]
    fn left_and_right_collapse_a_selection() {
        let (mut b, mut s) = setup("abcdef");
        s.sel = Selection { anchor: 4, head: 2 };
        s.move_left(&mut b, false, false);
        assert_eq!(show(&b, &s), "ab|cdef", "有選取時 ← 收到選取的左端");
        s.sel = Selection { anchor: 2, head: 4 };
        s.move_right(&mut b, false, false);
        assert_eq!(show(&b, &s), "abcd|ef", "有選取時 → 收到選取的右端");
    }

    #[test]
    fn shift_arrows_extend_the_selection_keeping_the_anchor() {
        let (mut b, mut s) = setup("ab|cdef");
        s.move_right(&mut b, true, false);
        s.move_right(&mut b, true, false);
        assert_eq!(show(&b, &s), "ab[cd]ef");
        assert_eq!(s.sel.anchor, 2);
        s.move_left(&mut b, true, false);
        s.move_left(&mut b, true, false);
        s.move_left(&mut b, true, false);
        assert_eq!(show(&b, &s), "a[b]cdef", "越過錨點後選取反向");
        assert_eq!(s.sel.anchor, 2);
    }

    #[test]
    fn horizontal_movement_steps_over_whole_graphemes() {
        // 注意：é 要明確寫成「e 加組合重音」（2 個 char），不是預組的單一字元
        let (mut b, mut s) = setup("|👍🏽e\u{301}你");
        s.move_right(&mut b, false, false);
        assert_eq!(s.caret(), 2, "跳過整個 👍🏽（2 個 char）");
        s.move_right(&mut b, false, false);
        assert_eq!(s.caret(), 4, "跳過整個 é（2 個 char）");
        s.move_left(&mut b, false, false);
        assert_eq!(s.caret(), 2);
        s.move_left(&mut b, false, false);
        assert_eq!(s.caret(), 0);
        s.move_left(&mut b, false, false);
        assert_eq!(s.caret(), 0, "開頭不再後退");
    }

    #[test]
    fn up_and_down_keep_the_goal_column_across_short_lines() {
        let (mut b, mut s) = setup("abcdef\nxy\nabcdef");
        s.sel = Selection::collapsed(5); // 第 0 行第 5 欄
        s.move_vertical(&mut b, 1, false);
        assert_eq!(show(&b, &s), "abcdef\nxy|\nabcdef", "短的一行：被迫靠到行尾");
        s.move_vertical(&mut b, 1, false);
        assert_eq!(show(&b, &s), "abcdef\nxy\nabcde|f", "回到長的一行：回到原本的第 5 欄");
        s.move_vertical(&mut b, -2, false);
        assert_eq!(show(&b, &s), "abcde|f\nxy\nabcdef");
    }

    #[test]
    fn vertical_movement_aligns_by_display_column_with_cjk() {
        // 第 0 行 "你好世界" 的游標在「好」後面（顯示欄位 4），往下到 "abcdefgh" 要停在第 4 欄
        let (mut b, mut s) = setup("你好|世界\nabcdefgh");
        s.move_vertical(&mut b, 1, false);
        assert_eq!(show(&b, &s), "你好世界\nabcd|efgh");
        // 反過來：英文第 4 欄往上，落在「好」後面
        s.move_vertical(&mut b, -1, false);
        assert_eq!(show(&b, &s), "你好|世界\nabcdefgh");
    }

    #[test]
    fn up_on_the_first_line_goes_to_start_and_down_on_the_last_to_end() {
        let (mut b, mut s) = setup("ab|c\ndef");
        s.move_vertical(&mut b, -1, false);
        assert_eq!(show(&b, &s), "|abc\ndef");
        s.move_vertical(&mut b, 1, false);
        s.move_vertical(&mut b, 1, false);
        assert_eq!(show(&b, &s), "abc\ndef|");
    }

    #[test]
    fn page_movement_moves_many_lines() {
        let text: String = (0..50).map(|i| format!("line{i}\n")).collect();
        let mut b = Buffer::from_text(&text);
        let mut s = EditorState::default();
        s.move_vertical(&mut b, 20, false);
        assert_eq!(b.line_of_char(s.caret()), 20);
        s.move_vertical(&mut b, -999, false);
        assert_eq!(s.caret(), 0);
    }

    #[test]
    fn smart_home_toggles_between_first_non_blank_and_line_start() {
        let (mut b, mut s) = setup("    foo|bar");
        s.home(&mut b, false);
        assert_eq!(show(&b, &s), "    |foobar", "先到第一個非空白字元");
        s.home(&mut b, false);
        assert_eq!(show(&b, &s), "|    foobar", "再按一次到行首");
        s.home(&mut b, false);
        assert_eq!(show(&b, &s), "    |foobar", "再按又回來");
        let (mut b, mut s) = setup("foo|bar");
        s.home(&mut b, false);
        assert_eq!(show(&b, &s), "|foobar", "沒有縮排：直接到行首");
        let (mut b, mut s) = setup("   |"); // 全是空白的行
        s.home(&mut b, false);
        assert_eq!(show(&b, &s), "|   ");
    }

    #[test]
    fn end_goes_to_the_end_of_the_line() {
        let (mut b, mut s) = setup("ab|c\ndef");
        s.end(&mut b, false);
        assert_eq!(show(&b, &s), "abc|\ndef");
        s.end(&mut b, true);
        assert_eq!(show(&b, &s), "abc|\ndef");
    }

    #[test]
    fn home_and_end_with_shift_select() {
        let (mut b, mut s) = setup("ab|cd");
        s.end(&mut b, true);
        assert_eq!(show(&b, &s), "ab[cd]");
        s.home(&mut b, true);
        assert_eq!(show(&b, &s), "[ab]cd", "錨點在原來的位置，反向選到行首");
    }

    #[test]
    fn doc_start_and_end() {
        let (mut b, mut s) = setup("a\nb|\nc");
        s.doc_start(&mut b, false);
        assert_eq!(s.caret(), 0);
        s.doc_end(&mut b, true);
        assert_eq!(show(&b, &s), "[a\nb\nc]");
    }

    #[test]
    fn word_movement_and_selection() {
        let (mut b, mut s) = setup("|foo bar baz");
        s.move_right(&mut b, false, true);
        assert_eq!(show(&b, &s), "foo |bar baz");
        s.move_right(&mut b, true, true);
        assert_eq!(show(&b, &s), "foo [bar ]baz");
        s.move_left(&mut b, false, true);
        assert_eq!(show(&b, &s), "foo |bar baz", "Ctrl+← 以游標（head）為準，不是收選取");
    }

    #[test]
    fn select_all_word_and_line() {
        let (mut b, mut s) = setup("foo bar\nbaz");
        s.select_all(&mut b);
        assert_eq!(show(&b, &s), "[foo bar\nbaz]");
        s.select_word_at(&mut b, 5);
        assert_eq!(show(&b, &s), "foo [bar]\nbaz");
        s.select_line_at(&mut b, 5);
        assert_eq!(show(&b, &s), "[foo bar\n]baz", "三擊選整行，含換行");
        s.select_line_at(&mut b, 9);
        assert_eq!(show(&b, &s), "foo bar\n[baz]", "最後一行沒有換行");
    }

    #[test]
    fn enter_keeps_the_indentation() {
        let (mut b, mut s) = setup("    foo|bar");
        s.enter(&mut b);
        assert_eq!(show(&b, &s), "    foo\n    |bar");
        let (mut b, mut s) = setup("\t\tx|");
        s.enter(&mut b);
        assert_eq!(show(&b, &s), "\t\tx\n\t\t|");
        let (mut b, mut s) = setup("plain|");
        s.enter(&mut b);
        assert_eq!(show(&b, &s), "plain\n|");
    }

    #[test]
    fn enter_in_the_middle_of_the_indent_only_copies_what_is_before_the_caret() {
        let (mut b, mut s) = setup("  |  foo");
        s.enter(&mut b);
        assert_eq!(show(&b, &s), "  \n  |  foo", "只複製游標前面的縮排");
    }

    /// 測試用：固定 Tab 與自動縮排設定，結束還原。
    struct Cfg(usize, bool, bool);
    impl Cfg {
        fn set(width: usize, spaces: bool) -> Self {
            Cfg(motion::set_tab_width(width), motion::set_tab_uses_spaces(spaces), motion::auto_indent())
        }
    }
    impl Drop for Cfg {
        fn drop(&mut self) {
            motion::set_tab_width(self.0);
            motion::set_tab_uses_spaces(self.1);
            motion::set_auto_indent(self.2);
        }
    }

    fn with_lang(text: &str, file: &str) -> (Buffer, EditorState) {
        let (b, mut s) = setup(text);
        s.language = crate::syntax::language_for_name(file);
        (b, s)
    }

    #[test]
    fn enter_after_an_opening_bracket_indents_and_between_brackets_opens_three_lines() {
        let _c = Cfg::set(4, true);
        let (mut b, mut s) = with_lang("fn f() {|", "a.rs");
        s.enter(&mut b);
        assert_eq!(show(&b, &s), "fn f() {\n    |");
        let (mut b, mut s) = with_lang("  if x {|}", "a.rs");
        s.enter(&mut b);
        assert_eq!(show(&b, &s), "  if x {\n      |\n  }");
        b.undo();
        assert_eq!(b.text(), "  if x {}", "一個復原步驟");
        let (mut b, mut s) = with_lang("def f():|", "a.py");
        s.enter(&mut b);
        assert_eq!(show(&b, &s), "def f():\n    |");
        // 沒有語言：只沿用縮排
        let (mut b, mut s) = setup("  a {|");
        s.enter(&mut b);
        assert_eq!(show(&b, &s), "  a {\n  |");
        // 關掉自動縮排
        motion::set_auto_indent(false);
        let (mut b, mut s) = with_lang("fn f() {|", "a.rs");
        s.enter(&mut b);
        assert_eq!(show(&b, &s), "fn f() {\n|");
    }

    #[test]
    fn typing_a_closer_at_the_start_of_a_blank_indented_line_dedents_it() {
        let _c = Cfg::set(4, true);
        let (mut b, mut s) = with_lang("fn f() {\n    |", "a.rs");
        assert!(s.insert_text(&mut b, "}"));
        assert_eq!(show(&b, &s), "fn f() {\n}|");
        b.undo();
        assert_eq!(b.text(), "fn f() {\n    ", "減縮排與輸入是同一步復原");
        // 行上已有文字：不動
        let (mut b, mut s) = with_lang("    x |", "a.rs");
        s.insert_text(&mut b, "}");
        assert_eq!(show(&b, &s), "    x }|");
        // 沒有語言：不動
        let (mut b, mut s) = setup("    |");
        s.insert_text(&mut b, "}");
        assert_eq!(show(&b, &s), "    }|");
        // 貼上不減縮排
        let (mut b, mut s) = with_lang("    |", "a.rs");
        s.paste(&mut b, "}");
        assert_eq!(show(&b, &s), "    }|");
    }

    #[test]
    fn multi_caret_enter_indents_each_line_by_the_language() {
        let _c = Cfg::set(2, true);
        let mut s = EditorState::default();
        s.language = crate::syntax::language_for_name("a.rs");
        let mut b = Buffer::from_text("a {\n b {}\n");
        s.sel = Selection::collapsed(3);
        s.extra = vec![Selection::collapsed(8)];
        s.multi_rev = b.revision();
        assert!(s.enter(&mut b));
        assert_eq!(b.text(), "a {\n  \n b {\n   }\n", "每個游標各自多縮排一層，不做三行展開");
    }

    #[test]
    fn tab_pads_with_spaces_to_the_next_tab_stop_when_spaces_replace_tabs() {
        let spaces = motion::set_tab_uses_spaces(true);
        let width = motion::set_tab_width(4);
        let (mut b, mut s) = setup("|x");
        s.tab(&mut b);
        assert_eq!(show(&b, &s), "    |x", "行首：一整個 Tab 欄寬");
        let (mut b, mut s) = setup("ab|c");
        s.tab(&mut b);
        assert_eq!(show(&b, &s), "ab  |c", "補到下一個 Tab 位置（第 4 欄）");
        let (mut b, mut s) = setup("你|");
        s.tab(&mut b);
        assert_eq!(show(&b, &s), "你  |", "全形字佔兩欄，只需要補兩個空白");
        motion::set_tab_width(8);
        let (mut b, mut s) = setup("ab|c");
        s.tab(&mut b);
        assert_eq!(show(&b, &s), "ab      |c", "Tab 欄寬 8：補 6 個空白");
        // 關掉之後還是插入 Tab 字元
        motion::set_tab_uses_spaces(false);
        let (mut b, mut s) = setup("|x");
        s.tab(&mut b);
        assert_eq!(show(&b, &s), "\t|x");
        motion::set_tab_width(width);
        motion::set_tab_uses_spaces(spaces);
    }

    #[test]
    fn tab_inserts_a_tab_and_shift_tab_unindents() {
        let (mut b, mut s) = setup("|x");
        s.tab(&mut b);
        assert_eq!(show(&b, &s), "\t|x");
        assert!(s.shift_tab(&mut b));
        assert_eq!(show(&b, &s), "|x");
        assert!(!s.shift_tab(&mut b), "沒有縮排可以退");

        let (mut b, mut s) = setup("      x|"); // 6 個空白：一次退 4 個
        s.shift_tab(&mut b);
        assert_eq!(show(&b, &s), "  x|");
        s.shift_tab(&mut b);
        assert_eq!(show(&b, &s), "x|", "不足 4 個就全退");
    }

    #[test]
    fn copy_and_cut() {
        let (mut b, mut s) = setup("abcdef");
        assert_eq!(s.selected_text(&b), None, "沒有選取：不複製");
        assert_eq!(s.cut(&mut b), None);
        s.sel = Selection { anchor: 4, head: 1 };
        assert_eq!(s.selected_text(&b).as_deref(), Some("bcd"));
        assert_eq!(s.cut(&mut b).as_deref(), Some("bcd"));
        assert_eq!(show(&b, &s), "a|ef");
        s.undo(&mut b);
        assert_eq!(b.text(), "abcdef");
    }

    #[test]
    fn paste_normalizes_newlines_and_is_its_own_undo_step() {
        let (mut b, mut s) = setup("ab|cd");
        s.insert_text(&mut b, "X");
        assert!(s.paste(&mut b, "1\r\n2\r3"));
        assert_eq!(b.text(), "abX1\n2\n3cd", "CRLF 與 CR 都統一成 LF");
        assert!(s.caret() == 8);
        s.undo(&mut b);
        assert_eq!(b.text(), "abXcd", "貼上自成一步，不與前面的打字合併");
        s.undo(&mut b);
        assert_eq!(b.text(), "abcd");
    }

    #[test]
    fn paste_replaces_the_selection() {
        let (mut b, mut s) = setup("abcdef");
        s.sel = Selection { anchor: 1, head: 5 };
        s.paste(&mut b, "你好");
        assert_eq!(show(&b, &s), "a你好|f");
        assert!(!s.paste(&mut b, ""), "空字串不動作");
    }

    #[test]
    fn undo_collapses_the_selection_and_places_the_caret() {
        let (mut b, mut s) = setup("hello world");
        s.sel = Selection { anchor: 0, head: 5 };
        s.backspace(&mut b, false);
        assert_eq!(show(&b, &s), "| world");
        s.undo(&mut b);
        assert_eq!(show(&b, &s), "hello| world", "復原刪除：游標在還原文字的後面");
        s.redo(&mut b);
        assert_eq!(show(&b, &s), "| world");
        assert!(!s.redo(&mut b));
    }

    #[test]
    fn clamp_pulls_the_selection_back_after_an_external_change() {
        let (mut b, mut s) = setup("abcdef|");
        s.sel = Selection { anchor: 2, head: 6 };
        b.remove(0..5); // 例如巨集刪掉了大半
        s.clamp(&b);
        assert_eq!(s.sel, Selection { anchor: 1, head: 1 });
    }

    // ── 輸入法 ──────────────────────────────────────────────

    #[test]
    fn ime_preedit_is_not_in_the_buffer() {
        let (mut b, mut s) = setup("ab|cd");
        s.ime_preedit(&mut b, "ㄋㄧˇ", Some(0..3));
        assert_eq!(b.text(), "abcd", "組字文字不在緩衝區裡");
        assert!(!b.can_undo(), "組字不會留下復原步驟");
        assert_eq!(s.preedit.as_ref().unwrap().text, "ㄋㄧˇ");
        assert_eq!(s.caret(), 2, "游標位置不變，組字畫在游標處");
        // 組字更新
        s.ime_preedit(&mut b, "你", None);
        assert_eq!(s.preedit.as_ref().unwrap().text, "你");
        assert_eq!(b.text(), "abcd");
    }

    #[test]
    fn ime_commit_writes_the_text_and_clears_the_preedit() {
        let (mut b, mut s) = setup("ab|cd");
        s.ime_preedit(&mut b, "ㄋㄧˇ", None);
        s.ime_commit(&mut b, "你");
        assert_eq!(show(&b, &s), "ab你|cd");
        assert!(s.preedit.is_none());
        s.undo(&mut b);
        assert_eq!(b.text(), "abcd", "確定的文字可以復原");
    }

    #[test]
    fn ime_typing_a_sentence_is_one_undo_step() {
        let (mut b, mut s) = setup("|");
        for word in ["你好", "，", "世界"] {
            s.ime_preedit(&mut b, "ㄅ", None);
            s.ime_commit(&mut b, word);
        }
        assert_eq!(b.text(), "你好，世界");
        s.undo(&mut b);
        assert_eq!(b.text(), "", "連續的輸入法輸入併成一步（和一般打字一樣）");
    }

    #[test]
    fn ime_empty_preedit_cancels_the_composition() {
        let (mut b, mut s) = setup("ab|");
        s.ime_preedit(&mut b, "ㄋ", None);
        s.ime_preedit(&mut b, "", None);
        assert!(s.preedit.is_none());
        assert_eq!(b.text(), "ab", "取消組字不會留下任何東西");
    }

    #[test]
    fn ime_empty_events_without_composition_do_not_touch_the_selection() {
        // 有些平台在沒組字時也會送空的 Preedit／Commit，不能因此把選取刪掉
        let (mut b, mut s) = setup("abcdef");
        s.sel = Selection { anchor: 1, head: 4 };
        s.ime_preedit(&mut b, "", None);
        s.ime_commit(&mut b, "");
        assert_eq!(b.text(), "abcdef");
        assert_eq!(s.sel, Selection { anchor: 1, head: 4 });
    }

    #[test]
    fn ime_newline_events_are_ignored() {
        let (mut b, mut s) = setup("ab|");
        s.ime_preedit(&mut b, "\n", None);
        s.ime_commit(&mut b, "\r");
        assert_eq!(b.text(), "ab");
        assert!(s.preedit.is_none());
    }

    #[test]
    fn ime_starting_a_composition_replaces_the_selection() {
        let (mut b, mut s) = setup("abcdef");
        s.sel = Selection { anchor: 1, head: 4 };
        s.ime_preedit(&mut b, "ㄋ", None);
        assert_eq!(b.text(), "aef", "開始組字時，選取的文字被取代（先刪掉）");
        assert_eq!(s.caret(), 1);
        // 組字中的後續更新不會再刪東西
        s.ime_preedit(&mut b, "ㄋㄧ", None);
        assert_eq!(b.text(), "aef");
        s.ime_commit(&mut b, "你");
        assert_eq!(show(&b, &s), "a你|ef");
    }

    #[test]
    fn ime_commit_with_selection_but_no_preedit_replaces_it() {
        // 某些平台直接送 Commit 而沒有先送 Preedit
        let (mut b, mut s) = setup("abcdef");
        s.sel = Selection { anchor: 1, head: 4 };
        s.ime_commit(&mut b, "你");
        assert_eq!(show(&b, &s), "a你|ef");
    }

    #[test]
    fn mouse_click_during_composition_interrupts_it() {
        let (mut b, mut s) = setup("ab|cd");
        s.ime_preedit(&mut b, "ㄋ", None);
        s.set_caret(&mut b, 4, false);
        assert!(s.preedit.is_none(), "組字被丟掉");
        assert!(s.interrupt_ime, "並要求輸入法也中斷");
        assert_eq!(b.text(), "abcd");
        assert_eq!(s.caret(), 4);
        // 沒有組字時點擊不會要求中斷
        s.interrupt_ime = false;
        s.set_caret(&mut b, 1, false);
        assert!(!s.interrupt_ime);
    }

    #[test]
    fn ime_delete_surrounding() {
        let (mut b, mut s) = setup("ab你好|cd");
        s.ime_delete_surrounding(&mut b, 2, 1);
        assert_eq!(show(&b, &s), "ab|d");
        s.ime_delete_surrounding(&mut b, 0, 0);
        assert_eq!(b.text(), "abd", "0 個字元不動作");
        s.ime_delete_surrounding(&mut b, 99, 99);
        assert_eq!(b.text(), "", "超出範圍會被截斷");
    }

    #[test]
    fn newline_normalization() {
        assert_eq!(normalize_newlines("a\r\nb\rc\nd"), "a\nb\nc\nd");
        assert_eq!(normalize_newlines("plain"), "plain");
    }

    #[test]
    fn ime_cursor_moved_to_the_start_is_not_mistaken_for_the_end() {
        let mut b = Buffer::from_text("");
        let mut s = EditorState::default();
        s.ime_preedit(&mut b, "ㄅㄧ", Some(2..2));
        // egui 把 Windows 上的 (0, 0) 轉成 None：上一次有明確位置，所以這是「移到最前面」
        s.ime_preedit(&mut b, "ㄅㄧ", None);
        assert_eq!(s.preedit.as_ref().unwrap().active, Some(0..0));
        // 還停在最前面時再往左：輸入法仍回報 (0, 0)，也要維持在最前面
        s.ime_preedit(&mut b, "ㄅㄧ", None);
        assert_eq!(s.preedit.as_ref().unwrap().active, Some(0..0));
        // 往右移回來
        s.ime_preedit(&mut b, "ㄅㄧ", Some(1..1));
        assert_eq!(s.preedit.as_ref().unwrap().active, Some(1..1));
    }

    #[test]
    fn ime_without_any_position_keeps_the_caret_at_the_end() {
        // 韓文輸入法等一直沒有位置的情況：維持 None（游標畫在最後）
        let mut b = Buffer::from_text("");
        let mut s = EditorState::default();
        s.ime_preedit(&mut b, "ㄱ", None);
        s.ime_preedit(&mut b, "가", None);
        assert_eq!(s.preedit.as_ref().unwrap().active, None);

    }

    /// 把 Windows 內建注音在 `FEDITOR_IME_LOG` 記錄到的事件依序餵進去，檢查推算的游標位置。
    #[test]
    fn cursor_follows_the_real_windows_ime_event_log() {
        let mut b = Buffer::from_text("");
        let mut s = EditorState::default();
        let mut step = |text: &str, active: Option<Range<usize>>, expect: usize| {
            s.ime_preedit(&mut b, text, active);
            assert_eq!(s.preedit.as_ref().unwrap().cursor, expect, "{text}");
        };
        step("ㄨ", Some(0..1), 1);
        step("ㄨㄛ", Some(1..2), 2);
        step("我", Some(0..1), 1);
        step("我ㄧ", Some(1..2), 2);
        step("我ㄧㄠ", Some(2..3), 3);
        step("我要", Some(1..2), 2);
        step("我要ㄉ", Some(2..3), 3);
        step("我要ㄉㄚ", Some(3..4), 4);
        step("我要打", Some(2..3), 3);
        step("我要打ㄗ", Some(3..4), 4);
        step("我要打字", Some(3..4), 4);
        step("我要打字", Some(3..4), 3); // 重複的事件＝按了一次左鍵：游標到這一段的前面
        // 再按左鍵：被標示的一段是「打」，新字會插在它前面
        step("我要打字", Some(2..3), 2);
        // 再往右：被標示的是「字」，新字插在它前面
        step("我要打字", Some(3..4), 3);
        // 這時打 ㄍ：插在「打」後面，游標在 ㄍ 後面（範圍回報的是後面的「字」）
        step("我要打ㄍ字", Some(4..5), 4);
        step("我要打ㄍㄟ字", Some(5..6), 5);
        step("我要打給字", Some(4..5), 4);
    }

    /// 第三份記錄：`我要去玩耍`，連按左右鍵。範圍沒變的重複事件也代表游標移動。
    #[test]
    fn cursor_toggles_on_repeated_events_and_follows_arrow_keys() {
        let mut b = Buffer::from_text("");
        let mut s = EditorState::default();
        let mut step = |text: &str, active: Option<Range<usize>>, expect: usize| {
            s.ime_preedit(&mut b, text, active);
            assert_eq!(s.preedit.as_ref().unwrap().cursor, expect, "{text}");
        };
        step("我要去玩ㄕㄨㄚ", Some(6..7), 7);
        step("我要去玩耍", Some(4..5), 5); // 轉換完成：游標在最後
        step("我要去玩耍", Some(4..5), 4); // 左：範圍沒變，游標到「耍」前面
        step("我要去玩耍", Some(3..5), 3); // 再左：被標示的是「玩耍」
        step("我要去玩耍", Some(4..5), 4); // 右
        step("我要去玩耍", Some(4..5), 5); // 再右：範圍沒變，游標回到最後面
        step("我要去玩耍", Some(4..5), 4); // 再左又回到「耍」前面
    }
    /// 第一份記錄：往左移到「打」再輸入，新字出現在「要」後面。
    #[test]
    fn cursor_when_typing_before_a_clause() {
        let mut b = Buffer::from_text("");
        let mut s = EditorState::default();
        s.ime_preedit(&mut b, "我要打字", Some(3..4));
        s.ime_preedit(&mut b, "我要打字", Some(2..3));
        assert_eq!(s.preedit.as_ref().unwrap().cursor, 2);
        s.ime_preedit(&mut b, "我要ㄍ打字", Some(3..4));
        assert_eq!(s.preedit.as_ref().unwrap().cursor, 3);
        s.ime_preedit(&mut b, "我要ㄍㄟ打字", Some(4..5));
        assert_eq!(s.preedit.as_ref().unwrap().cursor, 4);
    }

    #[test]
    fn cursor_uses_the_reported_position_when_the_ime_gives_a_plain_cursor() {
        let mut b = Buffer::from_text("");
        let mut s = EditorState::default();
        s.ime_preedit(&mut b, "ㄅㄚ", Some(2..2));
        assert_eq!(s.preedit.as_ref().unwrap().cursor, 2);
        s.ime_preedit(&mut b, "ㄅㄚ", Some(1..1));
        assert_eq!(s.preedit.as_ref().unwrap().cursor, 1);
        s.ime_preedit(&mut b, "ㄅㄚ", Some(0..0));
        assert_eq!(s.preedit.as_ref().unwrap().cursor, 0);
        // 沒有位置的輸入法（韓文）：文字有變動就在變動處後面
        let mut k = EditorState::default();
        k.ime_preedit(&mut b, "ㄱ", None);
        k.ime_preedit(&mut b, "가", None);
        assert_eq!(k.preedit.as_ref().unwrap().cursor, 1);
    }

    #[test]
    fn changed_end_handles_insertions_replacements_and_repeats() {
        assert_eq!(changed_end("", "abc"), 3);
        assert_eq!(changed_end("我要打字", "我要打ㄍ字"), 4);
        assert_eq!(changed_end("我要打ㄍㄟ字", "我要打給字"), 4);
        assert_eq!(changed_end("same", "same"), 4);
        assert_eq!(changed_end("aa", "aaa"), 3);
    }
}
