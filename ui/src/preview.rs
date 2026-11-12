//! 預覽：把文字檔轉成可以顯示的內容。這裡只做「文字 → 呈現模型」，畫面在 [`crate::preview_ui`]。
//!
//! 呈現模型（[`Rendered`]）只有幾種：Markdown 區塊、表格、樹、SVG、純文字，外加錯誤訊息。
//! 內建的預覽（Markdown、CSV／TSV、JSON／TOML／YAML、SVG）和使用者的 JavaScript 外掛
//! （設定資料夾的 `previews/*.js`）都是產生同一種模型，所以外掛能用到所有內建的呈現方式。
//!
//! 外掛檔頭用 `//` 註解宣告：
//!
//! ```text
//! // @name 存取記錄表格
//! // @extensions log, access
//! // @output table
//! function preview(text, info) { return { header: [...], rows: [[...]] }; }
//! ```
//!
//! `@output` 是 `markdown`（回傳字串）、`table`（`{ header, rows }`）、`tree`（任何 JSON 值）、
//! `svg`（回傳字串）或 `text`（回傳字串）。

use feditor_core::i18n::t;
use feditor_core::tf;
use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// 超過這個大小的檔案不預覽（解析與畫面都會變得太慢）。
pub const MAX_BYTES: usize = 5 * 1024 * 1024;
/// 表格最多顯示幾列。
pub const MAX_ROWS: usize = 5000;

// ── 呈現模型 ────────────────────────────────────────────────

/// 一段文字與它的樣式（Markdown 的行內元素）。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Span {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub strike: bool,
    pub code: bool,
    /// `<kbd>` 裡的文字（畫成按鍵的樣子）。
    pub kbd: bool,
    pub link: Option<String>,
    /// 行內的圖片（`![]()`、`<img>`、`<picture>`）；這個 span 的 `text` 是空的。
    pub image: Option<SpanImage>,
}

/// 一張圖片。`dark_url`／`light_url` 是 `<picture>` 裡依深淺色挑的來源（沒有就用 `url`），
/// `width`／`height` 是 `<img>` 上寫的大小（像素；沒寫或寫百分比就是 `None`）。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SpanImage {
    pub alt: String,
    pub url: String,
    pub dark_url: Option<String>,
    pub light_url: Option<String>,
    pub width: Option<f32>,
    pub height: Option<f32>,
}

impl SpanImage {
    /// 深色（`dark`）或淺色主題下要用哪個位址：`<picture>` 有對應的來源就用它，否則用 `url`。
    pub fn url_for(&self, dark: bool) -> &str {
        let themed = if dark { &self.dark_url } else { &self.light_url };
        themed.as_deref().unwrap_or(&self.url)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Block {
    Heading(u8, Vec<Span>),
    Paragraph(Vec<Span>),
    Code { lang: String, text: String },
    Quote(Vec<Block>),
    List { start: Option<u64>, items: Vec<Vec<Block>> },
    Table { header: Vec<Vec<Span>>, rows: Vec<Vec<Vec<Span>>> },
    Rule,
    /// 單獨成一段的圖片。
    Image(SpanImage),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Rendered {
    /// 區塊，加上最上層每個區塊在原文從第幾行開始（從 0 算）；外掛轉出來的 Markdown 不是原文，這個清單是空的。
    Markdown(Vec<Block>, Vec<usize>),
    Table { header: Vec<String>, rows: Vec<Vec<String>>, truncated: bool },
    Tree(Value),
    Svg(String),
    Text(String),
    Error(String),
}

// ── Markdown ────────────────────────────────────────────────

/// 把 Markdown 解析成區塊模型。原始 HTML 一律略過（不渲染、不執行）。
pub fn parse_markdown(text: &str) -> Vec<Block> {
    parse_markdown_lines(text).0
}

/// 同 [`parse_markdown`]，另外回傳最上層每個區塊在原文裡從第幾行開始（從 0 算，和區塊一一對應）；
/// 預覽和編輯區同步捲動時，靠它找出「畫面上這一段是原文的哪幾行」。
pub fn parse_markdown_lines(text: &str) -> (Vec<Block>, Vec<usize>) {
    let options = Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    let mut b = MdBuilder::default();
    b.containers.push(Vec::new());
    b.line_starts = std::iter::once(0).chain(text.match_indices('\n').map(|(i, _)| i + 1)).collect();
    for (event, range) in Parser::new_ext(text, options).into_offset_iter() {
        b.offset = range.start;
        b.event(event);
    }
    b.flush_paragraph();
    (b.containers.pop().unwrap_or_default(), b.starts)
}

#[derive(Default)]
struct MdBuilder {
    /// 目前開著的容器（根、引用、清單項目）各自累積的區塊。
    containers: Vec<Vec<Block>>,
    /// 開著的清單：（起始編號, 已完成的項目）。
    lists: Vec<(Option<u64>, Vec<Vec<Block>>)>,
    spans: Vec<Span>,
    bold: u32,
    italic: u32,
    strike: u32,
    link: Option<String>,
    /// 目前在幾層 `<kbd>` 裡。
    kbd: u32,
    code_block: Option<(String, String)>,
    image: Option<(String, String)>,
    table: Option<TableState>,
    /// 原始 HTML 區塊累積的文字（結束時一起解析）。
    html_block: Option<String>,
    /// 正在讀的 `<picture>`：（淺色來源, 深色來源）。
    picture: Option<(Option<String>, Option<String>)>,
    /// 正在處理的事件在原文裡的位元組位置。
    offset: usize,
    /// 原文每一行開頭的位元組位置。
    line_starts: Vec<usize>,
    /// 目前這個最上層區塊從原文的哪個位置開始（區塊結束、推進 `containers[0]` 時取走）。
    pending: Option<usize>,
    /// 最上層每個區塊的起始行，和 `containers[0]` 一一對應。
    starts: Vec<usize>,
}

#[derive(Default)]
struct TableState {
    header: Vec<Vec<Span>>,
    rows: Vec<Vec<Vec<Span>>>,
    row: Vec<Vec<Span>>,
}

impl MdBuilder {
    fn push_block(&mut self, block: Block) {
        if self.containers.len() == 1 {
            // 最上層的區塊：記下它從原文第幾行開始（一段原文拆成好幾個區塊時，後面的沿用同一個位置）
            let at = self.pending.take().unwrap_or(self.offset);
            self.starts.push(self.line_starts.partition_point(|&s| s <= at).saturating_sub(1));
        }
        if let Some(c) = self.containers.last_mut() {
            c.push(block);
        }
    }

    fn flush_paragraph(&mut self) {
        if !self.spans.is_empty() {
            let spans = std::mem::take(&mut self.spans);
            // 一段裡只有一張圖（旁邊最多是空白）：當成獨立的圖片區塊
            let images: Vec<&Span> = spans.iter().filter(|s| s.image.is_some()).collect();
            let only_blank_besides = spans.iter().all(|s| s.image.is_some() || s.text.trim().is_empty());
            if images.len() == 1 && only_blank_besides {
                if let Some(img) = images[0].image.clone() {
                    self.push_block(Block::Image(img));
                    return;
                }
            }
            self.push_block(Block::Paragraph(spans));
        }
    }

    fn span(&self, text: &str, code: bool) -> Span {
        Span {
            text: text.to_owned(),
            bold: self.bold > 0,
            italic: self.italic > 0,
            strike: self.strike > 0,
            code,
            kbd: self.kbd > 0,
            link: self.link.clone(),
            image: None,
        }
    }

    fn event(&mut self, event: Event<'_>) {
        match event {
            Event::Start(tag) => {
                let top_level = self.containers.len() == 1;
                self.start(tag);
                if top_level && self.pending.is_none() {
                    self.pending = Some(self.offset);
                }
            }
            Event::End(end) => self.end(end),
            Event::Text(s) => {
                if let Some((_, code)) = &mut self.code_block {
                    code.push_str(&s);
                } else if let Some((_, alt)) = &mut self.image {
                    alt.push_str(&s);
                } else {
                    let span = self.span(&s, false);
                    self.spans.push(span);
                }
            }
            Event::Code(s) => {
                let span = self.span(&s, true);
                self.spans.push(span);
            }
            Event::SoftBreak => {
                let span = self.span(" ", false);
                self.spans.push(span);
            }
            Event::HardBreak => {
                let span = self.span("\n", false);
                self.spans.push(span);
            }
            Event::Rule => {
                self.flush_paragraph();
                self.pending.get_or_insert(self.offset);
                self.push_block(Block::Rule);
            }
            Event::TaskListMarker(done) => {
                let span = self.span(if done { "☑ " } else { "☐ " }, false);
                self.spans.push(span);
            }
            Event::Html(s) => match &mut self.html_block {
                Some(buf) => buf.push_str(&s),
                None => self.html(&s),
            },
            Event::InlineHtml(s) => self.html(&s),
            // 註腳、數學式等：不渲染
            _ => {}
        }
    }

    /// 處理一段原始 HTML。只認得 `<kbd>`、`<picture>`／`<source>`／`<img>` 與 `<br>`，
    /// 其他標籤與 `script`／`style` 的內容一律略過；標籤上的屬性只讀圖片需要的那幾個。
    fn html(&mut self, raw: &str) {
        for piece in scan_html(raw) {
            match piece {
                HtmlPiece::Text(text) => {
                    if self.kbd > 0 && !text.is_empty() {
                        let span = self.span(&text, false);
                        self.spans.push(span);
                    }
                }
                HtmlPiece::Tag(tag) => match (tag.name.as_str(), tag.closing) {
                    ("kbd", false) => self.kbd += 1,
                    ("kbd", true) => self.kbd = self.kbd.saturating_sub(1),
                    ("br", _) => {
                        let span = self.span("\n", false);
                        self.spans.push(span);
                    }
                    ("picture", false) => self.picture = Some((None, None)),
                    ("picture", true) => self.picture = None,
                    ("source", false) => {
                        if let Some((light, dark)) = &mut self.picture {
                            let media = tag.attr("media").unwrap_or_default().to_ascii_lowercase();
                            let url = tag.attr("srcset").or_else(|| tag.attr("src")).map(first_url);
                            if let Some(url) = url.filter(|u| !u.is_empty()) {
                                if media.contains("prefers-color-scheme") && media.contains("dark") {
                                    dark.get_or_insert(url);
                                } else if media.contains("prefers-color-scheme") && media.contains("light") {
                                    light.get_or_insert(url);
                                }
                            }
                        }
                    }
                    ("img", false) => {
                        if let Some(url) = tag.attr("src").filter(|u| !u.is_empty()) {
                            let (light_url, dark_url) = self.picture.clone().unwrap_or((None, None));
                            let mut span = self.span("", false);
                            span.image = Some(SpanImage {
                                alt: tag.attr("alt").unwrap_or_default(),
                                url,
                                dark_url,
                                light_url,
                                width: tag.attr("width").and_then(|v| pixels(&v)),
                                height: tag.attr("height").and_then(|v| pixels(&v)),
                            });
                            self.spans.push(span);
                        }
                    }
                    _ => {}
                },
            }
        }
    }

    fn start(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Paragraph | Tag::Heading { .. } => self.flush_paragraph(),
            Tag::BlockQuote(_) => {
                self.flush_paragraph();
                self.containers.push(Vec::new());
            }
            Tag::CodeBlock(kind) => {
                self.flush_paragraph();
                let lang = match kind {
                    CodeBlockKind::Fenced(info) => info.split_whitespace().next().unwrap_or("").to_owned(),
                    CodeBlockKind::Indented => String::new(),
                };
                self.code_block = Some((lang, String::new()));
            }
            Tag::List(start) => {
                self.flush_paragraph();
                self.lists.push((start, Vec::new()));
            }
            Tag::HtmlBlock => {
                self.flush_paragraph();
                self.html_block = Some(String::new());
            }
            Tag::Item => self.containers.push(Vec::new()),
            Tag::Emphasis => self.italic += 1,
            Tag::Strong => self.bold += 1,
            Tag::Strikethrough => self.strike += 1,
            Tag::Link { dest_url, .. } => self.link = Some(dest_url.to_string()),
            Tag::Image { dest_url, .. } => {
                self.image = Some((dest_url.to_string(), String::new()));
            }
            Tag::Table(_) => {
                self.flush_paragraph();
                self.table = Some(TableState::default());
            }
            Tag::TableCell => self.spans.clear(),
            _ => {}
        }
    }

    fn end(&mut self, end: TagEnd) {
        match end {
            TagEnd::HtmlBlock => {
                if let Some(raw) = self.html_block.take() {
                    self.html(&raw);
                    self.flush_paragraph();
                }
            }
            TagEnd::Paragraph => self.flush_paragraph(),
            TagEnd::Heading(level) => {
                let spans = std::mem::take(&mut self.spans);
                self.push_block(Block::Heading(level as usize as u8, spans));
            }
            TagEnd::BlockQuote(_) => {
                self.flush_paragraph();
                let blocks = self.containers.pop().unwrap_or_default();
                self.push_block(Block::Quote(blocks));
            }
            TagEnd::CodeBlock => {
                if let Some((lang, text)) = self.code_block.take() {
                    self.push_block(Block::Code { lang, text: text.trim_end_matches('\n').to_owned() });
                }
            }
            TagEnd::List(_) => {
                if let Some((start, items)) = self.lists.pop() {
                    self.push_block(Block::List { start, items });
                }
            }
            TagEnd::Item => {
                self.flush_paragraph();
                let blocks = self.containers.pop().unwrap_or_default();
                if let Some((_, items)) = self.lists.last_mut() {
                    items.push(blocks);
                }
            }
            TagEnd::Emphasis => self.italic = self.italic.saturating_sub(1),
            TagEnd::Strong => self.bold = self.bold.saturating_sub(1),
            TagEnd::Strikethrough => self.strike = self.strike.saturating_sub(1),
            TagEnd::Link => self.link = None,
            TagEnd::Image => {
                if let Some((url, alt)) = self.image.take() {
                    let mut span = self.span("", false);
                    span.image = Some(SpanImage { alt, url, ..Default::default() });
                    self.spans.push(span);
                }
            }
            TagEnd::TableCell => {
                let cell = std::mem::take(&mut self.spans);
                if let Some(t) = &mut self.table {
                    t.row.push(cell);
                }
            }
            TagEnd::TableHead => {
                if let Some(t) = &mut self.table {
                    t.header = std::mem::take(&mut t.row);
                }
            }
            TagEnd::TableRow => {
                if let Some(t) = &mut self.table {
                    let row = std::mem::take(&mut t.row);
                    t.rows.push(row);
                }
            }
            TagEnd::Table => {
                if let Some(t) = self.table.take() {
                    self.push_block(Block::Table { header: t.header, rows: t.rows });
                }
            }
            _ => {}
        }
    }
}

// ── 原始 HTML（只認得幾個標籤） ──────────────────────────────

enum HtmlPiece {
    Text(String),
    Tag(HtmlTag),
}

struct HtmlTag {
    /// 小寫的標籤名稱。
    name: String,
    closing: bool,
    attrs: Vec<(String, String)>,
}

impl HtmlTag {
    fn attr(&self, name: &str) -> Option<String> {
        self.attrs.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone())
    }
}

/// 把 HTML 片段切成標籤與文字。註解、`script`／`style` 的內容直接丟掉；格式亂掉的就當文字略過，不會當掉。
fn scan_html(raw: &str) -> Vec<HtmlPiece> {
    let mut out = Vec::new();
    let mut rest = raw;
    while let Some(lt) = rest.find('<') {
        if lt > 0 {
            out.push(HtmlPiece::Text(decode_entities(&rest[..lt])));
        }
        rest = &rest[lt..];
        if let Some(after) = rest.strip_prefix("<!--") {
            rest = after.find("-->").map_or("", |e| &after[e + 3..]);
            continue;
        }
        let Some(end) = tag_end(rest) else {
            rest = ""; // 沒有結尾的 `<`：後面整段略過
            break;
        };
        let inner = &rest[1..end];
        rest = &rest[end + 1..];
        let Some(tag) = parse_tag(inner) else { continue };
        if !tag.closing && (tag.name == "script" || tag.name == "style") {
            // 內容到對應的結尾標籤為止都不要
            let close = format!("</{}", tag.name);
            let lower = rest.to_ascii_lowercase();
            rest = lower.find(&close).map_or("", |c| rest[c..].find('>').map_or("", |g| &rest[c + g + 1..]));
            continue;
        }
        out.push(HtmlPiece::Tag(tag));
    }
    if !rest.is_empty() {
        out.push(HtmlPiece::Text(decode_entities(rest)));
    }
    out
}

/// `s` 以 `<` 開頭：找到結尾 `>` 的位置（引號裡的 `>` 不算）。
fn tag_end(s: &str) -> Option<usize> {
    let mut quote: Option<char> = None;
    for (i, c) in s.char_indices().skip(1) {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(c),
            (None, '>') => return Some(i),
            _ => {}
        }
    }
    None
}

/// 解析 `<` 與 `>` 之間的內容，例如 `/kbd` 或 `img src="a.png" alt='圖'`。
fn parse_tag(inner: &str) -> Option<HtmlTag> {
    let inner = inner.trim().trim_end_matches('/').trim_end();
    let (closing, inner) = match inner.strip_prefix('/') {
        Some(rest) => (true, rest.trim_start()),
        None => (false, inner),
    };
    let name_len = inner.find(|c: char| !(c.is_ascii_alphanumeric() || c == '-')).unwrap_or(inner.len());
    let name = inner[..name_len].to_ascii_lowercase();
    if name.is_empty() || !name.starts_with(|c: char| c.is_ascii_alphabetic()) {
        return None;
    }
    let mut attrs = Vec::new();
    let mut rest = inner[name_len..].trim_start();
    while !rest.is_empty() {
        let key_len = rest.find(|c: char| c.is_whitespace() || c == '=').unwrap_or(rest.len());
        let key = rest[..key_len].to_ascii_lowercase();
        rest = rest[key_len..].trim_start();
        let mut value = String::new();
        if let Some(after) = rest.strip_prefix('=') {
            let after = after.trim_start();
            let (v, tail) = match after.chars().next() {
                Some(q @ ('"' | '\'')) => match after[1..].find(q) {
                    Some(e) => (&after[1..1 + e], &after[e + 2..]),
                    None => (&after[1..], ""),
                },
                _ => {
                    let e = after.find(char::is_whitespace).unwrap_or(after.len());
                    (&after[..e], &after[e..])
                }
            };
            value = decode_entities(v);
            rest = tail.trim_start();
        }
        if !key.is_empty() {
            attrs.push((key, value));
        }
    }
    Some(HtmlTag { name, closing, attrs })
}

fn decode_entities(s: &str) -> String {
    s.replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
}

/// `width="24"`、`"24px"` → 24；百分比或亂寫的值當作沒寫。
fn pixels(value: &str) -> Option<f32> {
    let v = value.trim().trim_end_matches("px").trim();
    v.parse::<f32>().ok().filter(|n| n.is_finite() && *n > 0.0 && *n <= 4096.0)
}

/// `srcset` 的第一個位址（`a.png 1x, b.png 2x` → `a.png`）。
fn first_url(srcset: String) -> String {
    srcset.split(',').next().unwrap_or("").split_whitespace().next().unwrap_or("").to_owned()
}
// ── CSV／TSV ────────────────────────────────────────────────

/// 解析分隔符號分隔的表格；第一列當標題列。超過 [`MAX_ROWS`] 列的部分不讀。
pub fn parse_table(text: &str, delimiter: u8) -> Rendered {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .delimiter(delimiter)
        .from_reader(text.as_bytes());
    let mut all: Vec<Vec<String>> = Vec::new();
    let mut truncated = false;
    for record in reader.records() {
        match record {
            Ok(r) => {
                if all.len() > MAX_ROWS {
                    truncated = true;
                    break;
                }
                all.push(r.iter().map(str::to_owned).collect());
            }
            Err(e) => return Rendered::Error(e.to_string()),
        }
    }
    let mut rows = all.into_iter();
    let header = rows.next().unwrap_or_default();
    Rendered::Table { header, rows: rows.collect(), truncated }
}

// ── JSON／TOML／YAML（樹） ──────────────────────────────────

/// 去掉函式庫錯誤訊息尾巴的位置說明（我們自己用行、欄重新組）。
fn strip_location(msg: &str) -> String {
    let msg = msg.split(" at line ").next().unwrap_or(msg);
    msg.lines().next().unwrap_or(msg).trim().to_owned()
}

fn at_line_column(line: usize, column: usize, msg: &str) -> Rendered {
    Rendered::Error(tf!("第 {} 行，第 {} 欄：{}", line, column, strip_location(msg)))
}

pub fn parse_json(text: &str) -> Rendered {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    match serde_json::from_str::<Value>(text) {
        Ok(v) => Rendered::Tree(v),
        Err(e) => at_line_column(e.line(), e.column(), &e.to_string()),
    }
}

/// TOML 的日期時間經由 serde 會變成帶特殊鍵的物件，換回字串。
fn fix_toml_dates(v: &mut Value) {
    match v {
        Value::Object(map) => {
            if map.len() == 1 {
                if let Some(Value::String(s)) = map.get("$__toml_private_datetime") {
                    *v = Value::String(s.clone());
                    return;
                }
            }
            for child in map.values_mut() {
                fix_toml_dates(child);
            }
        }
        Value::Array(items) => items.iter_mut().for_each(fix_toml_dates),
        _ => {}
    }
}

pub fn parse_toml(text: &str) -> Rendered {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    match toml::from_str::<Value>(text) {
        Ok(mut v) => {
            fix_toml_dates(&mut v);
            Rendered::Tree(v)
        }
        Err(e) => {
            let (line, col) = e
                .span()
                .map(|s| {
                    let before = &text[..s.start.min(text.len())];
                    let line = before.matches('\n').count() + 1;
                    let col = before.rsplit('\n').next().map_or(0, |l| l.chars().count()) + 1;
                    (line, col)
                })
                .unwrap_or((1, 1));
            at_line_column(line, col, e.message())
        }
    }
}

pub fn parse_yaml(text: &str) -> Rendered {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    match serde_yaml::from_str::<Value>(text) {
        Ok(v) => Rendered::Tree(v),
        Err(e) => match e.location() {
            Some(l) => at_line_column(l.line(), l.column(), &e.to_string()),
            None => Rendered::Error(strip_location(&e.to_string())),
        },
    }
}

// ── 外掛 ────────────────────────────────────────────────────

/// 外掛 `@output` 宣告的呈現方式。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Output {
    Markdown,
    Table,
    Tree,
    Svg,
    Text,
}

impl Output {
    fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "markdown" => Some(Output::Markdown),
            "table" => Some(Output::Table),
            "tree" => Some(Output::Tree),
            "svg" => Some(Output::Svg),
            "text" => Some(Output::Text),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Plugin {
    pub name: String,
    /// 小寫、不含點的副檔名。
    pub extensions: Vec<String>,
    pub output: Output,
    pub source: String,
    /// 檔案名稱（錯誤訊息用）。
    pub file: String,
}

/// 讀外掛檔頭（檔案開頭連續的 `//` 註解）。
pub fn parse_plugin(file: &str, source: &str) -> Result<Plugin, String> {
    let (mut name, mut extensions, mut output) = (None, Vec::new(), None);
    for line in source.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Some(comment) = line.strip_prefix("//") else { break };
        let comment = comment.trim();
        let Some((key, value)) = comment.strip_prefix('@').and_then(|c| c.split_once(char::is_whitespace)) else {
            continue;
        };
        let value = value.trim();
        match key {
            "name" => name = Some(value.to_owned()),
            "extensions" => {
                extensions = value
                    .split(',')
                    .map(|e| e.trim().trim_start_matches('.').to_ascii_lowercase())
                    .filter(|e| !e.is_empty())
                    .collect();
            }
            "output" => {
                output = Some(Output::parse(value).ok_or_else(|| {
                    tf!("@output 只能是 markdown、table、tree、svg 或 text，不是「{}」", value)
                })?);
            }
            _ => {}
        }
    }
    if extensions.is_empty() {
        return Err(t("缺少 @extensions（要處理哪些副檔名，例如 log, access）").to_owned());
    }
    let output = output.ok_or_else(|| t("缺少 @output（markdown、table、tree、svg 或 text）").to_owned())?;
    let name = name.unwrap_or_else(|| Path::new(file).file_stem().map_or_else(|| file.to_owned(), |s| s.to_string_lossy().into_owned()));
    Ok(Plugin { name, extensions, output, source: source.to_owned(), file: file.to_owned() })
}

/// 預覽外掛放的資料夾（設定資料夾下的 `previews`）。
pub fn previews_dir() -> Option<PathBuf> {
    crate::settings::Settings::default_path().and_then(|p| p.parent().map(|d| d.join("previews")))
}

/// 讀取資料夾裡所有的 `*.js` 外掛（依檔名排序）；有問題的檔案略過，原因放進回傳的訊息。
pub fn load_plugins(dir: Option<&Path>) -> (Vec<Arc<Plugin>>, Vec<String>) {
    let mut plugins = Vec::new();
    let mut messages = Vec::new();
    let Some(dir) = dir else { return (plugins, messages) };
    let Ok(entries) = std::fs::read_dir(dir) else { return (plugins, messages) };
    let mut files: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("js")))
        .collect();
    files.sort();
    for path in files {
        let label = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        match std::fs::read(&path) {
            Ok(bytes) => match parse_plugin(&label, &String::from_utf8_lossy(&bytes)) {
                Ok(p) => plugins.push(Arc::new(p)),
                Err(e) => messages.push(tf!("預覽外掛 {label} 有誤，已略過：{e}", label = label, e = e)),
            },
            Err(_) => messages.push(tf!("無法讀取預覽外掛 {label}", label = label)),
        }
    }
    (plugins, messages)
}

/// 外掛回傳的 JSON 轉成呈現模型。
fn plugin_result(output: Output, json: &str) -> Rendered {
    let Ok(value) = serde_json::from_str::<Value>(json) else {
        return Rendered::Error(t("外掛回傳的不是有效的 JSON").to_owned());
    };
    let need_string = |name: &str| Rendered::Error(tf!("{} 外掛要回傳字串", name));
    match output {
        Output::Tree => Rendered::Tree(value),
        Output::Markdown => match value {
            Value::String(s) => Rendered::Markdown(parse_markdown(&s), Vec::new()),
            _ => need_string("markdown"),
        },
        Output::Svg => match value {
            Value::String(s) => Rendered::Svg(s),
            _ => need_string("svg"),
        },
        Output::Text => match value {
            Value::String(s) => Rendered::Text(s),
            _ => need_string("text"),
        },
        Output::Table => {
            let cell = |v: &Value| match v {
                Value::Null => String::new(),
                Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            let header = value.get("header").and_then(Value::as_array);
            let rows = value.get("rows").and_then(Value::as_array);
            match (header, rows) {
                (Some(h), Some(r)) if r.iter().all(Value::is_array) => {
                    let truncated = r.len() > MAX_ROWS;
                    Rendered::Table {
                        header: h.iter().map(cell).collect(),
                        rows: r
                            .iter()
                            .take(MAX_ROWS)
                            .map(|row| row.as_array().map(|c| c.iter().map(cell).collect()).unwrap_or_default())
                            .collect(),
                        truncated,
                    }
                }
                _ => Rendered::Error(t("table 外掛要回傳 { header: [...], rows: [[...], ...] }").to_owned()),
            }
        }
    }
}

// ── 對應與執行 ──────────────────────────────────────────────

/// 內建的預覽種類。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Markdown,
    Csv,
    Tsv,
    Json,
    Toml,
    Yaml,
    Svg,
}

impl Kind {
    fn from_extension(ext: &str) -> Option<Self> {
        Some(match ext {
            "md" | "markdown" | "mdown" => Kind::Markdown,
            "csv" => Kind::Csv,
            "tsv" => Kind::Tsv,
            "json" => Kind::Json,
            "toml" => Kind::Toml,
            "yaml" | "yml" => Kind::Yaml,
            "svg" => Kind::Svg,
            _ => return None,
        })
    }

    pub fn label(self) -> &'static str {
        match self {
            Kind::Markdown => "Markdown",
            Kind::Csv => "CSV",
            Kind::Tsv => "TSV",
            Kind::Json => "JSON",
            Kind::Toml => "TOML",
            Kind::Yaml => "YAML",
            Kind::Svg => "SVG",
        }
    }
}

/// 一個檔案用哪個渲染器預覽。
#[derive(Clone, Debug)]
pub enum Renderer {
    Builtin(Kind),
    Plugin(Arc<Plugin>),
}

impl Renderer {
    /// 預覽窗格標題上顯示的名稱。
    pub fn label(&self) -> String {
        match self {
            Renderer::Builtin(k) => k.label().to_owned(),
            Renderer::Plugin(p) => p.name.clone(),
        }
    }

    /// 識別用的簽章：外掛內容改了，快取就要作廢。
    pub fn signature(&self) -> String {
        match self {
            Renderer::Builtin(k) => format!("builtin:{k:?}"),
            Renderer::Plugin(p) => format!("plugin:{}:{}", p.file, p.source.len()),
        }
    }
}

/// 依副檔名找出預覽用的渲染器：使用者的外掛優先（同一個副檔名取檔名排序在前的），沒有才用內建的。
pub fn resolve(path: Option<&Path>, plugins: &[Arc<Plugin>]) -> Option<Renderer> {
    let ext = path?.extension()?.to_string_lossy().to_ascii_lowercase();
    if let Some(p) = plugins.iter().find(|p| p.extensions.contains(&ext)) {
        return Some(Renderer::Plugin(p.clone()));
    }
    Kind::from_extension(&ext).map(Renderer::Builtin)
}

/// 檔案太大不預覽時顯示的訊息。
pub fn too_big() -> Rendered {
    Rendered::Error(tf!("檔案太大（超過 {} MB），不預覽", MAX_BYTES / 1024 / 1024))
}

/// 預覽。外掛可能跑到逾時，所以呼叫端要放在背景執行緒。
pub fn render(renderer: &Renderer, text: &str, path: Option<&Path>) -> Rendered {
    if text.len() > MAX_BYTES {
        return too_big();
    }
    match renderer {
        Renderer::Builtin(kind) => match kind {
            Kind::Markdown => {
                let (blocks, lines) = parse_markdown_lines(text);
                Rendered::Markdown(blocks, lines)
            }
            Kind::Csv => parse_table(text, b','),
            Kind::Tsv => parse_table(text, b'\t'),
            Kind::Json => parse_json(text),
            Kind::Toml => parse_toml(text),
            Kind::Yaml => parse_yaml(text),
            Kind::Svg => Rendered::Svg(text.to_owned()),
        },
        Renderer::Plugin(plugin) => {
            let path_str = path.map(|p| p.to_string_lossy().into_owned());
            let result = feditor_script_js::JsEngine::new()
                .and_then(|mut engine| engine.convert(&plugin.source, text, path_str.as_deref()));
            match result {
                Ok(json) => plugin_result(plugin.output, &json),
                Err(e) => Rendered::Error(e.to_string()),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(spans: &[Span]) -> String {
        spans.iter().map(|s| s.text.as_str()).collect()
    }

    #[test]
    fn markdown_headings_paragraphs_and_inline_styles() {
        let blocks = parse_markdown("# 標題\n\n一段 **粗** 和 *斜* 與 `code` 還有 [連結](http://x.y)。\n\n~~刪~~");
        assert_eq!(blocks.len(), 3);
        assert!(matches!(&blocks[0], Block::Heading(1, s) if plain(s) == "標題"));
        let Block::Paragraph(spans) = &blocks[1] else { panic!("{blocks:?}") };
        assert!(spans.iter().any(|s| s.bold && s.text == "粗"));
        assert!(spans.iter().any(|s| s.italic && s.text == "斜"));
        assert!(spans.iter().any(|s| s.code && s.text == "code"));
        assert!(spans.iter().any(|s| s.link.as_deref() == Some("http://x.y") && s.text == "連結"));
        let Block::Paragraph(spans) = &blocks[2] else { panic!() };
        assert!(spans[0].strike);
    }

    #[test]
    fn markdown_levels_and_rules() {
        let blocks = parse_markdown("## 二\n\n###### 六\n\n---\n");
        assert!(matches!(&blocks[0], Block::Heading(2, _)));
        assert!(matches!(&blocks[1], Block::Heading(6, _)));
        assert_eq!(blocks[2], Block::Rule);
    }

    #[test]
    fn markdown_code_blocks_keep_the_language_and_the_text() {
        let blocks = parse_markdown("```rust\nfn main() {}\n\nlet x = 1;\n```\n\n    縮排程式碼\n");
        assert_eq!(blocks[0], Block::Code { lang: "rust".into(), text: "fn main() {}\n\nlet x = 1;".into() });
        assert_eq!(blocks[1], Block::Code { lang: String::new(), text: "縮排程式碼".into() });
    }

    #[test]
    fn markdown_lists_nest_and_numbering_and_task_items() {
        let blocks = parse_markdown("- 一\n- 二\n  - 二之一\n\n3. 三\n4. 四\n\n- [x] 完成\n- [ ] 待辦\n");
        let Block::List { start: None, items } = &blocks[0] else { panic!("{blocks:?}") };
        assert_eq!(items.len(), 2);
        assert!(matches!(&items[0][0], Block::Paragraph(s) if plain(s) == "一"));
        assert!(items[1].iter().any(|b| matches!(b, Block::List { items, .. } if items.len() == 1)), "巢狀清單");
        let Block::List { start: Some(3), items } = &blocks[1] else { panic!("{blocks:?}") };
        assert_eq!(items.len(), 2);
        let Block::List { items, .. } = &blocks[2] else { panic!() };
        assert!(matches!(&items[0][0], Block::Paragraph(s) if plain(s) == "☑ 完成"));
        assert!(matches!(&items[1][0], Block::Paragraph(s) if plain(s) == "☐ 待辦"));
    }

    #[test]
    fn markdown_quotes_tables_and_images() {
        let blocks = parse_markdown("> 引用\n> 第二行\n\n| 甲 | 乙 |\n|---|---|\n| 1 | 2 |\n| 3 | 4 |\n\n![圖說](pic/a.png)\n");
        let Block::Quote(inner) = &blocks[0] else { panic!("{blocks:?}") };
        assert!(matches!(&inner[0], Block::Paragraph(s) if plain(s) == "引用 第二行"));
        let Block::Table { header, rows } = &blocks[1] else { panic!("{blocks:?}") };
        assert_eq!(header.iter().map(|c| plain(c)).collect::<Vec<_>>(), ["甲", "乙"]);
        assert_eq!(rows.len(), 2);
        assert_eq!(plain(&rows[1][1]), "4");
        assert_eq!(blocks[2], Block::Image(SpanImage { alt: "圖說".into(), url: "pic/a.png".into(), ..Default::default() }));
    }

    #[test]
    fn markdown_raw_html_is_never_rendered() {
        let blocks = parse_markdown("<script>alert(1)</script>\n\n文字 <b onclick=x>粗</b>\n\n<div>區塊</div>\n");
        let all: String = blocks
            .iter()
            .filter_map(|b| if let Block::Paragraph(s) = b { Some(plain(s)) } else { None })
            .collect();
        assert!(!all.contains("script") && !all.contains("onclick"), "{blocks:?}");
        assert!(all.contains("文字"));
    }

    #[test]
    fn kbd_tags_become_key_styled_spans_inline() {
        let blocks = parse_markdown("按 <kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<KBD>P</KBD> 開關預覽。");
        let Block::Paragraph(spans) = &blocks[0] else { panic!("{blocks:?}") };
        let keys: Vec<&str> = spans.iter().filter(|s| s.kbd).map(|s| s.text.as_str()).collect();
        assert_eq!(keys, ["Ctrl", "Shift", "P"], "{spans:?}");
        assert_eq!(plain(spans), "按 Ctrl+Shift+P 開關預覽。");
        assert!(spans.iter().filter(|s| !s.kbd).all(|s| s.text != "Ctrl"), "標籤外的字不是按鍵樣式");
        // 在粗體、表格裡也一樣
        let blocks = parse_markdown("**<kbd>Esc</kbd>**\n\n| 鍵 |\n|---|\n| <kbd>F5</kbd> |\n");
        let Block::Paragraph(spans) = &blocks[0] else { panic!() };
        assert!(spans.iter().any(|s| s.kbd && s.bold && s.text == "Esc"));
        let Block::Table { rows, .. } = &blocks[1] else { panic!("{blocks:?}") };
        assert!(rows[0][0].iter().any(|s| s.kbd && s.text == "F5"));
    }

    #[test]
    fn picture_picks_the_theme_specific_source_like_github() {
        let md = "<picture>\n  <source media=\"(prefers-color-scheme: dark)\" srcset=\"img/dark.png\">\n  <source media=\"(prefers-color-scheme: light)\" srcset=\"img/light.png 1x, img/light@2x.png 2x\">\n  <img src=\"img/fallback.png\" alt=\"標誌\">\n</picture>\n";
        let blocks = parse_markdown(md);
        let expect = SpanImage {
            alt: "標誌".into(),
            url: "img/fallback.png".into(),
            dark_url: Some("img/dark.png".into()),
            light_url: Some("img/light.png".into()),
            ..Default::default()
        };
        assert_eq!(blocks, vec![Block::Image(expect.clone())]);
        // 外面包一層 <p align="center"> 也行
        let wrapped = parse_markdown(&format!("<p align=\"center\">\n{md}</p>\n"));
        assert_eq!(wrapped, vec![Block::Image(expect.clone())]);
        assert_eq!(expect.url_for(true), "img/dark.png");
        assert_eq!(expect.url_for(false), "img/light.png");
        // 只有深色來源：淺色主題用 <img>
        let only_dark = parse_markdown("<picture><source media=\"(prefers-color-scheme: dark)\" srcset=\"d.png\"><img src=\"x.png\"></picture>");
        let Block::Image(img) = &only_dark[0] else { panic!("{only_dark:?}") };
        assert_eq!((img.url_for(false), img.url_for(true)), ("x.png", "d.png"));
    }

    #[test]
    fn images_stay_where_they_are_inline_in_paragraphs_headings_and_table_cells() {
        // 表格儲存格裡的 <picture>／<img> 留在儲存格裡，不會被搬到表格上面
        let md = "| 圖示 | 說明 |\n|---|---|\n| <picture><source media=\"(prefers-color-scheme: dark)\" srcset=\"d.png\"><img src=\"l.png\" width=\"24\" alt=\"開啟\"></picture> | 開啟檔案 |\n| ![存檔](s.png) | 存檔 |\n";
        let blocks = parse_markdown(md);
        assert_eq!(blocks.len(), 1, "只有一個表格區塊，沒有被搬出來的圖片：{blocks:?}");
        let Block::Table { rows, .. } = &blocks[0] else { panic!("{blocks:?}") };
        let img = rows[0][0].iter().find_map(|s| s.image.as_ref()).expect("儲存格裡有圖");
        assert_eq!((img.url.as_str(), img.dark_url.as_deref(), img.width), ("l.png", Some("d.png"), Some(24.0)));
        assert_eq!(plain(&rows[0][1]), "開啟檔案");
        assert_eq!(rows[1][0].iter().find_map(|s| s.image.as_ref()).map(|i| i.url.as_str()), Some("s.png"));
        // 段落中間的圖片是行內的
        let blocks = parse_markdown("前面 ![徽章](b.svg) 後面 <img src=\"c.png\" height=\"16px\">");
        let Block::Paragraph(spans) = &blocks[0] else { panic!("{blocks:?}") };
        assert_eq!(spans.iter().filter(|s| s.image.is_some()).count(), 2);
        assert!(plain(spans).contains("前面") && plain(spans).contains("後面"));
        assert_eq!(spans.iter().filter_map(|s| s.image.as_ref()).last().and_then(|i| i.height), Some(16.0));
        // 標題裡也可以
        let Block::Heading(_, spans) = &parse_markdown("# 標誌 ![x](x.png)")[0] else { panic!() };
        assert!(spans.iter().any(|s| s.image.is_some()));
        // 一排徽章（同一段裡好幾張圖）是一個段落，不是好幾個區塊
        let blocks = parse_markdown("<p>\n<img src=\"1.svg\"> <img src=\"2.svg\"> <img src=\"3.svg\">\n</p>\n");
        assert_eq!(blocks.len(), 1);
        let Block::Paragraph(spans) = &blocks[0] else { panic!("{blocks:?}") };
        assert_eq!(spans.iter().filter(|s| s.image.is_some()).count(), 3);
    }

    #[test]
    fn plain_img_and_br_tags_work_but_other_html_scripts_and_comments_do_not() {
        let blocks = parse_markdown("<img src=\"a.png\" alt=\"甲\" width=\"100\" height=\"50%\">\n");
        assert_eq!(
            blocks,
            vec![Block::Image(SpanImage { alt: "甲".into(), url: "a.png".into(), width: Some(100.0), height: None, ..Default::default() })],
            "百分比的高度當作沒寫"
        );
        let blocks = parse_markdown("第一行<br>第二行<br/>第三行");
        let Block::Paragraph(spans) = &blocks[0] else { panic!() };
        assert_eq!(plain(spans), "第一行\n第二行\n第三行");
        let blocks = parse_markdown("<div onclick=\"x()\">\n<script>alert(1)</script>\n<style>p{color:red}</style>\n<!-- 註解 <kbd>X</kbd> -->\n<a href=\"javascript:x\">連結文字</a>\n</div>\n");
        let text: String = blocks.iter().filter_map(|b| if let Block::Paragraph(s) = b { Some(plain(s)) } else { None }).collect();
        assert!(!text.contains("alert") && !text.contains("color:red") && !text.contains("註解") && !text.contains("javascript"), "{blocks:?}");
        assert!(!blocks.iter().any(|b| matches!(b, Block::Image(_))));
        // 沒有 src 的 <img>、壞掉的標籤都不會當掉
        let _ = parse_markdown("<img alt=\"沒有來源\">\n\n<picture><source srcset=\"a.png\">\n\n<kbd>沒關\n\n<img src=\"<\n\n< <> </> <1a>\n");
        // 大小的寫法
        assert_eq!(pixels("24"), Some(24.0));
        assert_eq!(pixels(" 24px "), Some(24.0));
        assert_eq!((pixels("50%"), pixels("abc"), pixels("-3"), pixels("0"), pixels("99999")), (None, None, None, None, None));
    }
    #[test]
    fn markdown_blocks_know_which_source_line_they_start_on() {
        let md = "# 標題\n\n第一段\n第二行\n\n- 一\n- 二\n\n```rust\nfn main() {}\n```\n\n---\n\n| 甲 | 乙 |\n|---|---|\n| 1 | 2 |\n\n> 引用\n\n最後一段";
        let (blocks, lines) = parse_markdown_lines(md);
        assert_eq!(blocks.len(), lines.len(), "每個最上層區塊都有起始行：{blocks:?} {lines:?}");
        assert_eq!(lines, vec![0, 2, 5, 8, 12, 14, 18, 20], "{blocks:?}");
        assert!(parse_markdown_lines("").1.is_empty());
        // 一段原文拆成好幾個區塊（原始 HTML）也要一一對應，不能少
        let (blocks, lines) = parse_markdown_lines("前\n\n<p>\n<img src=\"a.png\">\n</p>\n\n後");
        assert_eq!(blocks.len(), lines.len(), "{blocks:?} {lines:?}");
        assert!(lines.windows(2).all(|w| w[0] <= w[1]), "起始行由小到大：{lines:?}");
    }

    #[test]
    fn markdown_handles_empty_and_odd_input() {
        assert!(parse_markdown("").is_empty());
        let _ = parse_markdown("*未結束的強調 [壞連結](\n\n```\n未結束的程式碼");
    }

    #[test]
    fn csv_and_tsv_become_tables_with_the_first_row_as_the_header() {
        let r = parse_table("名稱,數量\n蘋果,3\n\"香,蕉\",5\n", b',');
        let Rendered::Table { header, rows, truncated } = r else { panic!() };
        assert_eq!(header, ["名稱", "數量"]);
        assert_eq!(rows, [["蘋果", "3"], ["香,蕉", "5"]]);
        assert!(!truncated);
        let Rendered::Table { header, rows, .. } = parse_table("a\tb\n1\t2\t3\n", b'\t') else { panic!() };
        assert_eq!(header, ["a", "b"]);
        assert_eq!(rows[0].len(), 3, "欄數不一致也能讀（flexible）");
        let Rendered::Table { header, rows, .. } = parse_table("\u{feff}x,y\n1,2", b',') else { panic!() };
        assert_eq!(header, ["x", "y"], "BOM 要去掉");
        assert_eq!(rows.len(), 1);
        let Rendered::Table { header, rows, .. } = parse_table("", b',') else { panic!() };
        assert!(header.is_empty() && rows.is_empty());
    }

    #[test]
    fn big_tables_are_cut_at_the_row_limit() {
        let text: String = (0..MAX_ROWS + 50).map(|i| format!("{i},x\n")).collect();
        let Rendered::Table { rows, truncated, .. } = parse_table(&text, b',') else { panic!() };
        assert!(truncated);
        assert_eq!(rows.len(), MAX_ROWS);
    }

    #[test]
    fn json_toml_and_yaml_become_trees_and_errors_say_where() {
        let Rendered::Tree(v) = parse_json(r#"{"b": [1, 2], "a": {"x": null}}"#) else { panic!() };
        assert_eq!(v["b"][1], 2);
        assert_eq!(v.as_object().unwrap().keys().collect::<Vec<_>>(), ["b", "a"], "保留原本的鍵順序");
        let Rendered::Error(e) = parse_json("{\n  \"a\": 1,\n  oops\n}") else { panic!() };
        assert!(e.contains("第 3 行"), "{e}");
        let Rendered::Tree(v) = parse_toml("title = \"T\"\nwhen = 1979-05-27T07:32:00Z\n[owner]\nname = \"我\"\n") else { panic!() };
        assert_eq!(v["owner"]["name"], "我");
        assert_eq!(v["when"], "1979-05-27T07:32:00Z", "日期時間轉成字串");
        let Rendered::Error(e) = parse_toml("a = 1\nb = \n") else { panic!() };
        assert!(e.contains("第 2 行"), "{e}");
        let Rendered::Tree(v) = parse_yaml("name: 我\nlist:\n  - 1\n  - two\n") else { panic!() };
        assert_eq!(v["list"][1], "two");
        assert!(matches!(parse_yaml("a: [1, 2"), Rendered::Error(_)));
    }

    const LOG_PLUGIN: &str = "// @name 記錄表\n// @extensions .LOG, access\n// @output table\n\nfunction preview(text) {\n  return { header: ['行', '長度'], rows: text.split('\\n').filter(l => l).map(l => [l, l.length]) };\n}\n";

    #[test]
    fn plugin_headers_are_parsed_and_validated() {
        let p = parse_plugin("log.js", LOG_PLUGIN).unwrap();
        assert_eq!((p.name.as_str(), p.output), ("記錄表", Output::Table));
        assert_eq!(p.extensions, ["log", "access"], "小寫、去掉點");
        let p = parse_plugin("無名.js", "// @extensions x\n// @output text\n").unwrap();
        assert_eq!(p.name, "無名", "沒寫 @name 就用檔名");
        assert!(parse_plugin("a.js", "// @output text\n").unwrap_err().contains("@extensions"));
        assert!(parse_plugin("a.js", "// @extensions x\n").unwrap_err().contains("@output"));
        assert!(parse_plugin("a.js", "// @extensions x\n// @output pdf\n").unwrap_err().contains("pdf"));
        // 檔頭只看開頭連續的註解：後面的 // @output 不算
        assert!(parse_plugin("a.js", "// @extensions x\nvar a;\n// @output text\n").is_err());
    }

    #[test]
    fn plugins_run_in_the_sandbox_and_their_results_become_the_declared_model() {
        let plugin = Arc::new(parse_plugin("log.js", LOG_PLUGIN).unwrap());
        let r = render(&Renderer::Plugin(plugin), "ab\ncde\n", Some(Path::new("x.log")));
        let Rendered::Table { header, rows, .. } = r else { panic!("{r:?}") };
        assert_eq!(header, ["行", "長度"]);
        assert_eq!(rows, [["ab", "2"], ["cde", "3"]], "數字轉成字串");
        let md = Arc::new(parse_plugin("m.js", "// @extensions x\n// @output markdown\nfunction preview(t) { return '# ' + t; }").unwrap());
        assert!(matches!(render(&Renderer::Plugin(md), "標題", None), Rendered::Markdown(b, _) if matches!(&b[0], Block::Heading(1, _))));
        let tree = Arc::new(parse_plugin("t.js", "// @extensions x\n// @output tree\nfunction preview(t) { return { n: t.length }; }").unwrap());
        assert!(matches!(render(&Renderer::Plugin(tree), "abc", None), Rendered::Tree(v) if v["n"] == 3));
        let svg = Arc::new(parse_plugin("s.js", "// @extensions x\n// @output svg\nfunction preview(t) { return '<svg/>'; }").unwrap());
        assert_eq!(render(&Renderer::Plugin(svg), "", None), Rendered::Svg("<svg/>".into()));
    }

    #[test]
    fn plugin_mistakes_become_error_messages_not_crashes() {
        let run = |header: &str, body: &str| {
            let p = Arc::new(parse_plugin("p.js", &format!("// @extensions x\n// @output {header}\n{body}")).unwrap());
            render(&Renderer::Plugin(p), "t", None)
        };
        assert!(matches!(run("table", "function preview() { return 5; }"), Rendered::Error(_)));
        assert!(matches!(run("table", "function preview() { return { header: [], rows: [1] }; }"), Rendered::Error(_)));
        assert!(matches!(run("markdown", "function preview() { return 5; }"), Rendered::Error(_)));
        assert!(matches!(run("text", "function preview() { throw new Error('壞'); }"), Rendered::Error(e) if e.contains('壞')));
        assert!(matches!(run("text", "var x = ;"), Rendered::Error(_)), "語法錯誤");
    }

    #[test]
    fn extensions_pick_the_renderer_and_plugins_win_over_builtins() {
        let log = Arc::new(parse_plugin("log.js", LOG_PLUGIN).unwrap());
        let md_plugin = Arc::new(parse_plugin("md.js", "// @extensions md\n// @output text\nfunction preview(t){return t}").unwrap());
        let plugins = vec![log, md_plugin];
        let label = |p: &str| resolve(Some(Path::new(p)), &plugins).map(|r| r.label());
        assert_eq!(label("a.LOG").as_deref(), Some("記錄表"));
        assert_eq!(label("README.md").as_deref(), Some("md"), "使用者的外掛蓋過內建的 Markdown");
        assert_eq!(label("a.json").as_deref(), Some("JSON"));
        assert_eq!(label("a.YML").as_deref(), Some("YAML"));
        assert_eq!(label("x.svg").as_deref(), Some("SVG"));
        assert_eq!(label("x.txt"), None);
        assert!(resolve(None, &plugins).is_none(), "沒存過檔的文件沒有預覽");
        assert!(resolve(Some(Path::new("noext")), &plugins).is_none());
        let md = resolve(Some(Path::new("a.md")), &[]).unwrap();
        assert_eq!(md.label(), "Markdown");
        assert_ne!(md.signature(), resolve(Some(Path::new("a.csv")), &[]).unwrap().signature());
    }

    #[test]
    fn the_shipped_example_plugins_load_and_work() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../examples/previews");
        let (plugins, messages) = load_plugins(Some(&dir));
        assert!(messages.is_empty(), "{messages:?}");
        let by_file = |f: &str| Renderer::Plugin(plugins.iter().find(|p| p.file == f).unwrap_or_else(|| panic!("{f}")).clone());
        // 記錄檔 → 表格
        let r = render(&by_file("log-table.js"), "2026-11-02 12:00:01 INFO 啟動\n\n2026-11-02 12:00:05 ERROR 出錯 了\n沒有格式的一行\n", None);
        let Rendered::Table { header, rows, .. } = r else { panic!("{r:?}") };
        assert_eq!(header, ["時間", "等級", "訊息"]);
        assert_eq!(rows[0], ["2026-11-02 12:00:01", "INFO", "啟動"]);
        assert_eq!(rows[1], ["2026-11-02 12:00:05", "ERROR", "出錯 了"]);
        assert_eq!(rows[2], ["", "", "沒有格式的一行"]);
        // INI → 樹
        let r = render(&by_file("ini-tree.js"), "; 註解\nname = 全域\n[db]\nhost = localhost\nport=5432\n\n[ui]\ntheme = dark\n", None);
        let Rendered::Tree(v) = r else { panic!("{r:?}") };
        assert_eq!(v["name"], "全域");
        assert_eq!(v["db"]["port"], "5432");
        assert_eq!(v["ui"]["theme"], "dark");
        // 待辦 → Markdown
        let r = render(&by_file("todo-markdown.js"), "# 今天\nx 買牛奶\n寫程式\n", None);
        let Rendered::Markdown(blocks, _) = r else { panic!("{r:?}") };
        assert!(matches!(&blocks[0], Block::Heading(1, s) if plain(s) == "今天"));
        let Block::List { items, .. } = &blocks[1] else { panic!("{blocks:?}") };
        assert!(matches!(&items[0][0], Block::Paragraph(s) if plain(s) == "☑ 買牛奶"));
        assert!(matches!(&items[1][0], Block::Paragraph(s) if plain(s) == "☐ 寫程式"));
        // 副檔名對得上
        assert!(resolve(Some(Path::new("a.cfg")), &plugins).is_some());
        assert!(resolve(Some(Path::new("t.todo")), &plugins).is_some());
    }

    #[test]
    fn too_big_files_are_not_previewed() {
        let big = "a".repeat(MAX_BYTES + 1);
        let r = render(&Renderer::Builtin(Kind::Markdown), &big, None);
        assert!(matches!(r, Rendered::Error(e) if e.contains("5")));
    }

    #[test]
    fn plugin_folders_load_in_name_order_and_bad_files_are_reported() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("b.js"), "// @extensions bb\n// @output text\nfunction preview(t){return t}").unwrap();
        std::fs::write(dir.path().join("a.js"), LOG_PLUGIN).unwrap();
        std::fs::write(dir.path().join("bad.js"), "// @output text\n").unwrap();
        std::fs::write(dir.path().join("note.txt"), "// @extensions x\n// @output text").unwrap();
        let (plugins, messages) = load_plugins(Some(dir.path()));
        assert_eq!(plugins.iter().map(|p| p.file.as_str()).collect::<Vec<_>>(), ["a.js", "b.js"]);
        assert_eq!(messages.len(), 1);
        assert!(messages[0].contains("bad.js") && messages[0].contains("@extensions"), "{messages:?}");
        let (none, msgs) = load_plugins(Some(&dir.path().join("不存在")));
        assert!(none.is_empty() && msgs.is_empty());
        assert!(load_plugins(None).0.is_empty());
    }
}
