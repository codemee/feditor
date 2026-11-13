//! 跨檔搜尋：在資料夾底下的文字檔裡找字串或規則表達式。
//!
//! - 讀檔時自動偵測編碼（Big5、UTF-16 都能搜），有 NUL 位元組的二進位檔會跳過；
//! - 預設不進 `.git`、`target`、`node_modules` 等資料夾，也不讀太大的檔案；
//! - 一行最多回報一筆（第一個符合的位置），整體有筆數上限；
//! - 可以隨時取消（檢查一個旗標），所以放在背景執行緒裡跑也能馬上停下來。

use crate::fileio;
use crate::i18n::t;
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
    /// 是否一併搜尋子資料夾；`false` 時只搜尋 `root` 這一層的檔案。
    pub recursive: bool,
}

impl Default for GrepOptions {
    fn default() -> Self {
        Self {
            search: SearchOptions::default(),
            file_filter: String::new(),
            recursive: true,
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
                if child.is_dir() && (!opts.recursive || opts.skip_dirs.iter().any(|d| *d == name)) {
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
        if !scan_text(&path, &text, &searcher, opts.max_hits, None, cancel, &mut summary, &mut on_hit) {
            break 'walk;
        }
    }
    Ok(summary)
}

/// 在已經在記憶體裡的文字（例如還沒存檔的文件）裡搜尋，結果的格式和 [`search_dir`] 一樣。
/// `path` 只是填進結果裡的路徑（沒有檔案時可以給空路徑）。
/// `within` 是位元組範圍：有的話只找落在這段範圍內的符合（行號與欄位仍然是對整份文字算）。
pub fn search_text(
    path: &Path,
    text: &str,
    pattern: &str,
    search: SearchOptions,
    max_hits: usize,
    within: Option<Range<usize>>,
    cancel: &AtomicBool,
    mut on_hit: impl FnMut(Hit),
) -> Result<Summary, SearchError> {
    let searcher = Searcher::new(pattern, search)?;
    let mut summary = Summary { files_searched: 1, ..Summary::default() };
    scan_text(path, text, &searcher, max_hits, within, cancel, &mut summary, &mut on_hit);
    Ok(summary)
}

/// 掃描一份文字，每一個符合回報一筆（記在它開始的那一行）。回傳 `false` 表示該整個停下來（到達上限或被取消）。
fn scan_text(
    path: &Path,
    text: &str,
    searcher: &Searcher,
    max_hits: usize,
    within: Option<Range<usize>>,
    cancel: &AtomicBool,
    summary: &mut Summary,
    on_hit: &mut impl FnMut(Hit),
) -> bool {
    let before = summary.hits;
    // 有搜尋範圍：只看範圍結尾以前的文字（符合不會跨出去），從範圍開頭開始找；
    // 行號與欄位還是對整份文字算
    let (lo, text_end) = match within {
        Some(r) => (r.start.min(text.len()), r.end.min(text.len()).max(r.start.min(text.len()))),
        None => (0, text.len()),
    };
    let hay = &text[..text_end];
    let line_end_from = |from: usize| text[from..].find('\n').map_or(text.len(), |i| from + i);
    let line_start0 = text[..lo].rfind('\n').map_or(0, |i| i + 1);
    let (mut line_no, mut line_start, mut pos) = (text[..line_start0].matches('\n').count(), line_start0, lo);
    let mut line_end = line_end_from(line_start);
    // 每一個符合都是一筆結果（同一行有幾個就列幾筆），筆數才和編輯區的計數一致
    while let Some(m) = searcher.find_next(hay, pos) {
        while m.start > line_end {
            line_start = line_end + 1;
            line_no += 1;
            line_end = line_end_from(line_start);
        }
        let line =text[line_start..line_end].strip_suffix('\r').unwrap_or(&text[line_start..line_end]);
        // 跨行的符合只標示第一行的部分
        let end_byte = m.end.min(line_start + line.len()).max(m.start);
        let start = text[line_start..m.start].chars().count();
        let end = start + text[m.start..end_byte].chars().count();
        let (text_shown, text_offset) = excerpt(line, start);
        on_hit(Hit {
            path: path.to_owned(),
            line: line_no,
            range: start..end,
            text: text_shown,
            text_offset,
        });
        summary.hits += 1;
        if summary.hits >= max_hits {
            summary.truncated = true;
            summary.files_with_hits += 1;
            return false;
        }
        if summary.hits % 256 == 0 && cancel.load(Ordering::Relaxed) {
            summary.cancelled = true;
            summary.files_with_hits += 1;
            return false;
        }
        // 下一筆從這一筆的結尾找；空的符合要多跳過一個字元，否則會原地踏步
        pos = if m.is_empty() {
            match text[m.end..text_end].chars().next() {
                Some(c) => m.end + c.len_utf8(),
                None => break,
            }
        } else {
            m.end
        };
    }
    if summary.hits > before {
        summary.files_with_hits += 1;
    }
    true
}

/// 直接改寫磁碟上的檔案：把所有符合的地方換成 `replacement`，回傳取代的筆數（沒有符合時不動檔案）。
/// 保留原本的編碼、BOM 與換行；含有無法解讀的位元組的檔案（存回去會損毀）不改，回傳錯誤。
pub fn replace_in_file(path: &Path, searcher: &Searcher, replacement: &str) -> Result<usize, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let decoded = fileio::decode(&bytes);
    if looks_binary(&bytes) || decoded.lossy {
        return Err(t("檔案含有無法解讀的位元組，不能安全改寫").to_owned());
    }
    let (out, n) = searcher.replace_all(&decoded.text, replacement);
    if n > 0 {
        fileio::write_file(path, &out, &decoded.format).map_err(|e| e.to_string())?;
    }
    Ok(n)
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
    fn every_match_is_listed_and_crlf_is_handled() {
        let d = tempfile::tempdir().unwrap();
        write(d.path(), "a.txt", b"foo foo foo\r\nbar foo\r\n");
        let (hits, s) = run(d.path(), "foo", &GrepOptions::default());
        assert_eq!(hits.len(), 4, "同一行有幾個符合就列幾筆");
        assert_eq!(s.hits, 4);
        assert_eq!(hits.iter().map(|h| (h.line, h.range.start)).collect::<Vec<_>>(), vec![(0, 0), (0, 4), (0, 8), (1, 4)]);
        assert_eq!(hits[3].text, "bar foo", "行尾的 \\r 不會留在文字裡");
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
    fn a_pattern_with_newline_matches_across_lines() {
        let d = tempfile::tempdir().unwrap();
        write(d.path(), "a.txt", b"x]\r\ny\r\nz]\r\n");
        let opts = GrepOptions {
            search: SearchOptions { regex: true, ..SearchOptions::default() },
            ..GrepOptions::default()
        };
        let (hits, _) = run(d.path(), r"\]\n", &opts);
        let got: Vec<_> = hits.iter().map(|h| (h.line, h.range.clone(), h.text.as_str())).collect();
        assert_eq!(got, vec![(0, 1..2, "x]"), (2, 1..2, "z]")]);
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
    fn subfolders_can_be_left_out() {
        let d = tempfile::tempdir().unwrap();
        write(d.path(), "top.txt", b"foo");
        write(d.path(), "sub/deep.txt", b"foo");
        let flat = GrepOptions { recursive: false, ..GrepOptions::default() };
        assert_eq!(run(d.path(), "foo", &flat).0.iter().map(name).collect::<Vec<_>>(), vec!["top.txt"]);
        assert_eq!(run(d.path(), "foo", &GrepOptions::default()).0.len(), 2);
    }

    #[test]
    fn text_in_memory_is_searched_like_a_file() {
        let mut hits = Vec::new();
        let s = search_text(
            Path::new(""),
            "a\nfoo\r\nx foo",
            "foo",
            SearchOptions::default(),
            100,
            None,
            &AtomicBool::new(false),
            |h| hits.push(h),
        )
        .unwrap();
        assert_eq!(hits.iter().map(|h| h.line).collect::<Vec<_>>(), vec![1, 2]);
        assert_eq!((s.hits, s.files_searched, s.files_with_hits), (2, 1, 1));
        assert!(search_text(Path::new(""), "x", "(", SearchOptions { regex: true, ..Default::default() }, 1, None, &AtomicBool::new(false), |_| {}).is_err());
    }

    #[test]
    fn a_search_range_limits_hits_but_keeps_line_numbers_and_columns() {
        let text = "foo\nxx foo foo\nfoo\n";
        // 範圍是第二行的第一個 foo 之後（"foo foo" 的後半）到第三行行首
        let from = text.find("foo foo").unwrap() + 3;
        let to = text.len() - 4; // 不含最後一行的 foo
        let mut hits = Vec::new();
        search_text(Path::new(""), text, "foo", SearchOptions::default(), 100, Some(from..to), &AtomicBool::new(false), |h| hits.push(h)).unwrap();
        assert_eq!(hits.iter().map(|h| (h.line, h.range.clone())).collect::<Vec<_>>(), vec![(1, 7..10)]);
    }

    #[test]
    fn every_match_is_a_hit_even_on_the_same_line() {
        let opts = SearchOptions { regex: true, ..Default::default() };
        let ranges = |text: &str, pat: &str| {
            let mut hits = Vec::new();
            search_text(Path::new(""), text, pat, opts, 100, None, &AtomicBool::new(false), |h| hits.push(h)).unwrap();
            hits.iter().map(|h| (h.line, h.range.clone())).collect::<Vec<_>>()
        };
        assert_eq!(ranges("a1 b22\nc333", r"\d+"), vec![(0, 1..2), (0, 4..6), (1, 1..4)]);
        // 空的符合也各算一筆，而且不會原地踏步
        assert_eq!(ranges("a\nb", "^"), vec![(0, 0..0), (1, 0..0)]);
        assert_eq!(ranges("ab", r"\d*"), vec![(0, 0..0), (0, 1..1), (0, 2..2)]);
    }

    #[test]
    fn replace_in_file_keeps_encoding_and_line_endings() {
        let d = tempfile::tempdir().unwrap();
        write(d.path(), "crlf.txt", b"foo\r\nbar foo\r\n");
        let sr = Searcher::new("foo", SearchOptions::default()).unwrap();
        assert_eq!(replace_in_file(&d.path().join("crlf.txt"), &sr, "X").unwrap(), 2);
        assert_eq!(std::fs::read(d.path().join("crlf.txt")).unwrap(), b"X\r\nbar X\r\n");
        // 沒有符合：不動檔案
        write(d.path(), "none.txt", b"bar");
        assert_eq!(replace_in_file(&d.path().join("none.txt"), &sr, "X").unwrap(), 0);
        // 二進位檔：拒絕
        write(d.path(), "bin.dat", b"foo\0foo");
        assert!(replace_in_file(&d.path().join("bin.dat"), &sr, "X").is_err());
        assert_eq!(std::fs::read(d.path().join("bin.dat")).unwrap(), b"foo\0foo");
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
