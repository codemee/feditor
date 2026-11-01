//! 文件狀態與開／存檔規則：與 egui 無關，方便單元測試。

use feditor_core::Buffer;
use feditor_core::fileio::{self, FileFormat, LineEnding, SaveError};
use std::fmt;
use std::path::{Path, PathBuf};

#[derive(Debug, Default)]
pub struct Document {
    pub path: Option<PathBuf>,
    /// 緩衝區內容（換行統一為 `\n`）。
    pub buffer: Buffer,
    /// 上一次同步（[sync](Self::sync)）時的緩衝區修訂編號。
    synced_rev: u64,
    /// 整份文字的快照，給尋找與存檔用；修訂編號沒變就不會重做。
    snapshot: String,
    snapshot_rev: Option<u64>,
    pub format: FileFormat,
    pub dirty: bool,
    /// 載入時有無法解碼的位元組（已換成 U+FFFD）。此時禁止覆寫原檔。
    pub lossy: bool,
    /// 上次載入／存檔時的內容指紋（位元組數、雜湊）。undo 回到這個內容時就不算未儲存。
    /// None 表示全新的空文件。
    saved: Option<(usize, u64)>,
    /// 存檔後改過編碼、換行或 BOM：此時就算文字相同也仍是未儲存。
    format_changed: bool,
    /// 最後一次讀取或寫入這個檔案時，磁碟上的修改時間。用來發現別的程式改了檔案。
    disk_mtime: Option<std::time::SystemTime>,
}

/// 緩衝區內容的指紋；只走過 rope 的各個區塊，不需要組出整份字串。
fn mtime_of(path: &Path) -> Option<std::time::SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

fn fingerprint(buf: &Buffer) -> (usize, u64) {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for chunk in buf.rope().chunks() {
        chunk.hash(&mut h);
    }
    (buf.len_bytes(), h.finish())
}

#[derive(Debug)]
pub enum SaveDocError {
    /// 尚未指定路徑，應改走「另存新檔」。
    NoPath,
    /// 內容含有解碼失敗的字元，覆寫原檔會造成資料損失。
    WouldLoseData,
    Save(SaveError),
}

impl fmt::Display for SaveDocError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SaveDocError::NoPath => f.write_str("尚未指定檔案路徑"),
            SaveDocError::WouldLoseData => f.write_str(
                "檔案載入時有無法解碼的位元組，覆寫原檔會造成資料損失。請另存新檔，或改用其他編碼重新載入",
            ),
            SaveDocError::Save(e) => e.fmt(f),
        }
    }
}

impl Document {
    pub fn new() -> Self {
        Self::default()
    }

    /// 整份文字（換行為 `\n`）。內容沒變時直接回傳上次的快照，所以重複呼叫不貴。
    pub fn text(&mut self) -> &str {
        let rev = self.buffer.revision();
        if self.snapshot_rev != Some(rev) {
            self.snapshot = self.buffer.text();
            self.snapshot_rev = Some(rev);
        }
        &self.snapshot
    }

    /// 緩衝區自上次同步以來有被改動就標記為未儲存，回傳是否有改動。
    /// 每一幀、以及程式直接改了文字之後都要呼叫。
    pub fn sync(&mut self) -> bool {
        let rev = self.buffer.revision();
        if rev == self.synced_rev {
            return false;
        }
        self.synced_rev = rev;
        // 位元組數相同才值得算雜湊（打字、刪除幾乎都會改變長度）
        let saved = self.saved.unwrap_or_else(|| fingerprint(&Buffer::new()));
        self.dirty = self.format_changed
            || self.buffer.len_bytes() != saved.0
            || fingerprint(&self.buffer) != saved;
        true
    }

    /// 磁碟上的檔案是不是被別的程式改過（修改時間和我們上次讀寫時不同）。
    /// 檔案不見了、或讀不到時間都不算變更。
    pub fn changed_on_disk(&self) -> bool {
        match (&self.path, self.disk_mtime) {
            (Some(p), Some(known)) => mtime_of(p).is_some_and(|now| now != known),
            _ => false,
        }
    }

    /// 承認目前磁碟上的版本（使用者決定不重新載入時，不要一直再問）。
    pub fn accept_disk_version(&mut self) {
        if let Some(p) = &self.path {
            self.disk_mtime = mtime_of(p);
        }
    }

    fn mark_saved(&mut self) {
        self.disk_mtime = self.path.as_deref().and_then(mtime_of);
        self.saved = Some(fingerprint(&self.buffer));
        self.format_changed = false;
        self.dirty = false;
    }

    /// 緩衝區的改動已經被抵銷（例如巨集失敗後退回原狀），不要因此標成未儲存。
    pub fn mark_synced(&mut self) {
        self.synced_rev = self.buffer.revision();
    }

    /// 顯示用檔名。
    pub fn name(&self) -> String {
        self.path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "未命名".to_owned())
    }

    /// 開啟檔案，回傳文件與需要告知使用者的警告。
    pub fn open(path: &Path) -> std::io::Result<(Self, Vec<String>)> {
        let bytes = std::fs::read(path)?;
        Ok(Self::from_decoded(path, fileio::decode(&bytes)))
    }

    /// 以指定編碼重新讀取目前的檔案（會捨棄記憶體中尚未儲存的修改）。
    pub fn reload_with(
        &mut self,
        encoding: &'static encoding_rs::Encoding,
    ) -> std::io::Result<Vec<String>> {
        let path = self.path.clone().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::NotFound, "文件尚未儲存，沒有可重新載入的檔案")
        })?;
        let bytes = std::fs::read(&path)?;
        let (doc, warnings) = Self::from_decoded(&path, fileio::decode_with(&bytes, encoding));
        *self = doc;
        Ok(warnings)
    }

    fn from_decoded(path: &Path, d: fileio::Decoded) -> (Self, Vec<String>) {
        let mut warnings = Vec::new();
        if d.lossy {
            warnings.push(format!(
                "檔案含有無法以 {} 解碼的位元組，已用 � 取代；為避免資料損失，將禁止覆寫原檔。可改用其他編碼重新載入",
                d.format.encoding.name()
            ));
        }
        if d.mixed_line_endings {
            warnings.push(format!(
                "檔案混用多種換行，存檔時會統一為 {}",
                d.format.line_ending.label()
            ));
        }
        let buffer = Buffer::from_text(&d.text);
        let saved = Some(fingerprint(&buffer));
        (
            Self {
                path: Some(path.to_owned()),
                saved,
                format_changed: false,
                disk_mtime: mtime_of(path),
                buffer,
                format: d.format,
                dirty: false,
                lossy: d.lossy,
                synced_rev: 0,
                snapshot: String::new(),
                snapshot_rev: None,
            },
            warnings,
        )
    }

    /// 儲存到目前路徑。
    pub fn save(&mut self) -> Result<(), SaveDocError> {
        let path = self.path.clone().ok_or(SaveDocError::NoPath)?;
        if self.lossy {
            return Err(SaveDocError::WouldLoseData);
        }
        let format = self.format.clone();
        fileio::write_file(&path, self.text(), &format).map_err(SaveDocError::Save)?;
        self.mark_saved();
        Ok(())
    }

    /// 另存新檔；成功後文件改指向新路徑。
    pub fn save_as(&mut self, path: PathBuf) -> Result<(), SaveDocError> {
        let format = self.format.clone();
        fileio::write_file(&path, self.text(), &format).map_err(SaveDocError::Save)?;
        self.path = Some(path);
        self.mark_saved();
        // 內容已寫到新檔，之後可正常覆寫這個新檔
        self.lossy = false;
        Ok(())
    }

    pub fn set_line_ending(&mut self, le: LineEnding) {
        if self.format.line_ending != le {
            self.format.line_ending = le;
            self.format_changed = true;
            self.dirty = true;
        }
    }

    pub fn set_encoding(&mut self, enc: &'static encoding_rs::Encoding) {
        if self.format.encoding != enc {
            self.format.set_encoding(enc);
            self.format_changed = true;
            self.dirty = true;
        }
    }

    pub fn set_bom(&mut self, bom: bool) {
        if self.format.bom != bom {
            self.format.bom = bom;
            self.format_changed = true;
            self.dirty = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use encoding_rs::{BIG5, UTF_8};

    fn write(dir: &tempfile::TempDir, name: &str, bytes: &[u8]) -> PathBuf {
        let p = dir.path().join(name);
        std::fs::write(&p, bytes).unwrap();
        p
    }

    #[test]
    fn open_edit_save_keeps_format() {
        let dir = tempfile::tempdir().unwrap();
        let p = write(&dir, "a.txt", b"a\r\nb\r\n");
        let (mut doc, warnings) = Document::open(&p).unwrap();
        assert!(warnings.is_empty());
        assert_eq!(doc.text(), "a\nb\n");
        let end = doc.buffer.len_chars();
        doc.buffer.insert(end, "c\n");
        assert!(doc.sync());
        assert!(doc.dirty);
        doc.save().unwrap();
        assert!(!doc.dirty);
        assert_eq!(std::fs::read(&p).unwrap(), b"a\r\nb\r\nc\r\n");
    }

    #[test]
    fn lossy_open_refuses_to_overwrite_original() {
        let dir = tempfile::tempdir().unwrap();
        let original = [b'a', 0xFF, 0xFE, b'b', 0x80];
        let p = write(&dir, "bad.txt", &original);
        let (mut doc, _) = Document::open(&p).unwrap();
        // 不管偵測成什麼編碼，強制以 UTF-8 解碼一定會 lossy
        doc.reload_with(UTF_8).unwrap();
        assert!(doc.lossy);
        assert!(matches!(doc.save(), Err(SaveDocError::WouldLoseData)));
        assert_eq!(std::fs::read(&p).unwrap(), original, "原檔不可被改動");

        // 另存新檔則允許，且之後可正常覆寫新檔
        let q = dir.path().join("copy.txt");
        doc.save_as(q.clone()).unwrap();
        assert!(!doc.lossy);
        assert_eq!(doc.path.as_deref(), Some(q.as_path()));
        doc.save().unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), original);
    }

    #[test]
    fn mixed_line_endings_warn() {
        let dir = tempfile::tempdir().unwrap();
        let p = write(&dir, "m.txt", b"a\r\nb\nc\n");
        let (_, warnings) = Document::open(&p).unwrap();
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("換行"));
    }

    #[test]
    fn unsaved_document_needs_save_as() {
        let mut doc = Document::new();
        assert!(matches!(doc.save(), Err(SaveDocError::NoPath)));
        assert_eq!(doc.name(), "未命名");
    }

    #[test]
    fn save_with_unmappable_char_fails_and_keeps_dirty() {
        let dir = tempfile::tempdir().unwrap();
        let p = write(&dir, "a.txt", "舊".as_bytes());
        let (mut doc, _) = Document::open(&p).unwrap();
        doc.buffer = Buffer::from_text("新😀");
        doc.dirty = true;
        doc.set_encoding(BIG5);
        assert!(matches!(doc.save(), Err(SaveDocError::Save(_))));
        assert!(doc.dirty, "存檔失敗不可清除 dirty");
        assert_eq!(std::fs::read(&p).unwrap(), "舊".as_bytes());
    }

    #[test]
    fn changing_format_marks_dirty_only_when_changed() {
        let mut doc = Document::new();
        doc.set_line_ending(doc.format.line_ending);
        doc.set_encoding(UTF_8);
        assert!(!doc.dirty);
        doc.set_encoding(BIG5);
        assert!(doc.dirty);
    }

    #[test]
    fn reload_with_other_encoding_fixes_mojibake() {
        let dir = tempfile::tempdir().unwrap();
        let big5 = fileio::encode(
            "你好，世界",
            &FileFormat {
                encoding: BIG5,
                bom: false,
                line_ending: LineEnding::Lf,
            },
        )
        .unwrap();
        let p = write(&dir, "b.txt", &big5);
        let (mut doc, _) = Document::open(&p).unwrap();
        doc.buffer = Buffer::from_text("亂碼");
        doc.dirty = true;
        doc.reload_with(BIG5).unwrap();
        assert_eq!(doc.text(), "你好，世界");
        assert!(!doc.dirty);
    }

    #[test]
    fn sync_marks_dirty_only_after_a_change_and_text_snapshot_follows() {
        let mut doc = Document::new();
        assert!(!doc.sync());
        assert!(!doc.dirty);
        doc.buffer.insert(0, "ab");
        assert!(doc.sync());
        assert!(doc.dirty);
        assert!(!doc.sync(), "沒有新的改動就不再回報");
        assert_eq!(doc.text(), "ab");
        doc.buffer.insert(2, "c");
        assert_eq!(doc.text(), "abc", "改動後快照要更新");
    }

    #[test]
    fn mark_synced_absorbs_a_cancelled_change() {
        let mut doc = Document::new();
        let depth = doc.buffer.undo_depth();
        doc.buffer.insert(0, "x");
        doc.buffer.rollback_to(depth);
        doc.mark_synced();
        assert!(!doc.sync());
        assert!(!doc.dirty);
    }

    #[test]
    fn undoing_back_to_the_saved_text_clears_dirty() {
        let dir = tempfile::tempdir().unwrap();
        let p = write(&dir, "a.txt", b"abc");
        let (mut doc, _) = Document::open(&p).unwrap();
        doc.buffer.insert(3, "d");
        assert!(doc.sync() && doc.dirty);
        doc.buffer.undo();
        assert!(doc.sync());
        assert!(!doc.dirty, "回到載入時的內容");
        doc.buffer.redo();
        doc.sync();
        assert!(doc.dirty);
        doc.save().unwrap();
        assert!(!doc.dirty);
        doc.buffer.undo();
        doc.sync();
        assert!(doc.dirty, "存檔後 undo 就和檔案不同");
        doc.buffer.redo();
        doc.sync();
        assert!(!doc.dirty, "redo 回到存檔內容");
    }

    #[test]
    fn same_length_edit_is_still_dirty_and_new_document_returns_to_clean() {
        let mut doc = Document::new();
        doc.buffer.insert(0, "x");
        doc.sync();
        assert!(doc.dirty);
        doc.buffer.undo();
        doc.sync();
        assert!(!doc.dirty);
        let dir = tempfile::tempdir().unwrap();
        let p = write(&dir, "b.txt", b"ab");
        let (mut doc, _) = Document::open(&p).unwrap();
        doc.buffer.replace(0..1, "z");
        doc.sync();
        assert!(doc.dirty, "長度相同但內容不同");
    }

    #[test]
    fn format_change_keeps_dirty_even_when_text_matches() {
        let dir = tempfile::tempdir().unwrap();
        let p = write(&dir, "a.txt", b"ab");
        let (mut doc, _) = Document::open(&p).unwrap();
        doc.set_encoding(BIG5);
        doc.buffer.insert(0, "x");
        doc.buffer.undo();
        doc.sync();
        assert!(doc.dirty);
    }

    #[test]
    fn detects_external_changes_and_accepting_silences_them() {
        let dir = tempfile::tempdir().unwrap();
        let p = write(&dir, "a.txt", b"one");
        let (mut doc, _) = Document::open(&p).unwrap();
        assert!(!doc.changed_on_disk());
        // 改寫檔案並把修改時間往後推，避免檔案系統時間精度造成誤判
        std::fs::write(&p, "two").unwrap();
        let later = std::time::SystemTime::now() + std::time::Duration::from_secs(5);
        std::fs::File::options().write(true).open(&p).unwrap().set_modified(later).unwrap();
        assert!(doc.changed_on_disk());
        doc.accept_disk_version();
        assert!(!doc.changed_on_disk());
        // 自己存檔不算外部變更
        doc.buffer.insert(0, "x");
        doc.sync();
        doc.save().unwrap();
        assert!(!doc.changed_on_disk());
    }

    #[test]
    fn missing_file_and_unsaved_documents_are_not_reported_as_changed() {
        let dir = tempfile::tempdir().unwrap();
        let p = write(&dir, "a.txt", b"one");
        let (doc, _) = Document::open(&p).unwrap();
        std::fs::remove_file(&p).unwrap();
        assert!(!doc.changed_on_disk());
        assert!(!Document::new().changed_on_disk());
    }
}
