//! 終端機：在窗格裡跑系統的 shell。
//!
//! 偽終端機（Windows 的 ConPTY、Unix 的 pty）、跳脫序列的解析與畫面格子都交給 `alacritty_terminal`；
//! 這裡只負責開 shell、改變大小、把按鍵翻成要送給 shell 的位元組。畫面與輸入的處理在 `terminal_ui.rs`。
//! 與畫面無關的部分（挑 shell、按鍵編碼）可以單元測試。

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use alacritty_terminal::event::{Event, EventListener, WindowSize};
use alacritty_terminal::event_loop::{EventLoop, EventLoopSender, Msg};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::{Config, Term, TermMode};
use alacritty_terminal::tty;
use eframe::egui;

/// 終端機大小（格數）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GridSize {
    pub cols: usize,
    pub lines: usize,
}

impl Dimensions for GridSize {
    fn total_lines(&self) -> usize {
        self.lines
    }
    fn screen_lines(&self) -> usize {
        self.lines
    }
    fn columns(&self) -> usize {
        self.cols
    }
}

/// 背景執行緒收到的事件：標題、結束、要回寫給 shell 的資料。
#[derive(Default)]
struct Shared {
    exited: AtomicBool,
    sender: parking_lot_free::Mutex<Option<EventLoopSender>>,
}

/// `std::sync::Mutex` 的薄包裝：不會因為別的執行緒 panic 而中毒。
mod parking_lot_free {
    #[derive(Default)]
    pub struct Mutex<T>(std::sync::Mutex<T>);
    impl<T> Mutex<T> {
        pub fn lock(&self) -> std::sync::MutexGuard<'_, T> {
            self.0.lock().unwrap_or_else(|e| e.into_inner())
        }
    }
}

#[derive(Clone)]
pub struct Listener {
    ctx: egui::Context,
    shared: Arc<Shared>,
}

impl EventListener for Listener {
    fn send_event(&self, event: Event) {
        match event {
            Event::Exit | Event::ChildExit(_) => {
                self.shared.exited.store(true, Ordering::Relaxed);
            }
            // 程式問終端機的問題（游標位置、裝置屬性…），答案要寫回 shell
            Event::PtyWrite(text) => {
                if let Some(tx) = self.shared.sender.lock().as_ref() {
                    let _ = tx.send(Msg::Input(text.into_bytes().into()));
                }
            }
            _ => {}
        }
        self.ctx.request_repaint();
    }
}

pub struct Terminal {
    pub term: Arc<FairMutex<Term<Listener>>>,
    sender: EventLoopSender,
    shared: Arc<Shared>,
    size: GridSize,
}

/// 要執行的 shell：程式與參數。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShellCommand {
    pub program: String,
    pub args: Vec<String>,
}

/// 系統預設的 shell。Unix 用 `$SHELL`（沒有就 `/bin/sh`）；Windows 優先用 PowerShell 7（`pwsh`），
/// 其次 Windows PowerShell，最後才是 `%COMSPEC%`（cmd）。
pub fn default_shell(env: &dyn Fn(&str) -> Option<String>, exists_in_path: &dyn Fn(&str) -> bool) -> ShellCommand {
    if cfg!(windows) {
        for exe in ["pwsh.exe", "powershell.exe"] {
            if exists_in_path(exe) {
                return ShellCommand { program: exe.to_owned(), args: vec!["-NoLogo".to_owned()] };
            }
        }
        let cmd = env("COMSPEC").filter(|s| !s.is_empty()).unwrap_or_else(|| "cmd.exe".to_owned());
        ShellCommand { program: cmd, args: Vec::new() }
    } else {
        let sh = env("SHELL").filter(|s| !s.is_empty()).unwrap_or_else(|| "/bin/sh".to_owned());
        ShellCommand { program: sh, args: Vec::new() }
    }
}

fn exists_in_path(exe: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|paths| std::env::split_paths(&paths).any(|d| d.join(exe).is_file()))
}

impl Terminal {
    /// 開一個 shell。`dir` 是起始資料夾；`cell` 是一格的大小（點），只用來告訴 shell 像素尺寸。
    pub fn spawn(ctx: &egui::Context, size: GridSize, cell: (u16, u16), dir: Option<PathBuf>) -> std::io::Result<Self> {
        let shell = default_shell(&|k| std::env::var(k).ok(), &exists_in_path);
        let options = tty::Options {
            shell: Some(tty::Shell::new(shell.program, shell.args)),
            working_directory: dir.filter(|d| d.is_dir()),
            drain_on_exit: true,
            env: HashMap::from([("TERM".to_owned(), "xterm-256color".to_owned()), ("COLORTERM".to_owned(), "truecolor".to_owned())]),
            #[cfg(target_os = "windows")]
            escape_args: false,
        };
        let window = window_size(size, cell);
        tty::setup_env();
        let pty = tty::new(&options, window, 0)?;
        let shared = Arc::new(Shared::default());
        let listener = Listener { ctx: ctx.clone(), shared: shared.clone() };
        let term = Arc::new(FairMutex::new(Term::new(Config::default(), &size, listener.clone())));
        let event_loop = EventLoop::new(term.clone(), listener, pty, true, false)?;
        let sender = event_loop.channel();
        *shared.sender.lock() = Some(sender.clone());
        event_loop.spawn();
        Ok(Self { term, sender, shared, size })
    }

    /// 視窗大小變了。格數沒變就什麼都不做。
    pub fn resize(&mut self, size: GridSize, cell: (u16, u16)) {
        if size == self.size || size.cols == 0 || size.lines == 0 {
            return;
        }
        self.size = size;
        self.term.lock().resize(size);
        let _ = self.sender.send(Msg::Resize(window_size(size, cell)));
    }

    /// 把資料送給 shell（當成使用者打的字）。
    pub fn write(&self, bytes: impl Into<std::borrow::Cow<'static, [u8]>>) {
        let _ = self.sender.send(Msg::Input(bytes.into()));
    }


    /// shell 已經結束。
    pub fn exited(&self) -> bool {
        self.shared.exited.load(Ordering::Relaxed)
    }

    /// 目前的終端機模式（要不要用應用程式游標鍵、括號貼上…）。
    pub fn mode(&self) -> TermMode {
        *self.term.lock().mode()
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        let _ = self.sender.send(Msg::Shutdown);
    }
}

fn window_size(size: GridSize, cell: (u16, u16)) -> WindowSize {
    WindowSize {
        num_lines: size.lines.min(u16::MAX as usize) as u16,
        num_cols: size.cols.min(u16::MAX as usize) as u16,
        cell_width: cell.0.max(1),
        cell_height: cell.1.max(1),
    }
}

// ── 按鍵編碼 ────────────────────────────────────────────────

/// 把一個按鍵（含修飾鍵）翻成要送給 shell 的位元組；不是終端機要處理的鍵回傳 `None`
/// （一般的可列印字元走文字事件，不走這裡）。`mode` 決定方向鍵的寫法（應用程式游標鍵模式）。
pub fn encode_key(key: egui::Key, mods: egui::Modifiers, mode: TermMode) -> Option<Vec<u8>> {
    use egui::Key::*;
    let app_cursor = mode.contains(TermMode::APP_CURSOR);
    // xterm 的修飾鍵參數：1 + shift(1) + alt(2) + ctrl(4)
    let m = 1 + u8::from(mods.shift) + 2 * u8::from(mods.alt) + 4 * u8::from(mods.ctrl);
    let csi = |final_byte: char| -> Vec<u8> {
        if m > 1 {
            format!("\x1b[1;{m}{final_byte}").into_bytes()
        } else if app_cursor && "ABCDHF".contains(final_byte) {
            format!("\x1bO{final_byte}").into_bytes()
        } else {
            format!("\x1b[{final_byte}").into_bytes()
        }
    };
    let tilde = |n: u8| -> Vec<u8> {
        if m > 1 { format!("\x1b[{n};{m}~").into_bytes() } else { format!("\x1b[{n}~").into_bytes() }
    };
    let bytes = match key {
        Enter => b"\r".to_vec(),
        Tab if mods.shift => b"\x1b[Z".to_vec(),
        Tab => b"\t".to_vec(),
        Escape => b"\x1b".to_vec(),
        Backspace if mods.ctrl => vec![0x08],
        Backspace => vec![0x7f],
        ArrowUp => csi('A'),
        ArrowDown => csi('B'),
        ArrowRight => csi('C'),
        ArrowLeft => csi('D'),
        Home => csi('H'),
        End => csi('F'),
        Insert => tilde(2),
        Delete => tilde(3),
        PageUp => tilde(5),
        PageDown => tilde(6),
        F1 => ss3_or_csi(m, 'P'),
        F2 => ss3_or_csi(m, 'Q'),
        F3 => ss3_or_csi(m, 'R'),
        F4 => ss3_or_csi(m, 'S'),
        F5 => tilde(15),
        F6 => tilde(17),
        F7 => tilde(18),
        F8 => tilde(19),
        F9 => tilde(20),
        F10 => tilde(21),
        F11 => tilde(23),
        F12 => tilde(24),
        Space if mods.ctrl => vec![0],
        // Ctrl+字母：控制字元（Ctrl+C = 0x03…）。Ctrl+C／X／V 是複製、剪下、貼上，由呼叫端處理
        k if mods.ctrl && !mods.command_only_mac() => {
            let c = k.name().chars().next().filter(char::is_ascii_alphabetic)?;
            let byte = (c.to_ascii_uppercase() as u8) & 0x1f;
            if mods.alt { vec![0x1b, byte] } else { vec![byte] }
        }
        _ => return None,
    };
    Some(bytes)
}

fn ss3_or_csi(m: u8, final_byte: char) -> Vec<u8> {
    if m > 1 { format!("\x1b[1;{m}{final_byte}").into_bytes() } else { format!("\x1bO{final_byte}").into_bytes() }
}

trait ModsExt {
    /// 只按了 macOS 的 Command（不是 Ctrl）。
    fn command_only_mac(&self) -> bool;
}

impl ModsExt for egui::Modifiers {
    fn command_only_mac(&self) -> bool {
        self.mac_cmd && !self.ctrl
    }
}

/// Alt＋可列印字元：前面加 ESC（讓 shell 當成 Meta 鍵）。
pub fn alt_text(text: &str) -> Vec<u8> {
    let mut out = vec![0x1b];
    out.extend_from_slice(text.as_bytes());
    out
}

/// 貼上的文字：換行統一成 `\r`；shell 要求括號貼上模式時加上包裝，並去掉文字裡會提早結束它的序列。
pub fn paste_bytes(text: &str, mode: TermMode) -> Vec<u8> {
    let body = text.replace("\r\n", "\r").replace('\n', "\r");
    if mode.contains(TermMode::BRACKETED_PASTE) {
        let body = body.replace("\x1b[201~", "");
        let mut out = b"\x1b[200~".to_vec();
        out.extend_from_slice(body.as_bytes());
        out.extend_from_slice(b"\x1b[201~");
        out
    } else {
        body.into_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Key, Modifiers};

    const NONE: Modifiers = Modifiers::NONE;
    const CTRL: Modifiers = Modifiers { ctrl: true, ..Modifiers::NONE };

    fn enc(key: Key, mods: Modifiers) -> Option<String> {
        encode_key(key, mods, TermMode::NONE).map(|b| String::from_utf8_lossy(&b).into_owned())
    }

    #[test]
    fn special_keys_become_the_usual_escape_sequences() {
        assert_eq!(enc(Key::Enter, NONE).as_deref(), Some("\r"));
        assert_eq!(enc(Key::Backspace, NONE).as_deref(), Some("\x7f"));
        assert_eq!(enc(Key::Tab, NONE).as_deref(), Some("\t"));
        assert_eq!(enc(Key::Tab, Modifiers::SHIFT).as_deref(), Some("\x1b[Z"));
        assert_eq!(enc(Key::ArrowUp, NONE).as_deref(), Some("\x1b[A"));
        assert_eq!(enc(Key::Delete, NONE).as_deref(), Some("\x1b[3~"));
        assert_eq!(enc(Key::PageDown, NONE).as_deref(), Some("\x1b[6~"));
        assert_eq!(enc(Key::F1, NONE).as_deref(), Some("\x1bOP"));
        assert_eq!(enc(Key::F5, NONE).as_deref(), Some("\x1b[15~"));
        // 修飾鍵：Ctrl+→ 是 ESC [ 1 ; 5 C
        assert_eq!(enc(Key::ArrowRight, CTRL).as_deref(), Some("\x1b[1;5C"));
        assert_eq!(enc(Key::ArrowLeft, Modifiers::ALT).as_deref(), Some("\x1b[1;3D"));
    }

    #[test]
    fn application_cursor_mode_changes_plain_arrows_only() {
        let app = TermMode::APP_CURSOR;
        let s = |k, m| String::from_utf8(encode_key(k, m, app).unwrap()).unwrap();
        assert_eq!(s(Key::ArrowUp, NONE), "\x1bOA");
        assert_eq!(s(Key::Home, NONE), "\x1bOH");
        assert_eq!(s(Key::ArrowUp, CTRL), "\x1b[1;5A", "有修飾鍵時照舊");
        assert_eq!(s(Key::PageUp, NONE), "\x1b[5~", "不是游標鍵");
    }

    #[test]
    fn ctrl_letters_are_control_characters_and_plain_letters_are_not_handled_here() {
        assert_eq!(encode_key(Key::C, CTRL, TermMode::NONE), Some(vec![3]));
        assert_eq!(encode_key(Key::D, CTRL, TermMode::NONE), Some(vec![4]));
        assert_eq!(encode_key(Key::L, CTRL, TermMode::NONE), Some(vec![12]));
        assert_eq!(encode_key(Key::Space, CTRL, TermMode::NONE), Some(vec![0]));
        assert_eq!(encode_key(Key::A, NONE, TermMode::NONE), None, "一般字母走文字事件");
        assert_eq!(encode_key(Key::Num1, CTRL, TermMode::NONE), None);
    }

    #[test]
    fn paste_normalizes_newlines_and_honors_bracketed_paste() {
        assert_eq!(paste_bytes("a\r\nb\nc", TermMode::NONE), b"a\rb\rc");
        let bracketed = paste_bytes("x\x1b[201~y", TermMode::BRACKETED_PASTE);
        assert_eq!(bracketed, b"\x1b[200~xy\x1b[201~");
        assert_eq!(alt_text("b"), b"\x1bb");
    }

    #[test]
    fn default_shell_prefers_the_users_shell_on_unix_and_powershell_on_windows() {
        let env = |k: &str| (k == "SHELL").then(|| "/bin/zsh".to_owned()).or_else(|| (k == "COMSPEC").then(|| "C:\\cmd.exe".to_owned()));
        let has = |exe: &str| exe == "powershell.exe";
        let sh = default_shell(&env, &has);
        if cfg!(windows) {
            assert_eq!(sh.program, "powershell.exe");
            let none = |_: &str| false;
            assert_eq!(default_shell(&env, &none).program, "C:\\cmd.exe");
            let both = |_: &str| true;
            assert_eq!(default_shell(&env, &both).program, "pwsh.exe");
        } else {
            assert_eq!(sh.program, "/bin/zsh");
            assert_eq!(default_shell(&|_| None, &has).program, "/bin/sh");
        }
    }

    #[test]
    fn a_real_shell_runs_a_command_and_its_output_reaches_the_grid() {
        let ctx = egui::Context::default();
        let size = GridSize { cols: 80, lines: 24 };
        let mut t = Terminal::spawn(&ctx, size, (8, 16), None).expect("開不了 shell");
        let command: &[u8] = if cfg!(windows) { b"echo feditor-terminal-ok\r\n" } else { b"echo feditor-terminal-ok\n" };
        t.write(command);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        let mut found = false;
        while std::time::Instant::now() < deadline && !found {
            std::thread::sleep(std::time::Duration::from_millis(200));
            let term = t.term.lock();
            let grid = term.grid();
            // 命令列本身也含這串字：要找「輸出」，也就是某一行只有這串字
            found = (0..grid.screen_lines() as i32).any(|l| {
                let row = &grid[alacritty_terminal::index::Line(l)];
                let text: String = (0..grid.columns()).map(|c| row[alacritty_terminal::index::Column(c)].c).collect();
                text.trim_end() == "feditor-terminal-ok"
            });
        }
        assert!(found, "沒有在畫面上看到 echo 的輸出");
        t.resize(GridSize { cols: 100, lines: 30 }, (8, 16));
        assert_eq!(t.term.lock().columns(), 100);
        t.write(if cfg!(windows) { &b"exit\r\n"[..] } else { &b"exit\n"[..] });
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        while !t.exited() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        assert!(t.exited(), "shell 結束後要能偵測到");
    }
}
