//! 專案命令：開啟某種語言的檔案時，從檔案所在資料夾往上找專案設定檔（`Cargo.toml`、`pyproject.toml`），
//! 把其中可以執行的命令列為「在終端機執行」的選項（設計見 `docs/project-commands.md`）。
//!
//! - 提供者（[`cargo_findings`]、[`python_findings`]）只讀設定檔、產生命令的描述，不碰工具偵測、不執行任何東西。
//! - 工具偵測（[`detect_tool`]）找命令需要的工具（`cargo`、`uv`、`poetry`），並用 `--version` 確認真的能跑。
//! - **工具不是可用的，它的命令就不會進入可選清單**，只留一筆說明（[`Notice`]），避免出現按了執行不了的項目。
//! - 結果由背景執行緒算好、快取（[`Resolver`]），和直譯器搜尋（`runtime_env::Resolver`）互不相干。

use std::collections::HashMap;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use eframe::egui;

/// 設定檔大小上限：超過就略過，不讓巨大的檔案拖慢搜尋。
const MAX_CONFIG_BYTES: u64 = 1024 * 1024;
/// 同一份設定檔最多列出幾個腳本／執行檔，其餘略過（清單太長沒有意義）。
const MAX_LISTED: usize = 12;

/// 一種命令列工具與它的安裝提示。
#[derive(Debug)]
pub struct ToolSpec {
    pub name: &'static str,
    /// `PATH` 找不到時，另外試的位置（相對於家目錄）；從圖形介面啟動時 `PATH` 常常不含這些。
    pub home_dirs: &'static [&'static str],
    pub hint_windows: &'static str,
    pub hint_unix: &'static str,
}

impl ToolSpec {
    pub fn hint(&self) -> &'static str {
        if cfg!(windows) { self.hint_windows } else { self.hint_unix }
    }
}

pub static CARGO: ToolSpec = ToolSpec {
    name: "cargo",
    home_dirs: &[".cargo/bin"],
    hint_windows: "scoop install rustup",
    hint_unix: "brew install rustup, or see https://rustup.rs",
};
pub static UV: ToolSpec = ToolSpec {
    name: "uv",
    home_dirs: &[".local/bin", ".cargo/bin"],
    hint_windows: "scoop install uv",
    hint_unix: "brew install uv",
};
pub static POETRY: ToolSpec = ToolSpec {
    name: "poetry",
    home_dirs: &[".local/bin"],
    hint_windows: "pipx install poetry",
    hint_unix: "pipx install poetry",
};

/// 這個語言有沒有專案命令的提供者。
pub fn supports(lang: &str) -> bool {
    matches!(lang, "Rust" | "Python")
}

/// 提供者產生的命令描述：還沒綁定工具的路徑。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandSpec {
    /// 設定裡記住手動選擇用的穩定識別：`cargo:run`、`cargo:example:demo`、`uv:script:serve`…
    pub id: String,
    /// 選單上顯示的名稱。
    pub label: String,
    /// 直接執行的檔案（`.venv` 裡的腳本）；`None` 表示用 [`Finding::tool`] 偵測到的工具。
    pub program: Option<PathBuf>,
    pub args: Vec<String>,
    /// 命令後面是否接目前的檔案（`uv run 檔案.py` 要，`cargo run` 不要）。
    pub takes_file: bool,
    /// 使用者寫的額外參數是否要放在 `--` 後面（交給 `cargo run` 跑的程式本身）。
    pub extra_sep: bool,
}

impl CommandSpec {
    fn new(id: impl Into<String>, program: &str, args: &[&str]) -> Self {
        let args: Vec<String> = args.iter().map(|a| (*a).to_owned()).collect();
        let label = std::iter::once(program.to_owned()).chain(args.iter().cloned()).collect::<Vec<_>>().join(" ");
        CommandSpec { id: id.into(), label, program: None, args, takes_file: false, extra_sep: false }
    }
    fn with_file(mut self, name: &str) -> Self {
        self.takes_file = true;
        self.label = format!("{} {name}", self.label);
        self
    }
    fn with_sep(mut self) -> Self {
        self.extra_sep = true;
        self
    }
}

/// 一個提供者在一份設定檔裡的發現。
#[derive(Clone, Debug)]
pub struct Finding {
    pub source: PathBuf,
    /// 命令執行的資料夾（設定檔所在的資料夾）。
    pub cwd: PathBuf,
    /// 命令需要的工具；`None` 表示命令自己就是可以直接執行的檔案。
    pub tool: Option<&'static ToolSpec>,
    /// 說明文字裡引用的依據，例如 `pyproject.toml、uv.lock`。
    pub evidence: String,
    pub commands: Vec<CommandSpec>,
}

/// 從 `start` 往上找名叫 `name` 的檔案：走到有 `.git` 的那一層、家目錄、或磁碟根目錄就停（和直譯器搜尋一樣，
/// 不會去用別的專案的設定）。
fn find_up(start: &Path, name: &str, home: Option<&Path>) -> Option<PathBuf> {
    for dir in start.ancestors() {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
        if dir.join(".git").exists() || home.is_some_and(|h| h == dir) {
            break;
        }
    }
    None
}

fn read_limited(path: &Path) -> Option<String> {
    if std::fs::metadata(path).ok()?.len() > MAX_CONFIG_BYTES {
        return None;
    }
    std::fs::read_to_string(path).ok()
}

fn read_toml(path: &Path) -> Option<toml::Table> {
    read_limited(path)?.parse::<toml::Table>().ok()
}

fn push_unique(list: &mut Vec<CommandSpec>, spec: CommandSpec) {
    if !list.iter().any(|c| c.id == spec.id) {
        list.push(spec);
    }
}

// ───────────────────────────── Cargo ─────────────────────────────

/// 檔案相對於 crate 資料夾的位置，決定要排在最前面的命令。
enum CargoTarget {
    Main,
    Bin(String),
    Example(String),
    Test(String),
}

fn cargo_target(rel: &[String]) -> Option<CargoTarget> {
    let stem = |s: &str| s.strip_suffix(".rs").map(str::to_owned);
    let parts: Vec<&str> = rel.iter().map(String::as_str).collect();
    match parts.as_slice() {
        ["src", "main.rs"] => Some(CargoTarget::Main),
        ["src", "bin", file] => stem(file).map(CargoTarget::Bin),
        ["src", "bin", dir, "main.rs"] => Some(CargoTarget::Bin((*dir).to_owned())),
        ["examples", file] => stem(file).map(CargoTarget::Example),
        ["examples", dir, "main.rs"] => Some(CargoTarget::Example((*dir).to_owned())),
        ["tests", file] => stem(file).map(CargoTarget::Test),
        ["tests", dir, "main.rs"] => Some(CargoTarget::Test((*dir).to_owned())),
        _ => None,
    }
}

/// crate 裡所有的執行檔名稱：`src/main.rs`（名稱是套件名）、`src/bin/*`、`[[bin]]`。
fn cargo_bins(root: &Path, doc: &toml::Table, package_name: &str) -> Vec<String> {
    let mut bins: Vec<String> = Vec::new();
    let mut add = |name: String| {
        if !name.is_empty() && !bins.contains(&name) {
            bins.push(name);
        }
    };
    if root.join("src/main.rs").is_file() {
        add(package_name.to_owned());
    }
    if let Ok(entries) = std::fs::read_dir(root.join("src/bin")) {
        let mut found: Vec<String> = entries
            .flatten()
            .filter_map(|e| {
                let path = e.path();
                if path.is_dir() {
                    path.join("main.rs").is_file().then(|| e.file_name().to_string_lossy().into_owned())
                } else {
                    path.extension().is_some_and(|x| x == "rs").then(|| path.file_stem().unwrap_or_default().to_string_lossy().into_owned())
                }
            })
            .collect();
        found.sort();
        found.into_iter().for_each(&mut add);
    }
    if let Some(list) = doc.get("bin").and_then(|b| b.as_array()) {
        for name in list.iter().filter_map(|b| b.get("name")).filter_map(|n| n.as_str()) {
            add(name.to_owned());
        }
    }
    bins
}

/// `.rs` 檔的 Cargo 專案命令。
pub fn cargo_findings(file: &Path, home: Option<&Path>) -> Vec<Finding> {
    let Some(dir) = file.parent() else { return Vec::new() };
    let Some(manifest) = find_up(dir, "Cargo.toml", home) else { return Vec::new() };
    let Some(root) = manifest.parent().map(Path::to_path_buf) else { return Vec::new() };
    let Some(doc) = read_toml(&manifest) else { return Vec::new() };

    let rel: Vec<String> = file
        .strip_prefix(&root)
        .map(|p| p.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect())
        .unwrap_or_default();
    let mut commands: Vec<CommandSpec> = Vec::new();

    if let Some(package) = doc.get("package").and_then(|p| p.as_table()) {
        let name = package.get("name").and_then(|n| n.as_str()).unwrap_or_default();
        let default_run = package.get("default-run").and_then(|n| n.as_str());
        let bins = cargo_bins(&root, &doc, name);
        // 只有一個執行檔（或指定了 default-run）時 `cargo run` 就夠；否則每個執行檔各一項
        let plain_run = bins.len() == 1 || default_run.is_some();
        let run_bin = |bin: &str| {
            if bins.len() == 1 && bins[0] == bin {
                CommandSpec::new("cargo:run", "cargo", &["run"]).with_sep()
            } else {
                CommandSpec::new(format!("cargo:bin:{bin}"), "cargo", &["run", "--bin", bin]).with_sep()
            }
        };
        match cargo_target(&rel) {
            Some(CargoTarget::Main) => push_unique(&mut commands, run_bin(name)),
            Some(CargoTarget::Bin(bin)) => push_unique(&mut commands, run_bin(&bin)),
            Some(CargoTarget::Example(example)) => push_unique(
                &mut commands,
                CommandSpec::new(format!("cargo:example:{example}"), "cargo", &["run", "--example", &example]).with_sep(),
            ),
            Some(CargoTarget::Test(test)) => push_unique(
                &mut commands,
                CommandSpec::new(format!("cargo:test:{test}"), "cargo", &["test", "--test", &test]).with_sep(),
            ),
            None => {}
        }
        if plain_run && !bins.is_empty() {
            push_unique(&mut commands, CommandSpec::new("cargo:run", "cargo", &["run"]).with_sep());
        } else {
            for bin in bins.iter().take(MAX_LISTED) {
                push_unique(&mut commands, run_bin(bin));
            }
        }
    }
    push_unique(&mut commands, CommandSpec::new("cargo:test", "cargo", &["test"]).with_sep());
    push_unique(&mut commands, CommandSpec::new("cargo:build", "cargo", &["build"]));

    vec![Finding { source: manifest, cwd: root, tool: Some(&CARGO), evidence: "Cargo.toml".to_owned(), commands }]
}

// ───────────────────────────── Python ─────────────────────────────

fn table_keys(doc: &toml::Table, path: &[&str]) -> Vec<String> {
    let mut value = Some(toml::Value::Table(doc.clone()));
    for key in path {
        value = value.and_then(|v| v.get(*key).cloned());
    }
    match value {
        Some(toml::Value::Table(t)) => t.keys().cloned().collect(),
        _ => Vec::new(),
    }
}

fn has_table(doc: &toml::Table, path: &[&str]) -> bool {
    let mut value: Option<&toml::Value> = None;
    for (i, key) in path.iter().enumerate() {
        value = if i == 0 { doc.get(*key) } else { value.and_then(|v| v.get(*key)) };
        if value.is_none() {
            return false;
        }
    }
    value.is_some_and(|v| v.is_table())
}

/// `.py` 檔的專案命令：uv、poetry，以及專案 `.venv` 裡已經裝好的腳本。
pub fn python_findings(file: &Path, home: Option<&Path>) -> Vec<Finding> {
    let Some(dir) = file.parent() else { return Vec::new() };
    let Some(manifest) = find_up(dir, "pyproject.toml", home) else { return Vec::new() };
    let Some(root) = manifest.parent().map(Path::to_path_buf) else { return Vec::new() };
    let Some(doc) = read_toml(&manifest) else { return Vec::new() };
    let file_name = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();

    let mut scripts = table_keys(&doc, &["project", "scripts"]);
    for name in table_keys(&doc, &["tool", "poetry", "scripts"]) {
        if !scripts.contains(&name) {
            scripts.push(name);
        }
    }
    scripts.truncate(MAX_LISTED);

    let uv_lock = root.join("uv.lock").is_file();
    let poetry_lock = root.join("poetry.lock").is_file();
    let uses_uv = uv_lock || has_table(&doc, &["tool", "uv"]);
    let uses_poetry = poetry_lock || has_table(&doc, &["tool", "poetry"]);
    let venv = root.join(".venv");
    let venv_ok = venv.join("pyvenv.cfg").is_file();

    // .venv 裡已經裝好的腳本：直接執行，不需要 uv 或 poetry
    let mut direct: Vec<CommandSpec> = Vec::new();
    let mut direct_names: Vec<&str> = Vec::new();
    if venv_ok {
        for name in &scripts {
            let exe = if cfg!(windows) { venv.join("Scripts").join(format!("{name}.exe")) } else { venv.join("bin").join(name) };
            if exe.is_file() {
                let mut spec = CommandSpec::new(format!("venv:script:{name}"), name, &[]);
                spec.program = Some(exe);
                direct.push(spec);
                direct_names.push(name);
            }
        }
    }

    let mut findings = Vec::new();
    let evidence = |tool_file: &str, uses: bool| {
        let mut parts = vec!["pyproject.toml".to_owned()];
        if uses && root.join(tool_file).is_file() {
            parts.push(tool_file.to_owned());
        }
        parts.join(", ")
    };
    if uses_uv {
        let mut commands = vec![CommandSpec::new("uv:file", "uv", &["run"]).with_file(&file_name).with_sep()];
        for name in scripts.iter().filter(|n| !direct_names.contains(&n.as_str())) {
            commands.push(CommandSpec::new(format!("uv:script:{name}"), "uv", &["run", name]).with_sep());
        }
        findings.push(Finding { source: manifest.clone(), cwd: root.clone(), tool: Some(&UV), evidence: evidence("uv.lock", uv_lock), commands });
    }
    if uses_poetry {
        let mut commands = vec![CommandSpec::new("poetry:file", "poetry", &["run", "python"]).with_file(&file_name).with_sep()];
        for name in scripts.iter().filter(|n| !direct_names.contains(&n.as_str())) {
            commands.push(CommandSpec::new(format!("poetry:script:{name}"), "poetry", &["run", name]).with_sep());
        }
        findings.push(Finding { source: manifest.clone(), cwd: root.clone(), tool: Some(&POETRY), evidence: evidence("poetry.lock", poetry_lock), commands });
    }
    if !direct.is_empty() {
        for spec in &mut direct {
            spec.extra_sep = false; // 腳本自己的參數，直接接在後面
        }
        findings.push(Finding { source: manifest, cwd: root, tool: None, evidence: "pyproject.toml".to_owned(), commands: direct });
    }
    findings
}

// ───────────────────────────── 工具偵測 ─────────────────────────────

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ToolStatus {
    Available { path: PathBuf, version: String },
    NotFound,
    /// 找到了但跑不起來（例如 rustup 的 `cargo` 沒有安裝任何工具鏈），附上原因。
    Broken(String),
}

/// 工具在 `PATH`（以及家目錄裡的固定位置）上的實際檔案；Windows 試 `.exe`、`.cmd`、`.bat`，
/// 並排除商店的假捷徑 `WindowsApps`。
pub fn find_tool(spec: &ToolSpec, home: Option<&Path>, path_var: Option<&OsStr>) -> Option<PathBuf> {
    let mut dirs: Vec<PathBuf> = path_var.map(|p| std::env::split_paths(p).collect()).unwrap_or_default();
    if let Some(home) = home {
        dirs.extend(spec.home_dirs.iter().map(|d| home.join(d)));
    }
    let exts: &[&str] = if cfg!(windows) { &[".exe", ".cmd", ".bat"] } else { &[""] };
    dirs.iter()
        .filter(|d| !d.to_string_lossy().to_ascii_lowercase().replace('\\', "/").contains("windowsapps"))
        .flat_map(|d| exts.iter().map(move |e| d.join(format!("{}{e}", spec.name))))
        .find(|p| p.is_file())
}

/// 偵測工具：先找檔案，找到了再用 `probe` 確認真的能執行（回傳版本號；不能執行回傳原因）。
pub fn detect_tool(
    spec: &ToolSpec,
    home: Option<&Path>,
    path_var: Option<&OsStr>,
    probe: &dyn Fn(&Path) -> Result<String, String>,
) -> ToolStatus {
    match find_tool(spec, home, path_var) {
        None => ToolStatus::NotFound,
        Some(path) => match probe(&path) {
            Ok(version) => ToolStatus::Available { path, version },
            Err(reason) => ToolStatus::Broken(reason),
        },
    }
}

/// 執行 `工具 --version`：成功回傳版本號；打不開、逾時、離開碼不是 0 都回傳原因（不會執行任何專案裡的東西）。
pub fn probe_tool(path: &Path, timeout: Duration) -> Result<String, String> {
    use std::io::Read;
    use std::process::{Command, Stdio};
    let mut cmd = Command::new(path);
    cmd.arg("--version").stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let mut child = cmd.spawn().map_err(|e| e.to_string())?;
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait().map_err(|e| e.to_string())? {
            Some(status) => break status,
            None if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("timeout".to_owned());
            }
            None => std::thread::sleep(Duration::from_millis(10)),
        }
    };
    let mut out = Vec::new();
    if let Some(mut s) = child.stdout.take() {
        let _ = s.read_to_end(&mut out);
    }
    let mut err = Vec::new();
    if let Some(mut s) = child.stderr.take() {
        let _ = s.read_to_end(&mut err);
    }
    if !status.success() {
        let text = String::from_utf8_lossy(&err);
        let line = text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
        let line: String = line.chars().take(160).collect();
        return Err(if line.is_empty() { format!("exit code {}", status.code().unwrap_or(-1)) } else { line });
    }
    let text = String::from_utf8_lossy(&out).into_owned() + &String::from_utf8_lossy(&err);
    Ok(crate::runtime_env::first_dotted_number(&text).unwrap_or_default())
}

// ───────────────────────────── 結果 ─────────────────────────────

/// 確認能執行的專案命令。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectCommand {
    pub id: String,
    pub label: String,
    pub program: PathBuf,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    /// 來源設定檔的檔名，顯示用。
    pub source: String,
    pub takes_file: bool,
    pub extra_sep: bool,
}

impl ProjectCommand {
    /// 滑鼠停在選單項目上時顯示：執行的資料夾與完整的命令列。
    pub fn tooltip(&self) -> String {
        let line = std::iter::once(self.program.display().to_string()).chain(self.args.iter().cloned()).collect::<Vec<_>>().join(" ");
        format!("{}\n{line}", self.cwd.display())
    }
}

/// 「找到專案設定，但工具用不了」的說明（不是可以按的項目）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notice {
    pub tool: &'static str,
    pub evidence: String,
    /// `None`：找不到工具；`Some`：找到了但跑不起來，附原因。
    pub problem: Option<String>,
    pub hint: &'static str,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProjectResolved {
    pub commands: Vec<ProjectCommand>,
    pub notices: Vec<Notice>,
    /// 命令來源的設定檔檔名（不重複），選單標題用。
    pub sources: Vec<String>,
}

impl ProjectResolved {
    pub fn is_empty(&self) -> bool {
        self.commands.is_empty() && self.notices.is_empty()
    }
    pub fn find(&self, id: &str) -> Option<&ProjectCommand> {
        self.commands.iter().find(|c| c.id == id)
    }
}

/// 同步做完整個搜尋。背景執行緒用它；測試也直接呼叫（`probe` 可以換成假的）。
pub fn resolve(
    lang: &str,
    file: &Path,
    home: Option<&Path>,
    path_var: Option<&OsStr>,
    probe: &dyn Fn(&Path) -> Result<String, String>,
) -> ProjectResolved {
    let findings = match lang {
        "Rust" => cargo_findings(file, home),
        "Python" => python_findings(file, home),
        _ => Vec::new(),
    };
    let mut out = ProjectResolved::default();
    let mut status: HashMap<&'static str, ToolStatus> = HashMap::new();
    for finding in findings {
        if finding.commands.is_empty() {
            continue;
        }
        let source = finding.source.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let bind = |out: &mut ProjectResolved, spec: &CommandSpec, program: PathBuf| {
            if out.commands.iter().any(|c| c.id == spec.id) {
                return;
            }
            out.commands.push(ProjectCommand {
                id: spec.id.clone(),
                label: spec.label.clone(),
                program,
                args: spec.args.clone(),
                cwd: finding.cwd.clone(),
                source: source.clone(),
                takes_file: spec.takes_file,
                extra_sep: spec.extra_sep,
            });
            if !out.sources.contains(&source) {
                out.sources.push(source.clone());
            }
        };
        match finding.tool {
            None => {
                for spec in &finding.commands {
                    if let Some(program) = spec.program.clone() {
                        bind(&mut out, spec, program);
                    }
                }
            }
            Some(tool) => match status.entry(tool.name).or_insert_with(|| detect_tool(tool, home, path_var, probe)).clone() {
                ToolStatus::Available { path, .. } => {
                    for spec in &finding.commands {
                        bind(&mut out, spec, path.clone());
                    }
                }
                other => {
                    if !out.notices.iter().any(|n| n.tool == tool.name) {
                        let problem = if let ToolStatus::Broken(reason) = other { Some(reason) } else { None };
                        out.notices.push(Notice { tool: tool.name, evidence: finding.evidence.clone(), problem, hint: tool.hint() });
                    }
                }
            },
        }
    }
    out
}

// ───────────────────────────── 背景搜尋與快取 ─────────────────────────────

const STALE_AFTER: Duration = Duration::from_secs(30);
const STALE_AFTER_EMPTY: Duration = Duration::from_secs(5);

enum Slot {
    Pending,
    Ready(Arc<ProjectResolved>, Instant, bool),
}

/// 背景搜尋與快取：依（語言, 檔案路徑）記住結果（命令和檔案的位置有關，例如 `examples/` 底下的檔案）；
/// 第一次問的時候回傳 `None` 並開背景執行緒去找，找完叫畫面重畫；結果太舊了先給舊的、同時在背景重找。
#[derive(Clone, Default)]
pub struct Resolver {
    slots: Arc<Mutex<HashMap<(String, PathBuf), Slot>>>,
}

impl Resolver {
    pub fn get(&self, ctx: &egui::Context, lang: &str, file: &Path) -> Option<Arc<ProjectResolved>> {
        if !supports(lang) {
            return None;
        }
        let key = (lang.to_owned(), file.to_path_buf());
        let mut slots = self.slots.lock().ok()?;
        let (current, spawn) = match slots.get(&key) {
            None => (None, true),
            Some(Slot::Pending) => (None, false),
            Some(Slot::Ready(r, at, refreshing)) => {
                let limit = if r.is_empty() { STALE_AFTER_EMPTY } else { STALE_AFTER };
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
            let (slots, ctx) = (self.slots.clone(), ctx.clone());
            std::thread::spawn(move || {
                let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).filter(|h| !h.is_empty()).map(PathBuf::from);
                let path = std::env::var_os("PATH");
                let probe = |p: &Path| probe_tool(p, Duration::from_secs(4));
                let resolved = resolve(&key.0, &key.1, home.as_deref(), path.as_deref(), &probe);
                if let Ok(mut slots) = slots.lock() {
                    slots.insert(key, Slot::Ready(Arc::new(resolved), Instant::now(), false));
                }
                ctx.request_repaint();
            });
        }
        current
    }

    /// 全部標成過期（使用者按了「重新搜尋」，或剛裝好工具）：下次問的時候在背景重找，找完之前先顯示舊的。
    pub fn refresh(&self) {
        let Ok(mut slots) = self.slots.lock() else { return };
        for slot in slots.values_mut() {
            if let Slot::Ready(_, at, _) = slot {
                *at = at.checked_sub(STALE_AFTER + Duration::from_secs(1)).unwrap_or(*at);
            }
        }
    }
}

// ───────────────────────────── 現在要用哪一個 ─────────────────────────────

/// 設定裡記住「手動選了專案命令」的鍵（值是命令的 `id`）。直譯器的選擇鍵不變（見 `runtime_env::choice_key`），
/// 所以舊的設定檔照樣讀得進來。
pub fn command_choice_key(lang: &str, project_root: &str) -> String {
    format!("{project_root}#cmd:{lang}")
}

/// 按下「執行」時實際會用的東西。
pub enum Active<'a> {
    Command(&'a ProjectCommand),
    Interpreter(&'a crate::runtime_env::Item),
    None,
}

/// 決定現在用什麼：手動選的專案命令（還在、而且能執行）優先；
/// 否則是直譯器（手動選的或自動找到的第一個能執行的，**預設仍是直譯器**）；
/// 都沒有（例如 Rust 沒有直譯器，或 Python 的直譯器都壞了）才用第一個專案命令。
pub fn active<'a>(
    manual_command: Option<&str>,
    manual_interpreter: Option<&str>,
    interpreters: Option<&'a crate::runtime_env::Resolved>,
    project: Option<&'a ProjectResolved>,
) -> Active<'a> {
    if let Some(command) = manual_command.and_then(|id| project.and_then(|p| p.find(id))) {
        return Active::Command(command);
    }
    if let Some(item) = interpreters.and_then(|r| r.chosen(manual_interpreter)) {
        return Active::Interpreter(item);
    }
    match project.and_then(|p| p.commands.first()) {
        Some(command) => Active::Command(command),
        None => Active::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(root: &Path, rel: &str, text: &str) -> PathBuf {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, text).unwrap();
        path
    }

    /// 在 `bin` 資料夾放一個假的工具檔案，回傳那個 `PATH`。
    fn path_with(bin: &Path, tools: &[&str]) -> std::ffi::OsString {
        std::fs::create_dir_all(bin).unwrap();
        for tool in tools {
            std::fs::write(bin.join(format!("{tool}{}", if cfg!(windows) { ".exe" } else { "" })), "").unwrap();
        }
        std::env::join_paths([bin]).unwrap()
    }

    fn ok_probe(_: &Path) -> Result<String, String> {
        Ok("1.0.0".to_owned())
    }

    fn labels(r: &ProjectResolved) -> Vec<&str> {
        r.commands.iter().map(|c| c.label.as_str()).collect()
    }

    const PKG: &str = "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n";

    #[test]
    fn cargo_lists_run_test_build_and_puts_the_commands_of_the_current_file_first() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "Cargo.toml", PKG);
        write(dir.path(), "src/main.rs", "fn main() {}");
        let example = write(dir.path(), "examples/hello.rs", "fn main() {}");
        let path = path_with(&dir.path().join("bin"), &["cargo"]);
        let r = resolve("Rust", &example, None, Some(&path), &ok_probe);
        assert_eq!(labels(&r), ["cargo run --example hello", "cargo run", "cargo test", "cargo build"]);
        assert!(r.notices.is_empty());
        assert_eq!(r.commands[0].cwd, dir.path());
        assert_eq!(r.sources, ["Cargo.toml"]);
        assert!(r.commands.iter().all(|c| !c.takes_file), "cargo 不接目前的檔案");
    }

    #[test]
    fn cargo_with_several_binaries_lists_each_with_bin_and_a_lib_has_no_run() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "Cargo.toml", PKG);
        write(dir.path(), "src/main.rs", "fn main() {}");
        let tool = write(dir.path(), "src/bin/tool.rs", "fn main() {}");
        let f = cargo_findings(&tool, None);
        let ids: Vec<&str> = f[0].commands.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, ["cargo:bin:tool", "cargo:bin:demo", "cargo:test", "cargo:build"], "有兩個執行檔又沒有 default-run：沒有單純的 cargo run");

        let lib = tempfile::tempdir().unwrap();
        write(lib.path(), "Cargo.toml", PKG);
        let src = write(lib.path(), "src/lib.rs", "");
        let ids: Vec<String> = cargo_findings(&src, None)[0].commands.iter().map(|c| c.id.clone()).collect();
        assert_eq!(ids, ["cargo:test", "cargo:build"], "只有函式庫：不能 run");
    }

    #[test]
    fn cargo_integration_tests_and_the_nearest_manifest_win() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "Cargo.toml", "[workspace]\nmembers = [\"member\"]\n");
        write(dir.path(), "member/Cargo.toml", "[package]\nname = \"member\"\nversion = \"0.1.0\"\n");
        write(dir.path(), "member/src/main.rs", "fn main() {}");
        let test = write(dir.path(), "member/tests/api.rs", "");
        let f = cargo_findings(&test, None);
        assert_eq!(f[0].cwd, dir.path().join("member"), "用最近的 Cargo.toml（成員自己的）");
        assert_eq!(f[0].commands[0].label, "cargo test --test api");
        // 虛擬 workspace 根目錄（沒有 [package]）只有 test、build
        let root_file = write(dir.path(), "xtask.rs", "");
        let ids: Vec<String> = cargo_findings(&root_file, None)[0].commands.iter().map(|c| c.id.clone()).collect();
        assert_eq!(ids, ["cargo:test", "cargo:build"]);
    }

    #[test]
    fn the_search_stops_at_a_git_folder_and_ignores_unreadable_manifests() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "Cargo.toml", PKG);
        write(dir.path(), "inner/.git/HEAD", "");
        let file = write(dir.path(), "inner/a.rs", "");
        assert!(cargo_findings(&file, None).is_empty(), "不會用 .git 外面別的專案的設定");
        let bad = tempfile::tempdir().unwrap();
        write(bad.path(), "Cargo.toml", "this is [not toml");
        let file = write(bad.path(), "a.rs", "");
        assert!(cargo_findings(&file, None).is_empty(), "讀不懂的設定檔直接略過");
    }

    #[test]
    fn a_missing_tool_produces_one_notice_and_no_clickable_command() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "Cargo.toml", PKG);
        let file = write(dir.path(), "src/main.rs", "fn main() {}");
        let empty_path = path_with(&dir.path().join("empty"), &[]);
        let r = resolve("Rust", &file, None, Some(&empty_path), &ok_probe);
        assert!(r.commands.is_empty(), "沒有 cargo：不放可以按的項目");
        assert_eq!(r.notices.len(), 1);
        assert_eq!(r.notices[0].tool, "cargo");
        assert_eq!(r.notices[0].evidence, "Cargo.toml");
        assert_eq!(r.notices[0].problem, None);
        assert!(!r.notices[0].hint.is_empty());
    }

    #[test]
    fn a_broken_tool_is_reported_with_its_reason_and_not_offered() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "Cargo.toml", PKG);
        let file = write(dir.path(), "src/main.rs", "fn main() {}");
        let path = path_with(&dir.path().join("bin"), &["cargo"]);
        let broken = |_: &Path| Err("rustup could not choose a version of cargo".to_owned());
        let r = resolve("Rust", &file, None, Some(&path), &broken);
        assert!(r.commands.is_empty());
        assert_eq!(r.notices[0].problem.as_deref(), Some("rustup could not choose a version of cargo"));
    }

    #[test]
    fn tools_are_also_found_in_their_fixed_home_folders() {
        let home = tempfile::tempdir().unwrap();
        let bin = home.path().join(".cargo/bin");
        path_with(&bin, &["cargo"]);
        let spec = &CARGO;
        let found = find_tool(spec, Some(home.path()), Some(OsStr::new(""))).expect("PATH 沒有也要試 ~/.cargo/bin");
        assert!(found.starts_with(&bin));
        assert!(find_tool(spec, None, Some(OsStr::new(""))).is_none());
    }

    #[test]
    fn uv_projects_offer_uv_commands_only_when_uv_is_installed() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "pyproject.toml", "[project]\nname = \"app\"\n[project.scripts]\nserve = \"app:main\"\n");
        write(dir.path(), "uv.lock", "");
        let file = write(dir.path(), "main.py", "print(1)");
        let none = path_with(&dir.path().join("none"), &[]);
        let r = resolve("Python", &file, None, Some(&none), &ok_probe);
        assert!(r.commands.is_empty(), "沒有 uv：不能按的項目一個都不放");
        assert_eq!(r.notices.len(), 1);
        assert_eq!((r.notices[0].tool, r.notices[0].evidence.as_str()), ("uv", "pyproject.toml, uv.lock"));

        let with_uv = path_with(&dir.path().join("with"), &["uv"]);
        let r = resolve("Python", &file, None, Some(&with_uv), &ok_probe);
        assert_eq!(labels(&r), ["uv run main.py", "uv run serve"]);
        assert!(r.commands[0].takes_file && !r.commands[1].takes_file);
        assert!(r.notices.is_empty());
    }

    #[test]
    fn venv_scripts_run_directly_even_without_uv() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "pyproject.toml", "[project]\nname = \"app\"\n[project.scripts]\nserve = \"app:main\"\n[tool.uv]\n");
        write(dir.path(), ".venv/pyvenv.cfg", "");
        let exe = if cfg!(windows) { ".venv/Scripts/serve.exe" } else { ".venv/bin/serve" };
        write(dir.path(), exe, "");
        let file = write(dir.path(), "main.py", "");
        let none = path_with(&dir.path().join("none"), &[]);
        let r = resolve("Python", &file, None, Some(&none), &ok_probe);
        assert_eq!(labels(&r), ["serve"], "有 .venv：腳本直接執行，不需要 uv");
        assert!(r.commands[0].program.ends_with(Path::new(exe).file_name().unwrap()));
        assert_eq!(r.notices.len(), 1, "uv 的專案設定還是提醒一下 uv 沒裝");
        // 有 uv 時，已經能直接執行的腳本不再重複列 `uv run serve`
        let with_uv = path_with(&dir.path().join("with"), &["uv"]);
        let r = resolve("Python", &file, None, Some(&with_uv), &ok_probe);
        assert_eq!(labels(&r), ["uv run main.py", "serve"]);
    }

    #[test]
    fn a_plain_pyproject_without_evidence_of_uv_or_poetry_shows_nothing() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "pyproject.toml", "[project]\nname = \"app\"\n[build-system]\nrequires = [\"setuptools\"]\n");
        let file = write(dir.path(), "main.py", "");
        let path = path_with(&dir.path().join("bin"), &["uv", "poetry"]);
        let r = resolve("Python", &file, None, Some(&path), &ok_probe);
        assert!(r.is_empty(), "光有 pyproject.toml 不代表要用 uv：什麼都不顯示");
    }

    #[test]
    fn poetry_projects_offer_poetry_run() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "pyproject.toml", "[tool.poetry]\nname = \"app\"\n[tool.poetry.scripts]\ncli = \"app:main\"\n");
        let file = write(dir.path(), "app.py", "");
        let path = path_with(&dir.path().join("bin"), &["poetry"]);
        let r = resolve("Python", &file, None, Some(&path), &ok_probe);
        assert_eq!(labels(&r), ["poetry run python app.py", "poetry run cli"]);
    }

    fn command(id: &str) -> ProjectCommand {
        ProjectCommand { id: id.to_owned(), label: id.to_owned(), program: PathBuf::from(id), args: vec![], cwd: PathBuf::new(), source: String::new(), takes_file: false, extra_sep: false }
    }

    fn interpreters(works: bool) -> crate::runtime_env::Resolved {
        let candidate = crate::runtime_env::Candidate {
            program: PathBuf::from("/py"),
            args: vec![],
            source: crate::runtime_env::Source::Global,
            origin: "PATH".to_owned(),
            env_dir: None,
        };
        crate::runtime_env::Resolved { items: vec![crate::runtime_env::Item { candidate, version: works.then(|| "3.12".to_owned()) }] }
    }

    #[test]
    fn the_interpreter_stays_the_default_and_a_chosen_command_wins_only_while_it_exists() {
        let project = ProjectResolved { commands: vec![command("cargo:run"), command("cargo:test")], ..Default::default() };
        let interp = interpreters(true);
        // 沒有手動選：預設仍是直譯器
        assert!(matches!(active(None, None, Some(&interp), Some(&project)), Active::Interpreter(_)));
        // 手動選了專案命令
        assert!(matches!(active(Some("cargo:test"), None, Some(&interp), Some(&project)), Active::Command(c) if c.id == "cargo:test"));
        // 選的命令已經不在了（設定檔改了、工具被移除）：退回直譯器
        assert!(matches!(active(Some("cargo:gone"), None, Some(&interp), Some(&project)), Active::Interpreter(_)));
        // 沒有直譯器（Rust）：用第一個專案命令
        assert!(matches!(active(None, None, None, Some(&project)), Active::Command(c) if c.id == "cargo:run"));
        // 直譯器都壞了：也用第一個專案命令
        let broken = interpreters(false);
        assert!(matches!(active(None, None, Some(&broken), Some(&project)), Active::Command(_)));
        // 什麼都沒有
        assert!(matches!(active(None, None, Some(&broken), Some(&ProjectResolved::default())), Active::None));
        assert!(matches!(active(None, None, None, None), Active::None));
    }

    #[test]
    fn other_languages_have_no_provider_and_the_resolver_ignores_them() {
        assert!(supports("Rust") && supports("Python") && !supports("Go"));
        let dir = tempfile::tempdir().unwrap();
        let file = write(dir.path(), "a.go", "");
        assert!(resolve("Go", &file, None, None, &ok_probe).is_empty());
        assert!(Resolver::default().get(&egui::Context::default(), "Go", &file).is_none());
    }

    #[test]
    fn resolver_computes_in_the_background_and_returns_the_cached_result() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "Cargo.toml", PKG);
        let file = write(dir.path(), "src/main.rs", "fn main() {}");
        let ctx = egui::Context::default();
        let resolver = Resolver::default();
        assert!(resolver.get(&ctx, "Rust", &file).is_none(), "第一次：背景開始找");
        let start = Instant::now();
        let found = loop {
            if let Some(r) = resolver.get(&ctx, "Rust", &file) {
                break r;
            }
            assert!(start.elapsed() < Duration::from_secs(20), "背景搜尋太久");
            std::thread::sleep(Duration::from_millis(20));
        };
        // 不管這台機器有沒有裝 cargo，都不會同時有命令和說明，而且一定有東西可以告訴使用者
        assert!(found.commands.is_empty() != found.notices.is_empty(), "{found:?}");
        resolver.refresh();
    }
}
