//! 自動換行：每一行佔幾個「畫面列」，以及畫面列和文字行之間的對應。
//!
//! 換行只影響畫面，不會改動文字。要知道第 N 個畫面列是哪一行的第幾列，必須知道每一行折成幾列，
//! 所以整份文件的列數要先算好（累計表）；內容、寬度沒變就沿用上次的結果。為了不讓每次打字都
//! 要重算太久，超過 [`MAX_LINES`] 行的文件不提供自動換行。

use feditor_core::{Buffer, motion};
use std::ops::Range;

/// 超過這個行數的文件不做自動換行（每次修改都要重算整份文件的列數）。
pub const MAX_LINES: usize = 100_000;

#[derive(Debug, Default, Clone)]
pub struct WrapMap {
    rev: u64,
    cols: usize,
    chars: usize,
    /// `starts[line]` 是第 `line` 行的第一個畫面列的編號；最後多一項是總列數。
    starts: Vec<usize>,
}

impl WrapMap {
    /// 以 `cols` 為折行寬度，讓累計表符合目前的文字。內容與寬度都沒變就什麼都不做。
    pub fn ensure(&mut self, buf: &Buffer, cols: usize) {
        let n = buf.line_count();
        if self.starts.len() == n + 1
            && self.rev == buf.revision()
            && self.cols == cols
            && self.chars == buf.len_chars()
        {
            return;
        }
        self.rev = buf.revision();
        self.cols = cols;
        self.chars = buf.len_chars();
        self.starts.clear();
        self.starts.reserve(n + 1);
        let mut rows = 0;
        for line in 0..n {
            self.starts.push(rows);
            rows += row_count(buf, line, cols);
        }
        self.starts.push(rows);
    }

    pub fn total_rows(&self) -> usize {
        self.starts.last().copied().unwrap_or(0)
    }

    /// 第 `line` 行第一個畫面列的編號。
    pub fn first_row(&self, line: usize) -> usize {
        self.starts[line.min(self.starts.len() - 1)]
    }

    /// 畫面列 `row` 屬於哪一行、是那一行的第幾列。
    pub fn locate(&self, row: usize) -> (usize, usize) {
        let lines = self.starts.len().saturating_sub(1);
        if lines == 0 {
            return (0, 0);
        }
        let line = (self.starts.partition_point(|s| *s <= row).saturating_sub(1)).min(lines - 1);
        (line, row.saturating_sub(self.starts[line]))
    }

    /// 組字文字插進游標所在行之後，那一行的列數可能變多：把後面的列都往後挪。
    pub fn with_extra_rows(&self, line: usize, extra: usize) -> WrapMap {
        let mut m = self.clone();
        for s in m.starts.iter_mut().skip(line + 1) {
            *s += extra;
        }
        m
    }
}

/// 某一行折成幾列。短的行不必讀文字內容。
pub fn row_count(buf: &Buffer, line: usize, cols: usize) -> usize {
    // 每個字元最多佔 TAB_WIDTH 欄，字數乘上它還放得下就一定只有一列
    if buf.line_len_chars(line) * motion::TAB_WIDTH <= cols {
        1
    } else {
        motion::wrap_rows(&buf.line_text(line), cols).len()
    }
}

/// 某一行每一列的字元範圍（相對於行首）。
pub fn rows_of(buf: &Buffer, line: usize, cols: usize) -> Vec<Range<usize>> {
    motion::wrap_rows(&buf.line_text(line), cols)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_counts_rows_and_locates_them() {
        // 6 欄：第 1 行折 3 列，第 2 行（空）1 列，第 3 行 1 列
        let b = Buffer::from_text("aaaaaaaaaaaaaaa\n\nbb");
        let mut m = WrapMap::default();
        m.ensure(&b, 6);
        assert_eq!(m.total_rows(), 3 + 1 + 1);
        assert_eq!((m.first_row(0), m.first_row(1), m.first_row(2)), (0, 3, 4));
        assert_eq!(m.locate(0), (0, 0));
        assert_eq!(m.locate(2), (0, 2));
        assert_eq!(m.locate(3), (1, 0));
        assert_eq!(m.locate(4), (2, 0));
        assert_eq!(m.locate(99), (2, 95), "超出範圍時停在最後一行");
    }

    #[test]
    fn map_is_rebuilt_when_text_or_width_changes() {
        let mut b = Buffer::from_text("aaaaaaaaaa");
        let mut m = WrapMap::default();
        m.ensure(&b, 5);
        assert_eq!(m.total_rows(), 2);
        m.ensure(&b, 10);
        assert_eq!(m.total_rows(), 1, "寬度改變");
        b.insert(0, "bbbbbbbbbb");
        m.ensure(&b, 10);
        assert_eq!(m.total_rows(), 2, "內容改變");
    }

    #[test]
    fn extra_rows_shift_the_following_lines() {
        let b = Buffer::from_text("a\nb\nc");
        let mut m = WrapMap::default();
        m.ensure(&b, 10);
        let shifted = m.with_extra_rows(0, 2);
        assert_eq!(shifted.first_row(1), 3);
        assert_eq!(shifted.first_row(0), 0);
        assert_eq!(shifted.total_rows(), 5);
    }

    #[test]
    fn rows_of_matches_row_count() {
        let b = Buffer::from_text("一二三四五六七八九十\nab");
        for cols in 2..20 {
            for line in 0..2 {
                assert_eq!(rows_of(&b, line, cols).len(), row_count(&b, line, cols));
            }
        }
    }
}
