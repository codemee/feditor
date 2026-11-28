//! 從 shell 的輸出裡撈出 alacritty_terminal 不處理的 OSC 序列：
//! - OSC 7：`file://主機/路徑`，shell 回報目前資料夾；
//! - OSC 9;9：ConEmu／Windows Terminal 的「目前資料夾」，路徑直接寫在後面；
//! - 通知：OSC 9（iTerm2）、OSC 777;notify（rxvt）、OSC 99（kitty）。
//!
//! alacritty_terminal 自己讀 PTY、自己剖析，沒有地方讓我們看原始位元組，所以把 PTY 包一層（[`Tapped`]）：
//! 讀出來的資料先給 [`Scanner`] 看一遍，再原封不動交給它。

use std::io;
use std::sync::Arc;

use alacritty_terminal::event::{OnResize, WindowSize};
use alacritty_terminal::tty::{ChildEvent, EventedPty, EventedReadWrite};
use polling::{Event, PollMode, Poller};

/// 撈到的東西。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Osc {
    /// shell 回報的目前資料夾（原始內容：`file://` 網址或路徑，還沒整理，見 [`report_to_path`]）。
    Cwd(String),
    /// 程式要通知使用者（內容不重要，只要知道有通知）。
    Notify,
    /// 程式問「你是什麼終端機」（XTVERSION：`CSI > q` 或 `CSI > 0 q`），要回 [`XTVERSION_REPLY`]。
    /// 這不是 OSC，但 alacritty_terminal 不回應，一樣在這裡攔下來。
    XtVersion,
}

/// 回答 XTVERSION 的內容：`DCS > | 名稱 版本 ST`。
pub fn xtversion_reply() -> String {
    format!("\x1bP>|feditor {}\x1b\\", env!("CARGO_PKG_VERSION"))
}

/// 單一序列最多留這麼多位元組；超過的（OSC 52 貼大量資料之類）整段忽略。
const MAX_PAYLOAD: usize = 4096;

#[derive(Default, Clone, Copy, PartialEq, Eq)]
enum State {
    #[default]
    Ground,
    /// 剛收到 ESC。
    Esc,
    /// 在 `ESC ]` 之後，收集內容中。
    Osc,
    /// 在 OSC 內容裡收到 ESC（可能是 `ESC \` 結尾）。
    OscEsc,
    /// 在 `ESC [` 之後，收集參數與中間字元，等結尾字元。
    Csi,
}

/// CSI 的參數最多收這麼多位元組；我們只認很短的 `> 0 q`，超過的一定不是。
const MAX_CSI: usize = 8;

/// 跨資料區塊的 OSC 掃描器：一個序列被切在兩次讀取之間也找得到。
#[derive(Default)]
pub struct Scanner {
    state: State,
    buf: Vec<u8>,
    overflow: bool,
}

impl Scanner {
    pub fn feed(&mut self, bytes: &[u8], out: &mut Vec<Osc>) {
        for &b in bytes {
            match self.state {
                State::Ground => {
                    if b == 0x1b {
                        self.state = State::Esc;
                    }
                }
                State::Esc => match b {
                    b']' => self.begin(),
                    b'[' => {
                        self.state = State::Csi;
                        self.buf.clear();
                    }
                    0x1b => {}
                    _ => self.state = State::Ground,
                },
                State::Csi => match b {
                    // 參數（0x30–0x3F）與中間字元（0x20–0x2F）
                    0x20..=0x3f => {
                        if self.buf.len() < MAX_CSI {
                            self.buf.push(b);
                        } else {
                            self.state = State::Ground;
                        }
                    }
                    // 結尾字元：只認 XTVERSION
                    0x40..=0x7e => {
                        if b == b'q' && (self.buf == b">" || self.buf == b">0") {
                            out.push(Osc::XtVersion);
                        }
                        self.state = State::Ground;
                        self.buf.clear();
                    }
                    0x1b => self.state = State::Esc,
                    _ => self.state = State::Ground, // CAN、SUB 與其他控制字元：放棄這個序列
                },
                State::Osc => match b {
                    0x07 => self.finish(out),
                    0x1b => self.state = State::OscEsc,
                    0x18 | 0x1a => self.state = State::Ground, // CAN、SUB：取消這個序列
                    _ => {
                        if self.buf.len() < MAX_PAYLOAD {
                            self.buf.push(b);
                        } else {
                            self.overflow = true;
                        }
                    }
                },
                State::OscEsc => match b {
                    b'\\' => self.finish(out),
                    b']' => self.begin(),
                    0x1b => self.state = State::Esc,
                    _ => self.state = State::Ground,
                },
            }
        }
    }

    fn begin(&mut self) {
        self.state = State::Osc;
        self.buf.clear();
        self.overflow = false;
    }

    fn finish(&mut self, out: &mut Vec<Osc>) {
        self.state = State::Ground;
        if !self.overflow {
            if let Some(osc) = parse(&self.buf) {
                out.push(osc);
            }
        }
        self.buf.clear();
    }
}

fn parse(payload: &[u8]) -> Option<Osc> {
    let text = String::from_utf8_lossy(payload);
    let (number, rest) = text.split_once(';')?;
    match number {
        "7" => Some(Osc::Cwd(rest.to_owned())),
        "9" => {
            if let Some(path) = rest.strip_prefix("9;") {
                // ConEmu：`9;9;"路徑"`
                return Some(Osc::Cwd(path.trim_matches('"').to_owned()));
            }
            let mut chars = rest.chars();
            // `9;1;…`、`9;4;…`（進度條）這類 ConEmu 的指令不是通知
            if chars.next().is_some_and(|c| c.is_ascii_digit()) && chars.next() == Some(';') {
                return None;
            }
            (!rest.is_empty()).then_some(Osc::Notify)
        }
        "777" => rest.starts_with("notify;").then_some(Osc::Notify),
        "99" => {
            // kitty：`99;參數;內容`。參數是 `key=value` 用冒號隔開：d=0 表示後面還有分段（等最後一段再通知）；
            // p= 是內容種類，只有標題與本文算通知（close、alive、? 是查詢或關閉）
            let metadata = rest.split(';').next().unwrap_or("");
            let mut notify = true;
            for pair in metadata.split(':') {
                match pair.split_once('=') {
                    Some(("d", "0")) => notify = false,
                    Some(("p", kind)) if !matches!(kind, "title" | "body") => notify = false,
                    _ => {}
                }
            }
            notify.then_some(Osc::Notify)
        }
        _ => None,
    }
}

/// 把 shell 回報的目前資料夾整理成 Windows 上可用的路徑；不是這台電腦上能用的路徑就是 `None`。
/// `computer` 是這台電腦的名稱（`file://` 的主機要是空的、`localhost` 或它才算本機）。
///
/// 認得：`file:///C:/Users/me`、`file://電腦名稱/C:/Users/me`、Git Bash 的 `file://電腦名稱/c/Users/me`，
/// 以及 ConEmu 直接寫的 `C:\Users\me`。WSL 的 `/home/me` 在 Windows 上不是路徑，不收。
pub fn report_to_path(raw: &str, computer: Option<&str>) -> Option<String> {
    let path = match raw.strip_prefix("file://") {
        Some(rest) => {
            let slash = rest.find('/')?;
            let host = &rest[..slash];
            let local = host.is_empty()
                || host.eq_ignore_ascii_case("localhost")
                || computer.is_some_and(|c| host.eq_ignore_ascii_case(c));
            if !local {
                return None;
            }
            let decoded = percent_decode(&rest[slash..])?;
            let p = decoded.strip_prefix('/').unwrap_or(&decoded);
            let bytes = p.as_bytes();
            if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
                p.to_owned() // /C:/Users/me
            } else if bytes.first().is_some_and(u8::is_ascii_alphabetic)
                && (bytes.len() == 1 || bytes[1] == b'/')
            {
                format!("{}:/{}", bytes[0] as char, p.get(2..).unwrap_or("")) // /c/Users/me（MSYS）
            } else {
                return None;
            }
        }
        None => {
            let b = raw.as_bytes();
            let drive = b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':';
            if !(drive || raw.starts_with("\\\\")) {
                return None;
            }
            raw.to_owned()
        }
    };
    Some(path)
}

fn percent_decode(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let hex = s.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// 除錯用的記錄（環境變數 `FEDITOR_PTY_DUMP` 有設才會有，平常不啟用）：每個終端機一份文字記錄（`.log`：
/// 什麼時間讀到幾個位元組、視窗大小變了、送了什麼給 shell）和一份原始輸出（`.bin`：shell 送來的位元組原封不動）。
/// 終端機畫面出問題時，把這兩個檔案拿去重播就能重現。
pub struct DumpLog {
    file: std::sync::Mutex<std::fs::File>,
    start: std::time::Instant,
}

impl DumpLog {
    pub fn note(&self, text: &str) {
        use std::io::Write;
        if let Ok(mut file) = self.file.lock() {
            let _ = writeln!(file, "{:>9.3} {text}", self.start.elapsed().as_secs_f64());
        }
    }
}

/// 依 `FEDITOR_PTY_DUMP`（檔名前綴）開一組記錄檔：`<前綴>-<行程編號>-<序號>.bin` 與 `.log`。沒設就是 `None`。
pub fn open_dump() -> Option<(std::fs::File, Arc<DumpLog>)> {
    static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let base = std::env::var_os("FEDITOR_PTY_DUMP").filter(|b| !b.is_empty())?;
    let name = format!(
        "{}-{}-{}",
        base.to_string_lossy(),
        std::process::id(),
        COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    );
    let bin = std::fs::File::create(format!("{name}.bin")).ok()?;
    let log = std::fs::File::create(format!("{name}.log")).ok()?;
    Some((
        bin,
        Arc::new(DumpLog {
            file: std::sync::Mutex::new(log),
            start: std::time::Instant::now(),
        }),
    ))
}

/// 讀的時候順便把資料給 [`Scanner`] 看的 PTY 讀取端。
pub struct TapReader<R> {
    inner: *mut R,
    scanner: Scanner,
    found: Vec<Osc>,
    on_found: Box<dyn FnMut(Osc) + Send>,
    dump: Option<(std::fs::File, Arc<DumpLog>)>,
    /// 事件迴圈登記讀取時給的（poller, 事件），讀到資料之後用來補投喚醒（見 [`TapReader::nudge`]）。
    wake: Arc<std::sync::Mutex<Option<(Arc<Poller>, Event)>>>,
}

impl<R> TapReader<R> {
    /// 讀到資料之後，替事件迴圈再補投一個「可讀」事件，讓它多讀一次、一路讀到管線真的空了。
    ///
    /// Windows 上 alacritty_terminal 的輸出是先由背景執行緒收進一條內部管線，管線有新資料才喚醒事件迴圈；
    /// 但事件迴圈每次最多處理約 64KB 就會先讓出（`MAX_LOCKED_READ`），管線裡可能還剩資料，而「下次有資料再喚醒」
    /// 要等讀到管線空了才會重新登記。程式一次吐出一大塊（Claude Code 還原很長的對話）就停下來時，
    /// 剩下的就留在管線裡沒人去讀，直到下一次輸入才被讀出來，畫面看起來停在中途。補投這個事件讓迴圈再跑一次：
    /// 那次讀到空，喚醒才會重新登記好。其他平台的輪詢是水平觸發，不需要。
    fn nudge(&self) {
        #[cfg(windows)]
        if let Some((poller, _)) = self.wake.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            use polling::os::iocp::{CompletionPacket, PollerIocpExt};
            // 事件迴圈只認 `PTY_READ_WRITE_TOKEN` 這個鍵（Windows 的 PTY 登記時把迴圈給的鍵換成它），用別的鍵會被忽略
            let _ = poller.post(CompletionPacket::new(Event::readable(
                alacritty_terminal::tty::PTY_READ_WRITE_TOKEN,
            )));
        }
    }
}

// SAFETY: `inner` 指向同一個 `Tapped` 裡面、放在 `Box` 中的 PTY 讀取端，只有這個 `TapReader` 會用它；
// 整個 `Tapped` 會跟著事件迴圈的執行緒一起搬過去，不會同時被兩個執行緒碰
unsafe impl<R: Send> Send for TapReader<R> {}

impl<R: io::Read> io::Read for TapReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        // SAFETY: 見 `Tapped::new`：`inner` 指向 `Box<P>` 裡的讀取端，`Tapped` 活著它就有效，而且沒有別的參考同時在用
        let n = unsafe { &mut *self.inner }.read(buf)?;
        if let Some((bin, log)) = &mut self.dump {
            use std::io::Write;
            let _ = bin.write_all(&buf[..n]);
            log.note(&format!("read {n} bytes"));
        }
        if n > 0 {
            self.nudge();
        }
        self.scanner.feed(&buf[..n], &mut self.found);
        for osc in self.found.drain(..) {
            (self.on_found)(osc);
        }
        Ok(n)
    }
}

/// 包住一個 PTY，行為和它一模一樣，只是讀出來的資料會先給 [`Scanner`] 看。
pub struct Tapped<P: EventedReadWrite> {
    pty: Box<P>,
    reader: TapReader<P::Reader>,
    /// 和 `reader.wake` 是同一份：登記讀取時記下 poller 與事件。
    wake: Arc<std::sync::Mutex<Option<(Arc<Poller>, Event)>>>,
}

impl<P: EventedReadWrite> Tapped<P> {
    pub fn new(mut pty: Box<P>, on_found: impl FnMut(Osc) + Send + 'static) -> Self {
        // 讀取端在 `Box` 裡，位置不會因為 `Tapped` 被搬動而改變
        let inner: *mut P::Reader = pty.reader();
        let wake = Arc::new(std::sync::Mutex::new(None));
        Self {
            pty,
            reader: TapReader {
                inner,
                scanner: Scanner::default(),
                found: Vec::new(),
                on_found: Box::new(on_found),
                dump: None,
                wake: wake.clone(),
            },
            wake,
        }
    }

    fn remember(&self, poller: &Arc<Poller>, event: Event) {
        *self.wake.lock().unwrap_or_else(|e| e.into_inner()) = Some((poller.clone(), event));
    }

    /// 開啟除錯記錄（見 [`open_dump`]）。
    pub fn with_dump(mut self, dump: Option<(std::fs::File, Arc<DumpLog>)>) -> Self {
        self.reader.dump = dump;
        self
    }
}

impl<P: EventedReadWrite> EventedReadWrite for Tapped<P> {
    type Reader = TapReader<P::Reader>;
    type Writer = P::Writer;

    unsafe fn register(
        &mut self,
        poller: &Arc<Poller>,
        event: Event,
        mode: PollMode,
    ) -> io::Result<()> {
        self.remember(poller, event);
        unsafe { self.pty.register(poller, event, mode) }
    }
    fn reregister(&mut self, poller: &Arc<Poller>, event: Event, mode: PollMode) -> io::Result<()> {
        self.remember(poller, event);
        self.pty.reregister(poller, event, mode)
    }
    fn deregister(&mut self, poller: &Arc<Poller>) -> io::Result<()> {
        self.pty.deregister(poller)
    }
    fn reader(&mut self) -> &mut Self::Reader {
        &mut self.reader
    }
    fn writer(&mut self) -> &mut Self::Writer {
        self.pty.writer()
    }
}

impl<P: EventedPty> EventedPty for Tapped<P> {
    fn next_child_event(&mut self) -> Option<ChildEvent> {
        self.pty.next_child_event()
    }
}

impl<P: EventedReadWrite + OnResize> OnResize for Tapped<P> {
    fn on_resize(&mut self, size: WindowSize) {
        self.pty.on_resize(size)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::sync::Mutex;

    fn scan(chunks: &[&[u8]]) -> Vec<Osc> {
        let mut s = Scanner::default();
        let mut out = Vec::new();
        for c in chunks {
            s.feed(c, &mut out);
        }
        out
    }

    #[test]
    fn cwd_reports_and_notifications_are_found_with_either_terminator() {
        assert_eq!(
            scan(&[b"\x1b]7;file:///C:/a\x07"]),
            vec![Osc::Cwd("file:///C:/a".into())]
        );
        assert_eq!(
            scan(&[b"x\x1b]7;file:///C:/a\x1b\\y"]),
            vec![Osc::Cwd("file:///C:/a".into())]
        );
        assert_eq!(
            scan(&[b"\x1b]9;9;\"C:\\Users\\me\"\x07"]),
            vec![Osc::Cwd("C:\\Users\\me".into())]
        );
        assert_eq!(scan(&[b"\x1b]9;build done\x07"]), vec![Osc::Notify]);
        assert_eq!(
            scan(&[b"\x1b]777;notify;Title;Body\x07"]),
            vec![Osc::Notify]
        );
        assert_eq!(scan(&[b"\x1b]99;i=1:d=1;Hello\x1b\\"]), vec![Osc::Notify]);
        assert_eq!(
            scan(&[b"\x1b]99;;Hello\x1b\\"]),
            vec![Osc::Notify],
            "沒有參數也算"
        );
    }

    #[test]
    fn xtversion_questions_are_found_but_other_csi_sequences_are_not() {
        assert_eq!(scan(&[b"\x1b[>q"]), vec![Osc::XtVersion]);
        assert_eq!(scan(&[b"\x1b[>0q"]), vec![Osc::XtVersion]);
        // Claude Code 啟動時的一串查詢：只有 XTVERSION 是我們的
        assert_eq!(
            scan(&[b"\x1b[>0q\x1b[?u\x1b]7501;?\x1b\\\x1b[c"]),
            vec![Osc::XtVersion]
        );
        assert_eq!(
            scan(&[b"\x1b[>1u\x1b[?25h\x1b[1;2q\x1b[>5q\x1b[q\x1b[0q\x1b[>0c"]),
            vec![]
        );
        // 被切在兩次讀取之間
        assert_eq!(scan(&[b"abc\x1b[", b">0", b"q"]), vec![Osc::XtVersion]);
        // 被 CAN 取消、或夾在文字裡
        assert_eq!(scan(&[b"\x1b[>0\x18q"]), vec![]);
        assert_eq!(scan(&[b"ls\r\n\x1b[>0q$ "]), vec![Osc::XtVersion]);
        // 太長的 CSI 不會一直收下去
        assert_eq!(scan(&[b"\x1b[>000000000000000q"]), vec![]);
        // CSI 之後緊接著 OSC 仍然正常
        assert_eq!(scan(&[b"\x1b[0m\x1b]9;done\x07"]), vec![Osc::Notify]);
        assert_eq!(
            xtversion_reply(),
            format!("\x1bP>|feditor {}\x1b\\", env!("CARGO_PKG_VERSION"))
        );
    }

    #[test]
    fn things_that_are_not_notifications_are_left_alone() {
        // 標題、剪貼簿、超連結：別的地方處理
        assert_eq!(
            scan(&[b"\x1b]0;title\x07\x1b]52;c;aGk=\x07\x1b]8;;https://a.b\x07"]),
            vec![]
        );
        // ConEmu 的進度條與其他指令
        assert_eq!(scan(&[b"\x1b]9;4;1;50\x07\x1b]9;1;100\x07"]), vec![]);
        // rxvt 的其他子指令；kitty 的分段（d=0）、關閉、查詢
        assert_eq!(
            scan(&[
                b"\x1b]777;preexec\x07\x1b]99;i=1:d=0;Hel\x07\x1b]99;p=close;\x07\x1b]99;p=?;\x07"
            ]),
            vec![]
        );
        assert_eq!(
            scan(&[b"\x1b]99;i=1:d=0:p=title;Hel\x07\x1b]99;i=1:p=body;lo\x07"]),
            vec![Osc::Notify],
            "分段的通知，只在最後一段響一次"
        );
        // 沒有結尾就被 CAN 取消
        assert_eq!(scan(&[b"\x1b]9;hello\x18 \x07"]), vec![]);
    }

    #[test]
    fn a_sequence_split_across_reads_is_still_found() {
        let whole: &[u8] = b"abc\x1b]7;file:///C:/Users/me\x1b\\def";
        for cut in 1..whole.len() {
            let found = scan(&[&whole[..cut], &whole[cut..]]);
            assert_eq!(
                found,
                vec![Osc::Cwd("file:///C:/Users/me".into())],
                "切在第 {cut} 個位元組"
            );
        }
    }

    #[test]
    fn an_oversized_sequence_is_ignored_and_the_scanner_recovers() {
        let mut big = b"\x1b]9;".to_vec();
        big.extend(std::iter::repeat(b'x').take(MAX_PAYLOAD * 3));
        big.extend_from_slice(b"\x07\x1b]9;ok\x07");
        assert_eq!(
            scan(&[&big]),
            vec![Osc::Notify],
            "太長的整段忽略，後面正常的照收"
        );
    }

    #[test]
    fn reported_folders_become_windows_paths_only_when_they_are_local() {
        let me = Some("MY-PC");
        assert_eq!(
            report_to_path("file:///C:/Users/me", me).as_deref(),
            Some("C:/Users/me")
        );
        assert_eq!(
            report_to_path("file://MY-PC/C:/My%20Docs", me).as_deref(),
            Some("C:/My Docs")
        );
        assert_eq!(
            report_to_path("file://localhost/d/work", me).as_deref(),
            Some("d:/work")
        );
        assert_eq!(
            report_to_path("file://my-pc/c/Users/me", me).as_deref(),
            Some("c:/Users/me")
        );
        assert_eq!(
            report_to_path("file://other-host/C:/x", me),
            None,
            "別台電腦的路徑"
        );
        assert_eq!(
            report_to_path("file:///home/me", me),
            None,
            "WSL 的路徑在 Windows 上不是路徑"
        );
        assert_eq!(
            report_to_path("file://MY-PC/%ZZ", me),
            None,
            "壞掉的百分比編碼"
        );
        assert_eq!(
            report_to_path(r"C:\Users\me", me).as_deref(),
            Some(r"C:\Users\me")
        );
        assert_eq!(
            report_to_path(r"\\server\share", me).as_deref(),
            Some(r"\\server\share")
        );
        assert_eq!(report_to_path("/home/me", me), None);
    }

    /// 假的 PTY：讀出固定的資料，其他都不做。
    struct Fake(io::Cursor<Vec<u8>>, Vec<u8>);
    impl EventedReadWrite for Fake {
        type Reader = io::Cursor<Vec<u8>>;
        type Writer = Vec<u8>;
        unsafe fn register(&mut self, _: &Arc<Poller>, _: Event, _: PollMode) -> io::Result<()> {
            Ok(())
        }
        fn reregister(&mut self, _: &Arc<Poller>, _: Event, _: PollMode) -> io::Result<()> {
            Ok(())
        }
        fn deregister(&mut self, _: &Arc<Poller>) -> io::Result<()> {
            Ok(())
        }
        fn reader(&mut self) -> &mut Self::Reader {
            &mut self.0
        }
        fn writer(&mut self) -> &mut Self::Writer {
            &mut self.1
        }
    }

    #[test]
    fn the_tapped_reader_passes_every_byte_through_and_reports_what_it_saw() {
        let data = b"hello \x1b]9;done\x07 world \x1b]7;file:///C:/x\x07".to_vec();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let sink = seen.clone();
        let mut tapped = Tapped::new(
            Box::new(Fake(io::Cursor::new(data.clone()), Vec::new())),
            move |o| sink.lock().unwrap().push(o),
        );
        // 搬動 `Tapped`（像交給事件迴圈那樣）之後，讀取端的位置也要還是對的
        let mut moved = Box::new(tapped);
        tapped = *moved;
        moved = Box::new(tapped);
        let mut got = Vec::new();
        let mut chunk = [0u8; 7];
        loop {
            let n = moved.reader().read(&mut chunk).unwrap();
            if n == 0 {
                break;
            }
            got.extend_from_slice(&chunk[..n]);
        }
        assert_eq!(got, data, "資料原封不動");
        assert_eq!(
            *seen.lock().unwrap(),
            vec![Osc::Notify, Osc::Cwd("file:///C:/x".into())]
        );
    }

    /// 事件迴圈每次只處理一部分就讓出，管線裡可能還剩資料；讀到資料之後要替它補投一個「可讀」事件，
    /// 不然剩下的要等到下一次輸入才會被讀出來（Claude Code 還原很長的對話時畫面停在中途）。
    #[cfg(windows)]
    #[test]
    fn reading_data_posts_a_readable_event_so_leftover_output_is_not_stranded() {
        let poller = Arc::new(Poller::new().unwrap());
        let mut tapped = Tapped::new(
            Box::new(Fake(io::Cursor::new(b"abc".to_vec()), Vec::new())),
            |_| {},
        );
        // 事件迴圈登記讀取（假的 PTY 自己什麼事件也不會送）
        unsafe {
            tapped
                .register(&poller, Event::readable(0), PollMode::Level)
                .unwrap()
        };
        let mut events = polling::Events::new();
        poller
            .wait(&mut events, Some(std::time::Duration::from_millis(50)))
            .unwrap();
        assert!(events.iter().next().is_none(), "還沒讀到資料之前沒有事件");
        let mut buf = [0u8; 8];
        assert_eq!(tapped.reader().read(&mut buf).unwrap(), 3);
        events.clear();
        poller
            .wait(&mut events, Some(std::time::Duration::from_secs(2)))
            .unwrap();
        let event = events.iter().next().expect("讀到資料之後要補投事件");
        assert!(
            event.readable && event.key == alacritty_terminal::tty::PTY_READ_WRITE_TOKEN,
            "事件迴圈只認這個鍵：{}",
            event.key
        );
        // 讀到空（資料讀完了）就不再補投，迴圈不會空轉
        events.clear();
        assert_eq!(tapped.reader().read(&mut buf).unwrap(), 0);
        poller
            .wait(&mut events, Some(std::time::Duration::from_millis(100)))
            .unwrap();
        assert!(events.iter().next().is_none(), "讀到空不補投");
    }
}
