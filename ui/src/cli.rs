//! 命令列參數：`feditor [選項] [檔案[:行[:欄]]]…`。
//!
//! 只負責解析（不碰畫面、不連線），主程式與 `single_instance` 用解析的結果去做事。

use std::path::{Path, PathBuf};

/// 要開的一個檔案，以及要跳到的位置（行、欄都從 1 起算）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    pub path: PathBuf,
    pub line: Option<usize>,
    pub col: Option<usize>,
}

impl Target {
    pub fn plain(path: impl Into<PathBuf>) -> Self {
        Target { path: path.into(), line: None, col: None }
    }
}

/// 解析後的命令列。
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Args {
    pub help: bool,
    pub version: bool,
    /// `-n`：不交給執行中的視窗，另外開一個新視窗。
    pub new_window: bool,
    /// `-w`：等這些檔案都關閉了才結束（給 `git commit` 這類呼叫編輯器的程式用）。
    pub wait: bool,
    pub targets: Vec<Target>,
}

pub const USAGE: &str = "\
用法：feditor [選項] [檔案[:行[:欄]]]…

選項：
  -n, --new-window   開新視窗，不交給已經開著的視窗
  -w, --wait         等檔案都關閉了才結束（例如 GIT_EDITOR=\"feditor --wait\"）
  -h, --help         顯示這段說明
  -v, --version      顯示版本
  --                 之後的都當成檔名

檔名後面可以接 :行 或 :行:欄，開啟後跳到那個位置，例如 feditor src/main.rs:120:5
";

/// 解析 `args`（不含程式名稱）。不認得的選項回傳錯誤訊息。
pub fn parse(args: impl IntoIterator<Item = std::ffi::OsString>) -> Result<Args, String> {
    parse_with(args, |p| p.exists())
}

fn parse_with(
    args: impl IntoIterator<Item = std::ffi::OsString>,
    exists: impl Fn(&Path) -> bool,
) -> Result<Args, String> {
    let mut out = Args::default();
    let mut only_files = false;
    for arg in args {
        let text = arg.to_string_lossy();
        if !only_files && text.starts_with('-') && text.len() > 1 {
            match text.as_ref() {
                "--" => only_files = true,
                "-n" | "--new-window" => out.new_window = true,
                "-w" | "--wait" => out.wait = true,
                "-h" | "--help" => out.help = true,
                "-v" | "--version" => out.version = true,
                // macOS 從 Finder 啟動舊式程式時會帶這個，不是檔名
                t if t.starts_with("-psn_") => {}
                t => return Err(format!("不認得的選項：{t}")),
            }
            continue;
        }
        out.targets.push(split_position(&PathBuf::from(&arg), &exists));
    }
    Ok(out)
}

/// 單一個參數／路徑字串拆成檔案與位置（用實際的檔案系統判斷檔案存不存在）。
/// macOS 會把命令列裡的路徑再用 Apple Event 送一次給程式，那條路也要走這裡。
pub fn parse_target(path: &Path) -> Target {
    split_position(path, &|p: &Path| p.exists())
}

/// 把 `檔案:行:欄` 拆開。整段本身就是存在的檔案（檔名真的有冒號）時不拆；
/// 拆掉之後的檔案不存在也照拆（FEditor 不允許檔名有冒號，當成要建立的新檔案）。
fn split_position(path: &Path, exists: &impl Fn(&Path) -> bool) -> Target {
    let whole = Target::plain(path);
    let Some(text) = path.to_str() else { return whole };
    if exists(path) {
        return whole;
    }
    let mut rest = text;
    let mut numbers: Vec<usize> = Vec::new();
    while numbers.len() < 2 {
        let Some((head, tail)) = rest.rsplit_once(':') else { break };
        match tail.parse::<usize>() {
            Ok(n) if !head.is_empty() => {
                numbers.push(n);
                rest = head;
            }
            _ => break,
        }
    }
    // 單一字母開頭的（Windows 的磁碟機代號，例如 `C:5`）不是檔名加位置
    if numbers.is_empty() || rest.chars().count() < 2 {
        return whole;
    }
    numbers.reverse();
    Target {
        path: PathBuf::from(rest),
        line: numbers.first().copied().filter(|&n| n > 0),
        col: numbers.get(1).copied().filter(|&n| n > 0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(args: &[&str], existing: &[&str]) -> Result<Args, String> {
        parse_with(args.iter().map(std::ffi::OsString::from), |p| existing.iter().any(|e| Path::new(e) == p))
    }

    #[test]
    fn plain_paths_and_flags() {
        let a = run(&["-n", "--wait", "a.txt", "b.txt"], &[]).unwrap();
        assert!(a.new_window && a.wait && !a.help && !a.version);
        assert_eq!(a.targets, vec![Target::plain("a.txt"), Target::plain("b.txt")]);
        assert!(run(&["-h"], &[]).unwrap().help);
        assert!(run(&["--version"], &[]).unwrap().version);
    }

    #[test]
    fn unknown_options_are_errors_but_double_dash_allows_dashed_names() {
        assert!(run(&["--nope"], &[]).is_err());
        assert_eq!(run(&["--", "-n"], &[]).unwrap().targets, vec![Target::plain("-n")]);
        assert_eq!(run(&["-"], &[]).unwrap().targets, vec![Target::plain("-")]);
        assert!(run(&["-psn_0_12345", "a"], &[]).unwrap().targets.len() == 1);
    }

    #[test]
    fn line_and_column_suffixes_are_split_unless_the_whole_name_exists() {
        let ex = ["src/main.rs"];
        let t = &run(&["src/main.rs:120:5"], &ex).unwrap().targets[0];
        assert_eq!((t.path.as_path(), t.line, t.col), (Path::new("src/main.rs"), Some(120), Some(5)));
        let t = &run(&["src/main.rs:7"], &ex).unwrap().targets[0];
        assert_eq!((t.line, t.col), (Some(7), None));
        // 拆掉之後的檔案不存在：照拆，當成要建立的新檔案
        let t = &run(&["new.txt:3"], &ex).unwrap().targets[0];
        assert_eq!((t.path.as_path(), t.line), (Path::new("new.txt"), Some(3)));
        // 磁碟機代號不是檔名
        assert_eq!(run(&["C:5"], &ex).unwrap().targets[0], Target::plain("C:5"));
        // 檔名本身就有冒號而且存在：不拆
        assert_eq!(run(&["a:1"], &["a:1", "a"]).unwrap().targets[0], Target::plain("a:1"));
        // 行號 0 當作沒指定；非數字不拆
        assert_eq!(run(&["src/main.rs:0"], &ex).unwrap().targets[0].line, None);
        assert_eq!(run(&["src/main.rs:x"], &ex).unwrap().targets[0], Target::plain("src/main.rs:x"));
    }
}
