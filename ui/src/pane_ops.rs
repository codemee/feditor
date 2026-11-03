//! 窗格（分割視窗）在 App 這一側的操作與畫面。版面本身（樹、位置、放置區）在 [`crate::panes`]，
//! 分頁的搬移在 [`crate::tabs`]。
//!
//! `App` 的 `doc`、`editor`、`tabs` 永遠代表**有焦點的窗格**，所以尋找、儲存、巨集、狀態列這些
//! 功能完全不用知道有窗格。沒有焦點的窗格的分頁集放在 `layout.others`，所有分頁內容都收在裡面。
//! 換焦點就是把兩邊對調（和切換分頁同一套做法）。

use crate::editor::model::EditorState;
use crate::editor::view::EditorView;
use crate::panes::{self, PaneId, Side, Zone};
use crate::settings::ViewOptions;
use crate::editor::model::Selection;
use crate::preview_pane::PreviewPane;
use crate::session::{Session, SessionPane, SessionPreview};
use crate::tabs::{Tab, TabSet};
use std::collections::HashMap;
use crate::{Action, App, Document, Icon, LogKind, editor_id, find_input_id, flat_list, icons, language, no_hover_outline, syntax, tight_buttons};
use eframe::egui;
use feditor_core::i18n::t;
use feditor_core::tf;
use std::ops::Range;

/// 窗格卡片的圓角半徑，以及編輯畫面離卡片邊緣的距離（要夠大，編輯區的角才不會超出卡片的圓角）。
const CARD_RADIUS: u8 = 8;
pub(crate) const CARD_INSET: f32 = 4.0;
/// 分頁的高度與上緣圓角。分頁列下緣沒有留白，作用中的分頁才能和編輯區接在一起。
pub(crate) const TAB_HEIGHT: f32 = 26.0;
/// 按一下箭頭，分頁列捲動多少。
const TAB_SCROLL_STEP: f32 = 160.0;
/// 分頁列和編輯內容之間多留的高度（用編輯區底色補滿）。
const TAB_BODY_PAD: f32 = 4.0;
const TAB_RADIUS: u8 = 7;

/// 窗格分頁列上的分頁操作。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabCmd {
    Switch(usize),
    Close(usize),
    Rename(usize),
}

/// 搬動分頁的去處。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// 放進這個窗格（接在它最後面）。
    Into(PaneId),
    /// 往這個窗格的某一側切出新窗格，分頁放進去。
    Split(PaneId, Side),
}

/// 正在被拖曳的分頁。
#[derive(Clone, Debug)]
pub struct TabDrag {
    pub pane: PaneId,
    pub index: usize,
    pub label: String,
    /// 拖的是預覽窗格（`pane` 是預覽窗格的編號，`index` 沒有意義）。
    pub preview: bool,
}

impl App {
    fn set_ref(&self, id: PaneId) -> Option<&TabSet> {
        if id == self.layout.focus { Some(&self.tabs) } else { self.layout.others.get(&id) }
    }

    fn set_mut(&mut self, id: PaneId) -> Option<&mut TabSet> {
        if id == self.layout.focus { Some(&mut self.tabs) } else { self.layout.others.get_mut(&id) }
    }

    pub(crate) fn pane_tab_count(&self, id: PaneId) -> usize {
        self.set_ref(id).map_or(0, TabSet::len)
    }

    fn pane_active(&self, id: PaneId) -> usize {
        self.set_ref(id).map_or(0, TabSet::active)
    }

    /// 窗格 `id` 的第 `i` 個分頁標籤：（檔名, 完整路徑, 有沒有未儲存的修改）。
    fn pane_label(&self, id: PaneId, i: usize) -> (String, String, bool) {
        if id == self.layout.focus {
            self.tabs.label(&self.doc, i)
        } else {
            self.layout.others[&id].label_stashed(i)
        }
    }

    /// 窗格 id 的第 i 個分頁標籤要顯示的文字：檔名，加上未儲存的 ●，檔案被刪或被改時再加註。
    pub(crate) fn pane_tab_text(&self, id: PaneId, i: usize) -> String {
        use crate::document::DiskState;
        let (name, _, dirty) = self.pane_label(id, i);
        let state = if id == self.layout.focus {
            self.tabs.disk_state(Some(&self.doc), i)
        } else {
            self.layout.others[&id].disk_state(None, i)
        };
        let mut text = if dirty { format!("{name} ●") } else { name };
        match state {
            DiskState::Synced => {}
            DiskState::Missing => text.push_str(t(" （已刪除）")),
            DiskState::Stale => text.push_str(t(" （磁碟上已變更）")),
        }
        text
    }

    // ── 焦點 ────────────────────────────────────────────────

    /// 換焦點的底層動作：目前有焦點的分頁集（內容已經收回）退到 `others`，`id` 那個成為有焦點的。
    /// 舊的窗格若已經不在版面裡（剛被關掉），它的分頁集就直接丟掉。
    fn switch_focus_stashed(&mut self, id: PaneId) {
        let Some(new) = self.layout.others.remove(&id) else { return };
        let old = std::mem::replace(&mut self.tabs, new);
        let old_id = self.layout.focus;
        if self.layout.root.contains(old_id) && !old.is_empty() {
            self.layout.others.insert(old_id, old);
        }
        self.layout.focus = id;
    }

    /// 讓窗格 `id` 成為有焦點的窗格。
    pub(crate) fn focus_pane(&mut self, id: PaneId) {
        if id == self.layout.focus || !self.layout.others.contains_key(&id) {
            return;
        }
        let recording = self.tabs.stash(&mut self.doc, &mut self.editor);
        self.switch_focus_stashed(id);
        self.tabs.unstash(&mut self.doc, &mut self.editor, recording);
        self.pending_editor_focus = true;
        self.tab_changed();
        self.check_external_changes();
    }

    // ── 搬動分頁、切割與關閉窗格 ─────────────────────────────

    /// 把窗格 `from` 的第 `idx` 個分頁搬到 `target`；搬完焦點在分頁所在的窗格。
    /// 來源窗格因此空了就關掉它。分頁是「搬」不是「複製」：同一份文件不會同時出現在兩個窗格。
    pub(crate) fn move_tab(&mut self, from: PaneId, idx: usize, target: Target) {
        let Some(src_len) = self.set_ref(from).map(TabSet::len) else { return };
        if idx >= src_len {
            return;
        }
        match target {
            Target::Into(to) => {
                // 目的地要是一般窗格（預覽窗格不能放分頁）
                if to == from || self.set_ref(to).is_none() {
                    return;
                }
            }
            Target::Split(at, _) => {
                // 目的地是一般窗格或預覽窗格都行（拖到預覽窗格的側邊，就是把預覽窗格切開）。
                // 只剩一個分頁的窗格往自己的側邊切，切完來源就空了，沒有意義
                if (self.set_ref(at).is_none() && !self.is_preview(at)) || (at == from && src_len == 1) {
                    return;
                }
            }
        }
        let recording = self.tabs.stash(&mut self.doc, &mut self.editor);
        let mut tab = self.set_mut(from).expect("來源窗格存在").take_tab(idx);
        tab.editor.scroll_to_caret = true;
        let emptied = self.set_mut(from).is_some_and(|s| s.is_empty());
        let dest = match target {
            Target::Into(to) => {
                let set = self.set_mut(to).expect("目的窗格存在");
                let at = set.len();
                set.push_tab(tab, at);
                to
            }
            Target::Split(at, side) => {
                let new_id = self.layout.alloc_id();
                self.layout.root.split(at, side, new_id);
                self.layout.others.insert(new_id, TabSet::from_tab(tab));
                new_id
            }
        };
        if emptied {
            self.layout.root.remove(from);
            if from != self.layout.focus {
                self.layout.others.remove(&from);
            }
        }
        if dest != self.layout.focus {
            self.switch_focus_stashed(dest);
        }
        self.tabs.unstash(&mut self.doc, &mut self.editor, recording);
        self.pending_editor_focus = true;
        self.tab_changed();
    }

    /// 關閉窗格：它的分頁全部移到相鄰的窗格（接在後面），不會關掉任何文件，也不會丟掉未儲存的修改。
    pub(crate) fn close_pane(&mut self, id: PaneId) {
        if self.tab_pane_count() <= 1 || self.set_ref(id).is_none() {
            return;
        }
        let recording = self.tabs.stash(&mut self.doc, &mut self.editor);
        let Some(near) = self.layout.root.remove(id) else {
            self.tabs.unstash(&mut self.doc, &mut self.editor, recording);
            return;
        };
        let near = self.nearest_tab_pane(near);
        let mut gone = if id == self.layout.focus {
            std::mem::take(&mut self.tabs)
        } else {
            self.layout.others.remove(&id).expect("要關的窗格存在")
        };
        let (tabs, active) = gone.drain();
        let dest = self.set_mut(near).expect("相鄰窗格存在");
        let start = dest.len();
        for (k, mut tab) in tabs.into_iter().enumerate() {
            tab.editor.scroll_to_caret = true;
            dest.push_tab(tab, start + k);
        }
        dest.activate(start + active);
        if near != self.layout.focus {
            self.switch_focus_stashed(near);
        }
        self.tabs.unstash(&mut self.doc, &mut self.editor, recording);
        self.pending_editor_focus = true;
        self.tab_changed();
    }

    /// 有焦點的窗格最後一個分頁被關掉了（呼叫端已經處理過未儲存的修改）：整個窗格一起關掉，
    /// 焦點交給相鄰的窗格。
    pub(crate) fn remove_focused_pane(&mut self) {
        let id = self.layout.focus;
        let Some(near) = self.layout.root.remove(id) else { return };
        let near = self.nearest_tab_pane(near);
        let recording = self.editor.recording.take();
        if let Some(set) = self.layout.others.remove(&near) {
            self.tabs = set; // 原本的分頁集（只剩被關掉的那個分頁）直接丟掉
            self.layout.focus = near;
            self.tabs.unstash(&mut self.doc, &mut self.editor, recording);
        }
        self.pending_editor_focus = true;
        self.tab_changed();
    }

    // ── 跨窗格的查詢 ─────────────────────────────────────────

    /// 這個路徑開在哪個窗格的哪個分頁。
    pub(crate) fn find_open_path(&self, path: &std::path::Path) -> Option<(PaneId, usize)> {
        self.layout.root.leaves().into_iter().find_map(|id| {
            let found = if id == self.layout.focus {
                self.tabs.find_path(&self.doc, path)
            } else {
                self.layout.others.get(&id).and_then(|s| s.find_path_stashed(path))
            };
            found.map(|i| (id, i))
        })
    }

    /// 所有窗格裡有未儲存修改的分頁：（窗格, 分頁編號）。
    pub(crate) fn all_dirty(&self) -> Vec<(PaneId, usize)> {
        let mut out = Vec::new();
        for id in self.layout.root.leaves() {
            let dirty = if id == self.layout.focus {
                self.tabs.dirty_tabs(&self.doc)
            } else {
                self.layout.others.get(&id).map(TabSet::dirty_tabs_stashed).unwrap_or_default()
            };
            out.extend(dirty.into_iter().map(|i| (id, i)));
        }
        out
    }

    /// 所有窗格開著的檔案與窗格版面，用來記住工作階段。
    pub(crate) fn session_all(&self) -> Session {
        let mut panes = Vec::new();
        for id in self.layout.root.leaves() {
            if self.is_preview(id) {
                continue;
            }
            let entries = if id == self.layout.focus {
                self.tabs.session_entries(Some((&self.doc, &self.editor)))
            } else {
                self.layout.others.get(&id).map(|s| s.session_entries(None)).unwrap_or_default()
            };
            let (mut files, mut active) = (Vec::new(), 0);
            for (path, caret, is_active) in entries {
                if is_active {
                    active = files.len();
                }
                files.push((path, caret));
            }
            panes.push(SessionPane { id, active, files });
        }
        let mut previews: Vec<SessionPreview> =
            self.previews.keys().map(|id| SessionPreview { id: *id, source: self.layout.focus }).collect();
        previews.sort_by_key(|p| p.id);
        Session {
            panes,
            previews,
            layout: Some(self.layout.root.to_spec(&|id| self.is_preview(id))),
            focus: self.layout.focus,
        }
    }

    /// 還原上次的窗格版面：每個窗格開回它的檔案（已經不存在的檔案略過），沒有檔案可開的窗格
    /// 整個拿掉，預覽窗格要是失去了它預覽的窗格也拿掉。回傳是不是真的還原了版面
    ///（工作階段沒有版面、只有一個窗格、或一個檔案都開不了時回傳 `false`，由呼叫端用單一窗格的方式處理）。
    pub(crate) fn restore_session_layout(&mut self, session: &Session) -> bool {
        let Some((mut root, leaves)) = session.layout.as_deref().and_then(panes::Node::from_spec) else {
            return false;
        };
        if leaves.len() < 2 {
            return false;
        }
        // 1. 每個一般窗格的分頁
        let mut sets: HashMap<PaneId, TabSet> = HashMap::new();
        for pane in &session.panes {
            if !leaves.iter().any(|(id, preview)| *id == pane.id && !preview) {
                continue;
            }
            let mut set: Option<TabSet> = None;
            let mut active = None;
            for (i, (path, caret)) in pane.files.iter().enumerate() {
                if !path.exists() {
                    continue;
                }
                let Ok((doc, warnings)) = Document::open(path) else { continue };
                for w in warnings {
                    self.push(LogKind::Warn, w);
                }
                let mut editor = EditorState::default();
                editor.sel = Selection::collapsed((*caret).min(doc.buffer.len_chars()));
                editor.scroll_to_caret = true;
                let tab = Tab { doc, editor };
                match &mut set {
                    None => set = Some(TabSet::from_tab(tab)),
                    Some(s) => {
                        let at = s.len();
                        s.push_tab(tab, at);
                    }
                }
                if i == pane.active {
                    active = set.as_ref().map(|s| s.len() - 1);
                }
            }
            if let Some(mut s) = set {
                s.activate(active.unwrap_or(0));
                sets.insert(pane.id, s);
            }
        }
        if sets.is_empty() {
            return false;
        }
        // 2. 沒有檔案可開的窗格從版面拿掉；預覽窗格整個程式只留一個（舊版存的可能有好幾個）
        let keep_preview = leaves.iter().find(|(_, preview)| *preview).map(|(id, _)| *id);
        for (id, preview) in &leaves {
            let keep = if *preview { Some(*id) == keep_preview } else { sets.contains_key(id) };
            if !keep {
                root.remove(*id);
            }
        }
        // 3. 裝上去：有焦點的窗格的分頁集放在 `self.tabs`，其他的放進 `layout.others`
        let focus = if sets.contains_key(&session.focus) {
            session.focus
        } else {
            root.leaves().into_iter().find(|id| sets.contains_key(id)).unwrap_or(session.focus)
        };
        self.layout.reserve_ids_up_to(leaves.iter().map(|(id, _)| *id).max().unwrap_or(0));
        self.layout.root = root;
        self.layout.others.clear();
        self.previews.clear();
        let mut focus_set = None;
        for (id, set) in sets {
            if id == focus {
                focus_set = Some(set);
            } else {
                self.layout.others.insert(id, set);
            }
        }
        self.layout.focus = focus;
        if let Some(id) = keep_preview {
            self.previews.insert(id, PreviewPane::new(focus));
        }
        if let Some(set) = focus_set {
            self.tabs = set;
            self.tabs.unstash(&mut self.doc, &mut self.editor, None);
        }
        self.find.invalidate();
        true
    }
    /// 窗格的編輯畫面左邊行號區的寬度（和 `EditorView` 算的一樣：最大行號的位數加 1.5 個字元寬）。
    /// 分頁列最左邊的分頁要從行號區右邊（也就是文字區）開始，底色才和編輯區一致。
    fn gutter_width(&self, ctx: &egui::Context, pane: PaneId) -> f32 {
        if !self.settings.view.show_line_numbers {
            return 0.0;
        }
        let lines = if pane == self.layout.focus {
            self.doc.buffer.line_count()
        } else {
            self.layout.others.get(&pane).map_or(1, |s| s.active_doc_stashed().buffer.line_count())
        };
        let font = egui::TextStyle::Monospace.resolve(&ctx.global_style());
        let char_w = ctx.fonts_mut(|f| f.glyph_width(&font, '0'));
        (lines.to_string().len() as f32 + 1.5) * char_w
    }

    // ── 拖曳分頁 ─────────────────────────────────────────────

    pub(crate) fn begin_tab_drag(&mut self, pane: PaneId, index: usize) {
        if index < self.pane_tab_count(pane) {
            let label = self.pane_label(pane, index).0;
            self.tab_drag = Some(TabDrag { pane, index, label, preview: false });
        }
    }

    /// 這個放置區對正在拖的分頁有沒有意義（放回原來的窗格、或只剩一個分頁還往自己切，都沒有）。
    fn drop_is_useful(&self, drag: &TabDrag, pane: PaneId, zone: Zone) -> bool {
        if drag.preview {
            // 預覽窗格只能放到一般窗格的左、右、下側（沒有「放進」這回事，也不能放到另一個預覽窗格上）
            return !self.is_preview(pane) && matches!(zone, Zone::Split(_));
        }
        if self.is_preview(pane) {
            // 預覽窗格不收分頁；拖到它的左、右、下側就是把預覽窗格切開
            return matches!(zone, Zone::Split(_));
        }
        match zone {
            Zone::Into => pane != drag.pane,
            Zone::Split(_) => !(pane == drag.pane && self.pane_tab_count(pane) <= 1),
        }
    }

    // ── 畫面 ─────────────────────────────────────────────────

    /// 一個窗格的分頁列：中間是分頁（可拖曳）。放不下時不顯示捲軸，改成左右兩側各一個細線的 `<` `>` 箭頭捲動；
    /// 右邊有「分頁清單」下拉鈕可以直接選一個分頁，最右邊（有多個窗格時）是關閉窗格的按鈕。
    pub(crate) fn pane_tab_bar(&self, ui: &mut egui::Ui, pane: PaneId, show_close_pane: bool) -> Option<Action> {
        let mut action = None;
        let ctx = ui.ctx().clone();
        let state_id = egui::Id::unique(("tab_scroll_state", pane));
        // 上一個畫面量到的：（捲動位置, 最大捲動量, 分頁放不下, 下一個畫面要強制設定的捲動位置）
        let (prev_offset, prev_max, overflow, force) =
            ctx.data(|d| d.get_temp::<(f32, f32, bool, Option<f32>)>(state_id)).unwrap_or((0.0, 0.0, false, None));
        let count = self.pane_tab_count(pane);
        let mut scroll_to = force;
        let mut new_state = None;
        // 外面要包一層 horizontal：with_layout 會佔滿所有可用的高度，不包的話分頁列會跑到窗格正中間
        ui.horizontal(|ui| {
            // 列高先撐到分頁的高度，右邊的按鈕才會和分頁文字垂直置中
            ui.set_min_height(TAB_HEIGHT);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                tight_buttons(ui);
                if show_close_pane
                    && icons::small_button(ui, Icon::ClosePane, t("關閉窗格（裡面的分頁會移到相鄰的窗格）"), true).clicked()
                {
                    action = Some(Action::ClosePane(pane));
                }
                if count > 1 {
                    self.tab_list_button(ui, pane, &mut action);
                }
                if overflow
                    && icons::small_button(ui, Icon::ChevronRight, t("向右捲動分頁"), prev_offset < prev_max - 0.5).clicked()
                {
                    scroll_to = Some((prev_offset + TAB_SCROLL_STEP).clamp(0.0, prev_max));
                }
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    if overflow
                        && icons::small_button(ui, Icon::ChevronLeft, t("向左捲動分頁"), prev_offset > 0.5).clicked()
                    {
                        scroll_to = Some((prev_offset - TAB_SCROLL_STEP).clamp(0.0, prev_max));
                    }
                    let mut area = egui::ScrollArea::horizontal()
                        .id_salt(("tab_scroll", pane))
                        .auto_shrink([false, true])
                        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden);
                    let forced = scroll_to;
                    if let Some(x) = forced {
                        area = area.horizontal_scroll_offset(x);
                    }
                    let out = area.show(ui, |ui| {
                        ui.horizontal(|ui| {
                            tight_buttons(ui);
                            no_hover_outline(ui);
                            for i in 0..count {
                                if let Some(a) = self.tab_widget(ui, pane, i) {
                                    action = Some(a);
                                }
                            }
                        });
                    });
                    let max = (out.content_size.x - out.inner_rect.width()).max(0.0);
                    let offset = out.state.clamped_offset().x;
                    // 滑鼠滾輪（直向）在分頁列上也能左右捲：egui 自己沒動的話，下一個畫面套用
                    let mut next_force = None;
                    if ui.rect_contains_pointer(out.inner_rect) && forced.is_none() {
                        let dy = ui.input(|i| i.smooth_scroll_delta.y);
                        if dy != 0.0 && (offset - prev_offset).abs() < 0.5 {
                            next_force = Some((offset - dy).clamp(0.0, max));
                        }
                    }
                    new_state = Some((offset, max, max > 1.0, next_force));
                });
            });
        });
        if let Some(state) = new_state {
            ctx.data_mut(|d| d.insert_temp(state_id, state));
            if state.2 != overflow || state.3.is_some() {
                ctx.request_repaint(); // 箭頭要出現或消失了／滾輪要套用
            }
        }
        action
    }

    /// 分頁清單：一個向下的箭頭，點開列出這個窗格所有的分頁，直接選一個就切過去。
    fn tab_list_button(&self, ui: &mut egui::Ui, pane: PaneId, action: &mut Option<Action>) {
        let button = icons::small_button(ui, Icon::ChevronDown, t("選擇分頁"), true);
        egui::Popup::menu(&button).show(|ui| {
            no_hover_outline(ui);
            egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                flat_list(ui, |ui| {
                    for i in 0..self.pane_tab_count(pane) {
                        let text = self.pane_tab_text(pane, i);
                        if ui.selectable_label(i == self.pane_active(pane), text).clicked() {
                            *action = Some(Action::PaneTab(pane, TabCmd::Switch(i)));
                            ui.close();
                        }
                    }
                });
            });
        });
    }
    /// 一個分頁：整個是一個元件，名稱在左，× 在右側內部，滑鼠移到分頁上才畫出 ×
    ///（× 的位置一直保留，顯示與隱藏時分頁寬度不會跳動）。可以拖曳。
    fn tab_widget(&self, ui: &mut egui::Ui, pane: PaneId, i: usize) -> Option<Action> {
        let mut action = None;
        let (_, full, _) = self.pane_label(pane, i);
        let text = self.pane_tab_text(pane, i);
        let active_here = i == self.pane_active(pane);
        let font = egui::TextStyle::Button.resolve(ui.style());
        let galley = ui.painter().layout_no_wrap(text, font, egui::Color32::WHITE);
        let (pad, close_w) = (8.0, 14.0);
        let size = egui::vec2(pad + galley.size().x + 6.0 + close_w + 4.0, TAB_HEIGHT);
        let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click_and_drag());
        let close = egui::Rect::from_center_size(
            egui::pos2(rect.right() - 4.0 - close_w / 2.0, rect.center().y),
            egui::vec2(close_w, close_w),
        );
        let close_resp = ui.interact(close, ui.make_persistent_id(("tab_close", pane, i)), egui::Sense::click());
        let over = response.hovered() || close_resp.hovered();
        // 作用中的分頁換了（切換、新增、開啟）：分頁列自動捲到它，窄的窗格裡才看得到現在是哪一個
        let seen = ui.ctx().data(|d| d.get_temp::<usize>(egui::Id::unique(("tab_last_active", pane))));
        if active_here {
            if seen != Some(i) {
                ui.scroll_to_rect(rect, Some(egui::Align::Center));
            }
            ui.ctx().data_mut(|d| d.insert_temp(egui::Id::unique(("tab_last_active", pane)), i));
        }
        let (fill, text_color, hover_fill, divider, accent) = {
            let v = ui.visuals();
            (
                v.extreme_bg_color,
                if active_here {
                    v.strong_text_color()
                } else if over {
                    v.text_color()
                } else {
                    v.weak_text_color()
                },
                v.widgets.hovered.weak_bg_fill.gamma_multiply(0.7),
                v.widgets.noninteractive.bg_stroke.color,
                v.selection.bg_fill,
            )
        };
        // 活頁簿標籤：作用中的分頁和下面的編輯區同一個底色，上方兩個圓角、下緣和編輯區接在一起；
        // 其他分頁沒有色塊，只有較淡的文字，滑鼠移上去才浮出一層淡底，分頁之間用細線隔開
        if active_here {
            ui.painter().rect_filled(
                rect,
                egui::CornerRadius { nw: TAB_RADIUS, ne: TAB_RADIUS, sw: 0, se: 0 },
                fill,
            );
            // 下緣一條細的強調線（像活頁簿的工作表標籤）：深色主題裡作用中與非作用中分頁的底色差很小，
            // 靠它一眼分得出來
            // 分頁列的捲動區會把畫在邊緣的東西裁掉，所以畫在自己的圖層上
            // 但分頁被捲出可見範圍時，線也要跟著消失：裁切範圍取分頁列的可見區域（往下多留一點給這條線）
            let layer = egui::LayerId::new(egui::Order::Middle, egui::Id::unique("tab_accent"));
            let clip = ui.clip_rect();
            let line_clip = egui::Rect::from_min_max(clip.min, egui::pos2(clip.max.x, clip.max.y + 3.0));
            ui.ctx().layer_painter(layer).with_clip_rect(line_clip).rect_filled(
                egui::Rect::from_min_max(
                    egui::pos2(rect.left() + 4.0, rect.bottom() - 2.0),
                    egui::pos2(rect.right() - 4.0, rect.bottom()),
                ),
                egui::CornerRadius::same(1),
                accent,
            );
        } else {
            if over {
                ui.painter().rect_filled(rect.shrink2(egui::vec2(0.0, 2.0)), egui::CornerRadius::same(5), hover_fill);
            }
            let next_is_active = i + 1 == self.pane_active(pane);
            if !over && !next_is_active && i + 1 < self.pane_tab_count(pane) {
                let x = rect.right() + 1.0;
                let c = rect.center().y;
                ui.painter().line_segment(
                    [egui::pos2(x, c - 7.0), egui::pos2(x, c + 7.0)],
                    egui::Stroke::new(1.0, divider),
                );
            }
        }
        let text_pos = egui::pos2(rect.left() + pad, rect.center().y - galley.size().y / 2.0);
        ui.painter().galley_with_override_text_color(text_pos, galley, text_color);
        if over {
            if close_resp.hovered() {
                ui.painter().rect_filled(close, egui::CornerRadius::same(3), ui.visuals().widgets.hovered.bg_fill);
            }
            let c = close.center();
            let d = 3.0;
            let stroke = egui::Stroke::new(1.0, text_color);
            ui.painter().line_segment([c + egui::vec2(-d, -d), c + egui::vec2(d, d)], stroke);
            ui.painter().line_segment([c + egui::vec2(-d, d), c + egui::vec2(d, -d)], stroke);
        }        if close_resp.clicked() {
            action = Some(Action::PaneTab(pane, TabCmd::Close(i)));
        } else if response.double_clicked() {
            action = Some(Action::PaneTab(pane, TabCmd::Rename(i)));
        } else if response.clicked() {
            action = Some(Action::PaneTab(pane, TabCmd::Switch(i)));
        } else if response.drag_started() {
            action = Some(Action::BeginTabDrag(pane, i));
        }
        if response.middle_clicked() {
            action = Some(Action::PaneTab(pane, TabCmd::Close(i)));
        }
        if close_resp.hovered() {
            close_resp.on_hover_text(t("關閉分頁"));
        } else if self.tab_drag.is_none() {
            response.on_hover_text(tf!("{full}\n雙按重新命名檔案", full = full));
        }
        action
    }

    /// 編輯區：依版面把每個窗格畫出來（各自的分頁列加編輯畫面），中間是可以拖曳的分隔線，
    /// 拖曳分頁時畫出放置區的預覽。
    pub(crate) fn panes_ui(&mut self, ui: &mut egui::Ui) -> Option<Action> {
        let ctx = ui.ctx().clone();
        let mut action = None;
        // 整個編輯區四周留一圈空隙，窗格卡片才不會貼著視窗邊緣、工具列與狀態列
        let full = ui.available_rect_before_wrap();
        let area = egui::Rect::from_min_max(full.min + egui::vec2(6.0, 2.0), full.max - egui::vec2(6.0, 6.0));
        let arranged = self.layout.root.arrange(area, panes::MIN_PANE);
        let multi = arranged.panes.len() > 1;
        let several_tab_panes = self.tab_pane_count() > 1;
        self.pane_rects = arranged.panes.clone();

        // 滑鼠按在別的窗格上：那個窗格取得焦點（拖曳分頁時不換）
        if self.tab_drag.is_none() {
            let pressed = ctx.input(|i| if i.pointer.any_pressed() { i.pointer.interact_pos() } else { None });
            if let Some(pos) = pressed {
                if let Some(&(id, _)) = arranged.panes.iter().find(|(_, r)| r.contains(pos)) {
                    if self.is_preview(id) {
                        // 預覽窗格也能取得輸入焦點：編輯區放掉鍵盤焦點，縮放快捷鍵改調預覽的字級
                        self.preview_focused = true;
                        ctx.memory_mut(|m| m.surrender_focus(editor_id(self.layout.focus)));
                    } else {
                        self.preview_focused = false;
                        self.focus_pane(id);
                    }
                }
            }
        }
        if std::mem::take(&mut self.pending_editor_focus) {
            self.preview_focused = false; // 換了分頁、窗格：焦點回到編輯區
            // 換了窗格或分頁之後，鍵盤也要回到編輯區。點分頁時，分頁元件自己會拿到鍵盤焦點，
            // 所以只有使用者正在搜尋框打字（例如用 Ctrl+Tab 換分頁）時才不搶
            let typing_in_find = ctx.memory(|m| m.has_focus(find_input_id()));
            if !typing_in_find {
                ctx.memory_mut(|m| m.request_focus(editor_id(self.layout.focus)));
            }
        }

        let mut shapes: Vec<(PaneId, egui::Rect, f32)> = Vec::new();
        for &(id, rect) in &arranged.panes {
            let mut child = ui.new_child(
                egui::UiBuilder::new()
                    .id_salt(("pane", id))
                    .max_rect(rect)
                    .layout(egui::Layout::top_down(egui::Align::Min)),
            );
            child.set_clip_rect(rect.intersect(ui.clip_rect()));

            // 窗格是圓角卡片：底色加框線，有焦點的窗格（有多個窗格時）框線用強調色，和分頁的圓角風格一致
            // 有鍵盤焦點的窗格框線用強調色：焦點在預覽時是預覽窗格，否則是有焦點的一般窗格
            let focused = if self.is_preview(id) {
                self.preview_focused
            } else {
                multi && id == self.layout.focus && !self.preview_focused
            };
            let (card_fill, card_stroke) = {
                let v = ui.visuals();
                let stroke = if focused {
                    egui::Stroke::new(1.5, v.selection.bg_fill)
                } else {
                    egui::Stroke::new(1.0, v.widgets.noninteractive.bg_stroke.color)
                };
                (v.panel_fill, stroke)
            };
            ui.painter().rect_filled(rect, CARD_RADIUS, card_fill);
            ui.painter().rect_stroke(rect, CARD_RADIUS, card_stroke, egui::StrokeKind::Inside);

            // 預覽窗格：自己的標題列與內容，不能放分頁，也不參與焦點
            if self.is_preview(id) {
                shapes.push((id, rect, 0.0));
                if let Some(a) = self.show_preview_pane(&mut child, id, rect) {
                    action = Some(a);
                }
                continue;
            }

            let mut bar_h = 0.0;
            if multi || self.pane_tab_count(id) > 1 {
                // 最左邊的分頁對齊編輯區的文字區（行號區的右邊），不是壓在行號區上
                let left = (CARD_INSET + self.gutter_width(&ctx, id)).round().clamp(8.0, 120.0) as i8;
                let framed = egui::Frame::new()
                    .inner_margin(egui::Margin { left, right: 8, top: 5, bottom: 0 })
                    .show(&mut child, |ui| {
                        ui.set_min_width(ui.available_width());
                        self.pane_tab_bar(ui, id, several_tab_panes)
                    });
                if let Some(a) = framed.inner {
                    action = Some(a);
                }
                bar_h = framed.response.rect.height();
            }
            shapes.push((id, rect, bar_h));

            // 編輯畫面放在卡片裡面，四周留一點邊（編輯區自己的背景是小圓角）
            let inset = CARD_INSET;
            let top = rect.min.y + bar_h.max(inset);
            // 有分頁列時，編輯內容再往下挪一點：作用中分頁下緣的強調線和被捲到最上緣的那一行文字才不會疊在一起。
            // 挪出來的這一條用編輯區的底色補上（文字區那一段），分頁還是和編輯區連成一體
            let pad = if bar_h > 0.0 { TAB_BODY_PAD } else { 0.0 };
            if pad > 0.0 {
                let strip_left = rect.min.x + inset + self.gutter_width(&ctx, id);
                let fill = ui.visuals().extreme_bg_color;
                ui.painter().rect_filled(
                    egui::Rect::from_min_max(egui::pos2(strip_left, top), egui::pos2(rect.max.x - inset, top + pad + 1.0)),
                    egui::CornerRadius::ZERO,
                    fill,
                );
            }
            let body = egui::Rect::from_min_max(
                egui::pos2(rect.min.x + inset, top + pad),
                egui::pos2(rect.max.x - inset, rect.max.y - inset),
            );
            child.scope_builder(egui::UiBuilder::new().max_rect(body), |ui| {
                // 編輯畫面是拿裁切範圍當背景範圍來畫的：裁切範圍要縮到內縮之後的區域，
                // 不然背景會連卡片的框線一起蓋掉
                ui.set_clip_rect(body.intersect(ui.clip_rect()));
                let App { doc, editor, find, settings, layout, .. } = &mut *self;
                let options = settings.view;
                if id == layout.focus {
                    let reveal = std::mem::take(&mut find.reveal);
                    let highlight = find.current_chars.clone();
                    // 使用者在編輯區改了文字：標成未儲存，之前找到的位置與筆數作廢
                    if show_pane_editor(ui, id, doc, editor, options, highlight, reveal) {
                        find.invalidate();
                    }
                } else if let Some(set) = layout.others.get_mut(&id) {
                    let (d, e) = set.active_slot_mut();
                    show_pane_editor(ui, id, d, e, options, None, false);
                }
            });
        }

        // 分隔線：拖曳調整比例（不會小於窗格的最小大小）
        for d in &arranged.dividers {
            let resp = ui.interact(d.rect, egui::Id::unique(("pane_divider", d.path.clone())), egui::Sense::drag());
            let icon = match d.axis {
                panes::Axis::Horizontal => egui::CursorIcon::ResizeHorizontal,
                panes::Axis::Vertical => egui::CursorIcon::ResizeVertical,
            };
            if resp.hovered() || resp.dragged() {
                ctx.set_cursor_icon(icon);
            }
            if resp.dragged() {
                if let Some(p) = resp.interact_pointer_pos() {
                    self.layout.root.set_ratio(&d.path, d.ratio_at(p));
                }
                ctx.request_repaint();
            }
            if resp.drag_stopped() {
                self.save_session(); // 分隔線拖完了：把新的比例記起來
            }
            // 平常只是兩張卡片之間的空隙；滑鼠移上去或拖曳時，中間浮出一條強調色的短圓角條
            if resp.hovered() || resp.dragged() {
                let grip = match d.axis {
                    panes::Axis::Horizontal => egui::vec2(3.0, (d.rect.height() * 0.3).clamp(24.0, 80.0)),
                    panes::Axis::Vertical => egui::vec2((d.rect.width() * 0.3).clamp(24.0, 80.0), 3.0),
                };
                ui.painter().rect_filled(
                    egui::Rect::from_center_size(d.rect.center(), grip),
                    egui::CornerRadius::same(2),
                    ui.visuals().selection.bg_fill,
                );
            }
        }

        if let Some(a) = self.drag_overlay(&ctx, &shapes) {
            action = Some(a);
        }
        action
    }

    /// 拖曳分頁中：畫放置區預覽與跟著滑鼠的分頁名稱；放開滑鼠時回傳搬動的動作。
    fn drag_overlay(&mut self, ctx: &egui::Context, shapes: &[(PaneId, egui::Rect, f32)]) -> Option<Action> {
        let drag = self.tab_drag.clone()?;
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.tab_drag = None;
            return None;
        }
        let pos = ctx.input(|i| i.pointer.hover_pos().or(i.pointer.latest_pos()));
        let hit = pos.and_then(|p| {
            shapes.iter().find(|(_, r, _)| r.contains(p)).map(|&(id, rect, bar_h)| (id, rect, panes::drop_zone(rect, bar_h, p)))
        });
        let useful = hit.filter(|&(id, _, zone)| self.drop_is_useful(&drag, id, zone));

        ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
        let layer = egui::LayerId::new(egui::Order::Foreground, egui::Id::unique("tab_drag_overlay"));
        let painter = ctx.layer_painter(layer);
        let accent = ctx.global_style().visuals.selection.bg_fill;
        if let Some((_, rect, zone)) = useful {
            let preview = panes::preview_rect(rect, zone).shrink(2.0);
            painter.rect_filled(preview, egui::CornerRadius::same(4), accent.gamma_multiply(0.35));
            painter.rect_stroke(preview, egui::CornerRadius::same(4), egui::Stroke::new(2.0, accent), egui::StrokeKind::Inside);
        }
        if let Some(p) = pos {
            let font = egui::TextStyle::Button.resolve(&ctx.global_style());
            let galley = painter.layout_no_wrap(drag.label.clone(), font, egui::Color32::WHITE);
            let rect = egui::Rect::from_min_size(p + egui::vec2(12.0, 8.0), galley.size() + egui::vec2(16.0, 8.0));
            painter.rect_filled(rect, egui::CornerRadius::same(4), accent);
            painter.galley(rect.min + egui::vec2(8.0, 4.0), galley, egui::Color32::WHITE);
        }
        ctx.request_repaint();

        if ctx.input(|i| i.pointer.any_released()) {
            self.tab_drag = None;
            return useful.map(|(id, _, zone)| {
                if drag.preview {
                    if let Zone::Split(side) = zone {
                        return Action::MovePreview(drag.pane, id, side);
                    }
                }
                let target = match zone {
                    Zone::Into => Target::Into(id),
                    Zone::Split(side) => Target::Split(id, side),
                };
                Action::MoveTab(drag.pane, drag.index, target)
            });
        }
        None
    }
}

/// 畫一個窗格的編輯畫面。回傳使用者這一幀有沒有改了文字。
fn show_pane_editor(
    ui: &mut egui::Ui,
    pane: PaneId,
    doc: &mut Document,
    editor: &mut EditorState,
    options: ViewOptions,
    highlight: Option<Range<usize>>,
    reveal: bool,
) -> bool {
    let resolved = syntax::resolve(doc.lang, doc.path.as_deref(), &doc.buffer);
    // 行註解符號：語言有就用語言的（手動選了語言、或沒有檔名時才有意義），否則看副檔名
    editor.comment_prefix = resolved
        .and_then(|l| l.line_comment())
        .unwrap_or_else(|| language::comment_prefix(doc.path.as_deref()));
    let language = if options.syntax { resolved } else { None };
    EditorView {
        id: editor_id(pane),
        buffer: &mut doc.buffer,
        state: editor,
        highlight,
        reveal,
        options,
        language,
    }
    .show(ui);
    doc.sync()
}
