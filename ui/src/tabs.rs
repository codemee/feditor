//! 多分頁：每個分頁有自己的文件與編輯狀態（游標、選取、復原歷史）。
//!
//! 目前作用中的分頁內容放在 `App` 的 `doc` 與 `editor`（程式其他部分都直接用它們），
//! 這裡的 `slots[active]` 只是個空位；切換分頁時把內容換進換出。與畫面無關，方便單元測試。

use crate::document::Document;
use crate::editor::model::EditorState;
use crate::session::Session;
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

    fn doc_at<'a>(&'a self, doc: &'a Document, i: usize) -> &'a Document {
        if i == self.active { doc } else { &self.slots[i].doc }
    }

    /// 這個路徑是不是已經開在某個分頁裡。
    pub fn find_path(&self, doc: &Document, path: &Path) -> Option<usize> {
        let canon = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_owned());
        let wanted = canon(path);
        (0..self.slots.len()).find(|&i| {
            self.doc_at(doc, i)
                .path
                .as_deref()
                .is_some_and(|p| canon(p) == wanted)
        })
    }

    /// 分頁標籤：（檔名, 完整路徑, 有沒有未儲存的修改）。
    pub fn label(&self, doc: &Document, i: usize) -> (String, String, bool) {
        let d = self.doc_at(doc, i);
        let full = d
            .path
            .as_ref()
            .map_or_else(|| "尚未儲存".to_owned(), |p| p.display().to_string());
        (d.name(), full, d.dirty)
    }

    /// 目前開著哪些檔案（只算有存過檔的分頁）與各自的游標位置，用來記住工作階段。
    pub fn session(&self, doc: &Document, editor: &EditorState) -> Session {
        let mut files = Vec::new();
        let mut active = 0;
        for i in 0..self.slots.len() {
            let (d, e) = if i == self.active {
                (doc, editor)
            } else {
                (&self.slots[i].doc, &self.slots[i].editor)
            };
            if let Some(p) = &d.path {
                if i == self.active {
                    active = files.len();
                }
                files.push((p.clone(), e.sel.head));
            }
        }
        Session { files, active }
    }

    /// 有未儲存修改的分頁編號。
    pub fn dirty_tabs(&self, doc: &Document) -> Vec<usize> {
        (0..self.slots.len())
            .filter(|&i| self.doc_at(doc, i).dirty)
            .collect()
    }
}

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
        let s = f.tabs.session(&f.doc, &f.editor);
        assert_eq!(s.files, vec![(a, 2), (b, 3)], "沒存過的分頁不記");
        assert_eq!(s.active, 1);
    }
}
