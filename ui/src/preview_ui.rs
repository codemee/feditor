//! 預覽的畫面：把 [`Rendered`] 畫出來。模型與轉換在 [`crate::preview`]。

use crate::preview::{Block, MAX_ROWS, Rendered, Span, SpanImage};
use crate::syntax::Kind as Tok;
use eframe::egui::{self, RichText};
use egui_extras::{Column, TableBuilder};
use feditor_core::i18n::t;
use feditor_core::tf;
use serde_json::Value;
use std::path::{Path, PathBuf};

/// 樹裡一個陣列或物件最多展開幾個子項（其餘只顯示「還有 N 項」）。
const TREE_LIMIT: usize = 300;
/// 圖片檔太大就不顯示。
const MAX_IMAGE_BYTES: u64 = 10 * 1024 * 1024;

/// 預覽與編輯區同步捲動用的資料：預覽每一幀把自己的捲動狀態填進來，也從這裡接收「請捲到某處」的要求。
#[derive(Default, Clone, Debug)]
pub struct ScrollSync {
    /// 要求這一幀把預覽捲到這個位置（像素，從內容頂端算起）；畫面讀走之後清掉。
    pub set_offset: Option<f32>,
    /// 預覽目前的捲動位置（像素）。只有可以同步的預覽（原文的 Markdown）才會更新。
    pub offset: f32,
    /// 預覽可視區的高度（像素）。
    pub viewport_h: f32,
    /// 最上層每個區塊頂端的位置（像素，從內容頂端算起），最後一個是整份內容的高度；空的表示這個預覽沒辦法同步。
    pub tops: Vec<f32>,
}

/// 畫出預覽內容。`base_dir` 是來源檔案所在的資料夾（Markdown 的圖片相對路徑以它為準），
/// `salt` 讓同時開著的多個預覽窗格互不干擾。
pub fn show(ui: &mut egui::Ui, rendered: &Rendered, base_dir: Option<&Path>, salt: u32, sync: &mut ScrollSync) {
    match rendered {
        Rendered::Markdown(blocks, _) => {
            let mut area = egui::ScrollArea::vertical().id_salt(("preview_md", salt)).auto_shrink([false, false]);
            if let Some(y) = sync.set_offset.take() {
                area = area.vertical_scroll_offset(y);
            }
            let mut tops = Vec::new();
            let out = area.show(ui, |ui| {
                let origin = ui.cursor().top();
                ui.add_space(4.0);
                markdown(ui, blocks, base_dir, 0, Some((&mut tops, origin)));
                tops.push(ui.cursor().top() - origin); // 最後一個：整份內容的高度
            });
            sync.offset = out.state.offset().y;
            sync.viewport_h = out.inner_rect.height();
            sync.tops = tops;
        }
        Rendered::Table { header, rows, truncated } => table(ui, header, rows, *truncated, salt),
        Rendered::Tree(value) => {
            egui::ScrollArea::both()
                .id_salt(("preview_tree", salt))
                .auto_shrink([false, false])
                .show(ui, |ui| tree(ui, value, salt));
        }
        Rendered::Svg(svg) => svg_image(ui, svg, salt),
        Rendered::Text(text) => {
            egui::ScrollArea::both()
                .id_salt(("preview_text", salt))
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let mono = mono_font(ui, 0.9);
                    ui.add(egui::Label::new(RichText::new(text).font(mono)).wrap_mode(egui::TextWrapMode::Extend));
                });
        }
        Rendered::Error(message) => {
            ui.add_space(8.0);
            ui.colored_label(ui.visuals().error_fg_color, message);
        }
    }
}

// ── Markdown ────────────────────────────────────────────────

fn markdown(ui: &mut egui::Ui, blocks: &[Block], base_dir: Option<&Path>, depth: usize, mut tops: Option<(&mut Vec<f32>, f32)>) {
    let body = egui::TextStyle::Body.resolve(ui.style()).size;
    for (i, block) in blocks.iter().enumerate() {
        // 最上層：記下每個區塊頂端的位置（相對內容頂端），同步捲動要用
        if let Some((tops, origin)) = tops.as_mut() {
            tops.push(ui.cursor().top() - *origin);
        }
        match block {
            Block::Heading(level, spans) => {
                let scale = [1.9, 1.6, 1.35, 1.2, 1.05, 1.0][usize::from(level.clamp(&1, &6) - 1)];
                ui.add_space(6.0);
                inline(ui, spans, Some(body * scale), true, base_dir);
                if *level <= 2 {
                    ui.separator();
                }
                ui.add_space(2.0);
            }
            Block::Paragraph(spans) => {
                inline(ui, spans, None, false, base_dir);
                // 清單與引用裡的段落靠緊一點
                ui.add_space(if depth == 0 { 6.0 } else { 1.0 });
            }
            Block::Code { text, .. } => {
                let fill = ui.visuals().code_bg_color;
                egui::Frame::new()
                    .fill(fill)
                    .corner_radius(4)
                    .inner_margin(8)
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        let mono = mono_font(ui, 0.9);
                        ui.add(egui::Label::new(RichText::new(text).font(mono)));
                    });
                ui.add_space(6.0);
            }
            Block::Quote(inner) => {
                let bar = ui.visuals().widgets.noninteractive.bg_stroke.color;
                let r = ui.horizontal(|ui| {
                    ui.add_space(12.0);
                    ui.vertical(|ui| markdown(ui, inner, base_dir, depth + 1, None));
                });
                let rect = r.response.rect;
                ui.painter().rect_filled(
                    egui::Rect::from_min_size(rect.min + egui::vec2(2.0, 0.0), egui::vec2(3.0, rect.height())),
                    egui::CornerRadius::same(1),
                    bar,
                );
            }
            Block::List { start, items } => {
                for (n, item) in items.iter().enumerate() {
                    ui.horizontal_top(|ui| {
                        let marker = match start {
                            Some(s) => format!("{}.", s + n as u64),
                            None => "•".to_owned(),
                        };
                        ui.add_space(8.0 + 8.0 * depth.min(3) as f32);
                        ui.label(marker);
                        ui.vertical(|ui| markdown(ui, item, base_dir, depth + 1, None));
                    });
                    ui.add_space(2.0);
                }
                ui.add_space(4.0);
            }
            Block::Table { header, rows } => {
                md_table(ui, header, rows, depth, i, base_dir);
                ui.add_space(6.0);
            }
            Block::Rule => {
                ui.separator();
            }
            Block::Image(img) => {
                show_image(ui, img, base_dir, true);
                ui.add_space(6.0);
            }
        }
    }
}

/// 一個段落的行內內容：每個樣式一段，接在一起自動換行。
fn inline(ui: &mut egui::Ui, spans: &[Span], size: Option<f32>, bold: bool, base_dir: Option<&Path>) {
    if spans.is_empty() {
        return;
    }
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        for s in spans {
            let mut rich = RichText::new(&s.text);
            if let Some(size) = size {
                rich = rich.size(size);
            }
            if bold || s.bold {
                rich = rich.strong();
            }
            if s.italic {
                rich = rich.italics();
            }
            if s.strike {
                rich = rich.strikethrough();
            }
            if s.code {
                // 標題裡的行內程式碼跟著標題的大小，其他的比內文略小
                let mono = size.map_or_else(|| mono_font(ui, 0.9), egui::FontId::monospace);
                rich = rich.font(mono).background_color(ui.visuals().code_bg_color);
            }
            if let Some(img) = &s.image {
                show_image(ui, img, base_dir, false);
                continue;
            }
            if s.kbd {
                key_cap(ui, &s.text);
                continue;
            }
            match &s.link {
                Some(url) => match classify_link(url, base_dir) {
                    // 網址、信箱：交給預設的瀏覽器（或對應的程式）
                    LinkTarget::Browser => {
                        ui.add(egui::Hyperlink::from_label_and_url(rich, url));
                    }
                    // 本機的檔案：在編輯器開新分頁（由預覽窗格處理，見 `take_clicked_link`）
                    LinkTarget::File(path) => {
                        let link = ui.add(egui::Label::new(rich.color(ui.visuals().hyperlink_color).underline()).sense(egui::Sense::click()));
                        let link = link.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(path.display().to_string());
                        if link.clicked() {
                            ui.ctx().data_mut(|d| d.insert_temp(link_click_id(), url.clone()));
                        }
                    }
                    // 同一份文件裡的段落連結（#標題）：只畫成連結的顏色
                    LinkTarget::Anchor => {
                        ui.label(rich.color(ui.visuals().hyperlink_color).underline());
                    }
                },
                None => {
                    ui.label(rich);
                }
            }
        }
    });
}

/// 預覽裡的等寬字型：大小依「內文」的字級換算（`scale` 倍），不用編輯器的字級——
/// 使用者把編輯器字級調大時，預覽裡的程式碼才不會比旁邊的內文大一截。
fn mono_font(ui: &egui::Ui, scale: f32) -> egui::FontId {
    egui::FontId::monospace(egui::TextStyle::Body.resolve(ui.style()).size * scale)
}

/// `<kbd>` 的文字：畫成一顆按鍵（小圓角框加淡底、等寬字）。
fn key_cap(ui: &mut egui::Ui, text: &str) {
    let (fill, stroke) = {
        let v = ui.visuals();
        (v.code_bg_color, egui::Stroke::new(1.0, v.widgets.noninteractive.bg_stroke.color))
    };
    ui.add_space(2.0);
    egui::Frame::new()
        .fill(fill)
        .stroke(stroke)
        .corner_radius(4)
        .inner_margin(egui::Margin { left: 5, right: 5, top: 1, bottom: 1 })
        .show(ui, |ui| {
            let mono = mono_font(ui, 0.8);
            ui.add(egui::Label::new(RichText::new(text).font(mono)).wrap_mode(egui::TextWrapMode::Extend));
        });
    ui.add_space(2.0);
}

/// 表格儲存格的內容：全部串成一段排版文字，可以在指定的寬度裡自動換行
/// （連結只畫成連結的顏色，儲存格裡不能點）。
fn cell_job(ui: &egui::Ui, spans: &[Span], bold: bool, width: f32) -> egui::text::LayoutJob {
    use egui::text::{LayoutJob, TextFormat};
    let (text_color, strong, code_bg, link_color) = {
        let v = ui.visuals();
        (v.text_color(), v.strong_text_color(), v.code_bg_color, v.hyperlink_color)
    };
    let body = egui::TextStyle::Body.resolve(ui.style());
    let mono = mono_font(ui, 0.9);
    let mut job = LayoutJob::default();
    job.wrap.max_width = width;
    for s in spans {
        let mut format = TextFormat {
            font_id: if s.code || s.kbd { mono.clone() } else { body.clone() },
            color: if bold || s.bold { strong } else { text_color },
            italics: s.italic,
            ..Default::default()
        };
        if s.strike {
            format.strikethrough = egui::Stroke::new(1.0, format.color);
        }
        if s.code || s.kbd {
            format.background = code_bg;
        }
        if s.link.is_some() {
            format.color = link_color;
            format.underline = egui::Stroke::new(1.0, link_color);
        }
        job.append(&s.text, 0.0, format);
    }
    job
}

/// Markdown 表格。每一欄的寬度依內容決定（放得下就不換行），整張表放不下才把比較寬的欄按比例縮窄，
/// 超過的部分換行；再放不下就左右捲動。
fn md_table(ui: &mut egui::Ui, header: &[Vec<Span>], rows: &[Vec<Vec<Span>>], depth: usize, index: usize, base_dir: Option<&Path>) {
    let columns = rows.iter().map(Vec::len).max().unwrap_or(0).max(header.len());
    if columns == 0 {
        return;
    }
    const PAD: f32 = 12.0;
    const MIN_COL: f32 = 64.0;
    let spacing = 16.0;
    // 每欄單行的寬度（取該欄所有儲存格最寬的）
    let font = egui::TextStyle::Body.resolve(ui.style());
    let natural_of = |ui: &egui::Ui, cell: &[Span]| -> f32 {
        let text: String = cell.iter().map(|s| s.text.replace('\n', " ")).collect();
        // 圖片佔的寬度：有寫 width 就用它，沒寫就讀圖片檔頭的真實寬度
        let images: f32 = cell.iter().filter_map(|s| s.image.as_ref()).map(|i| estimated_width(ui, i, base_dir) + 4.0).sum();
        ui.painter().layout_no_wrap(text, font.clone(), egui::Color32::WHITE).size().x + images
    };
    let mut widths: Vec<f32> = (0..columns)
        .map(|c| {
            let h = header.get(c).map_or(0.0, |cell| natural_of(ui, cell));
            let r = rows.iter().filter_map(|row| row.get(c)).map(|cell| natural_of(ui, cell)).fold(0.0, f32::max);
            h.max(r) + PAD
        })
        .collect();
    let available = ui.available_width();
    let total = widths.iter().sum::<f32>() + spacing * (columns as f32 - 1.0);
    if total > available {
        // 只縮比最小欄寬還寬的部分，窄的欄（編號、短文字）保持原樣
        let excess: f32 = widths.iter().map(|w| (w - MIN_COL).max(0.0)).sum();
        let keep = if excess > 0.0 { (1.0 - (total - available) / excess).max(0.0) } else { 1.0 };
        for w in &mut widths {
            if *w > MIN_COL {
                *w = MIN_COL + (*w - MIN_COL) * keep;
            }
        }
    }
    // 每個儲存格的內容寬度（欄寬扣掉留白）與它在列裡的 x 位置：位置由欄寬算好，和內容無關，
    // 所以某個儲存格的內容再怎麼超出，也不會把同一列後面的欄位擠歪，各列的欄位永遠對齊
    let cell_widths: Vec<f32> = widths.iter().map(|w| w - PAD).collect();
    let mut cell_x = Vec::with_capacity(columns);
    let mut x = 0.0;
    for cw in &cell_widths {
        cell_x.push(x);
        x += cw + spacing;
    }
    let row_width = cell_widths.iter().sum::<f32>() + spacing * (columns as f32 - 1.0);
    let stripe = ui.visuals().faint_bg_color;
    egui::ScrollArea::horizontal()
        .id_salt(ui.scope_id().with(("md_table_scroll", depth, index)))
        .auto_shrink([false, true])
        .show(ui, |ui| {
            // 一列：每個儲存格放在固定的位置、固定的寬度、頂端對齊，列高就是最高的那一格；
            // 底色（斑馬紋）等排完才知道高度，事後補畫
            let draw_row = |ui: &mut egui::Ui, cells: &[&[Span]], bold: bool, shaded: bool| {
                let origin = ui.cursor().min;
                let background = ui.painter().add(egui::Shape::Noop);
                let mut height = 18.0_f32;
                for (c, cw) in cell_widths.iter().enumerate() {
                    let cell = cells.get(c).copied().unwrap_or(&[]);
                    let top_left = egui::pos2(origin.x + cell_x[c], origin.y);
                    if cell.iter().any(|s| s.image.is_some()) {
                        // 有圖片的儲存格：用行內的排法（圖與字接在一起、放不下就換行），寬度固定
                        let rect = egui::Rect::from_min_size(top_left, egui::vec2(*cw, 10_000.0));
                        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(egui::Layout::top_down(egui::Align::Min)));
                        inline(&mut child, cell, None, bold, base_dir);
                        height = height.max(child.min_rect().height());
                    } else {
                        // 自己排好字（指定換行寬度）再畫在固定的位置：不靠標籤自己決定換行的位置
                        let galley = ui.painter().layout_job(cell_job(ui, cell, bold, *cw));
                        height = height.max(galley.size().y);
                        ui.painter().galley(top_left, galley, egui::Color32::WHITE);
                    }
                }
                let (row_rect, _) = ui.allocate_exact_size(egui::vec2(row_width, height), egui::Sense::hover());
                if shaded {
                    ui.painter().set(background, egui::Shape::rect_filled(row_rect.expand2(egui::vec2(4.0, 2.0)), 3.0, stripe));
                }
            };            let head: Vec<&[Span]> = header.iter().map(Vec::as_slice).collect();
            draw_row(ui, &head, true, false);
            ui.separator();
            for (n, row) in rows.iter().enumerate() {
                let cells: Vec<&[Span]> = row.iter().map(Vec::as_slice).collect();
                draw_row(ui, &cells, false, n % 2 == 0);
            }
        });
}

/// 連結點下去要做什麼。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LinkTarget {
    /// 網址、信箱等：交給預設的瀏覽器（或作業系統對應的程式）。
    Browser,
    /// 本機的檔案或資料夾：在編輯器開啟。
    File(PathBuf),
    /// 同一份文件裡的段落（`#標題`）。
    Anchor,
}

fn link_click_id() -> egui::Id {
    egui::Id::unique("preview_link_click")
}

/// 這一幀有沒有點到「要在編輯器開啟的」連結；有的話回傳連結文字，並清掉記錄。
pub fn take_clicked_link(ctx: &egui::Context) -> Option<String> {
    ctx.data_mut(|d| d.remove_temp::<String>(link_click_id()))
}

/// 百分號編碼還原（`%20` → 空白、`%E4%B8%AD` → 中）。
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = |b: u8| (b as char).to_digit(16);
        if bytes[i] == b'%' && i + 2 < bytes.len() + 0 && hex(bytes[i + 1]).is_some() && hex(bytes[i + 2]).is_some() {
            out.push((hex(bytes[i + 1]).unwrap() * 16 + hex(bytes[i + 2]).unwrap()) as u8);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// 判斷連結點下去該做什麼：`http`、`https`、`mailto` 與其他有協定的連結交給瀏覽器；`file://`、
/// 絕對路徑與相對路徑（以文件所在資料夾為準，去掉 `#段落` 與 `?查詢`）是本機檔案；只有 `#段落` 的是文件內的連結。
pub fn classify_link(url: &str, base_dir: Option<&Path>) -> LinkTarget {
    let url = url.trim();
    if url.is_empty() || url.starts_with('#') {
        return LinkTarget::Anchor;
    }
    let lower = url.to_ascii_lowercase();
    let path_text = if let Some(rest) = lower.strip_prefix("file://") {
        // file:///C:/a/b.txt → C:/a/b.txt；file:///home/a → /home/a；file://host/share 不處理（當成本機路徑的一部分）
        let rest = &url[url.len() - rest.len()..];
        let rest = rest.strip_prefix('/').filter(|r| r.chars().nth(1) == Some(':')).unwrap_or(rest);
        rest.to_owned()
    } else {
        // 協定：兩個以上的字母加冒號（排除 Windows 的磁碟機代號 C:\ 或 C:/）
        let scheme_len = url.find(':').filter(|&i| i >= 2 && url[..i].chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-')));
        if scheme_len.is_some() {
            return LinkTarget::Browser;
        }
        url.to_owned()
    };
    let without_extras = path_text.split(['#', '?']).next().unwrap_or("");
    if without_extras.is_empty() {
        return LinkTarget::Anchor;
    }
    let p = PathBuf::from(percent_decode(without_extras));
    LinkTarget::File(if p.is_absolute() { p } else { base_dir.map(|d| d.join(&p)).unwrap_or(p) })
}
/// 圖片在檔案系統上的位置：相對路徑以文件所在資料夾為準；遠端網址（`http(s)`、`data:`）不讀，回傳 `None`。
fn local_path(url: &str, base_dir: Option<&Path>) -> Option<PathBuf> {
    let lower = url.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("data:") {
        return None;
    }
    let p = PathBuf::from(url.replace("%20", " "));
    Some(if p.is_absolute() { p } else { base_dir.map(|d| d.join(&p)).unwrap_or(p) })
}

/// 圖片檔的像素寬度（只讀檔頭，結果快取；SVG 等讀不出來的回傳 `None`）。
fn pixel_width(ctx: &egui::Context, path: &Path) -> Option<f32> {
    let key = egui::Id::unique(("preview_image_width", path.to_path_buf()));
    if let Some(cached) = ctx.data(|d| d.get_temp::<Option<u32>>(key)) {
        return cached.map(|w| w as f32);
    }
    let width = image::ImageReader::open(path)
        .ok()
        .and_then(|r| r.with_guessed_format().ok())
        .and_then(|r| r.into_dimensions().ok())
        .map(|(w, _)| w);
    ctx.data_mut(|d| d.insert_temp(key, width));
    width.map(|w| w as f32)
}

/// 估計一張圖放進表格欄位時佔多寬（用來決定欄寬）：`<img>` 有寫 `width` 就用它，
/// 否則用圖片檔真實的寬度（最多 240），讀不到的先當 64。
fn estimated_width(ui: &egui::Ui, img: &SpanImage, base_dir: Option<&Path>) -> f32 {
    if let Some(w) = img.width {
        return w;
    }
    let path = local_path(img.url_for(ui.visuals().dark_mode), base_dir);
    path.and_then(|p| pixel_width(ui.ctx(), &p)).map_or(64.0, |w| w.min(240.0))
}

/// 畫一張圖片（獨立成段的或行內的）。只讀本機檔案（相對路徑以文件所在資料夾為準），不連網路；
/// `<picture>` 依目前是深色還是淺色主題挑圖（和 GitHub 一樣）；`<img>` 的 `width`／`height` 有寫就照著縮放
///（常見的 24 像素圖示），沒寫就用原本的大小。**最寬不會超過所在的版面寬度**（表格儲存格裡就是欄寬）：
/// 圖片比儲存格寬的話，那一列後面的欄位會被擠歪，各列的欄位就對不齊。讀不到的圖顯示成「[圖片] 說明」。
fn show_image(ui: &mut egui::Ui, img: &SpanImage, base_dir: Option<&Path>, block: bool) {
    let url = img.url_for(ui.visuals().dark_mode);
    let uri = local_path(url, base_dir).and_then(|p| include_file(ui.ctx(), &p));
    match uri {
        Some(uri) => {
            // 行內圖片不能用 available_width（那是這一行剩下的寬度，接近行尾時很小），要用整個版面的寬度
            let avail = if block { ui.available_width() } else { ui.max_rect().width() }.max(8.0);
            let image = egui::Image::new(uri);
            let image = match (img.width, img.height) {
                (Some(w), Some(h)) => {
                    let k = (avail / w).min(1.0);
                    image.fit_to_exact_size(egui::vec2(w * k, h * k))
                }
                (Some(w), None) => image.fit_to_exact_size(egui::vec2(w.min(avail), 4096.0)),
                (None, Some(h)) => image.fit_to_exact_size(egui::vec2(avail.min(4096.0), h)),
                (None, None) => image.max_width(avail).max_height(480.0),
            };
            ui.add(image);
        }
        None => {
            let label = if img.alt.is_empty() { url } else { img.alt.as_str() };
            ui.label(RichText::new(tf!("[圖片] {}", label)).weak());
        }
    }
}
/// 把本機圖片檔讀進 egui 的圖片快取（只讀一次），回傳可以給 `Image` 用的位址。
fn include_file(ctx: &egui::Context, path: &Path) -> Option<String> {
    let uri = format!("bytes://preview-file/{}", path.to_string_lossy().replace('\\', "/"));
    let flag = egui::Id::unique(("preview_file_loaded", &uri));
    if ctx.data(|d| d.get_temp::<bool>(flag)).is_none() {
        let meta = std::fs::metadata(path).ok()?;
        if !meta.is_file() || meta.len() > MAX_IMAGE_BYTES {
            return None;
        }
        let bytes = std::fs::read(path).ok()?;
        ctx.include_bytes(uri.clone(), bytes);
        ctx.data_mut(|d| d.insert_temp(flag, true));
    }
    Some(uri)
}

// ── 表格 ────────────────────────────────────────────────────

fn table(ui: &mut egui::Ui, header: &[String], rows: &[Vec<String>], truncated: bool, salt: u32) {
    let columns = rows.iter().map(Vec::len).max().unwrap_or(0).max(header.len());
    if columns == 0 {
        ui.label(RichText::new(t("（空的）")).weak());
        return;
    }
    egui::ScrollArea::horizontal()
        .id_salt(("preview_table_h", salt))
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let note_h = if truncated { 22.0 } else { 0.0 };
            let max_h = (ui.available_height() - note_h).max(40.0);
            TableBuilder::new(ui)
                .id_salt(("preview_table", salt))
                .striped(true)
                .resizable(true)
                .max_scroll_height(max_h)
                .columns(Column::initial(140.0).at_least(40.0).clip(true), columns)
                .header(24.0, |mut h| {
                    for i in 0..columns {
                        h.col(|ui| {
                            ui.add(egui::Label::new(RichText::new(header.get(i).map_or("", String::as_str)).strong()).truncate());
                        });
                    }
                })
                .body(|body| {
                    body.rows(22.0, rows.len(), |mut row| {
                        let r = &rows[row.index()];
                        for i in 0..columns {
                            row.col(|ui| {
                                ui.add(egui::Label::new(r.get(i).map_or("", String::as_str)).truncate());
                            });
                        }
                    });
                });
            if truncated {
                ui.label(RichText::new(tf!("只顯示前 {} 列", MAX_ROWS)).weak());
            }
        });
}

// ── 樹 ──────────────────────────────────────────────────────

fn tree(ui: &mut egui::Ui, root: &Value, salt: u32) {
    let colors = crate::theme::syntax_colors(ui.ctx(), ui.visuals().dark_mode);
    match root {
        Value::Object(_) | Value::Array(_) => children(ui, root, "", 0, salt, &colors),
        leaf => {
            leaf_row(ui, None, leaf, &colors);
        }
    }
}

fn children(ui: &mut egui::Ui, v: &Value, path: &str, depth: usize, salt: u32, colors: &[egui::Color32; 7]) {
    let mut shown = 0;
    let total;
    match v {
        Value::Object(map) => {
            total = map.len();
            for (k, c) in map.iter().take(TREE_LIMIT) {
                node(ui, k, c, &format!("{path}/{k}"), depth, salt, colors);
                shown += 1;
            }
        }
        Value::Array(items) => {
            total = items.len();
            for (i, c) in items.iter().take(TREE_LIMIT).enumerate() {
                node(ui, &i.to_string(), c, &format!("{path}/{i}"), depth, salt, colors);
                shown += 1;
            }
        }
        _ => return,
    }
    if total > shown {
        ui.label(RichText::new(tf!("…還有 {} 項", total - shown)).weak());
    }
}

fn node(ui: &mut egui::Ui, key: &str, v: &Value, path: &str, depth: usize, salt: u32, colors: &[egui::Color32; 7]) {
    match v {
        Value::Object(map) => container(ui, key, &format!("{{{}}}", map.len()), v, path, depth, salt, colors),
        Value::Array(items) => container(ui, key, &format!("[{}]", items.len()), v, path, depth, salt, colors),
        leaf => leaf_row(ui, Some(key), leaf, colors),
    }
}

#[allow(clippy::too_many_arguments)]
fn container(ui: &mut egui::Ui, key: &str, size: &str, v: &Value, path: &str, depth: usize, salt: u32, colors: &[egui::Color32; 7]) {
    let title = RichText::new(format!("{key}  {size}")).strong();
    egui::CollapsingHeader::new(title)
        .id_salt((salt, path))
        .default_open(depth < 1)
        .show(ui, |ui| children(ui, v, path, depth + 1, salt, colors));
}

fn leaf_row(ui: &mut egui::Ui, key: Option<&str>, v: &Value, colors: &[egui::Color32; 7]) {
    ui.horizontal(|ui| {
        if let Some(key) = key {
            ui.label(RichText::new(format!("{key}:")).strong());
        }
        let (text, tok) = match v {
            Value::String(s) => (clip(s), Tok::String),
            Value::Number(n) => (n.to_string(), Tok::Number),
            Value::Bool(b) => (b.to_string(), Tok::Keyword),
            _ => ("null".to_owned(), Tok::Keyword),
        };
        let mono = mono_font(ui, 0.9);
        ui.label(RichText::new(text).font(mono).color(colors[tok as usize]));
    });
}

/// 太長的字串只顯示前面一段（樹裡一行放不下整篇文章）。
fn clip(s: &str) -> String {
    let mut it = s.chars();
    let head: String = it.by_ref().take(200).collect();
    if it.next().is_some() { format!("{head}…") } else { head }
}

// ── SVG ─────────────────────────────────────────────────────

fn svg_image(ui: &mut egui::Ui, svg: &str, salt: u32) {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    svg.hash(&mut hasher);
    let uri = format!("bytes://preview-svg-{salt}-{:016x}.svg", hasher.finish());
    let ctx = ui.ctx().clone();
    let current = egui::Id::unique(("preview_svg_uri", salt));
    let previous = ctx.data(|d| d.get_temp::<String>(current));
    if previous.as_deref() != Some(uri.as_str()) {
        // 內容換了：舊的圖從快取清掉，新的放進去
        if let Some(old) = previous {
            ctx.forget_image(&old);
        }
        ctx.include_bytes(uri.clone(), svg.as_bytes().to_vec());
        ctx.data_mut(|d| d.insert_temp(current, uri.clone()));
    }
    egui::ScrollArea::both()
        .id_salt(("preview_svg", salt))
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let avail = ui.available_size();
            ui.add(egui::Image::new(uri).max_size(avail).maintain_aspect_ratio(true));
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(dir: &Path, name: &str, w: u32, h: u32) -> PathBuf {
        let path = dir.join(name);
        image::RgbaImage::new(w, h).save(&path).unwrap();
        path
    }

    #[test]
    fn image_widths_are_read_from_the_file_header_and_cached() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let slider = png(dir.path(), "slider.png", 130, 20);
        assert_eq!(pixel_width(&ctx, &slider), Some(130.0));
        std::fs::remove_file(&slider).unwrap();
        assert_eq!(pixel_width(&ctx, &slider), Some(130.0), "第二次用快取，不再讀檔");
        assert_eq!(pixel_width(&ctx, &dir.path().join("沒有這個檔.png")), None);
        std::fs::write(dir.path().join("a.svg"), "<svg/>").unwrap();
        assert_eq!(pixel_width(&ctx, &dir.path().join("a.svg")), None, "SVG 讀不出像素寬度");
    }

    #[test]
    fn table_column_width_estimates_follow_width_attributes_then_real_sizes_with_a_cap() {
        let dir = tempfile::tempdir().unwrap();
        png(dir.path(), "icon.png", 24, 24);
        png(dir.path(), "wide.png", 900, 40);
        let ctx = egui::Context::default();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            let img = |url: &str, width: Option<f32>| SpanImage { url: url.into(), width, ..Default::default() };
            let base = Some(dir.path());
            assert_eq!(estimated_width(ui, &img("icon.png", Some(16.0)), base), 16.0, "寫了 width 就用它");
            assert_eq!(estimated_width(ui, &img("icon.png", None), base), 24.0, "沒寫就用真實的寬度");
            assert_eq!(estimated_width(ui, &img("wide.png", None), base), 240.0, "太寬的最多算 240");
            assert_eq!(estimated_width(ui, &img("nope.png", None), base), 64.0, "讀不到的先當 64");
            assert_eq!(estimated_width(ui, &img("https://x.y/a.png", None), base), 64.0, "遠端圖片不讀");
        });
        output.textures_delta.clear();
    }

    #[test]
    fn links_go_to_the_browser_or_to_a_local_file_in_the_editor() {
        let base = Path::new("C:/docs");
        let kind = |u: &str| classify_link(u, Some(base));
        // 網址、信箱、其他協定：瀏覽器
        for u in ["https://example.com/a", "HTTP://EXAMPLE.COM", "mailto:me@x.org", "ftp://host/f", "tel:+886123", "vscode://file/x"] {
            assert_eq!(kind(u), LinkTarget::Browser, "{u}");
        }
        // 相對路徑：以文件所在資料夾為準；去掉 #段落 與 ?查詢，還原 %20
        assert_eq!(kind("other.md"), LinkTarget::File(base.join("other.md")));
        assert_eq!(kind("sub/a%20b.md#section"), LinkTarget::File(base.join("sub/a b.md")));
        assert_eq!(kind("./x.txt?raw=1"), LinkTarget::File(base.join("./x.txt")));
        assert_eq!(kind("%E4%B8%AD.md"), LinkTarget::File(base.join("中.md")));
        // 絕對路徑與 file:// 網址：Windows 的磁碟機代號只在 Windows 上算絕對路徑，其他系統看 / 開頭的
        if cfg!(windows) {
            assert_eq!(kind("C:/data/n.txt"), LinkTarget::File(PathBuf::from("C:/data/n.txt")));
            assert_eq!(kind("C:\\data\\n.txt"), LinkTarget::File(PathBuf::from("C:\\data\\n.txt")));
            assert_eq!(kind("file:///C:/data/a%20b.txt"), LinkTarget::File(PathBuf::from("C:/data/a b.txt")));
        } else {
            assert_eq!(kind("/etc/hosts"), LinkTarget::File(PathBuf::from("/etc/hosts")));
            assert_eq!(kind("file:///home/me/a%20b.txt"), LinkTarget::File(PathBuf::from("/home/me/a b.txt")));
        }
        // 文件內的段落連結與空連結
        assert_eq!(kind("#top"), LinkTarget::Anchor);
        assert_eq!(kind(""), LinkTarget::Anchor);
        assert_eq!(kind("#"), LinkTarget::Anchor);
        // 沒有文件所在資料夾（還沒存檔）：相對路徑原樣當成路徑
        assert_eq!(classify_link("a.md", None), LinkTarget::File(PathBuf::from("a.md")));
    }

    #[test]
    fn local_paths_resolve_against_the_document_folder_and_remote_urls_do_not() {
        let base = Path::new("C:/docs");
        assert_eq!(local_path("img/a%20b.png", Some(base)), Some(base.join("img/a b.png")));
        assert_eq!(local_path("https://example.com/a.png", Some(base)), None);
        assert_eq!(local_path("HTTP://example.com/a.png", Some(base)), None);
        assert_eq!(local_path("data:image/png;base64,AAAA", Some(base)), None);
        assert_eq!(local_path("a.png", None), Some(PathBuf::from("a.png")));
    }
}