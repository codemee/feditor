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
    /// 語言名稱（和 `syntax::Language::name` 一致）。
    pub language: &'static str,
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
    /// 取得版本的參數，與輸出裡版本號之前的文字（`Python 3.12.1` 的 `Python `）。
    pub version_args: &'static [&'static str],
    pub version_prefix: &'static str,
}

pub const PYTHON: Rule = Rule {
    language: "Python",
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
};

/// 這個語言有沒有找執行環境的規則。
pub fn rule_for(language: &str) -> Option<&'static Rule> {
    [&PYTHON].into_iter().find(|r| r.language == language)
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
    // 1. 往上找虛擬環境
    for dir in start.ancestors() {
        for name in rule.env_dirs {
            let env_dir = dir.join(name);
            let program = interpreter_in(rule, &env_dir);
            if env_dir.join(rule.env_marker).is_file() && program.is_file() {
                push(&mut out, Candidate { program, args: vec![], source: Source::Venv, origin: (*name).to_owned(), env_dir: Some(env_dir) });
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
            push(&mut out, Candidate { program, args: vec![], source: Source::EnvVar, origin: (*var).to_owned(), env_dir: Some(env_dir) });
        }
    }
    // 3. 全域
    let dirs: Vec<PathBuf> = path_var.map(|p| std::env::split_paths(p).collect()).unwrap_or_default();
    let is_store_stub = |p: &Path| p.to_string_lossy().to_ascii_lowercase().contains("windowsapps");
    let find = |name: &str| -> Option<PathBuf> {
        dirs.iter()
            .flat_map(|d| [d.join(name), d.join(format!("{name}.exe"))])
            .find(|p| p.is_file() && !is_store_stub(p))
    };
    for name in rule.programs {
        if let Some(program) = find(name) {
            push(&mut out, Candidate { program, args: vec![], source: Source::Global, origin: "PATH".to_owned(), env_dir: None });
        }
    }
    if cfg!(windows)
        && let Some((name, args)) = rule.windows_launcher
        && let Some(program) = find(name)
    {
        push(
            &mut out,
            Candidate { program, args: args.iter().map(|a| (*a).to_owned()).collect(), source: Source::Global, origin: name.to_owned(), env_dir: None },
        );
    }
    out
}

/// 從 `--version` 的輸出取出版本號（`Python 3.12.1` → `3.12.1`）；Python 2 把版本印在 stderr，所以兩邊合併來看。
pub fn parse_version(rule: &Rule, output: &str) -> Option<String> {
    output
        .lines()
        .map(str::trim)
        .find_map(|l| l.strip_prefix(rule.version_prefix))
        .and_then(|rest| rest.split_whitespace().next())
        .filter(|v| v.chars().next().is_some_and(|c| c.is_ascii_digit()))
        .map(str::to_owned)
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
    let mut text = String::new();
    if let Some(mut out) = child.stdout.take() {
        let _ = out.read_to_string(&mut text);
    }
    if let Some(mut err) = child.stderr.take() {
        let _ = err.read_to_string(&mut text);
    }
    parse_version(rule, &text)
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
        let key = (rule.language.to_owned(), dir.to_path_buf());
        let mut slots = self.slots.lock().ok()?;
        let (current, spawn) = match slots.get(&key) {
            None => (None, true),
            Some(Slot::Pending) => (None, false),
            Some(Slot::Ready(r, at, refreshing)) => (Some(r.clone()), !*refreshing && at.elapsed() > STALE_AFTER),
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

    /// 丟掉這個資料夾的結果，下次問的時候重找（使用者按了「重新搜尋」）。
    pub fn forget_dir(&self, dir: &Path) {
        if let Ok(mut slots) = self.slots.lock() {
            slots.retain(|(_, d), _| d != dir);
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
    fn only_languages_with_a_rule_are_searched() {
        assert!(rule_for("Python").is_some());
        assert!(rule_for("Rust").is_none());
    }
}
