//! 文件狀態與開／存檔規則：與 egui 無關，方便單元測試。

use feditor_core::Buffer;
use feditor_core::i18n::t;
use feditor_core::tf;
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
    /// 磁碟上的檔案和這個分頁的關係（被刪掉了、有尚未處理的外部修改…），分頁標籤會顯示。
    pub disk_state: DiskState,
    /// 語法上色用的語言：自動（預設）、純文字、或手動指定。只屬於這個分頁，不寫進檔案或工作階段。
    pub lang: crate::syntax::LangChoice,
    /// 這個分頁是終端機（不是文字文件）：有的話分頁顯示終端機畫面，`buffer` 等欄位只是占位。
    pub terminal: Option<Box<crate::terminal_pane::TerminalPane>>,
    /// 還沒有檔案的文件的序號（「未命名 2」的 2），好分辨好幾個未命名的分頁；0 是還沒編號。
    /// 由 `App` 統一編：開著的未命名文件裡最小的空號，文件存成檔案之後空號就讓出來。
    pub untitled_number: usize,
}

/// 磁碟上的檔案和分頁內容的關係。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DiskState {
    /// 正常（或這個文件沒有對應的檔案）。
    #[default]
    Synced,
    /// 檔案已經被刪除或移走；分頁裡的內容仍然保留。
    Missing,
    /// 檔案被其他程式改了，但分頁有未儲存的修改，等使用者決定要不要重新載入。
    Stale,
}

/// [Document::poll_disk] 發現的變化。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiskChange {
    None,
    /// 修改時間和上次讀寫時不同（包含被刪除後又出現）。
    Changed,
    /// 檔案剛剛不見了（只會回報一次）。
    Gone,
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

/// 檢查新檔名能不能用（只是名稱，不含路徑）：不能空白、不能含路徑分隔字元與 Windows 不允許的符號、
/// 不能是 `.`／`..`、不能以點或空白結尾（Windows 會悄悄去掉）、不能是 Windows 的保留裝置名。
pub fn validate_file_name(name: &str) -> Result<(), String> {
    if name.trim().is_empty() {
        return Err(t("檔名不能是空的").to_owned());
    }
    if let Some(bad) = name.chars().find(|c| c.is_control() || "\\/:*?\"<>|".contains(*c)) {
        let shown = if bad.is_control() { t("控制字元").to_owned() } else { tf!("「{bad}」", bad = bad) };
        return Err(tf!("檔名不能含有 {shown}", shown = shown));
    }
    if name == "." || name == ".." || name.ends_with('.') || name.ends_with(' ') {
        return Err(t("檔名不能是「.」或「..」，也不能以點或空白結尾").to_owned());
    }
    let stem = name.split('.').next().unwrap_or("").to_ascii_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.ends_with(|c: char| c.is_ascii_digit()));
    if reserved {
        return Err(tf!("「{stem}」是系統保留的名稱，不能當檔名", stem = stem));
    }
    Ok(())
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
            SaveDocError::NoPath => f.write_str(t("尚未指定檔案路徑")),
            SaveDocError::WouldLoseData => f.write_str(
                t("檔案載入時有無法解碼的位元組，覆寫原檔會造成資料損失。請另存新檔，或改用其他編碼重新載入"),
            ),
            SaveDocError::Save(e) => e.fmt(f),
        }
    }
}

impl Document {
    pub fn new() -> Self {
        Self::default()
    }

    /// 還不存在的檔案：空白的文件，但已經有路徑（分頁用這個檔名）、標成尚未儲存；
    /// 存檔時才真的建立檔案。在那之前磁碟上沒有檔案不算「被刪除」，內容改回空白也還是未儲存。
    pub fn new_with_path(path: PathBuf) -> Self {
        Self { path: Some(path), dirty: true, ..Self::default() }
    }

    /// 有路徑、但還沒存過（見 [`Document::new_with_path`]）。
    fn is_pending_new_file(&self) -> bool {
        self.path.is_some() && self.saved.is_none()
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
            || self.is_pending_new_file() // 檔案還沒建立：內容就算改回空白也還是未儲存
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

    /// 看一下磁碟上的檔案：被刪掉、又出現、或被改過。刪除只回報一次（之後 [DiskState::Missing] 記著）；
    /// 檔案重新出現時狀態回到 [DiskState::Synced]。不會改動 disk_mtime，所以「改過」會一直回報到
    /// 使用者處理（重新載入或 [ccept_disk_version](Self::accept_disk_version)）為止。
    pub fn poll_disk(&mut self) -> DiskChange {
        // 命令列指定的新檔案：磁碟上本來就還沒有，不是被刪除，也沒有可比對的版本
        if self.is_pending_new_file() {
            return DiskChange::None;
        }
        let Some(path) = &self.path else { return DiskChange::None };
        match std::fs::metadata(path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                if self.disk_state == DiskState::Missing {
                    DiskChange::None
                } else {
                    self.disk_state = DiskState::Missing;
                    DiskChange::Gone
                }
            }
            Err(_) => DiskChange::None,
            Ok(meta) => {
                if self.disk_state == DiskState::Missing {
                    self.disk_state = DiskState::Synced;
                }
                match (meta.modified().ok(), self.disk_mtime) {
                    (Some(now), Some(known)) if now != known => DiskChange::Changed,
                    _ => DiskChange::None,
                }
            }
        }
    }

    /// 承認目前磁碟上的版本（使用者決定不重新載入時，不要一直再問）。
    pub fn accept_disk_version(&mut self) {
        if let Some(p) = &self.path {
            self.disk_mtime = mtime_of(p);
        }
        if self.disk_state == DiskState::Stale {
            self.disk_state = DiskState::Synced;
        }
    }

    fn mark_saved(&mut self) {
        self.disk_mtime = self.path.as_deref().and_then(mtime_of);
        self.disk_state = DiskState::Synced;
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
        if let Some(terminal) = &self.terminal {
            return terminal.tab_title();
        }
        self.path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| match self.untitled_number {
                0 => t("未命名").to_owned(),
                n => format!("{} {n}", t("未命名")),
            })
    }

    /// 是不是還沒有檔案的文字文件（會編「未命名 N」的序號）。
    pub fn is_untitled(&self) -> bool {
        self.terminal.is_none() && self.path.is_none()
    }

    /// 終端機分頁：`number` 是標題上的編號，`dir` 是 shell 的起始資料夾，`shell` 是要執行的程式與參數（空字串是系統預設）。
    pub fn new_terminal(number: usize, dir: Option<PathBuf>, shell: String) -> Self {
        Self { terminal: Some(Box::new(crate::terminal_pane::TerminalPane::new(dir, number, shell))), ..Default::default() }
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
            std::io::Error::new(std::io::ErrorKind::NotFound, t("文件尚未儲存，沒有可重新載入的檔案"))
        })?;
        let bytes = std::fs::read(&path)?;
        let (doc, warnings) = Self::from_decoded(&path, fileio::decode_with(&bytes, encoding));
        let lang = self.lang; // 重新載入不改變使用者選的語言
        *self = doc;
        self.lang = lang;
        Ok(warnings)
    }

    fn from_decoded(path: &Path, d: fileio::Decoded) -> (Self, Vec<String>) {
        let mut warnings = Vec::new();
        if d.lossy {
            warnings.push(tf!(
                "檔案含有無法以 {} 解碼的位元組，已用 � 取代；為避免資料損失，將禁止覆寫原檔。可改用其他編碼重新載入",
                d.format.encoding.name()
            ));
        }
        if d.mixed_line_endings {
            warnings.push(tf!(
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
                disk_state: DiskState::Synced,
                lang: Default::default(),
                terminal: None,
                untitled_number: 0,
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

    /// 把磁碟上的檔案改名（同一個資料夾內），並更新這份文件的路徑。內容與未儲存狀態都不受影響。
    /// 只改大小寫（Windows、macOS 的檔案系統視為同一個檔案）也允許。回傳新路徑。
    pub fn rename_file(&mut self, new_name: &str) -> Result<PathBuf, String> {
        validate_file_name(new_name)?;
        let old = self.path.clone().ok_or(t("這份文件還沒有存成檔案"))?;
        let new = old.with_file_name(new_name);
        if new == old {
            return Ok(new);
        }
        let case_only = old.file_name().is_some_and(|n| n.to_string_lossy().to_lowercase() == new_name.to_lowercase());
        if new.exists() && !case_only {
            return Err(tf!("這個資料夾裡已經有「{new_name}」", new_name = new_name));
        }
        std::fs::rename(&old, &new).map_err(|e| tf!("無法重新命名：{e}", e = e))?;
        self.disk_mtime = mtime_of(&new);
        self.path = Some(new.clone());
        Ok(new)
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
        doc.lang = crate::syntax::LangChoice::Plain;
        doc.reload_with(BIG5).unwrap();
        assert_eq!(doc.text(), "你好，世界");
        assert!(!doc.dirty);
        assert_eq!(doc.lang, crate::syntax::LangChoice::Plain, "重新載入要保留語言選擇");
    }

    #[test]
    fn poll_disk_reports_changes_and_a_deletion_only_once() {
        let dir = tempfile::tempdir().unwrap();
        let p = write(&dir, "w.txt", b"one");
        let (mut doc, _) = Document::open(&p).unwrap();
        assert_eq!(doc.poll_disk(), DiskChange::None);
        // 被改過：一直回報到使用者處理
        let later = std::time::SystemTime::now() + std::time::Duration::from_secs(5);
        std::fs::File::options().write(true).open(&p).unwrap().set_modified(later).unwrap();
        assert_eq!(doc.poll_disk(), DiskChange::Changed);
        assert_eq!(doc.poll_disk(), DiskChange::Changed);
        doc.accept_disk_version();
        assert_eq!(doc.poll_disk(), DiskChange::None);
        // 被刪掉：只回報一次，內容還在
        std::fs::remove_file(&p).unwrap();
        assert_eq!(doc.poll_disk(), DiskChange::Gone);
        assert_eq!(doc.disk_state, DiskState::Missing);
        assert_eq!(doc.poll_disk(), DiskChange::None);
        assert_eq!(doc.text(), "one");
        // 又出現（內容不同、時間也不同）：狀態恢復、回報為被改過
        std::fs::write(&p, "two").unwrap();
        let later = std::time::SystemTime::now() + std::time::Duration::from_secs(20);
        std::fs::File::options().write(true).open(&p).unwrap().set_modified(later).unwrap();
        assert_eq!(doc.poll_disk(), DiskChange::Changed);
        assert_eq!(doc.disk_state, DiskState::Synced);
        // 存檔會把狀態清乾淨
        std::fs::remove_file(&p).unwrap();
        assert_eq!(doc.poll_disk(), DiskChange::Gone);
        doc.save().unwrap();
        assert_eq!(doc.disk_state, DiskState::Synced);
        assert_eq!(doc.poll_disk(), DiskChange::None);
    }

    #[test]
    fn file_names_are_validated() {
        for ok in ["a.txt", "報告 2026.md", ".gitignore", "no-extension", "console.log", "コン.txt"] {
            assert!(validate_file_name(ok).is_ok(), "{ok}");
        }
        for bad in ["", "   ", "a/b", "a\\b", "a:b", "a*", "q?", "\"x\"", "<a>", "a|b", ".", "..", "x.", "x ", "con", "CON.txt", "lpt1", "a\nb"] {
            assert!(validate_file_name(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn rename_file_moves_the_file_keeps_content_and_dirty_state() {
        let dir = tempfile::tempdir().unwrap();
        let p = write(&dir, "old.txt", b"hello");
        std::fs::write(dir.path().join("taken.txt"), "x").unwrap();
        let (mut doc, _) = Document::open(&p).unwrap();
        doc.buffer.insert(0, "x");
        doc.sync();
        assert!(doc.dirty);
        // 已經有同名檔案：不覆蓋
        assert!(doc.rename_file("taken.txt").unwrap_err().contains("已經有"));
        assert!(p.exists());
        // 檔名不合法
        assert!(doc.rename_file("a/b.txt").is_err());
        // 成功：檔案搬過去，路徑更新，內容與未儲存狀態不變
        let new = doc.rename_file("new name.txt").unwrap();
        assert_eq!(new, dir.path().join("new name.txt"));
        assert!(!p.exists() && new.exists());
        assert_eq!(doc.path.as_deref(), Some(new.as_path()));
        assert!(doc.dirty);
        assert_eq!(doc.text(), "xhello");
        // 之後存檔寫到新的位置，也不會被當成「被外部修改」
        doc.save().unwrap();
        assert!(!doc.changed_on_disk());
        assert_eq!(std::fs::read_to_string(&new).unwrap(), "xhello");
        // 改成一樣的名字：什麼都不做
        assert_eq!(doc.rename_file("new name.txt").unwrap(), new);
        // 只改大小寫
        let upper = doc.rename_file("NEW NAME.txt").unwrap();
        assert!(upper.exists());
        // 沒有路徑的文件不能改名
        assert!(Document::new().rename_file("a.txt").is_err());
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

    #[test]
    fn a_new_file_with_a_path_is_unsaved_until_it_is_saved_and_is_never_reported_missing() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("new.txt");
        let mut doc = Document::new_with_path(p.clone());
        assert_eq!(doc.name(), "new.txt");
        assert!(doc.dirty && doc.buffer.is_empty(), "空白、尚未儲存");
        assert_eq!(doc.poll_disk(), DiskChange::None, "檔案還不存在不算被刪除");
        assert_eq!(doc.disk_state, DiskState::Synced);
        // 打字再刪掉：內容和空白文件一樣，但檔案還沒建立，所以還是未儲存
        doc.buffer.insert(0, "x");
        assert!(doc.sync());
        doc.buffer.remove(0..1);
        assert!(doc.sync());
        assert!(doc.dirty, "改回空白也還是未儲存");
        // 存檔：建立檔案、之後和一般文件一樣
        doc.buffer.insert(0, "hello");
        doc.sync();
        doc.save().unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "hello");
        assert!(!doc.dirty);
        doc.buffer.remove(0..5);
        doc.sync();
        assert!(doc.dirty, "存過之後，改動又算未儲存");
        doc.buffer.insert(0, "hello");
        doc.sync();
        assert!(!doc.dirty, "回到存檔時的內容就不算未儲存了");
    }
}
