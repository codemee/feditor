//! 多分頁：每個分頁有自己的文件與編輯狀態（游標、選取、復原歷史）。
//!
//! 目前作用中的分頁內容放在 `App` 的 `doc` 與 `editor`（程式其他部分都直接用它們），
//! 這裡的 `slots[active]` 只是個空位；切換分頁時把內容換進換出。與畫面無關，方便單元測試。

use crate::document::Document;
use feditor_core::i18n::t;
use crate::editor::model::EditorState;
use std::path::Path;

#[derive(Default)]
pub struct Tab {
    pub doc: Document,
    pub editor: EditorState,
}

pub struct TabSet {
    slots: Vec<Tab>,
    active: usize,
}

impl Default for TabSet {
    fn default() -> Self {
        Self::new()
    }
}

impl TabSet {
    pub fn new() -> Self {
        Self {
            slots: vec![Tab::default()],
            active: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.slots.len()
    }

    pub fn active(&self) -> usize {
        self.active
    }

    /// 切換到第 `i` 個分頁。沒有變動（已經是它、或超出範圍）回傳 `false`。
    pub fn switch(&mut self, doc: &mut Document, editor: &mut EditorState, i: usize) -> bool {
        if i == self.active || i >= self.slots.len() {
            return false;
        }
        let recording = editor.recording.take();
        self.slots[self.active] = Tab {
            doc: std::mem::take(doc),
            editor: std::mem::take(editor),
        };
        self.load(doc, editor, i);
        editor.recording = recording;
        true
    }

    fn load(&mut self, doc: &mut Document, editor: &mut EditorState, i: usize) {
        let tab = std::mem::take(&mut self.slots[i]);
        *doc = tab.doc;
        *editor = tab.editor;
        self.active = i;
    }

    /// 在目前這個分頁的右邊新增一個分頁（內容是 `new_doc`），並切換過去。
    pub fn push(&mut self, doc: &mut Document, editor: &mut EditorState, new_doc: Document) {
        let recording = editor.recording.take();
        self.slots[self.active] = Tab {
            doc: std::mem::take(doc),
            editor: std::mem::take(editor),
        };
        let at = self.active + 1;
        self.slots.insert(at, Tab::default());
        *doc = new_doc;
        *editor = EditorState::default();
        editor.recording = recording;
        self.active = at;
    }

    /// 關掉第 `i` 個分頁（不問未儲存，呼叫端先處理）。最後一個分頁關掉後會留下一個空白文件。
    pub fn close(&mut self, doc: &mut Document, editor: &mut EditorState, i: usize) {
        if i >= self.slots.len() {
            return;
        }
        if i != self.active {
            self.slots.remove(i);
            if i < self.active {
                self.active -= 1;
            }
            return;
        }
        let recording = editor.recording.take();
        self.slots.remove(i);
        if self.slots.is_empty() {
            self.slots.push(Tab::default());
            *doc = Document::new();
            *editor = EditorState::default();
            self.active = 0;
        } else {
            let next = i.min(self.slots.len() - 1);
            self.load(doc, editor, next);
        }
        editor.recording = recording;
    }

    /// 下一個（`forward`）或上一個分頁的編號，循環。
    pub fn neighbor(&self, forward: bool) -> usize {
        let n = self.slots.len();
        if forward {
            (self.active + 1) % n
        } else {
            (self.active + n - 1) % n
        }
    }

    // ── 窗格用的操作 ──────────────────────────────────────────
    //
    // 一個 `TabSet` 屬於一個窗格。有焦點的窗格，作用中分頁的內容放在 `App` 的 `doc`／`editor`
    //（上面的 `switch` 等函式就是這樣用）；沒有焦點的窗格，所有分頁的內容都在 `slots` 裡。
    // 跨窗格的操作（搬分頁、換焦點）先用 [`TabSet::stash`] 把內容收回來，操作完再 [`TabSet::unstash`]。

    /// 只有一個分頁的分頁集。
    pub fn from_tab(tab: Tab) -> Self {
        Self { slots: vec![tab], active: 0 }
    }

    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// 把作用中分頁的內容收回 `slots`。回傳進行中的巨集錄製（它跟著使用者走，不屬於某個分頁）。
    pub fn stash(&mut self, doc: &mut Document, editor: &mut EditorState) -> Recording {
        let recording = editor.recording.take();
        self.slots[self.active] = Tab {
            doc: std::mem::take(doc),
            editor: std::mem::take(editor),
        };
        recording
    }

    /// [`TabSet::stash`] 的反向：把作用中分頁的內容換進 `doc`／`editor`，並接回錄製。
    pub fn unstash(&mut self, doc: &mut Document, editor: &mut EditorState, recording: Recording) {
        self.load(doc, editor, self.active);
        editor.recording = recording;
    }

    /// 取出第 `i` 個分頁（內容必須已經收回）。取完可能變成空的，呼叫端要接著處理（關掉那個窗格）。
    pub fn take_tab(&mut self, i: usize) -> Tab {
        let tab = self.slots.remove(i);
        if i < self.active || self.active >= self.slots.len() {
            self.active = self.active.saturating_sub(1).min(self.slots.len().saturating_sub(1));
        }
        tab
    }

    /// 在位置 `at`（超出就放最後）放進一個分頁，並讓它成為作用中分頁。
    pub fn push_tab(&mut self, tab: Tab, at: usize) {
        let at = at.min(self.slots.len());
        self.slots.insert(at, tab);
        self.active = at;
    }

    /// 讓第 `i` 個分頁成為作用中分頁（內容必須已經收回；超出範圍就不動）。
    pub fn activate(&mut self, i: usize) {
        if i < self.slots.len() {
            self.active = i;
        }
    }

    /// 沒有焦點的窗格：它目前顯示的分頁的文件（唯讀）。
    pub fn active_doc_stashed(&self) -> &Document {
        &self.slots[self.active].doc
    }

    /// 沒有焦點的窗格：它目前顯示的分頁內容，畫面要拿去畫。
    pub fn active_slot_mut(&mut self) -> (&mut Document, &mut EditorState) {
        let tab = &mut self.slots[self.active];
        (&mut tab.doc, &mut tab.editor)
    }

    /// 要查第 `i` 個分頁的文件。`live` 是「作用中分頁在外面」時的那一份（沒有焦點的窗格傳 `None`）。
    fn doc_in<'a>(&'a self, live: Option<&'a Document>, i: usize) -> &'a Document {
        match live {
            Some(doc) if i == self.active => doc,
            _ => &self.slots[i].doc,
        }
    }

    fn find_path_with(&self, live: Option<&Document>, path: &Path) -> Option<usize> {
        let canon = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_owned());
        let wanted = canon(path);
        (0..self.slots.len()).find(|&i| {
            self.doc_in(live, i)
                .path
                .as_deref()
                .is_some_and(|p| canon(p) == wanted)
        })
    }

    /// 這個路徑是不是已經開在某個分頁裡。
    pub fn find_path(&self, doc: &Document, path: &Path) -> Option<usize> {
        self.find_path_with(Some(doc), path)
    }

    /// 同上，給沒有焦點的窗格用（所有內容都在 `slots` 裡）。
    pub fn find_path_stashed(&self, path: &Path) -> Option<usize> {
        self.find_path_with(None, path)
    }

    fn label_with(&self, live: Option<&Document>, i: usize) -> (String, String, bool) {
        let d = self.doc_in(live, i);
        let full = d
            .path
            .as_ref()
            .map_or_else(|| t("尚未儲存").to_owned(), |p| p.display().to_string());
        (d.name(), full, d.dirty)
    }

    /// 分頁標籤：（檔名, 完整路徑, 有沒有未儲存的修改）。
    pub fn label(&self, doc: &Document, i: usize) -> (String, String, bool) {
        self.label_with(Some(doc), i)
    }

    /// 同上，給沒有焦點的窗格用。
    pub fn label_stashed(&self, i: usize) -> (String, String, bool) {
        self.label_with(None, i)
    }

    /// 要記進工作階段的分頁，依分頁順序，附上「是不是作用中分頁」：存過檔的檔案與終端機（沒存過的空白文件略過）。
    /// `live` 同 [`TabSet::session_entries`]。
    pub fn session_records(&self, live: Option<(&Document, &EditorState)>) -> Vec<(TabRecord, bool)> {
        (0..self.slots.len())
            .filter_map(|i| {
                let (d, e) = match live {
                    Some(pair) if i == self.active => pair,
                    _ => (&self.slots[i].doc, &self.slots[i].editor),
                };
                let record = if let Some(t) = &d.terminal {
                    TabRecord::Terminal {
                        number: t.number(),
                        name: t.custom_name().map(str::to_owned),
                        dir: t.session_dir(),
                        shell: Some(t.shell().to_owned()).filter(|s| !s.is_empty()),
                    }
                } else {
                    TabRecord::File(d.path.clone()?, e.sel.head)
                };
                Some((record, i == self.active))
            })
            .collect()
    }

    /// 這個分頁集裡有存過檔的分頁：（路徑, 游標位置, 是不是作用中分頁）。`live` 同 [`TabSet::doc_in`]。
    pub fn session_entries(&self, live: Option<(&Document, &EditorState)>) -> Vec<(std::path::PathBuf, usize, bool)> {
        (0..self.slots.len())
            .filter_map(|i| {
                let (d, e) = match live {
                    Some(pair) if i == self.active => pair,
                    _ => (&self.slots[i].doc, &self.slots[i].editor),
                };
                d.path.as_ref().map(|p| (p.clone(), e.sel.head, i == self.active))
            })
            .collect()
    }

    /// 內容在 slots 裡的分頁（可以直接改）。live 為 true 表示這是有焦點的分頁集，
    /// 作用中分頁的內容在外面（App 的 doc／ditor），不在這裡面。
    pub fn background_mut(&mut self, live: bool) -> impl Iterator<Item = &mut Tab> {
        let active = self.active;
        self.slots.iter_mut().enumerate().filter(move |(i, _)| !(live && *i == active)).map(|(_, t)| t)
    }

    /// 第 i 個分頁的磁碟狀態（live 同 [TabSet::doc_in]）。
    pub fn disk_state(&self, live: Option<&Document>, i: usize) -> crate::document::DiskState {
        self.doc_in(live, i).disk_state
    }

    /// 第 `i` 個分頁如果是終端機，它執行的 shell（程式加參數，空字串是系統預設）；不是終端機回傳 `None`。
    pub fn terminal_shell(&self, live: Option<&Document>, i: usize) -> Option<String> {
        self.doc_in(live, i).terminal.as_ref().map(|t| t.shell().to_owned())
    }

    /// 終端機分頁的編號（`live` 同 [`TabSet::doc_in`]）。
    pub fn terminal_tabs(&self, live: Option<&Document>) -> Vec<usize> {
        (0..self.slots.len()).filter(|&i| self.doc_in(live, i).terminal.is_some()).collect()
    }

    /// 這個分頁集裡所有的文件（含背景的分頁）。有焦點的窗格作用中分頁的內容在 `App` 手上，這裡那一格只是占位，要略過。
    pub fn docs_mut(&mut self, skip_active: bool) -> Vec<&mut Document> {
        let active = self.active;
        self.slots.iter_mut().enumerate().filter(|(i, _)| !(skip_active && *i == active)).map(|(_, s)| &mut s.doc).collect()
    }

    /// 這個分頁集裡所有的終端機（含背景的分頁）。有焦點的窗格作用中分頁的內容在 `App` 手上，不在這裡。
    pub fn terminals_mut(&mut self) -> Vec<&mut crate::terminal_pane::TerminalPane> {
        self.slots.iter_mut().filter_map(|s| s.doc.terminal.as_deref_mut()).collect()
    }

    /// 終端機分頁的（分頁編號, 終端機識別編號）。
    pub fn terminal_uids(&self, live: Option<&Document>) -> Vec<(usize, u64)> {
        (0..self.slots.len()).filter_map(|i| self.doc_in(live, i).terminal.as_ref().map(|t| (i, t.uid()))).collect()
    }

    /// 這個分頁集裡終端機分頁的標題編號。
    pub fn terminal_numbers(&self, live: Option<&Document>) -> Vec<usize> {
        (0..self.slots.len()).filter_map(|i| self.doc_in(live, i).terminal.as_ref().map(|t| t.number())).collect()
    }

    /// shell 已經結束的終端機分頁編號。
    pub fn exited_terminal_tabs(&self, live: Option<&Document>) -> Vec<usize> {
        (0..self.slots.len()).filter(|&i| self.doc_in(live, i).terminal.as_ref().is_some_and(|t| t.exited())).collect()
    }

    /// 有未儲存修改的分頁編號。
    pub fn dirty_tabs(&self, doc: &Document) -> Vec<usize> {
        (0..self.slots.len())
            .filter(|&i| self.doc_in(Some(doc), i).dirty)
            .collect()
    }

    /// 同上，給沒有焦點的窗格用。
    pub fn dirty_tabs_stashed(&self) -> Vec<usize> {
        (0..self.slots.len())
            .filter(|&i| self.slots[i].doc.dirty)
            .collect()
    }
}

/// 工作階段裡的一個分頁（見 [`TabSet::session_records`]）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TabRecord {
    File(std::path::PathBuf, usize),
    Terminal { number: usize, name: Option<String>, dir: Option<std::path::PathBuf>, shell: Option<String> },
}

/// 進行中的巨集錄製（見 `EditorState::recording`）。
pub type Recording = Option<Vec<crate::editor::commands::Recorded>>;

#[cfg(test)]
mod tests {
    use super::*;
    use feditor_core::Buffer;

    fn named(text: &str) -> Document {
        let mut d = Document::new();
        d.buffer = Buffer::from_text(text);
        d
    }

    struct Fixture {
        tabs: TabSet,
        doc: Document,
        editor: EditorState,
    }

    fn fixture() -> Fixture {
        Fixture {
            tabs: TabSet::new(),
            doc: named("first"),
            editor: EditorState::default(),
        }
    }

    impl Fixture {
        fn push(&mut self, text: &str) {
            self.tabs.push(&mut self.doc, &mut self.editor, named(text));
        }
        fn switch(&mut self, i: usize) -> bool {
            self.tabs.switch(&mut self.doc, &mut self.editor, i)
        }
        fn close(&mut self, i: usize) {
            self.tabs.close(&mut self.doc, &mut self.editor, i)
        }
    }

    #[test]
    fn push_adds_a_tab_to_the_right_and_activates_it() {
        let mut f = fixture();
        f.push("second");
        f.push("third");
        assert_eq!(f.tabs.len(), 3);
        assert_eq!(f.tabs.active(), 2);
        assert_eq!(f.doc.text(), "third");
        // 在中間插入：新分頁緊接在作用中分頁的右邊
        f.switch(0);
        f.push("inserted");
        assert_eq!(f.tabs.active(), 1);
        f.switch(2);
        assert_eq!(f.doc.text(), "second");
    }

    #[test]
    fn stash_and_unstash_round_trip_keep_text_state_and_the_recording() {
        let mut f = fixture();
        f.push("second");
        f.editor.sel.head = 4;
        f.editor.recording = Some(Vec::new());
        let rec = f.tabs.stash(&mut f.doc, &mut f.editor);
        assert!(rec.is_some(), "錄製交還給呼叫端");
        assert!(f.editor.recording.is_none() && f.doc.text().is_empty(), "內容已經收回分頁集");
        f.tabs.unstash(&mut f.doc, &mut f.editor, rec);
        assert_eq!(f.doc.text(), "second");
        assert_eq!(f.editor.sel.head, 4);
        assert!(f.editor.recording.is_some());
    }

    #[test]
    fn take_tab_keeps_the_active_tab_pointing_at_the_right_one() {
        // 取走作用中分頁前面的分頁：編號跟著減一，作用中的內容不變
        let mut f = fixture();
        f.push("b");
        f.push("c");
        f.push("d"); // first, b, c, d；作用中是 d
        f.tabs.stash(&mut f.doc, &mut f.editor);
        assert_eq!(f.tabs.take_tab(1).doc.text(), "b");
        assert_eq!(f.tabs.active(), 2);
        f.tabs.unstash(&mut f.doc, &mut f.editor, None);
        assert_eq!(f.doc.text(), "d");
        // 取走作用中的最後一個：改指向前一個
        f.tabs.stash(&mut f.doc, &mut f.editor);
        assert_eq!(f.tabs.take_tab(2).doc.text(), "d");
        assert_eq!(f.tabs.active(), 1);
        // 取走作用中的、後面還有分頁：原本在後面的補上來
        let mut g = fixture();
        g.push("b");
        g.push("c");
        g.switch(1);
        g.tabs.stash(&mut g.doc, &mut g.editor);
        g.tabs.take_tab(1);
        assert_eq!(g.tabs.active(), 1);
        g.tabs.unstash(&mut g.doc, &mut g.editor, None);
        assert_eq!(g.doc.text(), "c");
        // 取走後面的分頁：作用中不變
        let mut h = fixture();
        h.push("b");
        h.push("c");
        h.switch(0);
        h.tabs.stash(&mut h.doc, &mut h.editor);
        h.tabs.take_tab(2);
        assert_eq!(h.tabs.active(), 0);
    }

    fn named_tab(text: &str) -> Tab {
        Tab { doc: named(text), editor: EditorState::default() }
    }

    #[test]
    fn push_tab_inserts_at_a_position_and_activates_it() {
        let mut set = TabSet::from_tab(named_tab("a"));
        set.push_tab(named_tab("b"), 1);
        set.push_tab(named_tab("c"), 0);
        set.push_tab(named_tab("d"), 99);
        assert_eq!(set.len(), 4);
        assert_eq!(set.active(), 3, "超出範圍就放最後");
        let order: Vec<String> = (0..4).map(|i| set.label_stashed(i).0).collect();
        assert_eq!(order.len(), 4);
        let texts: Vec<String> = set.slots.iter_mut().map(|t| t.doc.text().to_owned()).collect();
        assert_eq!(texts, ["c", "a", "b", "d"]);
    }

    #[test]
    fn queries_work_on_a_set_whose_contents_are_all_stored() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("x.txt");
        std::fs::write(&p, "hi").unwrap();
        let (doc, _) = Document::open(&p).unwrap();
        let mut set = TabSet::from_tab(Tab { doc, editor: EditorState::default() });
        set.push_tab(named_tab("unsaved"), 1);
        assert_eq!(set.find_path_stashed(&p), Some(0));
        assert_eq!(set.find_path_stashed(&dir.path().join("nope.txt")), None);
        assert!(set.dirty_tabs_stashed().is_empty());
        let entries = set.session_entries(None);
        assert_eq!(entries.len(), 1, "沒存過檔的分頁不記");
        assert_eq!(entries[0].0, p);
        assert!(!entries[0].2, "作用中的是第二個分頁（沒檔名），所以這個不是作用中");
    }

    #[test]
    fn switching_keeps_each_tabs_text_and_editor_state() {
        let mut f = fixture();
        f.editor.sel.head = 3;
        f.editor.sel.anchor = 3;
        f.push("second");
        assert_eq!(f.editor.sel.head, 0, "新分頁的游標從頭開始");
        f.editor.sel.head = 5;
        f.editor.sel.anchor = 5;
        assert!(f.switch(0));
        assert_eq!(f.doc.text(), "first");
        assert_eq!(f.editor.sel.head, 3);
        assert!(f.switch(1));
        assert_eq!(f.doc.text(), "second");
        assert_eq!(f.editor.sel.head, 5);
        assert!(!f.switch(1), "切到自己不算");
        assert!(!f.switch(9), "超出範圍不算");
    }

    #[test]
    fn undo_history_is_per_tab() {
        let mut f = fixture();
        f.doc.buffer.insert(5, "!");
        f.push("second");
        f.switch(0);
        assert!(f.doc.buffer.can_undo());
        f.doc.buffer.undo();
        assert_eq!(f.doc.text(), "first");
        f.switch(1);
        assert!(!f.doc.buffer.can_undo(), "別的分頁的復原歷史不會跑過來");
    }

    #[test]
    fn closing_the_active_tab_activates_its_neighbour() {
        let mut f = fixture();
        f.push("second");
        f.push("third");
        f.switch(1);
        f.close(1);
        assert_eq!(f.tabs.len(), 2);
        assert_eq!(f.doc.text(), "third", "關掉中間的，換到原本右邊那個");
        f.close(1);
        assert_eq!(f.doc.text(), "first", "關掉最右邊的，換到左邊");
        assert_eq!(f.tabs.active(), 0);
    }

    #[test]
    fn closing_another_tab_keeps_the_active_one() {
        let mut f = fixture();
        f.push("second");
        f.push("third");
        f.close(0);
        assert_eq!(f.tabs.len(), 2);
        assert_eq!(f.tabs.active(), 1, "左邊的少了一個，作用中的編號跟著減一");
        assert_eq!(f.doc.text(), "third");
        f.close(0);
        assert_eq!(f.doc.text(), "third");
        assert_eq!(f.tabs.active(), 0);
    }

    #[test]
    fn closing_the_last_tab_leaves_an_empty_document() {
        let mut f = fixture();
        f.close(0);
        assert_eq!(f.tabs.len(), 1);
        assert_eq!(f.doc.text(), "");
        assert!(f.doc.path.is_none());
    }

    #[test]
    fn neighbor_wraps_around() {
        let mut f = fixture();
        f.push("b");
        f.push("c");
        assert_eq!(f.tabs.neighbor(true), 0);
        assert_eq!(f.tabs.neighbor(false), 1);
        f.switch(0);
        assert_eq!(f.tabs.neighbor(false), 2);
    }

    #[test]
    fn dirty_tabs_and_labels_cover_background_tabs() {
        let mut f = fixture();
        f.doc.dirty = true;
        f.push("b");
        assert_eq!(f.tabs.dirty_tabs(&f.doc), vec![0]);
        f.doc.dirty = true;
        assert_eq!(f.tabs.dirty_tabs(&f.doc), vec![0, 1]);
        let (name, full, dirty) = f.tabs.label(&f.doc, 0);
        assert_eq!((name.as_str(), full.as_str(), dirty), ("未命名", "尚未儲存", true));
    }

    #[test]
    fn find_path_matches_open_files_in_any_tab() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.txt");
        let b = dir.path().join("b.txt");
        std::fs::write(&a, "a").unwrap();
        std::fs::write(&b, "b").unwrap();
        let mut f = fixture();
        f.doc = Document::open(&a).unwrap().0;
        f.tabs.push(&mut f.doc, &mut f.editor, Document::open(&b).unwrap().0);
        assert_eq!(f.tabs.find_path(&f.doc, &a), Some(0));
        assert_eq!(f.tabs.find_path(&f.doc, &b), Some(1));
        assert_eq!(f.tabs.find_path(&f.doc, &dir.path().join("c.txt")), None);
        // 不同寫法的同一個路徑
        let dotted = dir.path().join(".").join("a.txt");
        assert_eq!(f.tabs.find_path(&f.doc, &dotted), Some(0));
    }

    #[test]
    fn a_recording_in_progress_follows_the_user_across_tabs() {
        let mut f = fixture();
        f.editor.start_recording();
        f.editor.record_typed("a");
        f.push("second");
        assert!(f.editor.is_recording(), "新增分頁不中斷錄製");
        f.editor.record_typed("b");
        f.switch(0);
        assert!(f.editor.is_recording());
        f.close(0);
        assert!(f.editor.is_recording());
        let src = f.editor.stop_recording().unwrap();
        assert!(src.contains("editor.type(\"ab\")"), "{src}");
    }

    #[test]
    fn session_lists_saved_files_with_carets_and_the_active_one() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.txt");
        let b = dir.path().join("b.txt");
        std::fs::write(&a, "aaaa").unwrap();
        std::fs::write(&b, "bbbb").unwrap();
        let mut f = fixture();
        f.doc = Document::open(&a).unwrap().0;
        f.editor.sel.head = 2;
        f.tabs.push(&mut f.doc, &mut f.editor, named("untitled"));
        f.tabs.push(&mut f.doc, &mut f.editor, Document::open(&b).unwrap().0);
        f.editor.sel.head = 3;
        let entries = f.tabs.session_entries(Some((&f.doc, &f.editor)));
        let files: Vec<_> = entries.iter().map(|(p, c, _)| (p.clone(), *c)).collect();
        assert_eq!(files, vec![(a, 2), (b, 3)], "沒存過的分頁不記");
        assert_eq!(entries.iter().position(|e| e.2), Some(1), "作用中的是第二個存過檔的分頁");
    }
}
