//! 工作階段：記住上次開著哪些檔案，下次不帶檔名啟動時自動開回來。
//!
//! 只記有存過檔的分頁（路徑與游標位置）；沒存過的空白文件不記。
//! 存在設定資料夾的 `session.conf`，格式很簡單：
//!
//! ```text
//! active = 1
//! file = C:\專案\a.txt<Tab>120
//! ```

use std::path::PathBuf;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Session {
    /// （路徑, 游標位置（字元））。
    pub files: Vec<(PathBuf, usize)>,
    /// 上次作用中的是 `files` 的第幾個。
    pub active: usize,
}

impl Session {
    pub fn parse(text: &str) -> Self {
        let mut s = Session::default();
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            match key.trim() {
                "active" => s.active = value.trim().parse().unwrap_or(0),
                "file" => {
                    let value = value.trim_start();
                    let (path, caret) = value.rsplit_once('\t').unwrap_or((value, "0"));
                    if !path.trim().is_empty() {
                        s.files.push((PathBuf::from(path.trim_end()), caret.trim().parse().unwrap_or(0)));
                    }
                }
                _ => {}
            }
        }
        s.active = s.active.min(s.files.len().saturating_sub(1));
        s
    }

    pub fn to_text(&self) -> String {
        let mut out = format!("active = {}\n", self.active);
        for (path, caret) in &self.files {
            out.push_str(&format!("file = {}\t{caret}\n", path.display()));
        }
        out
    }

    fn path() -> Option<PathBuf> {
        crate::settings::Settings::default_path().and_then(|p| p.parent().map(|d| d.join("session.conf")))
    }

    pub fn load() -> Self {
        Self::path()
            .and_then(|p| std::fs::read(p).ok())
            .map(|b| Self::parse(&String::from_utf8_lossy(&b)))
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let Some(path) = Self::path() else { return };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(path, self.to_text());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_keeps_paths_with_spaces_and_cjk() {
        let s = Session {
            files: vec![
                (PathBuf::from("C:\\我的 專案\\a b.txt"), 12),
                (PathBuf::from("/tmp/x.rs"), 0),
            ],
            active: 1,
        };
        assert_eq!(Session::parse(&s.to_text()), s);
    }

    #[test]
    fn bad_input_is_ignored_and_active_is_clamped() {
        let s = Session::parse("junk\nactive = 99\nfile = \nfile = /a\tnotanumber\nunknown = 1\n");
        assert_eq!(s.files, vec![(PathBuf::from("/a"), 0)]);
        assert_eq!(s.active, 0);
        assert_eq!(Session::parse(""), Session::default());
    }

    #[test]
    fn missing_caret_defaults_to_zero() {
        let s = Session::parse("file = /only/path\n");
        assert_eq!(s.files, vec![(PathBuf::from("/only/path"), 0)]);
    }
}
