//! 自動搜尋執行環境：開啟某種語言的檔案時，找出該用哪個直譯器（目前只有 Python）。
//!
//! 搜尋順序（找到的都會列出來，第一個可以執行的是預設）：
//! 1. 從檔案所在的資料夾往上，每一層找虛擬環境資料夾（`.venv`、`venv`、`env`；要有 `pyvenv.cfg` 才算）。
//!    走到有 `.git` 的那一層、使用者的家目錄、或磁碟根目錄就停，不會去用別的專案的環境。
//! 2. 環境變數指到的環境（`VIRTUAL_ENV`、`CONDA_PREFIX`）：從已啟用環境的終端機啟動 FEditor 時有。
//! 3. 全域的直譯器：`PATH` 上的 `python3`、`python`（Windows 另外有 `py` 啟動器；
//!    排除 Windows 商店的假捷徑 `WindowsApps`）。
//!
//! 每個候選都會在背景執行 `--version` 確認真的能用（壞掉的虛擬環境、macOS 沒裝開發工具時的
//! `/usr/bin/python3` 都會因此被略過）。規則是資料（[`Rule`]），之後要支援別的語言只要再寫一份規則。

use eframe::egui;
use std::collections::HashMap;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// 一個語言怎麼找執行環境。
#[derive(Debug)]
pub struct Rule {
    /// 顯示用的名稱，例如 `Python`。
    pub label: &'static str,
    /// 專案裡的環境資料夾名稱。
    pub env_dirs: &'static [&'static str],
    /// 環境資料夾裡一定有的檔案，用來確認那真的是環境（`pyvenv.cfg`）。
    pub env_marker: &'static str,
    /// 指到環境資料夾的環境變數。
    pub env_vars: &'static [&'static str],
    /// 直譯器在環境資料夾裡的相對路徑（Unix、Windows）。
    pub interpreter_unix: &'static str,
    pub interpreter_windows: &'static str,
    /// 全域的直譯器名稱（不含 `.exe`），依序找。
    pub programs: &'static [&'static str],
    /// 在 Windows 上額外試的啟動器（程式名稱與固定參數）。
    pub windows_launcher: Option<(&'static str, &'static [&'static str])>,
    /// 取得版本的參數，與輸出裡版本號之前的文字（`Python 3.12.1` 的 `Python `；空字串表示輸出一開始就是版本號）。
    pub version_args: &'static [&'static str],
    pub version_prefix: &'static str,
    /// 執行檔案時，放在檔案前面的固定參數（`cmd /c 檔案` 的 `/c`）。
    pub run_args: &'static [&'static str],
    /// Windows 上 `PATH` 找不到時，另外試的完整路徑（例如 Git 附的 bash）。
    pub windows_extra: &'static [&'static str],
    /// `PATH` 上要排除的路徑（不分大小寫、`\` 當成 `/` 之後，路徑裡含有這段文字的就略過）：
    /// Windows 商店的假捷徑（`windowsapps` 裡的 `python.exe` 打開只會跳商店）、
    /// `system32` 裡的 `bash.exe`（那是 WSL 的入口，不是能直接跑腳本的 bash）。
    pub skip_paths: &'static [&'static str],
    /// Windows 上找 bash 用：`PATH` 上有這個啟動器（`git-bash`，scoop 的 shim 也算）時，
    /// 用它所在的 Git 安裝資料夾裡的 bash（相對路徑清單）。啟動器本身會另開一個視窗，所以不直接拿來執行腳本。
    pub windows_wrapper: Option<(&'static str, &'static [&'static str])>,
    /// 這個直譯器沒有可以查的版本（`sh`）：能執行就算數，不顯示版本號。
    pub versionless: bool,
}

const NO_RULE: Rule = Rule {
    label: "",
    env_dirs: &[],
    env_marker: "",
    env_vars: &[],
    interpreter_unix: "",
    interpreter_windows: "",
    programs: &[],
    windows_launcher: None,
    version_args: &["--version"],
    version_prefix: "",
    run_args: &[],
    windows_extra: &[],
    skip_paths: &[],
    windows_wrapper: None,
    versionless: false,
};

pub const PYTHON: Rule = Rule {
    label: "Python",
    env_dirs: &[".venv", "venv", "env"],
    env_marker: "pyvenv.cfg",
    env_vars: &["VIRTUAL_ENV", "CONDA_PREFIX"],
    interpreter_unix: "bin/python",
    interpreter_windows: "Scripts/python.exe",
    programs: &["python3", "python"],
    windows_launcher: Some(("py", &["-3"])),
    version_args: &["--version"],
    version_prefix: "Python ",
    skip_paths: &["windowsapps"],
    ..NO_RULE
};

// 腳本（語言「Shell」）：直譯器依 shebang 或副檔名決定（見 [`rule_for_file`]），沒有虛擬環境，只在 `PATH` 找
const BASH: Rule = Rule {
    label: "bash",
    programs: &["bash"],
    // system32 與 WindowsApps 裡的 bash.exe 是 WSL 的入口，略過；改用 git-bash 所在的 Git 裡的 bash
    skip_paths: &["windows/system32", "windowsapps"],
    windows_wrapper: Some(("git-bash", &["bin/bash.exe", "usr/bin/bash.exe"])),
    windows_extra: &[
        r"C:\Program Files\Git\bin\bash.exe",
        r"C:\Program Files\Git\usr\bin\bash.exe",
        r"C:\Program Files (x86)\Git\bin\bash.exe",
    ],
    version_prefix: "version ",
    ..NO_RULE
};

const SH: Rule = Rule {
    label: "sh",
    programs: &["sh"],
    version_args: &["-c", "exit 0"],
    versionless: true,
    ..NO_RULE
};

const ZSH: Rule = Rule { label: "zsh", programs: &["zsh"], version_prefix: "zsh ", ..NO_RULE };

const POWERSHELL: Rule = Rule {
    label: "PowerShell",
    programs: &["pwsh", "powershell"],
    // 輸出就是版本號本身（7.4.1、5.1.22621.4391）
    version_args: &["-NoProfile", "-Command", "$PSVersionTable.PSVersion.ToString()"],
    ..NO_RULE
};

const CMD: Rule = Rule {
    label: "cmd",
    programs: &["cmd"],
    run_args: &["/c"],
    // `cmd /c ver` 印出 `Microsoft Windows [Version 10.0.22631.4460]`
    version_args: &["ver"],
    version_prefix: "[Version ",
    ..NO_RULE
};

/// 這個檔案該用哪份規則找執行環境；不能當腳本執行的回傳 `None`。
/// - Python：一律用 Python 的規則。
/// - Shell：先看第一行的 shebang（`#!/bin/bash`、`#!/usr/bin/env zsh`），沒有 shebang 再看副檔名
///   （`.sh`／`.bash` 是 bash、`.zsh`、`.ps1`／`.psm1` 是 PowerShell、`.bat`／`.cmd` 是 cmd）。
///   副檔名不明確、又沒有 shebang 的，不當成可以執行的腳本。
pub fn rule_for_file(language: &str, path: &Path, first_line: &str) -> Option<&'static Rule> {
    match language {
        "Python" => Some(&PYTHON),
        "Shell" => shell_rule(path, first_line),
        _ => None,
    }
}

/// shebang 指定的直譯器名稱（小寫、不含路徑與 `.exe`）；`#!/usr/bin/env -S bash -e` 取 env 後面的程式。
fn shebang_interpreter(first_line: &str) -> Option<String> {
    let rest = first_line.trim().strip_prefix("#!")?;
    let mut words = rest.split_whitespace();
    let program = words.next()?;
    let base = |s: &str| {
        let name = s.rsplit(['/', '\\']).next().unwrap_or(s).to_ascii_lowercase();
        name.strip_suffix(".exe").map(str::to_owned).unwrap_or(name)
    };
    let name = base(program);
    if name == "env" {
        return words.find(|w| !w.starts_with('-') && !w.contains('=')).map(base);
    }
    Some(name)
}

fn shell_rule(path: &Path, first_line: &str) -> Option<&'static Rule> {
    if first_line.trim_start().starts_with("#!") {
        return match shebang_interpreter(first_line)?.as_str() {
            "bash" => Some(&BASH),
            "sh" | "dash" | "ash" => Some(&SH),
            "zsh" => Some(&ZSH),
            "pwsh" | "powershell" => Some(&POWERSHELL),
            _ => None,
        };
    }
    match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "sh" | "bash" => Some(&BASH),
        "zsh" => Some(&ZSH),
        "ps1" | "psm1" => Some(&POWERSHELL),
        "bat" | "cmd" => Some(&CMD),
        _ => None,
    }
}

/// 設定裡記住手動選擇用的鍵：Python 沿用只有專案資料夾的寫法（和舊版相容），其他的再加上直譯器種類。
pub fn choice_key(rule: &Rule, project: &str) -> String {
    if rule.label == PYTHON.label { project.to_owned() } else { format!("{project}#{}", rule.label) }
}

/// 狀態列與清單上的名稱：`Python 3.12.1`；沒有版本號的只有名稱（`sh`）。
pub fn describe(rule: &Rule, item: &Item) -> String {
    match item.version.as_deref() {
        Some(v) if !v.is_empty() => format!("{} {v}", rule.label),
        _ => rule.label.to_owned(),
    }
}

/// 候選從哪裡來。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// 專案裡的虛擬環境資料夾。
    Venv,
    /// 環境變數指到的環境。
    EnvVar,
    /// `PATH` 上全域的直譯器。
    Global,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    pub program: PathBuf,
    /// 固定要帶的參數（`py -3` 的 `-3`）。
    pub args: Vec<String>,
    pub source: Source,
    /// 來源的簡短說明，顯示用：資料夾名稱（`.venv`）、環境變數名稱、或 `PATH`。
    pub origin: String,
    /// 環境資料夾（虛擬環境、環境變數來源才有）。
    pub env_dir: Option<PathBuf>,
}

impl Candidate {
    /// 設定裡記住手動選擇用的字串。
    pub fn key(&self) -> String {
        let mut text = self.program.display().to_string();
        for a in &self.args {
            text.push(' ');
            text.push_str(a);
        }
        text
    }
}

fn interpreter_in(rule: &Rule, env_dir: &Path) -> PathBuf {
    env_dir.join(if cfg!(windows) { rule.interpreter_windows } else { rule.interpreter_unix })
}

/// 搜尋停止的那一層：有 `.git` 的資料夾，沒有的話就是檔案所在的資料夾本身。
/// 手動選擇的直譯器以它為單位記住。
pub fn project_root(start: &Path) -> PathBuf {
    start.ancestors().find(|d| d.join(".git").exists()).unwrap_or(start).to_path_buf()
}

/// scoop 的 shim：`git-bash.exe` 旁邊有 `git-bash.shim`，裡面寫 `path = "真正的路徑"`。
/// 找不到 shim 檔、或指到的檔案不存在，就當成它本身就是真正的檔案。
pub fn resolve_shim(exe: &Path) -> PathBuf {
    std::fs::read_to_string(exe.with_extension("shim"))
        .ok()
        .and_then(|text| {
            text.lines().find_map(|line| {
                let (key, value) = line.split_once('=')?;
                (key.trim() == "path").then(|| PathBuf::from(value.trim().trim_matches('"')))
            })
        })
        .filter(|real| real.is_file())
        .unwrap_or_else(|| exe.to_path_buf())
}

/// 找出所有候選（只看檔案系統，不執行任何程式）：虛擬環境（近的在前）、環境變數、全域。
pub fn discover(
    rule: &Rule,
    start: &Path,
    home: Option<&Path>,
    env: &dyn Fn(&str) -> Option<String>,
    path_var: Option<&OsStr>,
) -> Vec<Candidate> {
    let mut out: Vec<Candidate> = Vec::new();
    let push = |out: &mut Vec<Candidate>, c: Candidate| {
        if !out.iter().any(|o| o.program == c.program && o.args == c.args) {
            out.push(c);
        }
    };
    // 執行檔案時放在檔案前面的固定參數
    let run_args = || -> Vec<String> { rule.run_args.iter().map(|a| (*a).to_owned()).collect() };
    // 1. 往上找虛擬環境
    for dir in start.ancestors() {
        for name in rule.env_dirs {
            let env_dir = dir.join(name);
            let program = interpreter_in(rule, &env_dir);
            if env_dir.join(rule.env_marker).is_file() && program.is_file() {
                push(&mut out, Candidate { program, args: run_args(), source: Source::Venv, origin: (*name).to_owned(), env_dir: Some(env_dir) });
            }
        }
        if dir.join(".git").exists() || home.is_some_and(|h| h == dir) {
            break;
        }
    }
    // 2. 環境變數
    for var in rule.env_vars {
        let Some(value) = env(var).filter(|v| !v.is_empty()) else { continue };
        let env_dir = PathBuf::from(value);
        let program = interpreter_in(rule, &env_dir);
        if program.is_file() {
            push(&mut out, Candidate { program, args: run_args(), source: Source::EnvVar, origin: (*var).to_owned(), env_dir: Some(env_dir) });
        }
    }
    // 3. 全域
    let dirs: Vec<PathBuf> = path_var.map(|p| std::env::split_paths(p).collect()).unwrap_or_default();
    let is_skipped = |p: &Path| {
        let norm = p.to_string_lossy().to_ascii_lowercase().replace('\\', "/");
        rule.skip_paths.iter().any(|s| norm.contains(s))
    };
    let find = |name: &str| -> Option<PathBuf> {
        dirs.iter()
            .flat_map(|d| [d.join(name), d.join(format!("{name}.exe"))])
            .find(|p| p.is_file() && !is_skipped(p))
    };
    for name in rule.programs {
        if let Some(program) = find(name) {
            push(&mut out, Candidate { program, args: run_args(), source: Source::Global, origin: "PATH".to_owned(), env_dir: None });
        }
    }
    if cfg!(windows)
        && let Some((name, args)) = rule.windows_launcher
        && let Some(program) = find(name)
    {
        let mut all: Vec<String> = args.iter().map(|a| (*a).to_owned()).collect();
        all.extend(run_args());
        push(&mut out, Candidate { program, args: all, source: Source::Global, origin: name.to_owned(), env_dir: None });
    }
    if cfg!(windows)
        && let Some((name, relatives)) = rule.windows_wrapper
        && let Some(launcher) = find(name)
    {
        // git-bash.exe 在 Git 的根資料夾；scoop 在 PATH 上放的是 shim，要先找到真正的位置
        if let Some(root) = resolve_shim(&launcher).parent() {
            for rel in relatives {
                let program = root.join(rel);
                if program.is_file() {
                    push(&mut out, Candidate { program, args: run_args(), source: Source::Global, origin: name.to_owned(), env_dir: None });
                }
            }
        }
    }
    if cfg!(windows) {
        for path in rule.windows_extra {
            let program = PathBuf::from(path);
            if program.is_file() {
                push(&mut out, Candidate { program, args: run_args(), source: Source::Global, origin: "Git".to_owned(), env_dir: None });
            }
        }
    }
    out
}

/// 從查版本的輸出取出版本號：找到 `version_prefix` 之後，取接著的英數字與點
/// （`Python 3.12.1` → `3.12.1`、`GNU bash, version 5.2.15(1)-release` → `5.2.15`）。
/// Python 2 把版本印在 stderr，所以呼叫端把兩邊合併來看。看不出版本號回傳 `None`。
///
/// 找不到 `version_prefix`（輸出被翻成別的語言了：沒有主控台視窗時 `cmd /c ver` 會印出「[版本 10.0…]」）
/// 就退回去找輸出裡第一個像 `10.0.26300.9818` 的點分數字。
pub fn parse_version(rule: &Rule, output: &str) -> Option<String> {
    let with_prefix = output.find(rule.version_prefix).and_then(|i| {
        let rest = output[i + rule.version_prefix.len()..].trim_start_matches([' ', '\t', '\r', '\n']);
        let token: String = rest.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '.').collect();
        token.chars().next().is_some_and(|c| c.is_ascii_digit()).then_some(token)
    });
    with_prefix.or_else(|| first_dotted_number(output))
}

/// 文字裡第一個「數字.數字…」的詞（前面不能緊接英數字）。
fn first_dotted_number(text: &str) -> Option<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_ascii_digit() && (i == 0 || !(chars[i - 1].is_ascii_alphanumeric() || chars[i - 1] == '.')) {
            let token: String = chars[i..].iter().take_while(|c| c.is_ascii_digit() || **c == '.').collect();
            let token = token.trim_end_matches('.').to_owned();
            if token.contains('.') {
                return Some(token);
            }
            i += token.len().max(1);
        } else {
            i += 1;
        }
    }
    None
}

/// 執行 `--version` 確認這個候選真的能用，回傳版本號；打不開、超過 `timeout`、或輸出看不懂都是 `None`。
pub fn probe_version(rule: &Rule, c: &Candidate, timeout: Duration) -> Option<String> {
    use std::io::Read;
    use std::process::{Command, Stdio};
    let mut cmd = Command::new(&c.program);
    cmd.args(&c.args).args(rule.version_args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW：不要閃出黑色視窗
    }
    let mut child = cmd.spawn().ok()?;
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait().ok()? {
            Some(status) => break status,
            None if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            None => std::thread::sleep(Duration::from_millis(10)),
        }
    };
    if !status.success() {
        return None;
    }
    if rule.versionless {
        return Some(String::new()); // 能執行就算數
    }
    // 輸出可能是本地的字碼頁（cmd 的「版本」是 Big5），不是 UTF-8：讀成位元組再寬鬆轉換，數字照樣讀得到
    let mut bytes = Vec::new();
    if let Some(mut out) = child.stdout.take() {
        let _ = out.read_to_end(&mut bytes);
    }
    if let Some(mut err) = child.stderr.take() {
        let _ = err.read_to_end(&mut bytes);
    }
    parse_version(rule, &String::from_utf8_lossy(&bytes))
}

/// 一個候選與它的版本（`None` 表示執行不了）。
#[derive(Clone, Debug)]
pub struct Item {
    pub candidate: Candidate,
    pub version: Option<String>,
}

/// 一個資料夾的搜尋結果。
#[derive(Clone, Debug, Default)]
pub struct Resolved {
    pub items: Vec<Item>,
}

impl Resolved {
    /// 現在要用的：手動選的（要在候選裡而且能執行）優先，否則第一個能執行的。
    pub fn chosen(&self, manual: Option<&str>) -> Option<&Item> {
        let works = |i: &&Item| i.version.is_some();
        manual
            .and_then(|m| self.items.iter().filter(works).find(|i| i.candidate.key() == m))
            .or_else(|| self.items.iter().find(works))
    }
}

/// 同步做完整個搜尋（探測每個候選的版本）。背景執行緒用它；測試也直接呼叫。
pub fn resolve(
    rule: &Rule,
    start: &Path,
    home: Option<&Path>,
    env: &dyn Fn(&str) -> Option<String>,
    path_var: Option<&OsStr>,
    probe: &dyn Fn(&Candidate) -> Option<String>,
) -> Resolved {
    Resolved { items: discover(rule, start, home, env, path_var).into_iter().map(|c| Item { version: probe(&c), candidate: c }).collect() }
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).filter(|h| !h.is_empty()).map(PathBuf::from)
}

/// 搜尋結果多久之後重找（使用者可能剛建好虛擬環境）。
const STALE_AFTER: Duration = Duration::from_secs(30);
/// 什麼都沒找到時重找得勤一點：使用者多半正在建環境，建好了要馬上看得到。
const STALE_AFTER_EMPTY: Duration = Duration::from_secs(5);

enum Slot {
    Pending,
    Ready(Arc<Resolved>, Instant, bool),
}

/// 背景搜尋與快取：依（語言, 起始資料夾）記住結果；第一次問的時候回傳 `None` 並開一個背景執行緒去找，
/// 找完叫畫面重畫。結果太舊了還是先給舊的，同時在背景重找。
#[derive(Clone, Default)]
pub struct Resolver {
    slots: Arc<Mutex<HashMap<(String, PathBuf), Slot>>>,
}

impl Resolver {
    pub fn get(&self, ctx: &egui::Context, rule: &'static Rule, dir: &Path) -> Option<Arc<Resolved>> {
        // 同一個資料夾裡 Python 與各種 shell 的結果各自記（用種類名稱區分）
        let key = (rule.label.to_owned(), dir.to_path_buf());
        let mut slots = self.slots.lock().ok()?;
        let (current, spawn) = match slots.get(&key) {
            None => (None, true),
            Some(Slot::Pending) => (None, false),
            Some(Slot::Ready(r, at, refreshing)) => {
                let limit = if r.items.iter().any(|i| i.version.is_some()) { STALE_AFTER } else { STALE_AFTER_EMPTY };
                (Some(r.clone()), !*refreshing && at.elapsed() > limit)
            }
        };
        if spawn {
            match slots.get_mut(&key) {
                Some(Slot::Ready(_, _, refreshing)) => *refreshing = true,
                _ => {
                    slots.insert(key.clone(), Slot::Pending);
                }
            }
            let (slots, ctx, dir) = (self.slots.clone(), ctx.clone(), dir.to_path_buf());
            std::thread::spawn(move || {
                let home = home_dir();
                let path = std::env::var_os("PATH");
                let probe = |c: &Candidate| probe_version(rule, c, Duration::from_secs(4));
                let resolved = resolve(rule, &dir, home.as_deref(), &|k| std::env::var(k).ok(), path.as_deref(), &probe);
                if let Ok(mut slots) = slots.lock() {
                    slots.insert(key, Slot::Ready(Arc::new(resolved), Instant::now(), false));
                }
                ctx.request_repaint();
            });
        }
        current
    }

    /// 把這個資料夾的結果標成過期（使用者按了「重新搜尋」）：下次問的時候在背景重找，
    /// 找完之前還是顯示舊的結果，畫面不會閃一下。`dir` 是 `None` 就是全部。
    pub fn refresh(&self, dir: Option<&Path>) {
        let Ok(mut slots) = self.slots.lock() else { return };
        for ((_, d), slot) in slots.iter_mut() {
            if let Slot::Ready(_, at, _) = slot
                && dir.is_none_or(|dir| dir == d)
            {
                *at = at.checked_sub(STALE_AFTER + Duration::from_secs(1)).unwrap_or(*at);
            }
        }
    }
}

/// 開終端機時要加上的環境變數：讓 shell 裡的 `python` 就是這份文件用的那個。
/// 不執行 `activate` 腳本（各種 shell 的腳本不同），直接設 `VIRTUAL_ENV` 並把環境的執行檔資料夾放到 `PATH` 最前面，
/// 所以 pwsh、cmd、bash、zsh 都適用。沒有專案裡的虛擬環境、也沒有手動選擇時回傳空的（用系統原本的環境）。
pub fn terminal_env(
    rule: &Rule,
    dir: &Path,
    manual: Option<&str>,
    home: Option<&Path>,
    env: &dyn Fn(&str) -> Option<String>,
    path_var: Option<&OsStr>,
) -> Vec<(String, String)> {
    let candidates = discover(rule, dir, home, env, path_var);
    let picked = manual
        .and_then(|m| candidates.iter().find(|c| c.key() == m))
        .or_else(|| candidates.iter().find(|c| c.source == Source::Venv));
    let Some(c) = picked else { return Vec::new() };
    let Some(bin) = c.program.parent() else { return Vec::new() };
    let mut dirs = vec![bin.to_path_buf()];
    if let Some(p) = path_var {
        dirs.extend(std::env::split_paths(p));
    }
    let mut out = Vec::new();
    if c.source != Source::Global
        && let Some(env_dir) = &c.env_dir
    {
        out.push(("VIRTUAL_ENV".to_owned(), env_dir.display().to_string()));
    }
    if let Ok(joined) = std::env::join_paths(dirs) {
        out.push(("PATH".to_owned(), joined.to_string_lossy().into_owned()));
    }
    out
}

/// 用真實的系統環境算 [`terminal_env`]。
pub fn terminal_env_here(rule: &Rule, dir: &Path, manual: Option<&str>) -> Vec<(String, String)> {
    let path = std::env::var_os("PATH");
    terminal_env(rule, dir, manual, home_dir().as_deref(), &|k| std::env::var(k).ok(), path.as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 在 `dir` 底下造一個假的虛擬環境（有 `pyvenv.cfg` 與直譯器檔案）。
    fn fake_venv(dir: &Path, name: &str) -> PathBuf {
        let env_dir = dir.join(name);
        let program = interpreter_in(&PYTHON, &env_dir);
        std::fs::create_dir_all(program.parent().unwrap()).unwrap();
        std::fs::write(&program, "").unwrap();
        std::fs::write(env_dir.join("pyvenv.cfg"), "home = /usr\n").unwrap();
        program
    }

    fn no_env(_: &str) -> Option<String> {
        None
    }

    #[test]
    fn the_nearest_virtual_environment_above_the_file_wins_and_a_git_root_stops_the_search() {
        let root = tempfile::tempdir().unwrap();
        let outer = root.path();
        let repo = outer.join("repo");
        let pkg = repo.join("pkg").join("sub");
        std::fs::create_dir_all(&pkg).unwrap();
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        let outer_python = fake_venv(outer, ".venv"); // 在 repo 外面：不該被用到
        let repo_python = fake_venv(&repo, ".venv");
        let pkg_python = fake_venv(&repo.join("pkg"), "venv");
        let found = discover(&PYTHON, &pkg, None, &no_env, None);
        let programs: Vec<&PathBuf> = found.iter().map(|c| &c.program).collect();
        assert_eq!(programs, vec![&pkg_python, &repo_python], "近的在前；.git 那一層之後不再往上");
        assert!(!programs.contains(&&outer_python));
        assert_eq!(found[0].origin, "venv");
        assert_eq!(found[0].source, Source::Venv);
        assert_eq!(project_root(&pkg), repo, "專案根是有 .git 的那一層");
    }

    #[test]
    fn a_folder_that_only_looks_like_an_environment_is_not_one() {
        let root = tempfile::tempdir().unwrap();
        // 有叫 .venv 的資料夾，但沒有 pyvenv.cfg：不算
        let program = interpreter_in(&PYTHON, &root.path().join(".venv"));
        std::fs::create_dir_all(program.parent().unwrap()).unwrap();
        std::fs::write(&program, "").unwrap();
        assert!(discover(&PYTHON, root.path(), None, &no_env, None).is_empty());
    }

    #[test]
    fn the_search_stops_at_the_home_folder() {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        let work = home.join("work");
        std::fs::create_dir_all(&work).unwrap();
        fake_venv(root.path(), ".venv"); // 在家目錄的上面
        assert!(discover(&PYTHON, &work, Some(&home), &no_env, None).is_empty());
        let in_home = fake_venv(&home, ".venv");
        assert_eq!(discover(&PYTHON, &work, Some(&home), &no_env, None)[0].program, in_home, "家目錄那一層本身還是會看");
    }

    #[test]
    fn environment_variables_and_global_interpreters_come_after_the_project_environment() {
        let root = tempfile::tempdir().unwrap();
        let work = root.path().join("work");
        std::fs::create_dir_all(&work).unwrap();
        let venv_python = fake_venv(&work, ".venv");
        let active = root.path().join("active");
        let active_python = fake_venv(root.path(), "active");
        let bin = root.path().join("globalbin");
        std::fs::create_dir_all(&bin).unwrap();
        let name = if cfg!(windows) { "python3.exe" } else { "python3" };
        std::fs::write(bin.join(name), "").unwrap();
        let store = root.path().join("WindowsApps");
        std::fs::create_dir_all(&store).unwrap();
        std::fs::write(store.join(if cfg!(windows) { "python.exe" } else { "python" }), "").unwrap();
        let path = std::env::join_paths([&store, &bin]).unwrap();
        let env = |k: &str| (k == "VIRTUAL_ENV").then(|| active.display().to_string());
        let found = discover(&PYTHON, &work, None, &env, Some(&path));
        let sources: Vec<Source> = found.iter().map(|c| c.source).collect();
        assert_eq!(sources, vec![Source::Venv, Source::EnvVar, Source::Global], "{found:?}");
        assert_eq!(found[0].program, venv_python);
        assert_eq!(found[1].program, active_python);
        assert_eq!(found[2].program, bin.join(name), "商店的假捷徑（WindowsApps）要排除");
        // 同一個直譯器只列一次：環境變數指到專案裡的 .venv 時不重複
        let env2 = |k: &str| (k == "VIRTUAL_ENV").then(|| work.join(".venv").display().to_string());
        assert_eq!(discover(&PYTHON, &work, None, &env2, None).len(), 1);
    }

    #[test]
    fn versions_are_read_from_either_output_stream_and_odd_output_is_rejected() {
        assert_eq!(parse_version(&PYTHON, "Python 3.12.1\n"), Some("3.12.1".to_owned()));
        assert_eq!(parse_version(&PYTHON, "\nPython 2.7.18\n"), Some("2.7.18".to_owned()));
        assert_eq!(parse_version(&PYTHON, "Python 3.13.0rc1 (main)\n"), Some("3.13.0rc1".to_owned()));
        assert_eq!(parse_version(&PYTHON, "xcode-select: note: no developer tools were found\n"), None);
        assert_eq!(parse_version(&PYTHON, "Python \n"), None);
        assert_eq!(parse_version(&PYTHON, ""), None);
    }

    #[test]
    fn the_first_working_candidate_is_the_default_and_a_manual_choice_overrides_it() {
        let root = tempfile::tempdir().unwrap();
        let work = root.path().join("work");
        std::fs::create_dir_all(&work).unwrap();
        let broken = fake_venv(&work, ".venv");
        let good = fake_venv(&work, "venv");
        let probe = |c: &Candidate| (c.program == good).then(|| "3.12.1".to_owned());
        let resolved = resolve(&PYTHON, &work, None, &no_env, None, &probe);
        assert_eq!(resolved.items.len(), 2);
        assert_eq!(resolved.items[0].candidate.program, broken);
        assert_eq!(resolved.chosen(None).unwrap().candidate.program, good, "壞掉的略過");
        // 手動選的優先；選到壞掉的或不在候選裡就忽略
        let both = |_: &Candidate| Some("3.11.0".to_owned());
        let resolved = resolve(&PYTHON, &work, None, &no_env, None, &both);
        let manual = resolved.items[1].candidate.key();
        assert_eq!(resolved.chosen(Some(&manual)).unwrap().candidate.program, good);
        assert_eq!(resolved.chosen(Some("/nowhere/python")).unwrap().candidate.program, broken);
        assert!(Resolved::default().chosen(None).is_none());
    }

    #[test]
    fn a_real_missing_program_is_not_usable() {
        let c = Candidate { program: PathBuf::from("/definitely/not/here/python"), args: vec![], source: Source::Global, origin: "PATH".into(), env_dir: None };
        assert_eq!(probe_version(&PYTHON, &c, Duration::from_secs(1)), None);
    }

    #[test]
    fn terminals_get_the_project_environment_on_the_path_but_global_ones_change_nothing() {
        let root = tempfile::tempdir().unwrap();
        let work = root.path().join("work");
        std::fs::create_dir_all(&work).unwrap();
        // 沒有虛擬環境：不動環境變數
        assert!(terminal_env(&PYTHON, &work, None, None, &no_env, None).is_empty());
        let program = fake_venv(&work, ".venv");
        let old_path = std::env::join_paths(["/usr/bin", "/bin"]).unwrap();
        let vars = terminal_env(&PYTHON, &work, None, None, &no_env, Some(&old_path));
        let get = |k: &str| vars.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
        assert_eq!(get("VIRTUAL_ENV"), Some(work.join(".venv").display().to_string()));
        let new_path = get("PATH").unwrap();
        let dirs: Vec<PathBuf> = std::env::split_paths(&new_path).collect();
        assert_eq!(dirs[0], program.parent().unwrap(), "環境的執行檔資料夾在最前面");
        assert_eq!(dirs.len(), 3, "原本的 PATH 還在後面");
        // 手動選了某個全域直譯器：只把它的資料夾放到 PATH 前面，不設 VIRTUAL_ENV
        let bin = root.path().join("globalbin");
        std::fs::create_dir_all(&bin).unwrap();
        let name = if cfg!(windows) { "python3.exe" } else { "python3" };
        std::fs::write(bin.join(name), "").unwrap();
        let path = std::env::join_paths([&bin]).unwrap();
        let manual = bin.join(name).display().to_string();
        let vars = terminal_env(&PYTHON, &work, Some(&manual), None, &no_env, Some(&path));
        assert!(vars.iter().all(|(k, _)| k != "VIRTUAL_ENV"));
        assert!(vars.iter().any(|(k, v)| k == "PATH" && std::env::split_paths(v).next() == Some(bin.clone())));
    }

    #[test]
    fn a_refresh_picks_up_an_environment_created_later_and_keeps_showing_the_old_result_meanwhile() {
        let root = tempfile::tempdir().unwrap();
        let work = root.path().join("work");
        std::fs::create_dir_all(&work).unwrap();
        let ctx = egui::Context::default();
        let resolver = Resolver::default();
        // 第一次問：還沒有結果，背景開始找
        assert!(resolver.get(&ctx, &PYTHON, &work).is_none());
        let wait_for = |resolver: &Resolver| {
            for _ in 0..400 {
                if let Some(r) = resolver.get(&ctx, &PYTHON, &work) {
                    return r;
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            panic!("等不到搜尋結果");
        };
        let first = wait_for(&resolver);
        let had_candidates = first.items.len();
        // 之後才建好虛擬環境：沒有重新搜尋前看不到
        fake_venv(&work, ".venv");
        assert_eq!(wait_for(&resolver).items.len(), had_candidates);
        // 標成過期：先還是給舊的，背景重找完就有新的環境了
        resolver.refresh(Some(&work));
        assert_eq!(resolver.get(&ctx, &PYTHON, &work).map(|r| r.items.len()), Some(had_candidates), "重找完成之前還是舊的");
        let mut found = false;
        for _ in 0..400 {
            if resolver.get(&ctx, &PYTHON, &work).is_some_and(|r| r.items.iter().any(|i| i.candidate.source == Source::Venv)) {
                found = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        assert!(found, "重新搜尋之後要找得到新建的環境");
    }

    fn label_for(language: &str, path: &str, first_line: &str) -> Option<&'static str> {
        rule_for_file(language, Path::new(path), first_line).map(|r| r.label)
    }

    #[test]
    fn only_python_and_runnable_shell_scripts_have_a_rule() {
        assert_eq!(label_for("Python", "a.py", ""), Some("Python"));
        assert_eq!(label_for("Rust", "a.rs", ""), None);
        assert_eq!(label_for("Markdown", "a.md", ""), None);
    }

    #[test]
    fn shell_scripts_pick_the_interpreter_from_the_shebang_then_the_extension() {
        // 沒有 shebang：看副檔名
        assert_eq!(label_for("Shell", "run.sh", ""), Some("bash"));
        assert_eq!(label_for("Shell", "run.BASH", "echo hi"), Some("bash"));
        assert_eq!(label_for("Shell", "run.zsh", ""), Some("zsh"));
        assert_eq!(label_for("Shell", "build.ps1", ""), Some("PowerShell"));
        assert_eq!(label_for("Shell", "mod.psm1", ""), Some("PowerShell"));
        assert_eq!(label_for("Shell", "x.bat", "@echo off"), Some("cmd"));
        assert_eq!(label_for("Shell", "X.CMD", ""), Some("cmd"));
        // shebang 優先，不管副檔名
        assert_eq!(label_for("Shell", "run.sh", "#!/bin/zsh"), Some("zsh"));
        assert_eq!(label_for("Shell", "script", "#!/usr/bin/env bash"), Some("bash"));
        assert_eq!(label_for("Shell", "script", "#!/usr/bin/env -S bash -e"), Some("bash"));
        assert_eq!(label_for("Shell", "script", "#!/bin/sh"), Some("sh"));
        assert_eq!(label_for("Shell", "script", "#! /bin/dash"), Some("sh"));
        assert_eq!(label_for("Shell", "script", "#!/usr/bin/pwsh"), Some("PowerShell"));
        assert_eq!(label_for("Shell", "x.sh", "#!C:\\Windows\\System32\\bash.exe"), Some("bash"));
        // 副檔名不明確又沒有 shebang：不是可以執行的腳本
        assert_eq!(label_for("Shell", "script", ""), None);
        assert_eq!(label_for("Shell", "script", "echo hi"), None);
        assert_eq!(label_for("Shell", "profile.txt", ""), None);
        assert_eq!(label_for("Shell", ".bashrc", ""), None, "沒有副檔名");
        // shebang 指到不支援的直譯器：不執行
        assert_eq!(label_for("Shell", "x.sh", "#!/usr/bin/fish"), None);
        assert_eq!(label_for("Shell", "x.sh", "#!"), None);
    }

    #[test]
    fn shell_versions_are_read_from_each_interpreters_own_output() {
        assert_eq!(parse_version(&BASH, "GNU bash, version 5.2.15(1)-release (x86_64-pc-linux-gnu)\nCopyright"), Some("5.2.15".to_owned()));
        assert_eq!(parse_version(&ZSH, "zsh 5.9 (x86_64-apple-darwin23.0)\n"), Some("5.9".to_owned()));
        assert_eq!(parse_version(&POWERSHELL, "7.4.1\r\n"), Some("7.4.1".to_owned()));
        assert_eq!(parse_version(&POWERSHELL, "5.1.22621.4391\n"), Some("5.1.22621.4391".to_owned()));
        assert_eq!(parse_version(&CMD, "\r\nMicrosoft Windows [Version 10.0.22631.4460]\r\n"), Some("10.0.22631.4460".to_owned()));
        assert_eq!(parse_version(&BASH, "WSL has no installed distributions"), None);
    }

    #[test]
    fn a_translated_version_line_still_gives_the_version_number() {
        // 本地語系的輸出：沒有英文的前綴，退回找第一個點分數字
        assert_eq!(parse_version(&CMD, "\r\nMicrosoft Windows [版本 10.0.26300.9818]\r\n"), Some("10.0.26300.9818".to_owned()));
        assert_eq!(parse_version(&BASH, "GNU bash, 版本 5.2.15(1)-release"), Some("5.2.15".to_owned()));
        assert_eq!(parse_version(&PYTHON, "Python 3.12.1"), Some("3.12.1".to_owned()), "有前綴時照舊");
        assert_eq!(parse_version(&BASH, "build 7 of 20"), None, "沒有點的數字不算版本");
        assert_eq!(parse_version(&BASH, "abc1.2 x"), None, "前面緊接英文字母的不算");
        assert_eq!(first_dotted_number("v2 then 3.4."), Some("3.4".to_owned()));
    }

    #[test]
    fn wsl_entry_points_are_skipped_and_git_bash_is_found_through_scoop_shims() {
        let root = tempfile::tempdir().unwrap();
        let sys = root.path().join("Windows").join("System32");
        let apps = root.path().join("Microsoft").join("WindowsApps");
        for d in [&sys, &apps] {
            std::fs::create_dir_all(d).unwrap();
            std::fs::write(d.join(if cfg!(windows) { "bash.exe" } else { "bash" }), "").unwrap();
        }
        let path = std::env::join_paths([&sys, &apps]).unwrap();
        assert!(discover(&BASH, root.path(), None, &no_env, Some(&path)).is_empty(), "WSL 的 bash 入口不算");
        // 一般路徑上的 bash 照常找得到
        let good = root.path().join("bin");
        std::fs::create_dir_all(&good).unwrap();
        std::fs::write(good.join(if cfg!(windows) { "bash.exe" } else { "bash" }), "").unwrap();
        let path = std::env::join_paths([&sys, &good]).unwrap();
        assert_eq!(discover(&BASH, root.path(), None, &no_env, Some(&path))[0].program, good.join(if cfg!(windows) { "bash.exe" } else { "bash" }));
        // scoop 的 shim：shims\git-bash.exe 旁邊的 .shim 檔指到真正的 Git 資料夾
        let git = root.path().join("apps").join("git").join("current");
        std::fs::create_dir_all(git.join("bin")).unwrap();
        std::fs::write(git.join("git-bash.exe"), "").unwrap();
        std::fs::write(git.join("bin").join("bash.exe"), "").unwrap();
        let shims = root.path().join("shims");
        std::fs::create_dir_all(&shims).unwrap();
        std::fs::write(shims.join("git-bash.exe"), "").unwrap();
        std::fs::write(shims.join("git-bash.shim"), format!("path = \"{}\"\n", git.join("git-bash.exe").display())).unwrap();
        assert_eq!(resolve_shim(&shims.join("git-bash.exe")), git.join("git-bash.exe"));
        // 沒有 shim 檔、或 shim 指到不存在的檔案：就用它本身
        assert_eq!(resolve_shim(&git.join("git-bash.exe")), git.join("git-bash.exe"));
        std::fs::write(shims.join("git-bash.shim"), "path = \"C:/nowhere/git-bash.exe\"\n").unwrap();
        assert_eq!(resolve_shim(&shims.join("git-bash.exe")), shims.join("git-bash.exe"));
        // Windows 上：PATH 只有 shim 與 WSL 入口時，找到的是 Git 裡真正的 bash.exe，來源標成 git-bash
        #[cfg(windows)]
        {
            std::fs::write(shims.join("git-bash.shim"), format!("path = \"{}\"\n", git.join("git-bash.exe").display())).unwrap();
            let path = std::env::join_paths([&sys, &shims]).unwrap();
            let found = discover(&BASH, root.path(), None, &no_env, Some(&path));
            assert_eq!(found.len(), 1, "{found:?}");
            assert_eq!((found[0].program.clone(), found[0].origin.as_str()), (git.join("bin").join("bash.exe"), "git-bash"));
        }
    }

    #[test]
    fn shell_candidates_come_from_the_path_and_keep_the_fixed_run_arguments() {
        let root = tempfile::tempdir().unwrap();
        let bin = root.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let (bash, cmd) = if cfg!(windows) { ("bash.exe", "cmd.exe") } else { ("bash", "cmd") };
        std::fs::write(bin.join(bash), "").unwrap();
        std::fs::write(bin.join(cmd), "").unwrap();
        let path = std::env::join_paths([&bin]).unwrap();
        let found = discover(&BASH, root.path(), None, &no_env, Some(&path));
        assert_eq!(found.iter().map(|c| c.program.clone()).collect::<Vec<_>>().first(), Some(&bin.join(bash)));
        assert!(found[0].args.is_empty() && found[0].source == Source::Global);
        let found = discover(&CMD, root.path(), None, &no_env, Some(&path));
        assert_eq!(found[0].args, vec!["/c".to_owned()], "cmd 要加 /c 才會執行檔案");
        // 虛擬環境的規則不影響 shell：就算旁邊有 .venv 也不會找到
        fake_venv(root.path(), ".venv");
        assert!(discover(&BASH, root.path(), None, &no_env, None).iter().all(|c| c.source == Source::Global));
        // Windows 商店的 pwsh 別名是真的能用的，不能像 Python 那樣被排除
        let store = root.path().join("WindowsApps");
        std::fs::create_dir_all(&store).unwrap();
        std::fs::write(store.join(if cfg!(windows) { "pwsh.exe" } else { "pwsh" }), "").unwrap();
        let path = std::env::join_paths([&store]).unwrap();
        assert_eq!(discover(&POWERSHELL, root.path(), None, &no_env, Some(&path)).len(), 1);
    }

    #[cfg(windows)]
    #[test]
    fn the_real_cmd_is_found_and_works() {
        let path = std::env::var_os("PATH");
        let found = discover(&CMD, Path::new("."), None, &|k| std::env::var(k).ok(), path.as_deref());
        assert!(!found.is_empty(), "PATH 上要找得到 cmd.exe：{path:?}");
        // 實際執行（沒有主控台視窗時，cmd 的輸出會是本地語系的「[版本 …]」，也要讀得出來）
        let version = probe_version(&CMD, &found[0], Duration::from_secs(5));
        assert!(version.as_deref().is_some_and(|v| v.starts_with("10.")), "{:?} 要能執行並讀出版本：{version:?}", found[0]);
    }

    /// 這台機器有 git-bash 時（沒有就略過）：真的找得到能用的 bash，而且不是 WSL 的入口。
    #[cfg(windows)]
    #[test]
    fn the_real_git_bash_is_found_when_it_is_installed() {
        let path = std::env::var_os("PATH");
        let on_path = std::env::split_paths(path.as_deref().unwrap_or_default()).any(|d| d.join("git-bash.exe").is_file());
        if !on_path {
            return;
        }
        let found = discover(&BASH, Path::new("."), None, &|k| std::env::var(k).ok(), path.as_deref());
        assert!(!found.is_empty(), "PATH 上有 git-bash，要找得到它的 bash");
        assert!(found.iter().all(|c| !c.program.to_string_lossy().to_ascii_lowercase().contains("system32")), "{found:?}");
        let version = probe_version(&BASH, &found[0], Duration::from_secs(10));
        assert!(version.is_some(), "{:?} 要能執行：{version:?}", found[0]);
    }

    #[test]
    fn versionless_interpreters_and_titles_and_choice_keys() {
        assert_eq!(parse_version(&SH, "anything"), None, "sh 沒有版本可以查");
        let item = |v: Option<&str>| Item {
            candidate: Candidate { program: PathBuf::from("x"), args: vec![], source: Source::Global, origin: "PATH".into(), env_dir: None },
            version: v.map(str::to_owned),
        };
        assert_eq!(describe(&PYTHON, &item(Some("3.12.1"))), "Python 3.12.1");
        assert_eq!(describe(&SH, &item(Some(""))), "sh", "沒有版本號就只有名稱");
        assert_eq!(describe(&PYTHON, &item(None)), "Python");
        // 手動選擇的鍵：Python 維持舊的寫法，其他的加上種類，同一個專案可以各選各的
        assert_eq!(choice_key(&PYTHON, "/p"), "/p");
        assert_eq!(choice_key(&BASH, "/p"), "/p#bash");
        assert_ne!(choice_key(&BASH, "/p"), choice_key(&ZSH, "/p"));
    }
}
