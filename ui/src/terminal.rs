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

/// 把「程式 參數…」這樣一行字拆成程式與參數；用雙引號包起來的部分（例如有空白的路徑）算一個詞。
pub fn split_command(line: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut started = false;
    for c in line.chars() {
        match c {
            '"' => {
                in_quotes = !in_quotes;
                started = true;
            }
            c if c.is_whitespace() && !in_quotes => {
                if started {
                    words.push(std::mem::take(&mut current));
                    started = false;
                }
            }
            c => {
                current.push(c);
                started = true;
            }
        }
    }
    if started {
        words.push(current);
    }
    words
}

/// 要執行的 shell：使用者在設定裡指定了（程式加參數）就用它，沒有就用系統預設的。
pub fn resolve_shell(custom: &str, env: &dyn Fn(&str) -> Option<String>, exists_in_path: &dyn Fn(&str) -> bool) -> ShellCommand {
    let mut words = split_command(custom).into_iter();
    match words.next() {
        Some(program) => ShellCommand { program, args: words.collect() },
        None => default_shell(env, exists_in_path),
    }
}

/// 偵測到的一個 shell：顯示名稱，與「指令」（程式加參數，格式同設定裡的 `terminal_shell`）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShellOption {
    pub label: String,
    pub command: String,
}

/// 路徑有空白就用雙引號包起來，才能放進「程式 參數…」的指令裡。
fn quote_if_needed(path: &str) -> String {
    if path.contains(' ') { format!("\"{path}\"") } else { path.to_owned() }
}

/// `/etc/shells`：每行一個 shell 的絕對路徑，`#` 開頭的是註解。
pub fn parse_etc_shells(text: &str) -> Vec<String> {
    text.lines().map(str::trim).filter(|l| l.starts_with('/')).map(str::to_owned).collect()
}

/// 這台機器上可以用的 shell（依常用程度排序）。`windows` 決定找法：
/// Windows 找 PowerShell 7、Windows PowerShell、cmd、Git Bash、WSL、Nushell；
/// 其他系統用 `$SHELL` 加上 `/etc/shells`（內容放在 `etc_shells`）裡實際存在的 shell。
pub fn shells_for(
    windows: bool,
    env: &dyn Fn(&str) -> Option<String>,
    in_path: &dyn Fn(&str) -> bool,
    file_exists: &dyn Fn(&str) -> bool,
    etc_shells: &str,
) -> Vec<ShellOption> {
    let mut out: Vec<ShellOption> = Vec::new();
    let mut add = |label: &str, command: String| {
        if !out.iter().any(|o| o.command == command || o.label == label) {
            out.push(ShellOption { label: label.to_owned(), command });
        }
    };
    if windows {
        if in_path("pwsh.exe") {
            add("PowerShell 7", "pwsh -NoLogo".to_owned());
        }
        if in_path("powershell.exe") {
            add("Windows PowerShell", "powershell -NoLogo".to_owned());
        }
        if in_path("cmd.exe") || env("COMSPEC").is_some_and(|c| !c.is_empty()) {
            add("Command Prompt (cmd)", "cmd".to_owned());
        }
        let var = |name: &str| env(name).filter(|v| !v.is_empty());
        let mut git_bash: Vec<String> = Vec::new();
        for (name, rest) in [("ProgramFiles", "Git\\bin\\bash.exe"), ("ProgramFiles(x86)", "Git\\bin\\bash.exe"), ("LOCALAPPDATA", "Programs\\Git\\bin\\bash.exe"), ("USERPROFILE", "scoop\\apps\\git\\current\\bin\\bash.exe")] {
            if let Some(base) = var(name) {
                git_bash.push(format!("{}\\{rest}", base.trim_end_matches('\\')));
            }
        }
        if let Some(path) = git_bash.into_iter().find(|p| file_exists(p)) {
            add("Git Bash", format!("{} --login -i", quote_if_needed(&path)));
        }
        if in_path("wsl.exe") {
            add("WSL", "wsl".to_owned());
        }
        if in_path("nu.exe") {
            add("Nushell", "nu".to_owned());
        }
    } else {
        let mut paths: Vec<String> = env("SHELL").filter(|s| !s.is_empty()).into_iter().collect();
        for p in parse_etc_shells(etc_shells) {
            if !paths.contains(&p) {
                paths.push(p);
            }
        }
        for p in paths.into_iter().filter(|p| file_exists(p)) {
            let name = p.rsplit('/').next().unwrap_or(&p).to_owned();
            add(&name, quote_if_needed(&p));
        }
    }
    out
}

/// 偵測這台機器上可以用的 shell。
pub fn detect_shells() -> Vec<ShellOption> {
    let etc = if cfg!(windows) { String::new() } else { std::fs::read_to_string("/etc/shells").unwrap_or_default() };
    shells_for(cfg!(windows), &|k| std::env::var(k).ok(), &exists_in_path, &|p| std::path::Path::new(p).is_file(), &etc)
}

/// shell 的種類：決定要送進去的指令怎麼加引號、怎麼換資料夾。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShellKind {
    /// bash、zsh、fish、sh…
    Posix,
    PowerShell,
    Cmd,
}

impl ShellKind {
    /// 由 shell 的程式名稱（可以帶路徑與 `.exe`）判斷。
    pub fn of(program: &str) -> Self {
        let name = program.rsplit(['/', '\\']).next().unwrap_or(program).to_ascii_lowercase();
        let name = name.strip_suffix(".exe").unwrap_or(&name);
        if name.contains("pwsh") || name.contains("powershell") {
            ShellKind::PowerShell
        } else if name == "cmd" {
            ShellKind::Cmd
        } else {
            ShellKind::Posix
        }
    }

    fn quote(self, text: &str) -> String {
        match self {
            ShellKind::Cmd => format!("\"{text}\""),
            ShellKind::PowerShell => format!("'{}'", text.replace('\'', "''")),
            ShellKind::Posix => format!("'{}'", text.replace('\'', r"'\''")),
        }
    }

    /// 執行 `program args… file` 的一行指令（不含換行）。
    pub fn run_line(self, program: &str, args: &[String], file: &str) -> String {
        let mut words = vec![self.quote(program)];
        words.extend(args.iter().map(|a| self.quote(a)));
        words.push(self.quote(file));
        let line = words.join(" ");
        // PowerShell 的字串開頭的指令要加呼叫運算子 &
        if self == ShellKind::PowerShell { format!("& {line}") } else { line }
    }

    /// 切到資料夾 `dir` 的一行指令（不含換行）。
    pub fn cd_line(self, dir: &str) -> String {
        match self {
            ShellKind::Cmd => format!("cd /d {}", self.quote(dir)),
            ShellKind::PowerShell => format!("Set-Location -LiteralPath {}", self.quote(dir)),
            ShellKind::Posix => format!("cd {}", self.quote(dir)),
        }
    }
}

/// 這個終端機（`custom_shell` 是設定的 shell 指令，空字串是系統預設）用的是哪一種 shell。
pub fn shell_kind(custom_shell: &str) -> ShellKind {
    ShellKind::of(&resolve_shell(custom_shell, &|k| std::env::var(k).ok(), &exists_in_path).program)
}

/// 沒有指定 shell 時實際會用的（系統預設的）shell。
pub fn system_default_shell() -> ShellCommand {
    default_shell(&|k| std::env::var(k).ok(), &exists_in_path)
}

/// 這個選項是不是目前的預設：設定裡有指定就比對指令；沒指定就看它的程式是不是系統預設的 shell。
pub fn is_default_shell(option: &ShellOption, configured: &str, system: &ShellCommand) -> bool {
    let configured = split_command(configured);
    let option_words = split_command(&option.command);
    if configured.is_empty() {
        let stem = |s: &str| std::path::Path::new(s).file_stem().map(|x| x.to_string_lossy().to_lowercase());
        option_words.first().is_some_and(|p| stem(p) == stem(&system.program))
    } else {
        option_words == configured
    }
}

/// 指令（程式加參數）裡那個程式的實際檔案路徑：寫了路徑就用它，只有程式名稱就在 PATH 裡找（也試加上 `.exe`）。
/// `path_var` 是 PATH 的內容，`is_file` 判斷檔案存不存在（測試時可以換掉）。
pub fn program_path_in(command: &str, path_var: Option<&std::ffi::OsStr>, is_file: &dyn Fn(&std::path::Path) -> bool) -> Option<PathBuf> {
    let program = split_command(command).into_iter().next()?;
    let direct = std::path::Path::new(&program);
    if direct.is_absolute() || direct.components().count() > 1 {
        return is_file(direct).then(|| direct.to_path_buf());
    }
    std::env::split_paths(path_var?).find_map(|dir| {
        [program.clone(), format!("{program}.exe")].into_iter().map(|name| dir.join(name)).find(|candidate| is_file(candidate))
    })
}

/// 指令裡那個程式的實際檔案路徑（用目前的 PATH 找）。
pub fn program_path(command: &str) -> Option<PathBuf> {
    program_path_in(command, std::env::var_os("PATH").as_deref(), &|p| p.is_file())
}

fn exists_in_path(exe: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|paths| std::env::split_paths(&paths).any(|d| d.join(exe).is_file()))
}

impl Terminal {
    /// 開一個 shell。`dir` 是起始資料夾；`cell` 是一格的大小（點），只用來告訴 shell 像素尺寸。
    pub fn spawn(
        ctx: &egui::Context,
        size: GridSize,
        cell: (u16, u16),
        dir: Option<PathBuf>,
        custom_shell: &str,
        extra_env: &[(String, String)],
    ) -> std::io::Result<Self> {
        let shell = resolve_shell(custom_shell, &|k| std::env::var(k).ok(), &exists_in_path);
        let mut env = HashMap::from([("TERM".to_owned(), "xterm-256color".to_owned()), ("COLORTERM".to_owned(), "truecolor".to_owned())]);
        env.extend(extra_env.iter().cloned()); // 執行環境（虛擬環境的 PATH 等）
        let options = tty::Options {
            shell: Some(tty::Shell::new(shell.program, shell.args)),
            working_directory: dir.filter(|d| d.is_dir()),
            drain_on_exit: true,
            env,
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
            // 只有單一字母的鍵（A～Z）才有對應的控制字元。名字比較長的鍵（ControlLeft、ShiftLeft、AltLeft…，
            // 單按修飾鍵本身也是一個按鍵事件）不能拿名字的第一個字母當字母用，否則單按 Ctrl 就變成 Ctrl+C
            let name = k.name();
            let mut letters = name.chars();
            let c = letters.next().filter(char::is_ascii_alphabetic).filter(|_| letters.next().is_none())?;
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

/// 滑鼠事件要送給程式時的編碼（程式用 `ESC[?1000h` 這類序列要求了滑鼠回報）。
/// `button` 是 0 左鍵、1 中鍵、2 右鍵、3 沒有按鍵（只有移動）、64 滾輪往上、65 滾輪往下；
/// `col`、`row` 從 0 起算；`release` 是放開按鍵；`motion` 是移動中。
/// 程式要求 SGR 格式（`ESC[?1006h`）就用 SGR，否則用傳統格式（座標超過 223 的格子送不出去，回傳 `None`）。
pub fn encode_mouse(button: u8, col: usize, row: usize, release: bool, mods: egui::Modifiers, motion: bool, mode: TermMode) -> Option<Vec<u8>> {
    let mut code = button;
    if mods.shift {
        code += 4;
    }
    if mods.alt {
        code += 8;
    }
    if mods.ctrl {
        code += 16;
    }
    if motion {
        code += 32;
    }
    let (x, y) = (col + 1, row + 1);
    if mode.contains(TermMode::SGR_MOUSE) {
        let last = if release { 'm' } else { 'M' };
        Some(format!("\x1b[<{code};{x};{y}{last}").into_bytes())
    } else {
        if x > 223 || y > 223 {
            return None;
        }
        // 傳統格式的放開不分哪個鍵，一律是 3
        let b = if release { 3 + (code & !3) } else { code };
        Some(vec![0x1b, b'[', b'M', 32 + b, 32 + x as u8, 32 + y as u8])
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

    #[test]
    fn shell_kinds_are_told_apart_by_program_name() {
        assert_eq!(ShellKind::of("pwsh"), ShellKind::PowerShell);
        assert_eq!(ShellKind::of(r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe"), ShellKind::PowerShell);
        assert_eq!(ShellKind::of("CMD.EXE"), ShellKind::Cmd);
        assert_eq!(ShellKind::of(r"C:\Windows\System32\cmd.exe"), ShellKind::Cmd);
        assert_eq!(ShellKind::of("/bin/zsh"), ShellKind::Posix);
        assert_eq!(ShellKind::of("nu"), ShellKind::Posix);
    }

    #[test]
    fn run_and_cd_lines_are_quoted_for_each_shell() {
        let args = vec!["-3".to_owned()];
        assert_eq!(ShellKind::Posix.run_line("/my dir/python", &args, "/p/it's.py"), r"'/my dir/python' '-3' '/p/it'\''s.py'");
        assert_eq!(
            ShellKind::PowerShell.run_line(r"C:\Program Files\py.exe", &args, r"C:\it's\a.py"),
            r"& 'C:\Program Files\py.exe' '-3' 'C:\it''s\a.py'"
        );
        assert_eq!(ShellKind::Cmd.run_line(r"C:\py.exe", &[], r"C:\a b\a.py"), r#""C:\py.exe" "C:\a b\a.py""#);
        assert_eq!(ShellKind::Posix.cd_line("/a b"), "cd '/a b'");
        assert_eq!(ShellKind::PowerShell.cd_line(r"C:\a b"), r"Set-Location -LiteralPath 'C:\a b'");
        assert_eq!(ShellKind::Cmd.cd_line(r"D:\a b"), r#"cd /d "D:\a b""#);
    }
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
    fn pressing_a_modifier_key_alone_sends_nothing() {
        // 單按 Ctrl／Shift／Alt 本身也是按鍵事件（Key::ControlLeft…），名字的第一個字母不能被當成 Ctrl+字母
        for key in [Key::ControlLeft, Key::ControlRight, Key::ShiftLeft, Key::ShiftRight, Key::AltLeft, Key::AltRight, Key::SuperLeft] {
            let all = Modifiers { ctrl: true, shift: true, alt: true, ..Modifiers::NONE };
            for mods in [CTRL, Modifiers::SHIFT, Modifiers::ALT, all] {
                assert_eq!(encode_key(key, mods, TermMode::NONE), None, "{key:?} {mods:?}");
            }
        }
        // 名字比較長的一般鍵也一樣：Copy／Cut／Paste 專用鍵不是 Ctrl+C／X／V
        assert_eq!(encode_key(Key::Copy, CTRL, TermMode::NONE), None);
        // 真的字母鍵不受影響
        assert_eq!(encode_key(Key::C, CTRL, TermMode::NONE), Some(vec![3]));
        assert_eq!(encode_key(Key::Z, CTRL, TermMode::NONE), Some(vec![26]));
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
    fn commands_split_into_program_and_arguments_and_a_custom_shell_overrides_the_default() {
        assert_eq!(split_command("pwsh -NoLogo"), vec!["pwsh", "-NoLogo"]);
        assert_eq!(split_command("  bash   --login  -i "), vec!["bash", "--login", "-i"]);
        assert_eq!(split_command(r#""C:\Program Files\Git\bin\bash.exe" --login"#), vec![r"C:\Program Files\Git\bin\bash.exe", "--login"]);
        assert_eq!(split_command(""), Vec::<String>::new());
        assert_eq!(split_command("   "), Vec::<String>::new());
        assert_eq!(split_command(r#"x "" y"#), vec!["x", "", "y"], "空的引號是一個空的參數");
        let env = |_: &str| None;
        let any = |_: &str| true;
        let custom = resolve_shell("nu --no-config", &env, &any);
        assert_eq!((custom.program.as_str(), custom.args.as_slice()), ("nu", &["--no-config".to_owned()][..]));
        assert_eq!(resolve_shell("", &env, &any), default_shell(&env, &any), "沒指定就用系統預設的");
    }

    #[test]
    fn windows_shells_are_found_from_the_path_and_known_git_bash_locations() {
        let env = |k: &str| match k {
            "ProgramFiles" => Some("C:\\Program Files".to_owned()),
            "COMSPEC" => Some("C:\\Windows\\System32\\cmd.exe".to_owned()),
            _ => None,
        };
        let in_path = |exe: &str| matches!(exe, "pwsh.exe" | "powershell.exe" | "cmd.exe" | "wsl.exe");
        let exists = |p: &str| p == "C:\\Program Files\\Git\\bin\\bash.exe";
        let list = shells_for(true, &env, &in_path, &exists, "");
        let labels: Vec<&str> = list.iter().map(|o| o.label.as_str()).collect();
        assert_eq!(labels, vec!["PowerShell 7", "Windows PowerShell", "Command Prompt (cmd)", "Git Bash", "WSL"]);
        assert_eq!(list[0].command, "pwsh -NoLogo");
        assert_eq!(list[3].command, "\"C:\\Program Files\\Git\\bin\\bash.exe\" --login -i", "路徑有空白要加引號");
        // 指令可以被拆回程式與參數
        assert_eq!(split_command(&list[3].command), vec!["C:\\Program Files\\Git\\bin\\bash.exe", "--login", "-i"]);
        // 什麼都沒有：清單是空的（不會報錯）
        assert!(shells_for(true, &|_| None, &|_| false, &|_| false, "").is_empty());
    }

    #[test]
    fn unix_shells_come_from_the_shell_variable_and_etc_shells_without_duplicates() {
        let env = |k: &str| (k == "SHELL").then(|| "/usr/bin/zsh".to_owned());
        let exists = |p: &str| matches!(p, "/usr/bin/zsh" | "/bin/bash" | "/usr/bin/bash" | "/bin/sh");
        let etc = "# 可用的 shell\n/bin/sh\n/bin/bash\n/usr/bin/bash\n/usr/bin/zsh\n/usr/bin/fish\nnologin\n";
        assert_eq!(parse_etc_shells(etc), vec!["/bin/sh", "/bin/bash", "/usr/bin/bash", "/usr/bin/zsh", "/usr/bin/fish"]);
        let list = shells_for(false, &env, &|_| false, &exists, etc);
        let labels: Vec<&str> = list.iter().map(|o| o.label.as_str()).collect();
        assert_eq!(labels, vec!["zsh", "sh", "bash"], "$SHELL 排第一；不存在的（fish）略過；同名的只留第一個");
        assert_eq!(list[0].command, "/usr/bin/zsh");
        // 沒有 /etc/shells 也至少有 $SHELL
        assert_eq!(shells_for(false, &env, &|_| false, &exists, "").len(), 1);
    }

    #[test]
    fn the_default_option_follows_the_setting_or_the_system_default_program() {
        let pwsh = ShellOption { label: "PowerShell 7".into(), command: "pwsh -NoLogo".into() };
        let cmd = ShellOption { label: "Command Prompt (cmd)".into(), command: "cmd".into() };
        let system = ShellCommand { program: "pwsh.exe".into(), args: vec!["-NoLogo".into()] };
        // 沒指定：看程式是不是系統預設的（不分大小寫與副檔名）
        assert!(is_default_shell(&pwsh, "", &system));
        assert!(!is_default_shell(&cmd, "  ", &system));
        // 有指定：比對指令（空白的差異不算）
        assert!(is_default_shell(&cmd, "cmd", &system));
        assert!(is_default_shell(&pwsh, "pwsh   -NoLogo", &system));
        assert!(!is_default_shell(&pwsh, "pwsh", &system), "參數不同就是不同的選項");
    }

    #[test]
    fn a_commands_program_is_found_on_the_path_or_by_its_own_path() {
        let sep = if cfg!(windows) { ";" } else { ":" };
        let path = std::ffi::OsString::from(format!("{}{sep}{}", if cfg!(windows) { "C:\\a" } else { "/a" }, if cfg!(windows) { "C:\\b" } else { "/b" }));
        let (a, b) = if cfg!(windows) { ("C:\\a", "C:\\b") } else { ("/a", "/b") };
        let exists = |p: &std::path::Path| p == std::path::Path::new(b).join("pwsh.exe") || p == std::path::Path::new(a).join("git");
        // 只有程式名稱：依序在 PATH 的每個資料夾找，也試加上 .exe
        assert_eq!(program_path_in("pwsh -NoLogo", Some(&path), &exists), Some(std::path::Path::new(b).join("pwsh.exe")));
        assert_eq!(program_path_in("git status", Some(&path), &exists), Some(std::path::Path::new(a).join("git")));
        assert_eq!(program_path_in("nope", Some(&path), &exists), None);
        assert_eq!(program_path_in("pwsh", None, &exists), None, "沒有 PATH 就找不到");
        assert_eq!(program_path_in("", Some(&path), &exists), None);
        // 寫了路徑（含引號與空白）：直接檢查那個檔案，不去 PATH 找
        let full = if cfg!(windows) { "\"C:\\Program Files\\Git\\bin\\bash.exe\" --login -i" } else { "\"/opt/my shell/bash\" --login" };
        let target = if cfg!(windows) { "C:\\Program Files\\Git\\bin\\bash.exe" } else { "/opt/my shell/bash" };
        assert_eq!(program_path_in(full, Some(&path), &|p| p == std::path::Path::new(target)), Some(PathBuf::from(target)));
        assert_eq!(program_path_in(full, Some(&path), &|_| false), None);
    }

    #[test]
    fn mouse_reports_use_sgr_when_asked_and_the_legacy_format_otherwise() {
        let none = egui::Modifiers::NONE;
        let sgr = TermMode::SGR_MOUSE;
        assert_eq!(encode_mouse(0, 4, 9, false, none, false, sgr), Some(b"\x1b[<0;5;10M".to_vec()));
        assert_eq!(encode_mouse(0, 4, 9, true, none, false, sgr), Some(b"\x1b[<0;5;10m".to_vec()), "放開用小寫 m，保留原本的按鍵");
        assert_eq!(encode_mouse(2, 0, 0, false, egui::Modifiers { ctrl: true, shift: true, ..none }, false, sgr), Some(b"\x1b[<22;1;1M".to_vec()), "Shift 4 ＋ Ctrl 16");
        assert_eq!(encode_mouse(3, 7, 2, false, none, true, sgr), Some(b"\x1b[<35;8;3M".to_vec()), "只有移動：按鍵 3 加動作 32");
        assert_eq!(encode_mouse(64, 1, 1, false, none, false, sgr), Some(b"\x1b[<64;2;2M".to_vec()), "滾輪往上");
        let legacy = TermMode::NONE;
        assert_eq!(encode_mouse(0, 0, 0, false, none, false, legacy), Some(vec![0x1b, b'[', b'M', 32, 33, 33]));
        assert_eq!(encode_mouse(1, 4, 9, true, none, false, legacy), Some(vec![0x1b, b'[', b'M', 35, 37, 42]), "傳統格式放開一律是 3");
        assert_eq!(encode_mouse(0, 300, 0, false, none, false, legacy), None, "座標超過 223 送不出去");
        assert!(encode_mouse(0, 300, 0, false, none, false, sgr).is_some(), "SGR 沒有這個限制");
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
        let mut t = Terminal::spawn(&ctx, size, (8, 16), None, "", &[]).expect("開不了 shell");
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
