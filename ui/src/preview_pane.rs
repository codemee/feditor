//! 預覽窗格：版面裡的一個葉子，顯示「目前有焦點的文件」的預覽。整個程式只有一個預覽窗格（所有文件共用），
//! 它和一般窗格一樣可以拖到版面的任何位置，也可以把分頁拖到它的左、右、下側把它切開。
//!
//! 預覽跟著有焦點的窗格：換窗格、換分頁、改文字都會更新。
//! 轉換（尤其是使用者的外掛）可能很慢，所以放在背景執行緒，文字改了之後停 0.3 秒才重新轉換。

use crate::panes::{self, PaneId, Side};
use crate::preview::{self, Rendered, Renderer};
use crate::{App, Document, LogKind};
use eframe::egui;
use feditor_core::i18n::t;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// 文字改了之後，停多久才重新轉換。
const DEBOUNCE: Duration = Duration::from_millis(300);

/// 預覽內容的識別：這些都沒變，就不用重新轉換。
#[derive(Clone, Debug, PartialEq)]
struct Key {
    path: Option<PathBuf>,
    revision: u64,
    signature: String,
}

struct Shown {
    key: Key,
    rendered: Rendered,
}

struct Pending {
    key: Key,
    slot: Arc<Mutex<Option<Rendered>>>,
}

pub struct PreviewPane {
    /// 預覽哪個窗格：永遠是目前有焦點的一般窗格（每個畫面更新；存工作階段時也用它）。
    pub source: PaneId,
    shown: Option<Shown>,
    pending: Option<Pending>,
    /// 文字從什麼時候開始和畫面上的預覽不一致（用來延遲轉換）。
    dirty_since: Option<Instant>,
    /// 這個窗格是尋找面板（不是預覽）：和預覽一樣不放分頁、可以拖到版面的任何位置，
    /// 但內容是搜尋面板，也不存進工作階段。
    pub search: bool,
}

impl PreviewPane {
    pub fn new(source: PaneId) -> Self {
        Self { source, shown: None, pending: None, dirty_since: None, search: false }
    }

    /// 尋找面板用的窗格。
    pub fn search_pane(source: PaneId) -> Self {
        Self { search: true, ..Self::new(source) }
    }

    /// 目前畫面上的預覽內容。
    pub fn rendered(&self) -> Option<&Rendered> {
        self.shown.as_ref().map(|s| &s.rendered)
    }
}

/// 更新預覽之後的狀態。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    /// 這個檔案沒有可用的預覽（沒存過檔、或副檔名沒有對應的渲染器）。
    NoRenderer,
    /// 還沒有任何結果（第一次轉換中）。
    Working,
    /// 有結果可以畫（可能是上一版，新的還在轉換）。
    Ready(String),
}

impl App {
    pub(crate) fn is_preview(&self, id: PaneId) -> bool {
        self.previews.contains_key(&id)
    }

    /// 一般窗格（有分頁的窗格）的數量，預覽窗格不算。
    pub(crate) fn tab_pane_count(&self) -> usize {
        self.layout.root.leaves().into_iter().filter(|id| !self.is_preview(*id)).count()
    }

    /// 預覽窗格的編號（整個程式只有一個；尋找面板的窗格不算）。
    pub(crate) fn preview_id(&self) -> Option<PaneId> {
        self.previews.iter().filter(|(_, p)| !p.search).map(|(id, _)| *id).min()
    }

    /// 尋找面板的窗格編號。
    pub(crate) fn search_pane_id(&self) -> Option<PaneId> {
        self.previews.iter().find(|(_, p)| p.search).map(|(id, _)| *id)
    }

    pub(crate) fn is_search_pane(&self, id: PaneId) -> bool {
        self.previews.get(&id).is_some_and(|p| p.search)
    }

    /// 讓版面裡有沒有尋找面板的窗格跟 `folder.open` 一致：要開就在有焦點的窗格下方切出來
    /// （之後可以拖到別的位置），要關就拿掉。
    pub(crate) fn sync_search_pane(&mut self) {
        match (self.folder.open, self.search_pane_id()) {
            (true, None) => {
                let focus = self.layout.focus;
                let rect = self.pane_rects.iter().find(|(id, _)| *id == focus).map(|(_, r)| *r);
                if rect.is_some_and(|r| !panes::can_split(r, Side::Bottom)) {
                    self.push(LogKind::Warn, t("窗格太小，放不下尋找面板"));
                    self.folder.open = false;
                    return;
                }
                let id = self.layout.alloc_id();
                if self.layout.root.split_with_ratio(focus, Side::Bottom, id, Some(0.62)) {
                    self.previews.insert(id, PreviewPane::search_pane(focus));
                } else {
                    self.folder.open = false;
                }
            }
            (false, Some(id)) => {
                self.previews.remove(&id);
                self.layout.root.remove(id);
            }
            _ => {}
        }
    }

    fn source_doc(&self, source: PaneId) -> Option<&Document> {
        if source == self.layout.focus {
            Some(&self.doc)
        } else {
            self.layout.others.get(&source).map(|s| s.active_doc_stashed())
        }
    }

    /// `near` 如果是預覽窗格，就改成版面裡第一個一般窗格（分頁只能搬進一般窗格）。
    pub(crate) fn nearest_tab_pane(&self, near: PaneId) -> PaneId {
        if !self.is_preview(near) {
            return near;
        }
        self.layout.root.leaves().into_iter().find(|id| !self.is_preview(*id)).unwrap_or(near)
    }

    /// 開始拖曳預覽窗格（拖它的標題）。
    pub(crate) fn begin_preview_drag(&mut self, id: PaneId) {
        if !self.previews.contains_key(&id) {
            return;
        }
        let name = if self.is_search_pane(id) { t("搜尋").to_owned() } else { self.doc.name() };
        self.tab_drag = Some(crate::pane_ops::TabDrag { pane: id, index: 0, label: name, preview: true });
    }

    /// 把預覽窗格 `id` 搬到一般窗格 `target` 的 `side` 側（預覽還是跟著有焦點的文件）。
    /// 目的地不是一般窗格、或來源不是預覽窗格時什麼都不做。
    pub(crate) fn move_preview(&mut self, id: PaneId, target: PaneId, side: Side) {
        if !self.is_preview(id) || self.is_preview(target) || !self.layout.others.contains_key(&target) && target != self.layout.focus {
            return;
        }
        self.layout.root.remove(id);
        if !self.layout.root.split(target, side, id) {
            self.previews.remove(&id); // 版面裡找不到目的窗格：不要留下懸空的預覽
        }
        self.save_session();
    }

    pub(crate) fn close_preview(&mut self, id: PaneId) {
        if self.previews.remove(&id).is_some() {
            self.layout.root.remove(id);
            self.preview_focused = false;
        }
    }

    /// 重新讀預覽資料夾裡的外掛（開啟設定面板、啟動時呼叫）。有問題的外掛放進輸出面板。
    pub(crate) fn refresh_preview_plugins(&mut self) {
        let (plugins, messages) = preview::load_plugins(preview::previews_dir().as_deref());
        self.preview_plugins = plugins;
        for m in messages {
            self.push(LogKind::Warn, m);
        }
    }

    /// 工具列的預覽鈕：已經有預覽窗格就關掉，沒有就在有焦點的窗格右邊開一個（整個程式只有一個預覽窗格）。
    /// 目前的文件沒有預覽也照開，預覽窗格會說明；之後切到有預覽的文件就會顯示。
    pub(crate) fn toggle_preview(&mut self) {
        let focus = self.layout.focus;
        if let Some(id) = self.preview_id() {
            self.close_preview(id);
            self.save_session();
            return;
        }
        let rect = self.pane_rects.iter().find(|(id, _)| *id == focus).map(|(_, r)| *r);
        if rect.is_some_and(|r| !panes::can_split(r, Side::Right)) {
            self.push(LogKind::Warn, t("窗格太窄，放不下預覽"));
            return;
        }
        let id = self.layout.alloc_id();
        if self.layout.root.split(focus, Side::Right, id) {
            self.previews.insert(id, PreviewPane::new(focus));
            self.save_session();
        }
    }

    /// 每個畫面呼叫一次：看來源文件有沒有變，需要的話在背景開始轉換，有結果就換上去。
    pub(crate) fn update_preview(&mut self, id: PaneId, ctx: &egui::Context) -> Status {
        // 預覽永遠是目前有焦點的文件
        let source = self.layout.focus;
        match self.previews.get_mut(&id) {
            Some(p) => p.source = source,
            None => return Status::NoRenderer,
        }
        let Some((path, revision, len)) =
            self.source_doc(source).map(|d| (d.path.clone(), d.buffer.revision(), d.buffer.len_bytes()))
        else {
            return Status::NoRenderer;
        };
        let Some(renderer) = preview::resolve(path.as_deref(), &self.preview_plugins) else {
            return Status::NoRenderer;
        };
        let key = Key { path: path.clone(), revision, signature: renderer.signature() };

        // 1. 背景轉換有結果了嗎
        let (start, wait) = {
            let Some(state) = self.previews.get_mut(&id) else { return Status::NoRenderer };
            if let Some(p) = &state.pending {
                let done = p.slot.lock().ok().and_then(|mut s| s.take());
                if let Some(rendered) = done {
                    let key = p.key.clone();
                    state.shown = Some(Shown { key, rendered });
                    state.pending = None;
                }
            }
            // 2. 畫面上（或轉換中）的預覽還是這個版本嗎
            let current = state.shown.as_ref().is_some_and(|s| s.key == key)
                || state.pending.as_ref().is_some_and(|p| p.key == key);
            if current {
                state.dirty_since = None;
                (false, None)
            } else {
                // 換了檔案或渲染器：馬上轉；只是文字改了：等使用者停下來
                let switched = state
                    .shown
                    .as_ref()
                    .is_none_or(|s| s.key.path != key.path || s.key.signature != key.signature);
                let since = *state.dirty_since.get_or_insert_with(Instant::now);
                if switched || since.elapsed() >= DEBOUNCE {
                    (true, None)
                } else {
                    (false, Some(DEBOUNCE - since.elapsed()))
                }
            }
        };

        // 3. 開始轉換
        if start {
            let text = if len > preview::MAX_BYTES { None } else { self.source_doc(source).map(|d| d.buffer.text()) };
            if let Some(state) = self.previews.get_mut(&id) {
                state.dirty_since = None;
                match text {
                    None => {
                        state.shown = Some(Shown { key: key.clone(), rendered: preview::too_big() });
                        state.pending = None;
                    }
                    Some(text) => {
                        let slot = Arc::new(Mutex::new(None));
                        state.pending = Some(Pending { key: key.clone(), slot: slot.clone() });
                        spawn_render(renderer.clone(), text, path, slot, ctx.clone());
                    }
                }
            }
        }
        if let Some(d) = wait {
            ctx.request_repaint_after(d);
        }
        if self.previews.get(&id).is_some_and(|p| p.pending.is_some()) {
            ctx.request_repaint_after(Duration::from_millis(60));
        }
        if self.previews.get(&id).is_some_and(|p| p.shown.is_some()) {
            Status::Ready(renderer.label())
        } else {
            Status::Working
        }
    }

    /// 預覽窗格的內容（標題列加預覽畫面）。
    pub(crate) fn show_preview_pane(&mut self, ui: &mut egui::Ui, id: PaneId, rect: egui::Rect) -> Option<crate::Action> {
        let ctx = ui.ctx().clone();
        let status = self.update_preview(id, &ctx);
        let source = self.previews.get(&id).map(|p| p.source)?;
        let (file_name, base_dir) = self
            .source_doc(source)
            .map(|d| (d.name(), d.path.as_ref().and_then(|p| p.parent().map(|d| d.to_path_buf()))))
            .unwrap_or_default();
        let mut action = None;

        let title = match &status {
            Status::Ready(label) => format!("{label} · {file_name}"),
            _ => file_name,
        };
        let framed = egui::Frame::new()
            .inner_margin(egui::Margin { left: 12, right: 8, top: 5, bottom: 0 })
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.set_min_height(crate::pane_ops::TAB_HEIGHT);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if crate::icons::small_button(ui, crate::Icon::ClosePane, t("關閉預覽"), true).clicked() {
                            action = Some(crate::Action::ClosePreview(id));
                        }
                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                            // 拖曳標題，可以像分頁一樣把預覽搬到別的窗格旁邊
                            let r = ui.add(
                                egui::Label::new(egui::RichText::new(&title).weak())
                                    .truncate()
                                    .selectable(false) // 拖曳時不要把標題文字反白
                                    .sense(egui::Sense::click_and_drag()),
                            );
                            let r = r
                                .on_hover_cursor(egui::CursorIcon::Grab)
                                .on_hover_text(t("拖曳標題，可以把預覽移到別的窗格旁邊"));
                            if r.drag_started() {
                                action = Some(crate::Action::BeginPreviewDrag(id));
                            }
                        });
                    });
                });
            });
        let bar_h = framed.response.rect.height();
        let inset = crate::pane_ops::CARD_INSET;
        let body = egui::Rect::from_min_max(
            egui::pos2(rect.min.x + inset, rect.min.y + bar_h),
            egui::pos2(rect.max.x - inset, rect.max.y - inset),
        );
        let fill = ui.visuals().extreme_bg_color;
        ui.painter().rect_filled(body, egui::CornerRadius::same(4), fill);
        let content = body.shrink(8.0);
        let font_size = self.settings.preview_font_size;
        ui.scope_builder(egui::UiBuilder::new().max_rect(content), |ui| {
            ui.set_clip_rect(content.intersect(ui.clip_rect()));
            scale_text_styles(ui, font_size);
            match &status {
                Status::NoRenderer => {
                    ui.label(
                        egui::RichText::new(t(
                            "這個檔案沒有預覽：內建支援 Markdown、CSV、TSV、JSON、TOML、YAML、SVG（檔案要有副檔名），也可以在預覽資料夾放外掛",
                        ))
                        .weak(),
                    );
                }
                Status::Working => {
                    ui.label(egui::RichText::new(t("正在產生預覽…")).weak());
                }
                Status::Ready(_) => {
                    if let Some(rendered) = self.previews.get(&id).and_then(PreviewPane::rendered) {
                        crate::preview_ui::show(ui, rendered, base_dir.as_deref(), id);
                    }
                    // 點了指向本機檔案的連結：在編輯器開啟（其他連結已經交給瀏覽器了）
                    if let Some(url) = crate::preview_ui::take_clicked_link(ui.ctx()) {
                        if let crate::preview_ui::LinkTarget::File(path) = crate::preview_ui::classify_link(&url, base_dir.as_deref()) {
                            action = Some(crate::Action::OpenLocalFile(path));
                        }
                    }
                }
            }
        });
        action
    }
}

/// 預覽窗格內容用的字級：內文（與按鈕）用 `body`，其他依比例（標題、等寬字、小字）。
/// 只改這個區域的樣式，編輯區和其他介面不受影響。
fn scale_text_styles(ui: &mut egui::Ui, body: f32) {
    for (style, font) in ui.style_mut().text_styles.iter_mut() {
        font.size = match style {
            egui::TextStyle::Small => body * 0.85,
            egui::TextStyle::Heading => body * 1.4,
            egui::TextStyle::Monospace => body * 0.9,
            _ => body,
        };
    }
}

/// 在背景執行緒轉換，結果放進 `slot` 並叫畫面重畫。
fn spawn_render(
    renderer: Renderer,
    text: String,
    path: Option<PathBuf>,
    slot: Arc<Mutex<Option<Rendered>>>,
    ctx: egui::Context,
) {
    std::thread::spawn(move || {
        let rendered = preview::render(&renderer, &text, path.as_deref());
        if let Ok(mut s) = slot.lock() {
            *s = Some(rendered);
        }
        ctx.request_repaint();
    });
}
