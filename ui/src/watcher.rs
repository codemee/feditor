//! 監看已開啟檔案所在的資料夾：外部程式改了、刪了、搬了檔案，就通知畫面去檢查。
//!
//! 只收「有事情發生了」這個訊號，不分析是哪個檔案——事件裡的路徑在各平台的寫法不一致
//! （大小寫、符號連結、macOS 的 `/private`），直接比對路徑不可靠。收到訊號後，呼叫端逐一比對
//! 每個文件的修改時間，那才是準的，而且只是幾個 `stat`。事件會先等一小段時間（去抖動），
//! 讓寫入到一半的檔案寫完再處理。

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use notify::{EventKind, RecursiveMode, Watcher};

use crate::LogKind;
use feditor_core::tf;

/// 最後一個事件之後等多久才算穩定。
const DEBOUNCE: Duration = Duration::from_millis(250);

pub struct FileWatcher {
    watcher: Option<notify::RecommendedWatcher>,
    rx: mpsc::Receiver<Instant>,
    dirs: HashSet<PathBuf>,
    last_event: Option<Instant>,
}

/// 要監看的資料夾：每個檔案所在的資料夾（監看資料夾才抓得到「刪除後重建」這類換檔式的存檔）。
pub fn watch_dirs<'a>(paths: impl IntoIterator<Item = &'a Path>) -> HashSet<PathBuf> {
    paths
        .into_iter()
        .map(|p| match p.parent() {
            Some(d) if !d.as_os_str().is_empty() => d.to_owned(),
            _ => PathBuf::from("."),
        })
        .collect()
}

impl FileWatcher {
    /// `ctx` 用來在背景執行緒收到事件時叫醒畫面。系統不支援監看時，仍可使用，只是永遠收不到事件
    ///（呼叫端另外有定時檢查當後備）。
    pub fn new(ctx: eframe::egui::Context) -> Self {
        let (tx, rx) = mpsc::channel();
        let watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            // 純讀取（Access）不是變更；錯誤（例如事件佇列溢位）保險起見也當成有變更
            if matches!(&res, Ok(ev) if matches!(ev.kind, EventKind::Access(_))) {
                return;
            }
            let _ = tx.send(Instant::now());
            ctx.request_repaint_after(DEBOUNCE);
        })
        .ok();
        Self { watcher, rx, dirs: HashSet::new(), last_event: None }
    }

    /// 讓監看的資料夾和 `wanted` 一致（多的取消、少的加上）。沒變就什麼都不做。
    pub fn sync(&mut self, wanted: HashSet<PathBuf>) {
        if wanted == self.dirs {
            return;
        }
        if let Some(w) = &mut self.watcher {
            for gone in self.dirs.difference(&wanted) {
                let _ = w.unwatch(gone);
            }
            for new in wanted.difference(&self.dirs) {
                // 資料夾不存在、沒有權限：略過，定時檢查會補上
                let _ = w.watch(new, RecursiveMode::NonRecursive);
            }
        }
        self.dirs = wanted;
    }

    /// 有沒有穩定下來的變更要處理（最後一個事件已經過了去抖動時間）。
    pub fn take_changes(&mut self) -> bool {
        // 事件的時間是它發生的時候（不是這裡讀到的時候），去抖動才算得準
        while let Ok(at) = self.rx.try_recv() {
            self.last_event = Some(self.last_event.map_or(at, |old| old.max(at)));
        }
        match self.last_event {
            Some(t) if t.elapsed() >= DEBOUNCE => {
                self.last_event = None;
                true
            }
            _ => false,
        }
    }

    /// 還有事件在等去抖動（畫面要記得稍後再來看）。
    pub fn pending(&self) -> bool {
        self.last_event.is_some()
    }
}

/// 見 `App::check_background_docs`：處理一個沒有顯示在畫面上的分頁。
pub fn refresh_background_tab(tab: &mut crate::tabs::Tab, logs: &mut Vec<(LogKind, String)>) {
    use crate::document::{DiskChange, DiskState};
    let name = tab.doc.name();
    match tab.doc.poll_disk() {
        DiskChange::None => {}
        DiskChange::Gone => logs.push((
            LogKind::Warn,
            tf!("{name} 已被刪除或移走，分頁裡的內容仍然保留；存檔會重新建立這個檔案", name = name),
        )),
        DiskChange::Changed if tab.doc.dirty => {
            // 只提醒一次；等使用者切過去時再決定
            if tab.doc.disk_state != DiskState::Stale {
                tab.doc.disk_state = DiskState::Stale;
                logs.push((
                    LogKind::Warn,
                    tf!("{name} 已被其他程式修改，但你有尚未儲存的修改；切換到這個分頁時會詢問要不要重新載入", name = name),
                ));
            }
        }
        DiskChange::Changed => match tab.doc.reload_with(tab.doc.format.encoding) {
            Ok(warnings) => {
                tab.editor.extra.clear();
                tab.editor.clamp(&tab.doc.buffer);
                logs.push((LogKind::Info, tf!("{name} 已被其他程式修改，已自動重新載入", name = name)));
                logs.extend(warnings.into_iter().map(|w| (LogKind::Warn, w)));
            }
            Err(e) => {
                tab.doc.accept_disk_version();
                logs.push((LogKind::Error, tf!("無法重新載入 {name}：{e}", name = name, e = e)));
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn watch_dirs_collects_each_parent_once() {
        let dirs = watch_dirs([Path::new("/a/x.txt"), Path::new("/a/y.txt"), Path::new("/b/z.txt"), Path::new("rel.txt")]);
        assert_eq!(dirs.len(), 3);
        assert!(dirs.contains(Path::new("/a")) && dirs.contains(Path::new("/b")) && dirs.contains(Path::new(".")));
    }

    #[test]
    fn a_change_in_a_watched_folder_is_reported_after_the_debounce_and_only_once() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.txt");
        std::fs::write(&file, "1").unwrap();
        let mut w = FileWatcher::new(eframe::egui::Context::default());
        w.sync(watch_dirs([file.as_path()]));
        assert!(!w.take_changes(), "還沒有事件");
        // 等監看真正開始（不同平台的啟動時間不一）再改檔案
        std::thread::sleep(Duration::from_millis(300));
        let deadline = Instant::now() + Duration::from_secs(15);
        let mut seen = false;
        let mut n = 2;
        while Instant::now() < deadline && !seen {
            std::fs::write(&file, n.to_string()).unwrap();
            n += 1;
            std::thread::sleep(Duration::from_millis(400));
            seen = w.take_changes();
        }
        assert!(seen, "改了檔案卻沒有收到事件");
        assert!(!w.pending());
        assert!(!w.take_changes(), "同一批事件只報一次");
        // 取消監看之後，不再收到事件
        w.sync(HashSet::new());
        std::thread::sleep(Duration::from_millis(300));
        while w.take_changes() {}
        std::fs::write(&file, "later").unwrap();
        std::thread::sleep(Duration::from_millis(600));
        assert!(!w.take_changes() && !w.pending());
    }
}
