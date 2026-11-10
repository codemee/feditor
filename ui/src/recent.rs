//! 最近開啟的檔案：最多 [`MAX_RECENT`] 個，最近的在最前面，存在設定檔（`recent = 路徑`，一行一個）。
//!
//! 與畫面、作業系統都無關，方便單元測試。工具列「開啟」旁的下拉清單、Windows 工作列的跳躍清單、
//! macOS 的 Dock 選單都用這一份清單（後兩者見 `recent_os`）。

use std::path::Path;

/// 最多記幾個。
pub const MAX_RECENT: usize = 10;

/// 兩個路徑字串是不是同一個檔案（Windows 的路徑不分大小寫，也不分 `/`、`\`）。
fn same(a: &str, b: &str) -> bool {
    if cfg!(windows) {
        a.replace('/', "\\").eq_ignore_ascii_case(&b.replace('/', "\\"))
    } else {
        a == b
    }
}

/// 路徑能不能記進設定檔（設定檔一行一個，含換行的記不了）。
pub fn recordable(path: &str) -> bool {
    !path.is_empty() && !path.contains(['\r', '\n'])
}

/// 把 `path` 放到清單最前面（已經在清單裡就移到最前面），超過 [`MAX_RECENT`] 個的最舊的丟掉。
/// 相對路徑先換成絕對路徑，所以從不同的資料夾用相對路徑開同一個檔案也只會有一筆。
/// 回傳實際記下的路徑字串；記不了（含換行等）回傳 `None`。
pub fn push(list: &mut Vec<String>, path: &Path) -> Option<String> {
    let absolute = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let text = absolute.to_string_lossy().into_owned();
    if !recordable(&text) {
        return None;
    }
    list.retain(|p| !same(p, &text));
    list.insert(0, text.clone());
    list.truncate(MAX_RECENT);
    Some(text)
}

/// 從清單拿掉 `path`（開不了的檔案）。
pub fn remove(list: &mut Vec<String>, path: &str) {
    list.retain(|p| !same(p, path));
}

/// 整理從設定檔讀進來的清單：去掉不能用的與重複的（留最前面那個），最多 [`MAX_RECENT`] 個。
pub fn clean(list: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(list.len().min(MAX_RECENT));
    for path in list {
        if recordable(&path) && !out.iter().any(|p| same(p, &path)) {
            out.push(path);
        }
        if out.len() == MAX_RECENT {
            break;
        }
    }
    out
}

/// 檔名與所在的資料夾（顯示用）。
pub fn name_and_folder(path: &str) -> (String, String) {
    let p = Path::new(path);
    let name = p.file_name().map_or_else(|| path.to_owned(), |n| n.to_string_lossy().into_owned());
    let folder = p.parent().map(|d| d.to_string_lossy().into_owned()).unwrap_or_default();
    (name, folder)
}

/// 資料夾路徑太長時只留最後的部分（前面用 `…` 代替），選單才不會被撐得很寬。以字元計算，不會切在字的中間。
pub fn shorten_folder(folder: &str, max_chars: usize) -> String {
    let count = folder.chars().count();
    if count <= max_chars || max_chars < 2 {
        return folder.to_owned();
    }
    let tail: String = folder.chars().skip(count - (max_chars - 1)).collect();
    format!("…{tail}")
}

/// 工作列跳躍清單上每一項的標題：用檔名；清單裡有檔名相同的檔案時，加上資料夾才分得出來。
pub fn jump_titles(list: &[String]) -> Vec<String> {
    list.iter()
        .map(|path| {
            let (name, folder) = name_and_folder(path);
            let duplicated = list.iter().filter(|other| name_and_folder(other).0.eq_ignore_ascii_case(&name)).count() > 1;
            if duplicated && !folder.is_empty() { format!("{name} ({folder})") } else { name }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn abs(name: &str) -> String {
        std::path::absolute(name).unwrap().to_string_lossy().into_owned()
    }

    #[test]
    fn the_newest_file_goes_first_and_reopening_moves_it_to_the_front() {
        let mut list = Vec::new();
        push(&mut list, Path::new("a.txt"));
        push(&mut list, Path::new("b.txt"));
        push(&mut list, Path::new("c.txt"));
        assert_eq!(list, [abs("c.txt"), abs("b.txt"), abs("a.txt")]);
        push(&mut list, Path::new("a.txt"));
        assert_eq!(list, [abs("a.txt"), abs("c.txt"), abs("b.txt")], "已經在清單裡：移到最前面，不重複");
    }

    #[test]
    fn only_the_latest_ten_are_kept() {
        let mut list = Vec::new();
        for i in 0..15 {
            push(&mut list, Path::new(&format!("f{i}.txt")));
        }
        assert_eq!(list.len(), MAX_RECENT);
        assert_eq!(list[0], abs("f14.txt"));
        assert_eq!(list[9], abs("f5.txt"), "最舊的五個被丟掉");
    }

    #[test]
    fn paths_that_cannot_live_on_one_settings_line_are_refused() {
        let mut list = Vec::new();
        assert_eq!(push(&mut list, Path::new("bad\nname.txt")), None);
        assert!(list.is_empty());
        assert_eq!(clean(vec!["a".into(), "".into(), "x\ny".into(), "a".into(), "b".into()]), ["a", "b"], "空的、含換行的、重複的都去掉");
    }

    #[test]
    fn removing_and_cleaning_work() {
        let mut list = vec![abs("a.txt"), abs("b.txt")];
        remove(&mut list, &abs("a.txt"));
        assert_eq!(list, [abs("b.txt")]);
        let many: Vec<String> = (0..30).map(|i| format!("p{i}")).collect();
        assert_eq!(clean(many).len(), MAX_RECENT);
    }

    #[test]
    fn windows_paths_ignore_case_and_slash_direction_but_unix_paths_do_not() {
        let mut list = vec!["C:\\Code\\a.txt".to_owned()];
        remove(&mut list, "c:/code/A.TXT");
        assert_eq!(list.is_empty(), cfg!(windows));
    }

    #[test]
    fn names_and_folders_are_split_for_display() {
        let path = abs("sub/note.md");
        let (name, folder) = name_and_folder(&path);
        assert_eq!(name, "note.md");
        assert!(folder.ends_with("sub"), "{folder}");
        assert_eq!(name_and_folder("plain").0, "plain");
    }

    #[test]
    fn long_folders_keep_only_their_tail() {
        assert_eq!(shorten_folder("short", 44), "short");
        let long = "C:\\非常長的資料夾\\再一層\\再一層\\最後一層";
        let shown = shorten_folder(long, 12);
        assert_eq!(shown.chars().count(), 12);
        assert!(shown.starts_with('…') && shown.ends_with("最後一層"), "{shown}");
        assert_eq!(shorten_folder("abc", 1), "abc", "上限太小就不縮");
    }

    #[test]
    fn jump_list_titles_add_the_folder_only_when_names_repeat() {
        let list = vec![abs("x/a.txt"), abs("y/a.txt"), abs("z/b.txt")];
        let titles = jump_titles(&list);
        assert!(titles[0].starts_with("a.txt (") && titles[0].contains('x'), "{titles:?}");
        assert!(titles[1].starts_with("a.txt (") && titles[1].contains('y'));
        assert_eq!(titles[2], "b.txt", "檔名沒重複的只寫檔名");
    }
}
