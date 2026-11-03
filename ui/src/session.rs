//! 工作階段：記住上次開著哪些檔案與窗格版面，下次不帶檔名啟動時自動開回來。
//!
//! 只記有存過檔的分頁（路徑與游標位置）；沒存過的空白文件不記。
//! 存在設定資料夾的 `session.conf`，格式很簡單：
//!
//! ```text
//! focus = 2
//! layout = H0.5[P0,V0.4[P2,W3]]
//! preview = 3,2
//! pane = 0
//! active = 1
//! file = C:\專案\a.txt<Tab>120
//! pane = 2
//! active = 0
//! file = C:\專案\b.md<Tab>0
//! ```
//!
//! `layout` 是窗格版面（見 [`crate::panes::Node::to_spec`]）：`P` 是一般窗格、`W` 是預覽窗格；
//! `preview = 預覽窗格,它預覽的窗格`。每個 `pane = N` 之後的 `active`／`file` 屬於那個窗格。
//! 舊版只有一串 `active`／`file`（沒有 `pane`、`layout`）：當成只有一個窗格。

use std::path::PathBuf;

/// 窗格裡的一個終端機分頁：記下名字，重開時開一個新的 shell 放回同樣的位置。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionTerminal {
    /// 在窗格所有分頁（檔案加終端機）裡的第幾個位置。
    pub at: usize,
    /// 預設名稱「終端機 N」的 N。
    pub number: usize,
    /// 使用者自己取的名字；沒有就是預設名稱。
    pub name: Option<String>,
}

/// 一個窗格開著的分頁（檔案與終端機），依分頁順序。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SessionPane {
    pub id: u32,
    /// 作用中的是第幾個分頁（檔案與終端機合在一起算）。
    pub active: usize,
    /// （路徑, 游標位置（字元））。
    pub files: Vec<(PathBuf, usize)>,
    pub terminals: Vec<SessionTerminal>,
}

/// 窗格裡的一個分頁，還原時照順序開。
pub enum SessionEntry<'a> {
    File(&'a PathBuf, usize),
    Terminal(&'a SessionTerminal),
}

impl SessionPane {
    /// 檔案與終端機依 `at` 排回原來的分頁順序。
    pub fn entries(&self) -> Vec<SessionEntry<'_>> {
        let mut out: Vec<SessionEntry<'_>> = self.files.iter().map(|(p, c)| SessionEntry::File(p, *c)).collect();
        let mut terminals: Vec<&SessionTerminal> = self.terminals.iter().collect();
        terminals.sort_by_key(|t| t.at);
        for t in terminals {
            out.insert(t.at.min(out.len()), SessionEntry::Terminal(t));
        }
        out
    }
}

/// 預覽窗格：`id` 預覽 `source` 窗格目前的分頁。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SessionPreview {
    pub id: u32,
    pub source: u32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Session {
    pub panes: Vec<SessionPane>,
    pub previews: Vec<SessionPreview>,
    /// 窗格版面；舊版的工作階段檔沒有，只有一個窗格。
    pub layout: Option<String>,
    /// 有焦點的窗格。
    pub focus: u32,
}

impl Session {
    /// 所有窗格裡的檔案數。
    #[cfg(test)]
    pub fn file_count(&self) -> usize {
        self.panes.iter().map(|p| p.files.len()).sum()
    }

    pub fn parse(text: &str) -> Self {
        let mut s = Session::default();
        let mut current: Option<usize> = None;
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let key = key.trim();
            match key {
                "focus" => s.focus = value.trim().parse().unwrap_or(0),
                "layout" => {
                    let spec = value.trim();
                    s.layout = (!spec.is_empty()).then(|| spec.to_owned());
                }
                "preview" => {
                    if let Some((id, source)) = value.split_once(',') {
                        if let (Ok(id), Ok(source)) = (id.trim().parse(), source.trim().parse()) {
                            s.previews.push(SessionPreview { id, source });
                        }
                    }
                }
                "pane" => {
                    if let Ok(id) = value.trim().parse() {
                        s.panes.push(SessionPane { id, ..Default::default() });
                        current = Some(s.panes.len() - 1);
                    }
                }
                "terminal" => {
                    // terminal = 位置<Tab>編號<Tab>名字（名字可以是空的）
                    let i = *current.get_or_insert_with(|| {
                        s.panes.push(SessionPane::default());
                        s.panes.len() - 1
                    });
                    let mut parts = value.trim_start().splitn(3, '\t');
                    let at = parts.next().and_then(|v| v.trim().parse().ok());
                    let number = parts.next().and_then(|v| v.trim().parse().ok());
                    if let (Some(at), Some(number)) = (at, number) {
                        let name = parts.next().map(str::trim).filter(|n| !n.is_empty()).map(str::to_owned);
                        s.panes[i].terminals.push(SessionTerminal { at, number, name });
                    }
                }
                "active" | "file" => {
                    // 沒有 `pane =` 的舊格式：檔案都屬於窗格 0
                    let i = *current.get_or_insert_with(|| {
                        s.panes.push(SessionPane::default());
                        s.panes.len() - 1
                    });
                    let pane = &mut s.panes[i];
                    if key == "active" {
                        pane.active = value.trim().parse().unwrap_or(0);
                    } else {
                        let value = value.trim_start();
                        let (path, caret) = value.rsplit_once('\t').unwrap_or((value, "0"));
                        if !path.trim().is_empty() {
                            pane.files.push((PathBuf::from(path.trim_end()), caret.trim().parse().unwrap_or(0)));
                        }
                    }
                }
                _ => {}
            }
        }
        for pane in &mut s.panes {
            let total = pane.files.len() + pane.terminals.len();
            pane.active = pane.active.min(total.saturating_sub(1));
        }
        s
    }

    pub fn to_text(&self) -> String {
        let mut out = format!("focus = {}\n", self.focus);
        if let Some(layout) = &self.layout {
            out.push_str(&format!("layout = {layout}\n"));
        }
        for p in &self.previews {
            out.push_str(&format!("preview = {},{}\n", p.id, p.source));
        }
        for pane in &self.panes {
            out.push_str(&format!("pane = {}\nactive = {}\n", pane.id, pane.active));
            for (path, caret) in &pane.files {
                out.push_str(&format!("file = {}\t{caret}\n", path.display()));
            }
            for t in &pane.terminals {
                // 名字裡的換行與 Tab 會破壞格式，換成空白
                let name: String = t.name.as_deref().unwrap_or("").chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
                out.push_str(&format!("terminal = {}\t{}\t{}\n", t.at, t.number, name));
            }
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

    fn pane(id: u32, active: usize, files: &[(&str, usize)]) -> SessionPane {
        SessionPane { id, active, files: files.iter().map(|(p, c)| (PathBuf::from(p), *c)).collect(), terminals: Vec::new() }
    }

    #[test]
    fn round_trip_keeps_layout_panes_previews_and_paths_with_spaces_and_cjk() {
        let s = Session {
            panes: vec![
                pane(0, 1, &[("C:\\我的 專案\\a b.txt", 12), ("/tmp/x.rs", 0)]),
                pane(2, 0, &[("/tmp/y.md", 5)]),
            ],
            previews: vec![SessionPreview { id: 3, source: 2 }],
            layout: Some("H0.5[P0,V0.400[P2,W3]]".to_owned()),
            focus: 2,
        };
        assert_eq!(Session::parse(&s.to_text()), s);
        assert_eq!(s.file_count(), 3);
    }

    #[test]
    fn terminal_tabs_round_trip_in_their_original_positions() {
        let mut p = pane(0, 1, &[("/a.txt", 5), ("/b.txt", 7)]);
        p.terminals = vec![
            SessionTerminal { at: 1, number: 2, name: Some("伺服器 = 8080".to_owned()) },
            SessionTerminal { at: 3, number: 1, name: None },
        ];
        let s = Session { panes: vec![p], previews: vec![], layout: Some("P0".to_owned()), focus: 0 };
        let back = Session::parse(&s.to_text());
        assert_eq!(back, s);
        let kinds: Vec<String> = back.panes[0]
            .entries()
            .iter()
            .map(|e| match e {
                SessionEntry::File(p, _) => p.display().to_string(),
                SessionEntry::Terminal(t) => format!("T{}", t.number),
            })
            .collect();
        assert_eq!(kinds, vec!["/a.txt", "T2", "/b.txt", "T1"], "檔案與終端機照原本的順序排回去");
        // 壞掉的 terminal 行略過；名字裡的控制字元不會破壞格式；作用中的位置夾在範圍內
        let bad = Session::parse("pane = 0\nactive = 9\nterminal = x\t1\t\nterminal = 0\t3\ta\tb\nfile = /f\t0\n");
        assert_eq!(bad.panes[0].terminals, vec![SessionTerminal { at: 0, number: 3, name: Some("a\tb".to_owned()) }]);
        assert_eq!(bad.panes[0].active, 1);
        let weird = SessionTerminal { at: 0, number: 1, name: Some("a\nb\tc".to_owned()) };
        let round = Session::parse(&Session { panes: vec![SessionPane { terminals: vec![weird], ..Default::default() }], ..Default::default() }.to_text());
        assert_eq!(round.panes[0].terminals[0].name.as_deref(), Some("a b c"));
    }

    #[test]
    fn the_old_flat_format_becomes_one_pane() {
        let s = Session::parse("active = 1\nfile = /a\t3\nfile = /b\t4\n");
        assert_eq!(s.layout, None);
        assert_eq!(s.panes, vec![pane(0, 1, &[("/a", 3), ("/b", 4)])]);
        assert_eq!(s.focus, 0);
    }

    #[test]
    fn bad_input_is_ignored_and_active_is_clamped_per_pane() {
        let s = Session::parse("junk\nactive = 99\nfile = \nfile = /a\tnotanumber\nunknown = 1\n");
        assert_eq!(s.panes, vec![pane(0, 0, &[("/a", 0)])]);
        assert_eq!(Session::parse(""), Session::default());
        let s = Session::parse("pane = 1\nactive = 5\nfile = /x\npane = 2\nactive = 1\nfile = /y\nfile = /z\n");
        assert_eq!(s.panes[0].active, 0, "只有一個檔案，作用中最多是 0");
        assert_eq!(s.panes[1].active, 1);
        // 壞掉的預覽、窗格編號略過
        let s = Session::parse("preview = a,b\npreview = 3\npreview = 4,1\npane = x\n");
        assert_eq!(s.previews, vec![SessionPreview { id: 4, source: 1 }]);
        assert!(s.panes.is_empty());
    }

    #[test]
    fn missing_caret_defaults_to_zero() {
        let s = Session::parse("file = /only/path\n");
        assert_eq!(s.panes[0].files, vec![(PathBuf::from("/only/path"), 0)]);
    }
}
