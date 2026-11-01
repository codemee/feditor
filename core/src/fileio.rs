//! 檔案讀寫：編碼偵測、換行處理、安全存檔。
//!
//! 緩衝區內的文字一律使用 `\n` 換行；原檔的換行格式記錄在 [`FileFormat`]，存檔時還原。

use chardetng::{EncodingDetector, Iso2022JpDetection, Utf8Detection};
use encoding_rs::{Encoding, UTF_8, UTF_16BE, UTF_16LE};
use std::fmt;
use std::io::Write as _;
use std::path::Path;

/// 偵測編碼時最多取樣的位元組數。
const DETECT_SAMPLE: usize = 4 * 1024 * 1024;

/// 常用編碼，供 UI 顯示選項。
pub fn common_encodings() -> [&'static Encoding; 7] {
    [
        UTF_8,
        encoding_rs::BIG5,
        encoding_rs::GBK,
        encoding_rs::SHIFT_JIS,
        encoding_rs::EUC_KR,
        UTF_16LE,
        UTF_16BE,
    ]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineEnding {
    Lf,
    CrLf,
    Cr,
}

impl LineEnding {
    pub const ALL: [LineEnding; 3] = [LineEnding::CrLf, LineEnding::Lf, LineEnding::Cr];

    pub fn as_str(self) -> &'static str {
        match self {
            LineEnding::Lf => "\n",
            LineEnding::CrLf => "\r\n",
            LineEnding::Cr => "\r",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            LineEnding::Lf => "LF",
            LineEnding::CrLf => "CRLF",
            LineEnding::Cr => "CR",
        }
    }

    /// 當前平台的慣用換行。
    pub fn platform() -> Self {
        if cfg!(windows) {
            LineEnding::CrLf
        } else {
            LineEnding::Lf
        }
    }
}

/// 檔案的磁碟格式。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileFormat {
    pub encoding: &'static Encoding,
    /// 是否有 BOM（UTF-8、UTF-16 才有意義）。
    pub bom: bool,
    pub line_ending: LineEnding,
}

impl Default for FileFormat {
    fn default() -> Self {
        Self {
            encoding: UTF_8,
            bom: false,
            line_ending: LineEnding::platform(),
        }
    }
}

impl FileFormat {
    /// 顯示用名稱，例如 `UTF-8`、`UTF-8 BOM`、`Big5`。
    pub fn encoding_label(&self) -> String {
        if self.bom && self.encoding == UTF_8 {
            "UTF-8 BOM".to_owned()
        } else {
            self.encoding.name().to_owned()
        }
    }

    /// 切換編碼時調整 BOM：UTF-16 預設帶 BOM，其他編碼不帶。
    pub fn set_encoding(&mut self, encoding: &'static Encoding) {
        self.encoding = encoding;
        self.bom = encoding == UTF_16LE || encoding == UTF_16BE;
    }
}

/// 解碼結果。
#[derive(Debug, Clone)]
pub struct Decoded {
    /// 已把換行統一為 `\n` 的文字。
    pub text: String,
    pub format: FileFormat,
    /// 解碼時遇到無法解讀的位元組（已換成 U+FFFD）。此時存回原檔會造成資料損失。
    pub lossy: bool,
    /// 原檔混用多種換行（存檔時會統一為 `format.line_ending`）。
    pub mixed_line_endings: bool,
}

/// 自動偵測編碼並解碼：BOM → 嚴格 UTF-8 → `chardetng`（偏向繁體中文 Big5）。
pub fn decode(bytes: &[u8]) -> Decoded {
    if let Some((encoding, len)) = Encoding::for_bom(bytes) {
        return finish(&bytes[len..], encoding, true);
    }
    if std::str::from_utf8(bytes).is_ok() {
        return finish(bytes, UTF_8, false);
    }
    let mut detector = EncodingDetector::new(Iso2022JpDetection::Deny);
    detector.feed(&bytes[..bytes.len().min(DETECT_SAMPLE)], true);
    let encoding = detector.guess(Some(b"tw"), Utf8Detection::Deny);
    finish(bytes, encoding, false)
}

/// 以指定編碼解碼（使用者覆寫自動偵測時用）。若檔案有與該編碼相符的 BOM 會被剝除。
pub fn decode_with(bytes: &[u8], encoding: &'static Encoding) -> Decoded {
    match Encoding::for_bom(bytes) {
        Some((bom_enc, len)) if bom_enc == encoding => finish(&bytes[len..], encoding, true),
        _ => finish(bytes, encoding, false),
    }
}

fn finish(bytes: &[u8], encoding: &'static Encoding, bom: bool) -> Decoded {
    let (cow, lossy) = encoding.decode_without_bom_handling(bytes);
    let (text, line_ending, mixed) = normalize_newlines(cow.into_owned());
    Decoded {
        text,
        format: FileFormat {
            encoding,
            bom,
            line_ending,
        },
        lossy,
        mixed_line_endings: mixed,
    }
}

/// 回傳（統一為 `\n` 的文字、主要換行格式、是否混用）。
fn normalize_newlines(text: String) -> (String, LineEnding, bool) {
    let (mut lf, mut crlf, mut cr) = (0usize, 0usize, 0usize);
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\r' if bytes.get(i + 1) == Some(&b'\n') => {
                crlf += 1;
                i += 1;
            }
            b'\r' => cr += 1,
            b'\n' => lf += 1,
            _ => {}
        }
        i += 1;
    }
    let kinds = [lf, crlf, cr].iter().filter(|&&n| n > 0).count();
    let dominant = if crlf > 0 && crlf >= lf && crlf >= cr {
        LineEnding::CrLf
    } else if lf > 0 && lf >= cr {
        LineEnding::Lf
    } else if cr > 0 {
        LineEnding::Cr
    } else {
        LineEnding::platform()
    };
    let text = if crlf + cr > 0 {
        text.replace("\r\n", "\n").replace('\r', "\n")
    } else {
        text
    };
    (text, dominant, kinds > 1)
}

/// 目標編碼無法表示某個字元。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnmappableChar {
    pub ch: char,
    /// 1 起算的行號與欄號（欄以字元計）。
    pub line: usize,
    pub column: usize,
}

impl fmt::Display for UnmappableChar {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "字元「{}」(U+{:04X}) 無法以目標編碼表示（第 {} 行，第 {} 欄）",
            self.ch, self.ch as u32, self.line, self.column
        )
    }
}

impl std::error::Error for UnmappableChar {}

/// 依格式編碼文字。遇到目標編碼無法表示的字元時回傳錯誤，絕不靜默替換。
pub fn encode(text: &str, format: &FileFormat) -> Result<Vec<u8>, UnmappableChar> {
    // 先統一換行（巨集可能插入了 \r\n），再轉成目標換行
    let unified;
    let text = if text.contains('\r') {
        unified = text.replace("\r\n", "\n").replace('\r', "\n");
        unified.as_str()
    } else {
        text
    };
    let converted;
    let text = if format.line_ending == LineEnding::Lf {
        text
    } else {
        converted = text.replace('\n', format.line_ending.as_str());
        converted.as_str()
    };

    let enc = format.encoding;
    if enc == UTF_8 {
        let mut out = Vec::with_capacity(text.len() + 3);
        if format.bom {
            out.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
        }
        out.extend_from_slice(text.as_bytes());
        return Ok(out);
    }
    if enc == UTF_16LE || enc == UTF_16BE {
        // encoding_rs 不提供 UTF-16 編碼輸出，自己寫
        let le = enc == UTF_16LE;
        let mut out = Vec::with_capacity(text.len() * 2 + 2);
        let mut push = |u: u16| {
            out.extend_from_slice(&if le { u.to_le_bytes() } else { u.to_be_bytes() });
        };
        if format.bom {
            push(0xFEFF);
        }
        text.encode_utf16().for_each(&mut push);
        return Ok(out);
    }

    let (bytes, _, had_unmappable) = enc.encode(text);
    if had_unmappable {
        return Err(find_unmappable(text, enc));
    }
    Ok(bytes.into_owned())
}

/// 找出第一個無法編碼的字元（只在錯誤路徑呼叫，效能不重要）。
fn find_unmappable(text: &str, enc: &'static Encoding) -> UnmappableChar {
    let (mut line, mut column) = (1, 1);
    let mut buf = [0u8; 4];
    for ch in text.chars() {
        let (_, _, bad) = enc.encode(ch.encode_utf8(&mut buf));
        if bad {
            return UnmappableChar { ch, line, column };
        }
        if ch == '\n' {
            line += 1;
            column = 1;
        } else {
            column += 1;
        }
    }
    // 理論上不會到這裡（整體編碼報錯就一定有某個字元失敗）
    UnmappableChar {
        ch: '\u{FFFD}',
        line,
        column,
    }
}

#[derive(Debug)]
pub enum SaveError {
    Encode(UnmappableChar),
    Io(std::io::Error),
}

impl fmt::Display for SaveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SaveError::Encode(e) => e.fmt(f),
            SaveError::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for SaveError {}

impl From<std::io::Error> for SaveError {
    fn from(e: std::io::Error) -> Self {
        SaveError::Io(e)
    }
}

/// 讀檔並解碼。
pub fn read_file(path: &Path) -> std::io::Result<(Vec<u8>, Decoded)> {
    let bytes = std::fs::read(path)?;
    let decoded = decode(&bytes);
    Ok((bytes, decoded))
}

/// 安全存檔：先編碼（失敗就完全不動磁碟），再寫入同目錄暫存檔，成功後才改名覆蓋原檔。
/// 因此任何失敗都不會破壞原本的檔案。
pub fn write_file(path: &Path, text: &str, format: &FileFormat) -> Result<(), SaveError> {
    let bytes = encode(text, format).map_err(SaveError::Encode)?;

    let dir = path.parent().filter(|p| !p.as_os_str().is_empty());
    let name = path
        .file_name()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "路徑沒有檔名"))?;
    let mut tmp_name = std::ffi::OsString::from(".");
    tmp_name.push(name);
    tmp_name.push(".feditor-tmp");
    let tmp = match dir {
        Some(d) => d.join(&tmp_name),
        None => tmp_name.into(),
    };

    let result = (|| -> std::io::Result<()> {
        let mut file = std::fs::File::create(&tmp)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result.map_err(SaveError::Io)
}

#[cfg(test)]
mod tests {
    use super::*;
    use encoding_rs::BIG5;

    const ZH: &str = "臺灣是位於東亞的島嶼，擁有豐富的自然景觀與多元的人文風貌。\
        許多人喜歡在週末前往山區健行，或是到海邊欣賞日出。\n\
        夜市裡有各式各樣的小吃，例如滷肉飯、珍珠奶茶、蚵仔煎與臭豆腐，深受遊客喜愛。\n";

    fn fmt(enc: &'static Encoding, bom: bool, le: LineEnding) -> FileFormat {
        FileFormat {
            encoding: enc,
            bom,
            line_ending: le,
        }
    }

    #[test]
    fn plain_utf8() {
        let d = decode("hello\n你好\n".as_bytes());
        assert_eq!(d.format.encoding, UTF_8);
        assert!(!d.format.bom);
        assert!(!d.lossy);
        assert_eq!(d.text, "hello\n你好\n");
        assert_eq!(d.format.line_ending, LineEnding::Lf);
    }

    #[test]
    fn empty_file() {
        let d = decode(b"");
        assert_eq!(d.text, "");
        assert_eq!(d.format.encoding, UTF_8);
        assert_eq!(d.format.line_ending, LineEnding::platform());
    }

    #[test]
    fn utf8_bom_roundtrip() {
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice("你好".as_bytes());
        let d = decode(&bytes);
        assert!(d.format.bom);
        assert_eq!(d.text, "你好");
        assert_eq!(encode(&d.text, &d.format).unwrap(), bytes);
    }

    #[test]
    fn utf16_roundtrip_with_surrogates() {
        for enc in [UTF_16LE, UTF_16BE] {
            let f = fmt(enc, true, LineEnding::CrLf);
            let bytes = encode("a😀你\nb", &f).unwrap();
            let d = decode(&bytes);
            assert_eq!(d.format.encoding, enc);
            assert!(d.format.bom);
            assert_eq!(d.text, "a😀你\nb");
            assert_eq!(d.format.line_ending, LineEnding::CrLf);
            assert!(!d.lossy);
        }
    }

    #[test]
    fn big5_is_detected_and_roundtrips() {
        let f = fmt(BIG5, false, LineEnding::Lf);
        let bytes = encode(ZH, &f).unwrap();
        assert!(std::str::from_utf8(&bytes).is_err());
        let d = decode(&bytes);
        assert_eq!(d.format.encoding, BIG5, "偵測成 {}", d.format.encoding.name());
        assert_eq!(d.text, ZH);
        assert!(!d.lossy);
        assert_eq!(encode(&d.text, &d.format).unwrap(), bytes);
    }

    #[test]
    fn decode_with_overrides_detection() {
        let bytes = encode("你好", &fmt(BIG5, false, LineEnding::Lf)).unwrap();
        assert_eq!(decode_with(&bytes, BIG5).text, "你好");
        assert_ne!(decode_with(&bytes, UTF_8).text, "你好");
    }

    #[test]
    fn invalid_bytes_are_flagged_lossy() {
        let d = decode_with(&[b'a', 0xFF, b'b'], UTF_8);
        assert!(d.lossy);
        assert!(d.text.contains('\u{FFFD}'));
    }

    #[test]
    fn crlf_is_normalized_and_restored() {
        let d = decode(b"a\r\nb\r\nc");
        assert_eq!(d.text, "a\nb\nc");
        assert_eq!(d.format.line_ending, LineEnding::CrLf);
        assert!(!d.mixed_line_endings);
        assert_eq!(encode(&d.text, &d.format).unwrap(), b"a\r\nb\r\nc");
    }

    #[test]
    fn lone_cr_is_supported() {
        let d = decode(b"a\rb\r");
        assert_eq!(d.text, "a\nb\n");
        assert_eq!(d.format.line_ending, LineEnding::Cr);
    }

    #[test]
    fn mixed_line_endings_are_flagged_and_unified() {
        let d = decode(b"a\r\nb\r\nc\nd\n");
        assert!(d.mixed_line_endings);
        assert_eq!(d.format.line_ending, LineEnding::CrLf);
        assert_eq!(d.text, "a\nb\nc\nd\n");
    }

    #[test]
    fn macro_inserted_crlf_does_not_double() {
        let f = fmt(UTF_8, false, LineEnding::CrLf);
        assert_eq!(encode("a\r\nb\nc", &f).unwrap(), b"a\r\nb\r\nc");
    }

    #[test]
    fn unmappable_char_is_an_error_not_a_silent_replacement() {
        let f = fmt(BIG5, false, LineEnding::Lf);
        let e = encode("第一行\nab😀c", &f).unwrap_err();
        assert_eq!(e.ch, '😀');
        assert_eq!((e.line, e.column), (2, 3));
    }

    #[test]
    fn write_then_read_back() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.txt");
        let f = fmt(BIG5, false, LineEnding::CrLf);
        write_file(&path, ZH, &f).unwrap();
        let (_, d) = read_file(&path).unwrap();
        assert_eq!(d.text, ZH);
        assert_eq!(d.format.encoding, BIG5);
        assert_eq!(d.format.line_ending, LineEnding::CrLf);
        let leftovers: Vec<_> = std::fs::read_dir(dir.path()).unwrap().collect();
        assert_eq!(leftovers.len(), 1, "不應留下暫存檔");
    }

    #[test]
    fn failed_save_leaves_original_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.txt");
        std::fs::write(&path, "原本的內容").unwrap();
        let f = fmt(BIG5, false, LineEnding::Lf);
        let r = write_file(&path, "新內容😀", &f);
        assert!(matches!(r, Err(SaveError::Encode(_))));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "原本的內容");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn save_overwrites_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.txt");
        std::fs::write(&path, "old old old old").unwrap();
        write_file(&path, "new", &fmt(UTF_8, false, LineEnding::Lf)).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new");
    }

    #[test]
    fn set_encoding_adjusts_bom() {
        let mut f = FileFormat::default();
        f.set_encoding(UTF_16LE);
        assert!(f.bom);
        f.set_encoding(BIG5);
        assert!(!f.bom);
        assert_eq!(f.encoding_label(), "Big5");
        f.set_encoding(UTF_8);
        f.bom = true;
        assert_eq!(f.encoding_label(), "UTF-8 BOM");
    }
}
