//! 跨檔搜尋的狀態與背景執行緒（見 `core/src/grep.rs`）。與畫面無關，方便單元測試。

use eframe::egui;
use feditor_core::i18n::t;
use feditor_core::tf;
use feditor_core::grep::{self, GrepOptions, Hit, Summary};
use feditor_core::search::{SearchError, SearchOptions};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError, channel};

enum Msg {
    Hit(Hit),
    Done(Result<Summary, SearchError>),
}

struct Running {
    rx: Receiver<Msg>,
    cancel: Arc<AtomicBool>,
}

#[derive(Default)]
pub struct FolderSearch {
    pub open: bool,
    pub query: String,
    /// 要搜尋的資料夾（使用者可以直接改）。
    pub dir: String,
    /// 檔名過濾，例如 `*.rs;*.toml`。
    pub filter: String,
    pub case_sensitive: bool,
    pub whole_word: bool,
    pub regex: bool,
    pub hits: Vec<Hit>,
    /// 給使用者看的狀態：進度、統計或錯誤。
    pub status: String,
    /// 下一個畫面要把焦點放到搜尋框。
    pub focus_query: bool,
    running: Option<Running>,
    /// 這次搜尋的資料夾（顯示相對路徑用）。
    pub root: PathBuf,
}

impl FolderSearch {
    pub fn is_running(&self) -> bool {
        self.running.is_some()
    }

    pub fn options(&self) -> GrepOptions {
        GrepOptions {
            search: SearchOptions {
                regex: self.regex,
                case_sensitive: self.case_sensitive,
                whole_word: self.whole_word,
            },
            file_filter: self.filter.clone(),
            ..GrepOptions::default()
        }
    }

    /// 開始搜尋（在背景執行緒）。上一次還沒結束就先取消它。
    pub fn start(&mut self, ctx: &egui::Context) {
        self.stop();
        self.hits.clear();
        self.root = PathBuf::from(self.dir.trim());
        if self.query.is_empty() {
            self.status = t("請輸入要搜尋的文字").to_owned();
            return;
        }
        if !self.root.exists() {
            self.status = tf!("找不到資料夾：{}", self.root.display());
            return;
        }
        let (tx, rx) = channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let (root, query, opts) = (self.root.clone(), self.query.clone(), self.options());
        let (flag, ctx) = (cancel.clone(), ctx.clone());
        std::thread::spawn(move || {
            let mut sent = 0usize;
            let result = grep::search_dir(&root, &query, &opts, &flag, |hit| {
                let _ = tx.send(Msg::Hit(hit));
                sent += 1;
                if sent % 64 == 0 {
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
                    Ok(s) => describe(&s),
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

    /// 結果顯示用的路徑：盡量相對於搜尋的資料夾。
    pub fn display_path(&self, hit: &Hit) -> String {
        hit.path
            .strip_prefix(&self.root)
            .unwrap_or(&hit.path)
            .display()
            .to_string()
    }
}

fn describe(s: &Summary) -> String {
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
    use std::time::{Duration, Instant};

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
            std::fs::write(dir.path().join(name), content).unwrap();
        }
        let fs = FolderSearch {
            dir: dir.path().display().to_string(),
            ..FolderSearch::default()
        };
        (dir, fs)
    }

    #[test]
    fn search_runs_in_the_background_and_collects_hits() {
        let (_d, mut fs) = setup(&[("a.txt", "foo\nbar"), ("b.txt", "xfoo")]);
        fs.query = "foo".into();
        fs.start(&egui::Context::default());
        wait_done(&mut fs);
        assert_eq!(fs.hits.len(), 2);
        assert!(fs.status.contains("2 筆"), "{}", fs.status);
        assert_eq!(fs.display_path(&fs.hits[0]), "a.txt");
    }

    #[test]
    fn empty_query_missing_folder_and_bad_regex_report_errors() {
        let (_d, mut fs) = setup(&[("a.txt", "foo")]);
        fs.start(&egui::Context::default());
        assert!(!fs.is_running());
        assert!(fs.status.contains("請輸入"));
        fs.query = "foo".into();
        fs.dir = "Z:\\definitely\\missing".into();
        fs.start(&egui::Context::default());
        assert!(fs.status.contains("找不到資料夾"));
        let (_d2, mut fs) = setup(&[("a.txt", "foo")]);
        fs.query = "(".into();
        fs.regex = true;
        fs.start(&egui::Context::default());
        wait_done(&mut fs);
        assert!(fs.hits.is_empty());
        assert!(!fs.status.is_empty() && !fs.status.contains("筆"), "{}", fs.status);
    }

    #[test]
    fn a_new_search_replaces_the_old_results() {
        let (_d, mut fs) = setup(&[("a.txt", "foo bar")]);
        fs.query = "foo".into();
        fs.start(&egui::Context::default());
        wait_done(&mut fs);
        assert_eq!(fs.hits.len(), 1);
        fs.query = "nothing-here".into();
        fs.start(&egui::Context::default());
        wait_done(&mut fs);
        assert!(fs.hits.is_empty());
        assert!(fs.status.contains("沒有找到"), "{}", fs.status);
    }

    #[test]
    fn stop_cancels_a_running_search() {
        let (_d, mut fs) = setup(&[("a.txt", &"foo\n".repeat(1000))]);
        fs.query = "foo".into();
        fs.start(&egui::Context::default());
        fs.stop();
        assert!(!fs.is_running());
    }
}
