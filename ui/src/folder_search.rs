//! 尋找面板的搜尋狀態與背景執行緒（見 `core/src/grep.rs`）：搜尋範圍可以是目前文件（記憶體裡的內容，
//! 含還沒存檔的）或資料夾（含或不含子資料夾）。結果一邊搜一邊收進 `hits`。與畫面無關，方便單元測試。
//!
//! 搜尋字串、選項與取代文字放在 [`crate::find::FindState`]，這裡只管範圍、結果與執行緒。

use eframe::egui;
use feditor_core::grep::{self, GrepOptions, Hit, Summary};
use feditor_core::i18n::t;
use feditor_core::search::{SearchError, SearchOptions};
use feditor_core::tf;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::time::{Duration, Instant};

/// 結果多久重畫一次（毫秒）：搜尋中新結果要儘快出現，但不用每一筆都重畫。
const REPAINT_EVERY: Duration = Duration::from_millis(60);

enum Msg {
    Hit(Hit),
    Done(Result<Summary, SearchError>),
}

struct Running {
    rx: Receiver<Msg>,
    cancel: Arc<AtomicBool>,
}

/// 目前文件的搜尋是用什麼條件做的：條件或文件內容變了才需要重做。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stamp {
    pub query: String,
    pub options: (bool, bool, bool),
    pub path: Option<PathBuf>,
    pub revision: u64,
    pub len: usize,
}

pub struct FolderSearch {
    pub open: bool,
    /// 搜尋範圍：`true` 是資料夾，`false` 是目前文件。
    pub folder: bool,
    /// 資料夾範圍時是否包含子資料夾。
    pub recursive: bool,
    /// 要搜尋的資料夾（使用者可以直接改）。
    pub dir: String,
    /// 使用者自己改過資料夾：不再自動跟著作用中分頁的資料夾走。
    pub dir_custom: bool,
    /// 檔名過濾，例如 `*.rs;*.toml`。
    pub filter: String,
    pub hits: Vec<Hit>,
    /// 表格裡選取的那一筆。
    pub selected: Option<usize>,
    /// 給使用者看的狀態：進度、統計或錯誤。
    pub status: String,
    /// 下一個畫面要把表格捲到這一列（第二個值：`true` 捲到置中，`false` 剛好捲到看得見）。
    pub scroll_to: Option<(usize, bool)>,
    /// 這次搜尋的結果出齊後，表格是否已經先捲到目前畫面上的那一筆了。
    pub anchored: bool,
    /// 鍵盤焦點在結果表格上：上下鍵、PageUp／PageDown、Home／End 移動選取的那一筆。
    pub table_focus: bool,
    /// 結果表格上一個畫面佔的範圍（滑鼠點在表格外面就取消表格焦點）。
    pub table_rect: egui::Rect,
    /// 結果因為太多而被截斷。
    pub truncated: bool,
    /// 這次搜尋的資料夾（顯示相對路徑用）。
    pub root: PathBuf,
    /// 上一次搜尋的是目前文件（不是資料夾）：結果的路徑要顯示成 [`FolderSearch::doc_label`]。
    pub text_scope: bool,
    pub doc_label: String,
    /// 目前文件搜尋的條件快照，用來判斷要不要重搜。
    pub stamp: Option<Stamp>,
    running: Option<Running>,
}

impl Default for FolderSearch {
    fn default() -> Self {
        Self {
            open: false,
            folder: false,
            recursive: true,
            dir: String::new(),
            dir_custom: false,
            filter: String::new(),
            hits: Vec::new(),
            selected: None,
            status: String::new(),
            scroll_to: None,
            anchored: false,
            table_focus: false,
            table_rect: egui::Rect::NOTHING,
            truncated: false,
            root: PathBuf::new(),
            text_scope: false,
            doc_label: String::new(),
            stamp: None,
            running: None,
        }
    }
}

impl FolderSearch {
    pub fn is_running(&self) -> bool {
        self.running.is_some()
    }

    /// 清掉結果（搜尋字串空了、或換了範圍）。
    pub fn clear(&mut self) {
        self.stop();
        self.hits.clear();
        self.selected = None;
        self.truncated = false;
        self.status.clear();
        self.stamp = None;
        self.anchored = false;
        self.scroll_to = None;
    }

    /// 開始在資料夾裡搜尋（背景執行緒）。上一次還沒結束就先取消它。
    pub fn start(&mut self, ctx: &egui::Context, query: &str, search: SearchOptions) {
        self.clear();
        self.text_scope = false;
        self.root = PathBuf::from(self.dir.trim());
        if query.is_empty() {
            self.status = t("請輸入要搜尋的文字").to_owned();
            return;
        }
        if !self.root.exists() {
            self.status = tf!("找不到資料夾：{}", self.root.display());
            return;
        }
        let opts = GrepOptions {
            search,
            file_filter: self.filter.clone(),
            recursive: self.recursive,
            ..GrepOptions::default()
        };
        let (root, query) = (self.root.clone(), query.to_owned());
        self.run(ctx, move |cancel, on_hit| grep::search_dir(&root, &query, &opts, cancel, on_hit));
    }

    /// 開始在目前文件的內容裡搜尋（背景執行緒）。`path` 是文件的檔案（沒存過檔就是空路徑），
    /// `label` 是結果表格「檔名」欄要顯示的名字。
    pub fn start_text(
        &mut self,
        ctx: &egui::Context,
        label: &str,
        path: PathBuf,
        text: String,
        query: &str,
        search: SearchOptions,
    ) {
        self.clear();
        self.text_scope = true;
        self.doc_label = label.to_owned();
        let max = GrepOptions::default().max_hits;
        let query = query.to_owned();
        self.run(ctx, move |cancel, on_hit| {
            grep::search_text(&path, &text, &query, search, max, cancel, on_hit)
        });
    }

    fn run(
        &mut self,
        ctx: &egui::Context,
        job: impl FnOnce(&AtomicBool, &mut dyn FnMut(Hit)) -> Result<Summary, SearchError> + Send + 'static,
    ) {
        let (tx, rx) = channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let (flag, ctx) = (cancel.clone(), ctx.clone());
        std::thread::spawn(move || {
            let mut last = Instant::now();
            let result = job(&flag, &mut |hit| {
                let _ = tx.send(Msg::Hit(hit));
                if last.elapsed() >= REPAINT_EVERY {
                    last = Instant::now();
                    ctx.request_repaint();
                }
            });
            let _ = tx.send(Msg::Done(result));
            ctx.request_repaint();
        });
        self.running = Some(Running { rx, cancel });
        self.status = t("搜尋中…").to_owned();
    }

    /// 取消進行中的搜尋。
    pub fn stop(&mut self) {
        if let Some(r) = self.running.take() {
            r.cancel.store(true, Ordering::Relaxed);
        }
    }

    /// 收下背景執行緒送來的結果。回傳這次有沒有新資料（有的話畫面要重畫）。
    pub fn poll(&mut self) -> bool {
        let Some(r) = &self.running else {
            return false;
        };
        let mut changed = false;
        let mut finished = None;
        loop {
            match r.rx.try_recv() {
                Ok(Msg::Hit(h)) => {
                    self.hits.push(h);
                    changed = true;
                }
                Ok(Msg::Done(result)) => {
                    finished = Some(result);
                    break;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    finished = Some(Ok(Summary { cancelled: true, ..Summary::default() }));
                    break;
                }
            }
        }
        match finished {
            Some(result) => {
                self.running = None;
                self.status = match result {
                    Ok(s) => {
                        self.truncated = s.truncated;
                        describe(&s, self.text_scope)
                    }
                    Err(e) => e.to_string(),
                };
                true
            }
            None => {
                if changed {
                    self.status = tf!("搜尋中… 已找到 {} 筆", self.hits.len());
                }
                changed
            }
        }
    }

    /// 結果顯示用的路徑：盡量相對於搜尋的資料夾；目前文件的結果顯示文件的名字。
    pub fn display_path(&self, hit: &Hit) -> String {
        if self.text_scope {
            return self.doc_label.clone();
        }
        hit.path
            .strip_prefix(&self.root)
            .unwrap_or(&hit.path)
            .display()
            .to_string()
    }

    /// 選取上一筆或下一筆結果（到頭就繞回另一端）。回傳新選取的編號。
    pub fn step(&mut self, backward: bool) -> Option<usize> {
        let n = self.hits.len();
        if n == 0 {
            return None;
        }
        let next = match (self.selected, backward) {
            (None, false) => 0,
            (None, true) => n - 1,
            (Some(i), false) => (i + 1) % n,
            (Some(i), true) => (i + n - 1) % n,
        };
        self.selected = Some(next);
        self.scroll_to = Some((next, false));
        Some(next)
    }

    /// 結果裡出現過的檔案（依出現順序、不重複）。
    pub fn distinct_paths(&self) -> Vec<PathBuf> {
        let mut out: Vec<PathBuf> = Vec::new();
        for h in &self.hits {
            if out.last() != Some(&h.path) && !out.contains(&h.path) {
                out.push(h.path.clone());
            }
        }
        out
    }

    /// 某個檔案的內容變了（例如剛取代過一筆）：只重搜那個檔案，換掉它在表格裡的那幾列，
    /// 其他檔案的結果與選取位置不動。
    pub fn refresh_file(&mut self, path: &Path, text: &str, query: &str, search: SearchOptions) {
        let first = self.hits.iter().position(|h| h.path == path);
        self.hits.retain(|h| h.path != path);
        let mut fresh = Vec::new();
        let _ = grep::search_text(path, text, query, search, usize::MAX, &AtomicBool::new(false), |h| fresh.push(h));
        let at = first.unwrap_or(self.hits.len()).min(self.hits.len());
        self.hits.splice(at..at, fresh);
        if let Some(sel) = self.selected {
            self.selected = (!self.hits.is_empty()).then(|| sel.min(self.hits.len() - 1));
        }
    }
}

fn describe(s: &Summary, text_scope: bool) -> String {
    if text_scope && !s.cancelled {
        return if s.hits == 0 { t("找不到").to_owned() } else { tf!("{n} 筆", n = s.hits) };
    }
    let base = tf!(
        "{} 筆，分布在 {} 個檔案（共搜尋 {} 個檔案）",
        s.hits, s.files_with_hits, s.files_searched
    );
    if s.cancelled {
        tf!("已取消：{base}", base = base)
    } else if s.truncated {
        tf!("{}；結果太多，只列出前 {} 筆，請縮小範圍", base, s.hits)
    } else if s.hits == 0 {
        tf!("沒有找到（共搜尋 {} 個檔案）", s.files_searched)
    } else {
        base
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wait_done(fs: &mut FolderSearch) {
        let start = Instant::now();
        while fs.is_running() {
            fs.poll();
            assert!(start.elapsed() < Duration::from_secs(10), "搜尋沒有結束");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn setup(files: &[(&str, &str)]) -> (tempfile::TempDir, FolderSearch) {
        let dir = tempfile::tempdir().unwrap();
        for (name, content) in files {
            let p = dir.path().join(name);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, content).unwrap();
        }
        let fs = FolderSearch {
            dir: dir.path().display().to_string(),
            folder: true,
            ..FolderSearch::default()
        };
        (dir, fs)
    }

    fn go(fs: &mut FolderSearch, query: &str) {
        fs.start(&egui::Context::default(), query, SearchOptions::default());
        wait_done(fs);
    }

    #[test]
    fn search_runs_in_the_background_and_collects_hits() {
        let (_d, mut fs) = setup(&[("a.txt", "foo\nbar"), ("b.txt", "xfoo")]);
        go(&mut fs, "foo");
        assert_eq!(fs.hits.len(), 2);
        assert!(fs.status.contains("2 筆"), "{}", fs.status);
        assert_eq!(fs.display_path(&fs.hits[0]), "a.txt");
    }

    #[test]
    fn empty_query_missing_folder_and_bad_regex_report_errors() {
        let (_d, mut fs) = setup(&[("a.txt", "foo")]);
        go(&mut fs, "");
        assert!(!fs.is_running());
        assert!(fs.status.contains("請輸入"));
        fs.dir = "Z:\\definitely\\missing".into();
        go(&mut fs, "foo");
        assert!(fs.status.contains("找不到資料夾"));
        let (_d2, mut fs) = setup(&[("a.txt", "foo")]);
        fs.start(&egui::Context::default(), "(", SearchOptions { regex: true, ..Default::default() });
        wait_done(&mut fs);
        assert!(fs.hits.is_empty());
        assert!(!fs.status.is_empty() && !fs.status.contains("筆"), "{}", fs.status);
    }

    #[test]
    fn a_new_search_replaces_the_old_results() {
        let (_d, mut fs) = setup(&[("a.txt", "foo bar")]);
        go(&mut fs, "foo");
        assert_eq!(fs.hits.len(), 1);
        go(&mut fs, "nothing-here");
        assert!(fs.hits.is_empty());
        assert!(fs.status.contains("沒有找到"), "{}", fs.status);
    }

    #[test]
    fn stop_cancels_a_running_search() {
        let (_d, mut fs) = setup(&[("a.txt", &"foo\n".repeat(1000))]);
        fs.start(&egui::Context::default(), "foo", SearchOptions::default());
        fs.stop();
        assert!(!fs.is_running());
    }

    #[test]
    fn subfolders_follow_the_recursive_switch() {
        let (_d, mut fs) = setup(&[("a.txt", "foo"), ("sub/b.txt", "foo")]);
        go(&mut fs, "foo");
        assert_eq!(fs.hits.len(), 2);
        assert_eq!(fs.display_path(&fs.hits[1]), format!("sub{}b.txt", std::path::MAIN_SEPARATOR));
        fs.recursive = false;
        go(&mut fs, "foo");
        assert_eq!(fs.hits.len(), 1);
    }

    #[test]
    fn the_current_document_is_searched_from_memory() {
        let mut fs = FolderSearch::default();
        fs.start_text(
            &egui::Context::default(),
            "筆記.txt",
            PathBuf::new(),
            "a\nfoo\nb foo".into(),
            "foo",
            SearchOptions::default(),
        );
        wait_done(&mut fs);
        assert_eq!(fs.hits.iter().map(|h| h.line).collect::<Vec<_>>(), vec![1, 2]);
        assert_eq!(fs.display_path(&fs.hits[0]), "筆記.txt");
        assert!(fs.status.contains("2 筆") && !fs.status.contains("檔案"), "{}", fs.status);
    }

    #[test]
    fn stepping_wraps_around_and_starts_from_either_end() {
        let (_d, mut fs) = setup(&[("a.txt", "foo\nfoo\nfoo")]);
        go(&mut fs, "foo");
        assert_eq!(fs.step(false), Some(0));
        assert_eq!(fs.step(false), Some(1));
        assert_eq!(fs.step(false), Some(2));
        assert_eq!(fs.step(false), Some(0));
        assert_eq!(fs.step(true), Some(2));
        fs.selected = None;
        assert_eq!(fs.step(true), Some(2));
        fs.clear();
        assert_eq!(fs.step(false), None);
    }

    #[test]
    fn refreshing_one_file_replaces_only_its_rows() {
        let (d, mut fs) = setup(&[("a.txt", "foo\nfoo"), ("b.txt", "foo")]);
        go(&mut fs, "foo");
        fs.selected = Some(0);
        let a = d.path().join("a.txt");
        fs.refresh_file(&a, "foo", "foo", SearchOptions::default());
        assert_eq!(fs.hits.len(), 2);
        assert_eq!(fs.hits[0].path, a);
        assert_eq!(fs.selected, Some(0));
        // 一筆都不剩：列被拿掉，選取跟著收斂
        fs.selected = Some(1);
        fs.refresh_file(&a, "bar", "foo", SearchOptions::default());
        assert_eq!(fs.hits.len(), 1);
        assert_eq!(fs.selected, Some(0));
        assert_eq!(fs.distinct_paths(), vec![d.path().join("b.txt")]);
    }
}
