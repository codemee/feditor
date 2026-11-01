//! 跨檔搜尋：在資料夾底下的文字檔裡找字串或規則表達式。
//!
//! - 讀檔時自動偵測編碼（Big5、UTF-16 都能搜），有 NUL 位元組的二進位檔會跳過；
//! - 預設不進 `.git`、`target`、`node_modules` 等資料夾，也不讀太大的檔案；
//! - 一行最多回報一筆（第一個符合的位置），整體有筆數上限；
//! - 可以隨時取消（檢查一個旗標），所以放在背景執行緒裡跑也能馬上停下來。

use crate::fileio;
use crate::search::{SearchError, SearchOptions, Searcher};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

/// 一筆符合的結果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hit {
    pub path: PathBuf,
    /// 第幾行（0 起算）。
    pub line: usize,
    /// 符合處在那一行的字元範圍（相對於整行，不是相對於 `text`）。
    pub range: Range<usize>,
    /// 那一行的文字（太長會截短，只留符合處附近）。
    pub text: String,
    /// `text` 在整行裡從第幾個字元開始。
    pub text_offset: usize,
}

#[derive(Clone, Debug)]
pub struct GrepOptions {
    pub search: SearchOptions,
    /// 檔名過濾，例如 `*.rs;*.toml`（用 `;` 或 `,` 分隔）；空字串表示全部。
    pub file_filter: String,
    /// 結果上限，到了就停止。
    pub max_hits: usize,
    /// 超過這個大小的檔案不讀。
    pub max_file_bytes: u64,
    /// 要略過的資料夾名稱。
    pub skip_dirs: Vec<String>,
}

impl Default for GrepOptions {
    fn default() -> Self {
        Self {
            search: SearchOptions::default(),
            file_filter: String::new(),
            max_hits: 5000,
            max_file_bytes: 20 * 1024 * 1024,
            skip_dirs: [".git", ".hg", ".svn", "target", "node_modules"]
                .into_iter()
                .map(String::from)
                .collect(),
        }
    }
}

/// 搜尋結束時的統計。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Summary {
    pub files_searched: usize,
    pub files_with_hits: usize,
    pub hits: usize,
    /// 因為到達 `max_hits` 而提早結束。
    pub truncated: bool,
    /// 被使用者取消。
    pub cancelled: bool,
}

/// 檔名是否符合過濾字串（`*` 任意多個字元、`?` 一個字元，不分大小寫）。
pub fn matches_filter(name: &str, filter: &str) -> bool {
    let patterns: Vec<&str> = filter
        .split([';', ','])
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect();
    if patterns.is_empty() {
        return true;
    }
    let name: Vec<char> = name.to_lowercase().chars().collect();
    patterns.iter().any(|p| {
        let pat: Vec<char> = p.to_lowercase().chars().collect();
        glob(&pat, &name)
    })
}

fn glob(pat: &[char], name: &[char]) -> bool {
    match pat.split_first() {
        None => name.is_empty(),
        Some(('*', rest)) => (0..=name.len()).any(|i| glob(rest, &name[i..])),
        Some(('?', rest)) => !name.is_empty() && glob(rest, &name[1..]),
        Some((c, rest)) => name.first() == Some(c) && glob(rest, &name[1..]),
    }
}

/// 在 `root`（資料夾或單一檔案）底下搜尋 `pattern`，每找到一筆就呼叫 `on_hit`。
/// `cancel` 被設成 `true` 就盡快結束。規則表達式有誤時回傳錯誤。
pub fn search_dir(
    root: &Path,
    pattern: &str,
    opts: &GrepOptions,
    cancel: &AtomicBool,
    mut on_hit: impl FnMut(Hit),
) -> Result<Summary, SearchError> {
    let searcher = Searcher::new(pattern, opts.search)?;
    let mut summary = Summary::default();
    let mut stack = vec![root.to_owned()];
    'walk: while let Some(path) = stack.pop() {
        if cancel.load(Ordering::Relaxed) {
            summary.cancelled = true;
            break;
        }
        let Ok(meta) = std::fs::metadata(&path) else {
            continue;
        };
        if meta.is_dir() {
            let Ok(read) = std::fs::read_dir(&path) else {
                continue;
            };
            let mut children: Vec<PathBuf> = read.filter_map(Result::ok).map(|e| e.path()).collect();
            // 反向放進堆疊，取出時就是檔名由小到大
            children.sort_by(|a, b| b.file_name().cmp(&a.file_name()));
            for child in children {
                let name = child
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                if child.is_dir() && opts.skip_dirs.iter().any(|d| *d == name) {
                    continue;
                }
                if child.is_file() && !matches_filter(&name, &opts.file_filter) {
                    continue;
                }
                stack.push(child);
            }
            continue;
        }
        if !meta.is_file() || meta.len() > opts.max_file_bytes {
            continue;
        }
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        if looks_binary(&bytes) {
            continue;
        }
        summary.files_searched += 1;
        let text = fileio::decode(&bytes).text;
        let before = summary.hits;
        for (line_no, line) in text.split('\n').enumerate() {
            let line = line.strip_suffix('\r').unwrap_or(line);
            let Some(m) = searcher.find_next(line, 0) else {
                continue;
            };
            let start = line[..m.start].chars().count();
            let end = start + line[m.start..m.end].chars().count();
            let (text, text_offset) = excerpt(line, start);
            on_hit(Hit {
                path: path.clone(),
                line: line_no,
                range: start..end,
                text,
                text_offset,
            });
            summary.hits += 1;
            if summary.hits >= opts.max_hits {
                summary.truncated = true;
                summary.files_with_hits += 1;
                break 'walk;
            }
            if summary.hits % 256 == 0 && cancel.load(Ordering::Relaxed) {
                summary.cancelled = true;
                summary.files_with_hits += 1;
                break 'walk;
            }
        }
        if summary.hits > before {
            summary.files_with_hits += 1;
        }
    }
    Ok(summary)
}

/// 二進位檔：前 8000 個位元組裡有 NUL，而且不是帶 BOM 的 UTF-16。
fn looks_binary(bytes: &[u8]) -> bool {
    let head = &bytes[..bytes.len().min(8000)];
    if head.starts_with(&[0xFF, 0xFE]) || head.starts_with(&[0xFE, 0xFF]) {
        return false;
    }
    head.contains(&0)
}

/// 行太長時只留符合處附近：回傳（文字, 它在整行裡從第幾個字元開始）。
fn excerpt(line: &str, match_start: usize) -> (String, usize) {
    const BEFORE: usize = 60;
    const MAX: usize = 240;
    let total = line.chars().count();
    if total <= MAX {
        return (line.to_owned(), 0);
    }
    let from = match_start.saturating_sub(BEFORE);
    (line.chars().skip(from).take(MAX).collect(), from)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, name: &str, content: &[u8]) {
        let p = dir.join(name);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, content).unwrap();
    }

    fn run(dir: &Path, pattern: &str, opts: &GrepOptions) -> (Vec<Hit>, Summary) {
        let mut hits = Vec::new();
        let s = search_dir(dir, pattern, opts, &AtomicBool::new(false), |h| hits.push(h)).unwrap();
        (hits, s)
    }

    fn name(h: &Hit) -> String {
        h.path.file_name().unwrap().to_string_lossy().into_owned()
    }

    #[test]
    fn finds_lines_across_files_in_order() {
        let d = tempfile::tempdir().unwrap();
        write(d.path(), "b.txt", b"nothing\nfoo here\nfoo again");
        write(d.path(), "a.txt", b"x foo y");
        write(d.path(), "sub/c.txt", "中文 foo".as_bytes());
        let (hits, s) = run(d.path(), "foo", &GrepOptions::default());
        let got: Vec<(String, usize)> = hits.iter().map(|h| (name(h), h.line)).collect();
        assert_eq!(
            got,
            vec![("a.txt".into(), 0), ("b.txt".into(), 1), ("b.txt".into(), 2), ("c.txt".into(), 0)]
        );
        assert_eq!(s.hits, 4);
        assert_eq!(s.files_searched, 3);
        assert_eq!(s.files_with_hits, 3);
        assert!(!s.truncated && !s.cancelled);
        let cjk = hits.iter().find(|h| name(h) == "c.txt").unwrap();
        assert_eq!(cjk.range, 3..6, "位置是字元，不是位元組");
    }

    #[test]
    fn one_hit_per_line_and_crlf_is_handled() {
        let d = tempfile::tempdir().unwrap();
        write(d.path(), "a.txt", b"foo foo foo\r\nbar foo\r\n");
        let (hits, _) = run(d.path(), "foo", &GrepOptions::default());
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].range, 0..3);
        assert_eq!(hits[1].text, "bar foo", "行尾的 \\r 不會留在文字裡");
    }

    #[test]
    fn options_regex_case_and_whole_word() {
        let d = tempfile::tempdir().unwrap();
        write(d.path(), "a.txt", b"Foo\nfoobar\nfoo\n");
        let plain = GrepOptions::default();
        assert_eq!(run(d.path(), "foo", &plain).0.len(), 3, "預設不分大小寫");
        let cs = GrepOptions {
            search: SearchOptions { case_sensitive: true, ..plain.search },
            ..plain.clone()
        };
        assert_eq!(run(d.path(), "foo", &cs).0.len(), 2);
        let ww = GrepOptions {
            search: SearchOptions { whole_word: true, ..plain.search },
            ..plain.clone()
        };
        assert_eq!(run(d.path(), "foo", &ww).0.len(), 2, "foobar 不算");
        let re = GrepOptions {
            search: SearchOptions { regex: true, ..plain.search },
            ..plain
        };
        assert_eq!(run(d.path(), "^foo$", &re).0.len(), 2);
        let err = search_dir(d.path(), "(", &re, &AtomicBool::new(false), |_| {});
        assert!(err.is_err(), "規則有誤要回報");
    }

    #[test]
    fn skips_binary_big_and_ignored_directories() {
        let d = tempfile::tempdir().unwrap();
        write(d.path(), "bin.dat", b"foo\0\0\0foo");
        write(d.path(), "target/x.txt", b"foo");
        write(d.path(), ".git/y.txt", b"foo");
        write(d.path(), "node_modules/z.txt", b"foo");
        write(d.path(), "keep.txt", b"foo");
        write(d.path(), "big.txt", &vec![b'a'; 2000]);
        let opts = GrepOptions { max_file_bytes: 1000, ..GrepOptions::default() };
        let (hits, s) = run(d.path(), "foo", &opts);
        assert_eq!(hits.iter().map(name).collect::<Vec<_>>(), vec!["keep.txt"]);
        assert_eq!(s.files_searched, 1);
    }

    #[test]
    fn searches_other_encodings() {
        let d = tempfile::tempdir().unwrap();
        let big5 = fileio::encode(
            "你好，世界",
            &fileio::FileFormat {
                encoding: encoding_rs::BIG5,
                bom: false,
                line_ending: fileio::LineEnding::Lf,
            },
        )
        .unwrap();
        write(d.path(), "big5.txt", &big5);
        let mut utf16 = vec![0xFF, 0xFE];
        for u in "世界你好".encode_utf16() {
            utf16.extend(u.to_le_bytes());
        }
        write(d.path(), "u16.txt", &utf16);
        let (hits, _) = run(d.path(), "世界", &GrepOptions::default());
        let names: Vec<String> = hits.iter().map(name).collect();
        assert_eq!(names, vec!["big5.txt", "u16.txt"]);
    }

    #[test]
    fn file_filter_matches_globs() {
        assert!(matches_filter("main.rs", ""));
        assert!(matches_filter("main.rs", "*.rs"));
        assert!(matches_filter("MAIN.RS", "*.rs"));
        assert!(matches_filter("a.toml", "*.rs; *.toml"));
        assert!(matches_filter("a.toml", "*.rs,*.toml"));
        assert!(!matches_filter("a.txt", "*.rs;*.toml"));
        assert!(matches_filter("test1.c", "test?.c"));
        assert!(!matches_filter("test12.c", "test?.c"));
        let d = tempfile::tempdir().unwrap();
        write(d.path(), "a.rs", b"foo");
        write(d.path(), "a.txt", b"foo");
        let opts = GrepOptions { file_filter: "*.rs".into(), ..GrepOptions::default() };
        assert_eq!(run(d.path(), "foo", &opts).0.len(), 1);
    }

    #[test]
    fn stops_at_the_hit_limit_and_when_cancelled() {
        let d = tempfile::tempdir().unwrap();
        write(d.path(), "a.txt", "foo\n".repeat(50).as_bytes());
        let opts = GrepOptions { max_hits: 10, ..GrepOptions::default() };
        let (hits, s) = run(d.path(), "foo", &opts);
        assert_eq!(hits.len(), 10);
        assert!(s.truncated);
        let cancel = AtomicBool::new(true);
        let s = search_dir(d.path(), "foo", &GrepOptions::default(), &cancel, |_| panic!("不該有結果")).unwrap();
        assert!(s.cancelled);
    }

    #[test]
    fn long_lines_are_excerpted_around_the_match() {
        let d = tempfile::tempdir().unwrap();
        let line = format!("{}needle{}", "a".repeat(500), "b".repeat(500));
        write(d.path(), "a.txt", line.as_bytes());
        let (hits, _) = run(d.path(), "needle", &GrepOptions::default());
        let h = &hits[0];
        assert_eq!(h.range, 500..506);
        assert_eq!(h.text_offset, 440);
        assert!(h.text.chars().count() <= 240);
        let rel = h.range.start - h.text_offset;
        assert_eq!(&h.text.chars().skip(rel).take(6).collect::<String>(), "needle");
    }

    #[test]
    fn a_single_file_can_be_the_root() {
        let d = tempfile::tempdir().unwrap();
        write(d.path(), "a.txt", b"foo\nbar");
        write(d.path(), "b.txt", b"foo");
        let (hits, _) = run(&d.path().join("a.txt"), "foo", &GrepOptions::default());
        assert_eq!(hits.len(), 1);
    }
}
