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

/// `--send` 要送進終端機的一項東西，依命令列上的順序送。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SendItem {
    /// 文字，當成使用者打的字送進去（換行是 Enter，不會自動在最後補 Enter）；`--paste` 時用括號貼上送。
    Text(String),
    /// 按鍵，例如 `Enter`、`Ctrl+C`、`Up`（寫法見 [`crate::terminal::parse_key`]）。
    Key(String),
    /// 標準輸入的內容（最後一個換行去掉）。只在命令列解析之後、送出之前用 [`SendArgs::resolve_inputs`] 換成 `Text`。
    Stdin,
    /// 檔案的內容（最後一個換行去掉）。同上，送出之前換成 `Text`。
    File(String),
}

/// `feditor --send`：把文字或按鍵送進執行中的 FEditor 裡的某個終端機。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SendArgs {
    /// 終端機的名稱；沒指定就用目前的終端機。
    pub terminal: Option<String>,
    pub items: Vec<SendItem>,
    /// `--paste`：文字用「括號貼上」送（程式有開括號貼上模式時，換行不會被當成 Enter）。
    pub paste: bool,
}

impl SendArgs {
    /// 把 `Stdin`、`File` 項目換成讀到的內容（最後一個換行去掉）；讀不到、或內容是空的就回傳錯誤，
    /// 不會悄悄送出空字串。沒有這類項目就不會去讀。`stdin` 只會讀一次，`file` 依檔名各讀一次。
    pub fn resolve_inputs(
        &mut self,
        stdin: impl FnOnce() -> Result<String, String>,
        file: impl Fn(&str) -> Result<String, String>,
    ) -> Result<(), String> {
        let trim = |text: String| text.strip_suffix("\r\n").or_else(|| text.strip_suffix('\n')).unwrap_or(&text).to_owned();
        let mut stdin = Some(stdin);
        let mut stdin_text: Option<String> = None;
        for item in &mut self.items {
            let (text, what) = match item {
                SendItem::Stdin => {
                    if stdin_text.is_none() {
                        stdin_text = Some(trim((stdin.take().ok_or("標準輸入只能讀一次")?)()?));
                    }
                    (stdin_text.clone().unwrap_or_default(), "標準輸入".to_owned())
                }
                SendItem::File(path) => (trim(file(path)?), format!("檔案 {path}")),
                _ => continue,
            };
            if text.is_empty() {
                return Err(format!("{what}是空的，沒有東西可以送"));
            }
            *item = SendItem::Text(text);
        }
        Ok(())
    }
}

/// 解析後的命令列。
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Args {
    pub send: Option<SendArgs>,
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

把文字或按鍵送進終端機（--send 要放在第一個）：
  feditor --send [-t 名稱] [-e] [-k 按鍵]… [--paste] [--stdin] [文字]…
      送進執行中的 FEditor 的終端機（不會把視窗叫到前面）。

      -t, --terminal 名稱   終端機的名稱（分頁上最前面那段：預設的「終端機 2」或自己取的名字；
                            預設名稱也可以只寫編號 2）。沒指定時用目前的終端機，
                            只有一個終端機時就是它
      --text 文字           明確指定一段文字（文字以 - 開頭時用）；直接寫的文字也一樣
      -k, --key 按鍵        送一個按鍵，例如 Enter、Esc、Tab、Up、Ctrl+C、Alt+B、F5
      -e, --enter           送 Enter，等於 -k Enter
      --paste               文字用「括號貼上」送：多行文字的換行不會被當成 Enter
                            （Claude Code 這類互動程式適用；要送出再加 -e）
      -f, --file 檔案       把檔案的內容當成文字送（最後一個換行去掉），例如 feditor --send -f 提示.txt --paste -e
      --stdin               從標準輸入讀文字（最後一個換行去掉），
                            例如 Get-Content 檔案 -Raw | feditor --send --stdin --paste -e
      文字、按鍵依命令列上的順序送，文字不會自動補 Enter。例如：
        feditor --send -t 伺服器 \"cargo run\" -e
        feditor --send -t 2 -k Ctrl+C
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
    let mut args = args.into_iter().peekable();
    // 第一個參數是 --send：把文字、按鍵送進終端機，後面的參數都是 --send 的
    if args.peek().is_some_and(|a| a == "--send") {
        args.next();
        out.send = Some(parse_send(args)?);
        return Ok(out);
    }
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

/// 解析 `--send` 後面的參數。
fn parse_send(args: impl Iterator<Item = std::ffi::OsString>) -> Result<SendArgs, String> {
    let mut out = SendArgs::default();
    let mut args = args;
    let mut literal = false; // `--` 之後都是文字
    while let Some(arg) = args.next() {
        let text = arg.to_string_lossy().into_owned();
        if literal || !text.starts_with('-') || text == "-" {
            out.items.push(SendItem::Text(text));
            continue;
        }
        let mut value = |name: &str| args.next().map(|v| v.to_string_lossy().into_owned()).ok_or_else(|| format!("{name} 後面要接值"));
        match text.as_str() {
            "--" => literal = true,
            "-t" | "--terminal" => out.terminal = Some(value(&text)?),
            "--text" => out.items.push(SendItem::Text(value(&text)?)),
            "-k" | "--key" => {
                let key = value(&text)?;
                crate::terminal::parse_key(&key)?;
                out.items.push(SendItem::Key(key));
            }
            "-e" | "--enter" => out.items.push(SendItem::Key("Enter".to_owned())),
            "--paste" => out.paste = true,
            "--stdin" => out.items.push(SendItem::Stdin),
            "-f" | "--file" => out.items.push(SendItem::File(value(&text)?)),
            t => return Err(format!("--send 不認得的選項：{t}（文字以 - 開頭時請用 --text）")),
        }
    }
    if out.items.is_empty() {
        return Err("--send 要至少給一段文字、一個按鍵或 --stdin".to_owned());
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
    fn send_parses_terminal_text_and_keys_in_order() {
        let a = run(&["--send", "-t", "伺服器", "cargo run", "-e", "--text", "-x", "-k", "Ctrl+C"], &[]).unwrap();
        let send = a.send.unwrap();
        assert_eq!(send.terminal.as_deref(), Some("伺服器"));
        assert_eq!(
            send.items,
            vec![
                SendItem::Text("cargo run".into()),
                SendItem::Key("Enter".into()),
                SendItem::Text("-x".into()),
                SendItem::Key("Ctrl+C".into()),
            ]
        );
        assert!(a.targets.is_empty());
        // `--` 之後都是文字
        let send = run(&["--send", "--", "-k"], &[]).unwrap().send.unwrap();
        assert_eq!(send.items, vec![SendItem::Text("-k".into())]);
    }

    #[test]
    fn send_rejects_bad_input_and_only_the_first_argument_makes_it_a_send() {
        assert!(run(&["--send"], &[]).is_err(), "沒有內容");
        assert!(run(&["--send", "-k", "Nonsense"], &[]).is_err(), "不認得的按鍵");
        assert!(run(&["--send", "-t"], &[]).is_err(), "缺值");
        assert!(run(&["--send", "--bogus", "x"], &[]).is_err());
        // 單獨的 send 現在只是檔名
        let a = run(&["send"], &[]).unwrap();
        assert!(a.send.is_none() && a.targets == vec![Target::plain("send")]);
        // --send 不是第一個參數：不認得的選項
        assert!(run(&["a.txt", "--send"], &[]).is_err());
    }

    #[test]
    fn paste_stdin_and_file_options_are_resolved_before_sending() {
        let send = run(&["--send", "--paste", "--stdin", "-e", "--file", "提示.txt"], &[]).unwrap().send.unwrap();
        assert!(send.paste);
        assert_eq!(send.items, vec![SendItem::Stdin, SendItem::Key("Enter".into()), SendItem::File("提示.txt".into())]);
        // 只有 --stdin 或 --file 也算有內容
        assert!(run(&["--send", "--stdin"], &[]).is_ok() && run(&["--send", "-f", "x"], &[]).is_ok());
        assert!(run(&["--send", "--file"], &[]).is_err(), "缺檔名");
        let no_file = |_: &str| -> Result<String, String> { panic!("不該讀檔") };
        let mut send = send;
        send.resolve_inputs(|| Ok("a\nb\n".to_owned()), |p| Ok(format!("內容來自{p}\r\n"))).unwrap();
        assert_eq!(send.items[0], SendItem::Text("a\nb".into()), "最後一個換行去掉");
        assert_eq!(send.items[2], SendItem::Text("內容來自提示.txt".into()), "檔案內容也去掉最後一個換行");
        let mut crlf = SendArgs { items: vec![SendItem::Stdin], ..Default::default() };
        crlf.resolve_inputs(|| Ok("x\r\n\r\n".to_owned()), no_file).unwrap();
        assert_eq!(crlf.items[0], SendItem::Text("x\r\n".into()), "只去掉最後一個");
        let mut none = SendArgs { items: vec![SendItem::Text("t".into())], ..Default::default() };
        none.resolve_inputs(|| panic!("沒有 --stdin 不該去讀"), no_file).unwrap();
        // 讀不到、或內容是空的：回報錯誤，不會悄悄送出空字串（PowerShell 的管線不會傳給視窗程式，讀到的就是空的）
        let mut bad = SendArgs { items: vec![SendItem::Stdin], ..Default::default() };
        assert!(bad.resolve_inputs(|| Err("讀不到".into()), no_file).is_err());
        let mut empty = SendArgs { items: vec![SendItem::Stdin], ..Default::default() };
        assert!(empty.resolve_inputs(|| Ok(String::new()), no_file).unwrap_err().contains("標準輸入是空的"));
        let mut empty_file = SendArgs { items: vec![SendItem::File("a".into())], ..Default::default() };
        assert!(empty_file.resolve_inputs(|| Ok("x".into()), |_| Ok("\n".into())).unwrap_err().contains("檔案 a是空的"));
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
