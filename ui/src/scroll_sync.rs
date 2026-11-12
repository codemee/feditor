//! 預覽與編輯區的同步捲動：決定每一幀要不要、往哪邊推。純計算，和畫面無關，方便測試。
//!
//! 兩邊各有一個「位置」：編輯區是最上面看到第幾行（帶小數），預覽是捲動的像素。
//! 預覽用「每個最上層區塊在原文從第幾行開始」和「每個區塊頂端的像素」兩張表換算。
//! 每一幀比較兩邊的位置和上一幀有沒有變：誰動了就以誰為準，推另一邊過去。
//! 推過去之後那一邊也會動（結果回到我們這裡），所以推完後的幾幀不比較，只記下實際的位置，免得兩邊互相追。

/// 推完之後，要忽略幾幀（讓被推的那一邊真的捲過去）。
const SETTLE_FRAMES: u8 = 2;
/// 編輯區位置變動小於這個行數不算動（浮點誤差）。
const LINE_EPS: f32 = 0.02;
/// 預覽位置變動小於這個像素數不算動。
const PIXEL_EPS: f32 = 0.5;

#[derive(Default, Debug)]
pub struct SyncState {
    last_line: Option<f32>,
    last_offset: Option<f32>,
    settle: u8,
}

/// 這一幀要做的事。
#[derive(Default, Debug, PartialEq)]
pub struct Plan {
    /// 要把預覽捲到這個位置（像素）。
    pub preview_offset: Option<f32>,
    /// 要把編輯區捲到這一行（帶小數）。
    pub editor_line: Option<f32>,
}

/// 這一幀看到的兩邊的樣子。
pub struct View<'a> {
    /// 最上層每個區塊在原文從第幾行開始。
    pub lines: &'a [usize],
    /// 每個區塊頂端的像素；最後一個是整份內容的高度（所以比 `lines` 多一個）。
    pub tops: &'a [f32],
    /// 原文總共幾行。
    pub total_lines: usize,
    pub editor_line: f32,
    pub editor_at_bottom: bool,
    pub preview_offset: f32,
    pub preview_viewport_h: f32,
}

impl SyncState {
    /// 忘掉之前的位置（換了文件、或預覽不能同步時），下一次會以編輯區為準對齊。
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn step(&mut self, v: &View) -> Plan {
        if v.lines.is_empty() || v.tops.len() != v.lines.len() + 1 {
            self.reset();
            return Plan::default();
        }
        if self.settle > 0 {
            self.settle -= 1;
            self.last_line = Some(v.editor_line);
            self.last_offset = Some(v.preview_offset);
            return Plan::default();
        }
        let editor_moved = self.last_line.is_none_or(|l| (v.editor_line - l).abs() > LINE_EPS);
        let preview_moved = self.last_offset.is_some_and(|o| (v.preview_offset - o).abs() > PIXEL_EPS);
        if editor_moved {
            let y = if v.editor_at_bottom {
                max_offset(v)
            } else {
                offset_at_line(v.lines, v.tops, v.total_lines, v.editor_line)
            };
            self.settle = SETTLE_FRAMES;
            self.last_line = Some(v.editor_line);
            self.last_offset = Some(y);
            return Plan { preview_offset: Some(y), editor_line: None };
        }
        if preview_moved {
            let line = if v.preview_offset >= max_offset(v) - 1.0 {
                v.total_lines as f32 // 預覽捲到底：編輯區也捲到底
            } else {
                line_at_offset(v.lines, v.tops, v.total_lines, v.preview_offset)
            };
            self.settle = SETTLE_FRAMES;
            self.last_line = Some(line);
            self.last_offset = Some(v.preview_offset);
            return Plan { preview_offset: None, editor_line: Some(line) };
        }
        self.last_line = Some(v.editor_line);
        self.last_offset = Some(v.preview_offset);
        Plan::default()
    }
}

/// 預覽最多能捲到哪裡。
fn max_offset(v: &View) -> f32 {
    (v.tops.last().copied().unwrap_or(0.0) - v.preview_viewport_h).max(0.0)
}

/// 區塊 `i` 涵蓋的原文行：從它的起始行到下一個區塊的起始行（最後一個到文件結尾）。
fn span_of(lines: &[usize], total_lines: usize, i: usize) -> (f32, f32) {
    let start = lines[i] as f32;
    let end = lines.get(i + 1).map_or(total_lines, |&l| l) as f32;
    (start, end.max(start))
}

/// 編輯區最上面是第 `line` 行（帶小數）時，預覽該捲到的像素位置。
pub fn offset_at_line(lines: &[usize], tops: &[f32], total_lines: usize, line: f32) -> f32 {
    // 最後一個起始行不超過 `line` 的區塊（好幾個區塊從同一行開始時取最後一個）
    let i = lines.partition_point(|&l| l as f32 <= line);
    if i == 0 {
        // 第一個區塊之前（開頭的空行）：從頂端到第一個區塊之間按比例
        let first = lines[0] as f32;
        return if first > 0.0 { tops[0] * (line / first).clamp(0.0, 1.0) } else { 0.0 };
    }
    let i = i - 1;
    let (start, end) = span_of(lines, total_lines, i);
    let frac = if end > start { ((line - start) / (end - start)).clamp(0.0, 1.0) } else { 0.0 };
    tops[i] + frac * (tops[i + 1] - tops[i])
}

/// 預覽捲到像素位置 `offset` 時，編輯區最上面該是第幾行（帶小數）。
pub fn line_at_offset(lines: &[usize], tops: &[f32], total_lines: usize, offset: f32) -> f32 {
    let n = lines.len();
    let i = tops[..n].partition_point(|&t| t <= offset).saturating_sub(1);
    if offset < tops[0] {
        // 第一個區塊之前
        return if tops[0] > 0.0 { lines[0] as f32 * (offset / tops[0]).clamp(0.0, 1.0) } else { 0.0 };
    }
    let (start, end) = span_of(lines, total_lines, i);
    let height = tops[i + 1] - tops[i];
    let frac = if height > 0.0 { ((offset - tops[i]) / height).clamp(0.0, 1.0) } else { 0.0 };
    start + frac * (end - start)
}

#[cfg(test)]
mod tests {
    use super::*;

    // 三個區塊：從第 0、4、10 行開始，文件共 20 行；頂端在 0、100、150 像素，內容高 400
    const LINES: [usize; 3] = [0, 4, 10];
    const TOPS: [f32; 4] = [0.0, 100.0, 150.0, 400.0];

    fn view(editor_line: f32, offset: f32) -> View<'static> {
        View { lines: &LINES, tops: &TOPS, total_lines: 20, editor_line, editor_at_bottom: false, preview_offset: offset, preview_viewport_h: 200.0 }
    }

    #[test]
    fn lines_and_pixels_convert_in_both_directions_inside_a_block() {
        // 第 2 行：第一個區塊（0–4 行）的一半 → 50 像素
        assert_eq!(offset_at_line(&LINES, &TOPS, 20, 2.0), 50.0);
        // 第 7 行：第二個區塊（4–10 行）的一半 → 125 像素
        assert_eq!(offset_at_line(&LINES, &TOPS, 20, 7.0), 125.0);
        // 最後一個區塊到文件結尾（10–20 行）：第 15 行 → 275 像素
        assert_eq!(offset_at_line(&LINES, &TOPS, 20, 15.0), 275.0);
        assert_eq!(line_at_offset(&LINES, &TOPS, 20, 50.0), 2.0);
        assert_eq!(line_at_offset(&LINES, &TOPS, 20, 125.0), 7.0);
        assert_eq!(line_at_offset(&LINES, &TOPS, 20, 275.0), 15.0);
        // 來回換算回到原來的位置
        for line in [0.0, 1.5, 4.0, 9.9, 10.0, 19.0] {
            let back = line_at_offset(&LINES, &TOPS, 20, offset_at_line(&LINES, &TOPS, 20, line));
            assert!((back - line).abs() < 0.001, "{line} → {back}");
        }
    }

    #[test]
    fn blank_lines_before_the_first_block_scale_to_the_top() {
        let lines = [3usize, 8];
        let tops = [30.0f32, 90.0, 200.0];
        assert_eq!(offset_at_line(&lines, &tops, 12, 0.0), 0.0);
        assert_eq!(offset_at_line(&lines, &tops, 12, 1.5), 15.0);
        assert_eq!(line_at_offset(&lines, &tops, 12, 15.0), 1.5);
    }

    #[test]
    fn blocks_that_start_on_the_same_line_do_not_divide_by_zero() {
        let lines = [0usize, 5, 5, 9];
        let tops = [0.0f32, 50.0, 60.0, 90.0, 150.0];
        assert!(offset_at_line(&lines, &tops, 12, 5.0).is_finite());
        assert!(line_at_offset(&lines, &tops, 12, 55.0).is_finite());
    }

    #[test]
    fn the_side_that_moved_drives_the_other_and_the_echo_does_not_bounce_back() {
        let mut s = SyncState::default();
        // 第一次：以編輯區為準，把預覽對齊過去
        let p = s.step(&view(2.0, 0.0));
        assert_eq!(p, Plan { preview_offset: Some(50.0), editor_line: None });
        // 預覽真的捲過去之後（幾幀內）不再推
        for _ in 0..SETTLE_FRAMES {
            assert_eq!(s.step(&view(2.0, 50.0)), Plan::default());
        }
        assert_eq!(s.step(&view(2.0, 50.0)), Plan::default(), "沒人動就不推");
        // 使用者捲預覽：編輯區跟著
        let p = s.step(&view(2.0, 125.0));
        assert_eq!(p, Plan { preview_offset: None, editor_line: Some(7.0) });
        for _ in 0..SETTLE_FRAMES {
            s.step(&view(7.0, 125.0));
        }
        assert_eq!(s.step(&view(7.0, 125.0)), Plan::default(), "編輯區被推過去之後不再推回預覽");
        // 使用者捲編輯區：預覽跟著
        let p = s.step(&view(15.0, 125.0));
        assert_eq!(p, Plan { preview_offset: Some(275.0), editor_line: None });
    }

    #[test]
    fn both_ends_line_up_and_an_unsyncable_preview_resets() {
        let mut s = SyncState::default();
        // 編輯區在最底：預覽捲到它能捲的最底（400 - 200 = 200）
        let mut v = view(14.0, 0.0);
        v.editor_at_bottom = true;
        assert_eq!(s.step(&v).preview_offset, Some(200.0));
        // 預覽在最底：編輯區捲到文件結尾（交給畫面夾到最底）
        let mut s = SyncState::default();
        s.step(&view(0.0, 0.0));
        for _ in 0..SETTLE_FRAMES {
            s.step(&view(0.0, 0.0));
        }
        assert_eq!(s.step(&view(0.0, 200.0)).editor_line, Some(20.0));
        // 沒有對應表（外掛轉的 Markdown、不是 Markdown）：什麼都不做，也忘掉位置
        let empty = View { lines: &[], tops: &[], total_lines: 20, editor_line: 3.0, editor_at_bottom: false, preview_offset: 0.0, preview_viewport_h: 200.0 };
        assert_eq!(s.step(&empty), Plan::default());
        assert_eq!(s.last_line, None);
    }
}
