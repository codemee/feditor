//! 窗格版面：把編輯區切成好幾個窗格，每個窗格有自己的分頁列。
//!
//! 版面是一棵二元樹：葉子是窗格，節點是「左右」或「上下」的分割。這裡只處理版面本身
//! （切割、關閉、算每個窗格的位置、拖曳分隔線、拖曳分頁時的放置區），與分頁內容、畫面無關，
//! 所以可以單元測試。

use eframe::egui::{Pos2, Rect, Vec2, pos2, vec2};
use std::collections::HashMap;

pub type PaneId = u32;

/// 窗格最小的大小（含分頁列）。分隔線拖不過這個大小，太小的窗格也不能再切。
pub const MIN_PANE: Vec2 = vec2(240.0, 140.0);
/// 兩個窗格之間的空隙（也是分隔線可以抓取的寬度）。窗格是圓角卡片，不能緊貼在一起。
pub const DIVIDER: f32 = 8.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Axis {
    /// 左右並排：第一個在左邊。
    Horizontal,
    /// 上下堆疊：第一個在上面。
    Vertical,
}

/// 把分頁拖到窗格的哪一側，就往那個方向切出新窗格。（沒有「上側」。）
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Side {
    Left,
    Right,
    Bottom,
}

/// 拖曳分頁時，滑鼠所在位置的放置區。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Zone {
    /// 往這一側切出新窗格。
    Split(Side),
    /// 放進這個窗格（成為它的分頁）。
    Into,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Node {
    Leaf(PaneId),
    Split {
        axis: Axis,
        /// 第一個子節點佔的比例（0～1，不含分隔線）。
        ratio: f32,
        first: Box<Node>,
        second: Box<Node>,
    },
}

/// 一條分隔線：位置與它所屬的分割節點（用從根走下來的路徑表示：`false` 第一個、`true` 第二個）。
#[derive(Clone, Debug, PartialEq)]
pub struct Divider {
    pub path: Vec<bool>,
    pub axis: Axis,
    pub rect: Rect,
    /// 這個分割節點整體佔的範圍，拖曳時用它換算比例。
    pub area: Rect,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Arranged {
    pub panes: Vec<(PaneId, Rect)>,
    pub dividers: Vec<Divider>,
}

impl Node {
    /// 所有窗格，依畫面上由左到右、由上到下的順序。
    pub fn leaves(&self) -> Vec<PaneId> {
        match self {
            Node::Leaf(id) => vec![*id],
            Node::Split { first, second, .. } => {
                let mut v = first.leaves();
                v.extend(second.leaves());
                v
            }
        }
    }

    pub fn contains(&self, id: PaneId) -> bool {
        self.leaves().contains(&id)
    }

    /// 把窗格 `target` 往 `side` 切開，新窗格 `new_id` 放在那一側。找不到 `target` 回傳 `false`。
    pub fn split(&mut self, target: PaneId, side: Side, new_id: PaneId) -> bool {
        match self {
            Node::Leaf(id) if *id == target => {
                let (axis, first, second) = match side {
                    Side::Left => (Axis::Horizontal, new_id, target),
                    Side::Right => (Axis::Horizontal, target, new_id),
                    Side::Bottom => (Axis::Vertical, target, new_id),
                };
                *self = Node::Split {
                    axis,
                    ratio: 0.5,
                    first: Box::new(Node::Leaf(first)),
                    second: Box::new(Node::Leaf(second)),
                };
                true
            }
            Node::Leaf(_) => false,
            Node::Split { first, second, .. } => first.split(target, side, new_id) || second.split(target, side, new_id),
        }
    }

    /// 移除窗格 `id`，它的兄弟節點補上它父節點的位置。回傳離它最近的窗格（內容要交給它）；
    /// 找不到，或它是唯一的窗格（不能移除）時回傳 `None`。
    pub fn remove(&mut self, id: PaneId) -> Option<PaneId> {
        let Node::Split { first, second, .. } = self else {
            return None;
        };
        if matches!(**first, Node::Leaf(x) if x == id) {
            let sibling = (**second).clone();
            let near = sibling.nearest_leaf(true);
            *self = sibling;
            return Some(near);
        }
        if matches!(**second, Node::Leaf(x) if x == id) {
            let sibling = (**first).clone();
            let near = sibling.nearest_leaf(false);
            *self = sibling;
            return Some(near);
        }
        first.remove(id).or_else(|| second.remove(id))
    }

    /// 子樹裡最靠近某一端的窗格：`from_start` 為真取最左／最上，否則取最右／最下。
    fn nearest_leaf(&self, from_start: bool) -> PaneId {
        match self {
            Node::Leaf(id) => *id,
            Node::Split { first, second, .. } => {
                if from_start { first.nearest_leaf(true) } else { second.nearest_leaf(false) }
            }
        }
    }

    /// 這棵子樹沿著 `axis` 方向至少要多大。
    fn min_extent(&self, axis: Axis, min: Vec2) -> f32 {
        match self {
            Node::Leaf(_) => match axis {
                Axis::Horizontal => min.x,
                Axis::Vertical => min.y,
            },
            Node::Split { axis: a, first, second, .. } => {
                let (x, y) = (first.min_extent(axis, min), second.min_extent(axis, min));
                if *a == axis { x + y + DIVIDER } else { x.max(y) }
            }
        }
    }

    /// 算出每個窗格與分隔線的位置。窗格不會小於 `min`（除非整個區域本來就放不下，那時按比例縮小）。
    pub fn arrange(&self, area: Rect, min: Vec2) -> Arranged {
        let mut out = Arranged::default();
        self.arrange_into(area, min, &mut Vec::new(), &mut out);
        out
    }

    fn arrange_into(&self, area: Rect, min: Vec2, path: &mut Vec<bool>, out: &mut Arranged) {
        match self {
            Node::Leaf(id) => out.panes.push((*id, area)),
            Node::Split { axis, ratio, first, second } => {
                let extent = match axis {
                    Axis::Horizontal => area.width(),
                    Axis::Vertical => area.height(),
                };
                let total = (extent - DIVIDER).max(0.0);
                let (min_a, min_b) = (first.min_extent(*axis, min), second.min_extent(*axis, min));
                let a = if total >= min_a + min_b {
                    (ratio * total).clamp(min_a, total - min_b)
                } else {
                    total * min_a / (min_a + min_b)
                };
                let (rect_a, bar, rect_b) = match axis {
                    Axis::Horizontal => (
                        Rect::from_min_max(area.min, pos2(area.min.x + a, area.max.y)),
                        Rect::from_min_max(pos2(area.min.x + a, area.min.y), pos2(area.min.x + a + DIVIDER, area.max.y)),
                        Rect::from_min_max(pos2(area.min.x + a + DIVIDER, area.min.y), area.max),
                    ),
                    Axis::Vertical => (
                        Rect::from_min_max(area.min, pos2(area.max.x, area.min.y + a)),
                        Rect::from_min_max(pos2(area.min.x, area.min.y + a), pos2(area.max.x, area.min.y + a + DIVIDER)),
                        Rect::from_min_max(pos2(area.min.x, area.min.y + a + DIVIDER), area.max),
                    ),
                };
                out.dividers.push(Divider { path: path.clone(), axis: *axis, rect: bar, area });
                path.push(false);
                first.arrange_into(rect_a, min, path, out);
                path.pop();
                path.push(true);
                second.arrange_into(rect_b, min, path, out);
                path.pop();
            }
        }
    }

    /// 設定 `path` 指到的分割節點的比例。
    pub fn set_ratio(&mut self, path: &[bool], value: f32) {
        if let Node::Split { ratio, first, second, .. } = self {
            match path.split_first() {
                None => *ratio = value.clamp(0.05, 0.95),
                Some((false, rest)) => first.set_ratio(rest, value),
                Some((true, rest)) => second.set_ratio(rest, value),
            }
        }
    }
}

impl Divider {
    /// 滑鼠拖到 `pointer` 時，這個分割節點該有的比例。
    pub fn ratio_at(&self, pointer: Pos2) -> f32 {
        let (pos, start, extent) = match self.axis {
            Axis::Horizontal => (pointer.x, self.area.min.x, self.area.width()),
            Axis::Vertical => (pointer.y, self.area.min.y, self.area.height()),
        };
        let total = (extent - DIVIDER).max(1.0);
        ((pos - start - DIVIDER / 2.0) / total).clamp(0.0, 1.0)
    }
}

/// 窗格 `rect` 能不能往 `side` 再切一刀：切完兩邊都不能小於 [`MIN_PANE`]。
pub fn can_split(rect: Rect, side: Side) -> bool {
    match side {
        Side::Left | Side::Right => rect.width() >= 2.0 * MIN_PANE.x + DIVIDER,
        Side::Bottom => rect.height() >= 2.0 * MIN_PANE.y + DIVIDER,
    }
}

/// 拖曳分頁時，滑鼠在窗格 `rect` 的 `pos` 位置對應的放置區。
/// 分頁列（最上面 `tab_bar_h` 高）一律是「放進窗格」；內容區的左右各 1/4、下方 1/3 是切割區，
/// 其餘是「放進窗格」。切了會讓窗格太小的那一側不提供切割。
pub fn drop_zone(rect: Rect, tab_bar_h: f32, pos: Pos2) -> Zone {
    if pos.y < rect.min.y + tab_bar_h {
        return Zone::Into;
    }
    let body = Rect::from_min_max(pos2(rect.min.x, rect.min.y + tab_bar_h), rect.max);
    let fx = (pos.x - body.min.x) / body.width().max(1.0);
    let fy = (pos.y - body.min.y) / body.height().max(1.0);
    let side = if fx < 0.25 {
        Some(Side::Left)
    } else if fx > 0.75 {
        Some(Side::Right)
    } else if fy > 2.0 / 3.0 {
        Some(Side::Bottom)
    } else {
        None
    };
    match side {
        Some(s) if can_split(rect, s) => Zone::Split(s),
        _ => Zone::Into,
    }
}

/// 放開滑鼠後新窗格會佔的範圍，畫成預覽。
pub fn preview_rect(rect: Rect, zone: Zone) -> Rect {
    match zone {
        Zone::Into => rect,
        Zone::Split(Side::Left) => Rect::from_min_max(rect.min, pos2(rect.center().x, rect.max.y)),
        Zone::Split(Side::Right) => Rect::from_min_max(pos2(rect.center().x, rect.min.y), rect.max),
        Zone::Split(Side::Bottom) => Rect::from_min_max(pos2(rect.min.x, rect.center().y), rect.max),
    }
}

/// 版面加上各個窗格的內容。`T` 是窗格的內容；目前有焦點的那個窗格的內容不放在這裡
/// （由呼叫端另外持有），所以 `others` 只有沒焦點的窗格。
pub struct Layout<T> {
    pub root: Node,
    pub others: HashMap<PaneId, T>,
    pub focus: PaneId,
    next_id: PaneId,
}

impl<T> Default for Layout<T> {
    fn default() -> Self {
        Self { root: Node::Leaf(0), others: HashMap::new(), focus: 0, next_id: 1 }
    }
}

impl<T> Layout<T> {
    pub fn alloc_id(&mut self) -> PaneId {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    pub fn pane_count(&self) -> usize {
        self.root.leaves().len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area(w: f32, h: f32) -> Rect {
        Rect::from_min_size(pos2(0.0, 0.0), vec2(w, h))
    }

    fn rect_of(a: &Arranged, id: PaneId) -> Rect {
        a.panes.iter().find(|(i, _)| *i == id).unwrap().1
    }

    #[test]
    fn splitting_puts_the_new_pane_on_the_chosen_side() {
        for (side, expect_leaves, axis) in [
            (Side::Left, vec![1, 0], Axis::Horizontal),
            (Side::Right, vec![0, 1], Axis::Horizontal),
            (Side::Bottom, vec![0, 1], Axis::Vertical),
        ] {
            let mut root = Node::Leaf(0);
            assert!(root.split(0, side, 1));
            assert_eq!(root.leaves(), expect_leaves, "{side:?}");
            assert!(matches!(root, Node::Split { axis: a, .. } if a == axis));
        }
        let mut root = Node::Leaf(0);
        assert!(!root.split(9, Side::Left, 1), "不存在的窗格切不了");
    }

    #[test]
    fn splits_nest_in_any_direction() {
        let mut root = Node::Leaf(0);
        root.split(0, Side::Right, 1);
        root.split(1, Side::Bottom, 2);
        root.split(0, Side::Left, 3);
        assert_eq!(root.leaves(), vec![3, 0, 1, 2]);
        let a = root.arrange(area(1000.0, 600.0), MIN_PANE);
        assert_eq!(a.panes.len(), 4);
        assert_eq!(a.dividers.len(), 3);
        // 窗格互不重疊，也都在區域內
        for (i, (_, r)) in a.panes.iter().enumerate() {
            assert!(area(1000.0, 600.0).contains_rect(*r));
            for (_, other) in &a.panes[i + 1..] {
                assert!(!r.shrink(0.5).intersects(other.shrink(0.5)), "{r:?} 與 {other:?} 重疊");
            }
        }
    }

    #[test]
    fn removing_a_pane_gives_its_space_to_the_sibling_and_names_the_neighbour() {
        let mut root = Node::Leaf(0);
        root.split(0, Side::Right, 1);
        root.split(1, Side::Bottom, 2);
        // 拿掉 1（右上）：兄弟 2（右下）補位；1 在 first，最近的是兄弟的第一個
        assert_eq!(root.remove(1), Some(2));
        assert_eq!(root.leaves(), vec![0, 2]);
        // 拿掉 0：兄弟 2 成為唯一的窗格
        assert_eq!(root.remove(0), Some(2));
        assert_eq!(root, Node::Leaf(2));
        assert_eq!(root.remove(2), None, "唯一的窗格不能移除");
        assert_eq!(Node::Leaf(5).remove(7), None);
    }

    #[test]
    fn removing_the_second_child_names_the_last_leaf_of_the_first() {
        let mut root = Node::Leaf(0);
        root.split(0, Side::Right, 1); // [0 | 1]
        root.split(0, Side::Bottom, 2); // [(0 / 2) | 1]
        assert_eq!(root.remove(1), Some(2), "左邊子樹裡離它最近的是最右／最下的 2");
    }

    #[test]
    fn panes_never_get_smaller_than_the_minimum() {
        let mut root = Node::Leaf(0);
        root.split(0, Side::Right, 1);
        // 拖到極端比例
        for ratio in [0.0, 0.01, 0.99, 1.0] {
            root.set_ratio(&[], ratio);
            let a = root.arrange(area(800.0, 500.0), MIN_PANE);
            for (_, r) in &a.panes {
                assert!(r.width() >= MIN_PANE.x - 0.01, "比例 {ratio}：寬 {}", r.width());
            }
            let total: f32 = a.panes.iter().map(|(_, r)| r.width()).sum::<f32>() + DIVIDER;
            assert!((total - 800.0).abs() < 0.01, "窗格加分隔線要剛好填滿");
        }
    }

    #[test]
    fn a_too_small_area_shrinks_panes_proportionally_instead_of_overlapping() {
        let mut root = Node::Leaf(0);
        root.split(0, Side::Right, 1);
        let a = root.arrange(area(300.0, 400.0), MIN_PANE);
        let (r0, r1) = (rect_of(&a, 0), rect_of(&a, 1));
        assert!(r0.max.x <= r1.min.x);
        assert!(r0.width() > 0.0 && r1.width() > 0.0);
    }

    #[test]
    fn nested_minimums_add_up() {
        // 左邊一個窗格，右邊上下兩個：右邊的最小寬度還是一個窗格寬，不是兩倍
        let mut root = Node::Leaf(0);
        root.split(0, Side::Right, 1);
        root.split(1, Side::Bottom, 2);
        let a = root.arrange(area(2.0 * MIN_PANE.x + DIVIDER, 2.0 * MIN_PANE.y + DIVIDER), MIN_PANE);
        for (_, r) in &a.panes {
            assert!(r.width() >= MIN_PANE.x - 0.01 && r.height() >= MIN_PANE.y - 0.01, "{r:?}");
        }
    }

    #[test]
    fn dragging_a_divider_changes_the_ratio_and_stays_within_limits() {
        let mut root = Node::Leaf(0);
        root.split(0, Side::Right, 1);
        let a = root.arrange(area(1005.0, 500.0), MIN_PANE);
        let d = &a.dividers[0];
        assert_eq!(d.axis, Axis::Horizontal);
        let ratio = d.ratio_at(pos2(250.0 + DIVIDER / 2.0, 100.0));
        assert!((ratio - 0.25).abs() < 0.001, "{ratio}");
        root.set_ratio(&d.path, ratio);
        let b = root.arrange(area(1005.0, 500.0), MIN_PANE);
        assert!((rect_of(&b, 0).width() - 250.0).abs() < 0.01);
        // 拖出視窗外也不會超過範圍
        root.set_ratio(&d.path, d.ratio_at(pos2(-500.0, 0.0)));
        let c = root.arrange(area(1005.0, 500.0), MIN_PANE);
        assert!(rect_of(&c, 0).width() >= MIN_PANE.x - 0.01);
    }

    #[test]
    fn set_ratio_follows_the_path_into_nested_splits() {
        let mut root = Node::Leaf(0);
        root.split(0, Side::Right, 1);
        root.split(1, Side::Bottom, 2);
        let a = root.arrange(area(1000.0, 800.0), MIN_PANE);
        let inner = a.dividers.iter().find(|d| d.axis == Axis::Vertical).unwrap().clone();
        assert_eq!(inner.path, vec![true]);
        root.set_ratio(&inner.path, 0.25);
        let b = root.arrange(area(1000.0, 800.0), MIN_PANE);
        let (top, bottom) = (rect_of(&b, 1), rect_of(&b, 2));
        assert!(top.height() < bottom.height());
    }

    #[test]
    fn small_panes_cannot_be_split_further() {
        let big = area(2.0 * MIN_PANE.x + DIVIDER, 2.0 * MIN_PANE.y + DIVIDER);
        assert!(can_split(big, Side::Left) && can_split(big, Side::Right) && can_split(big, Side::Bottom));
        let narrow = area(2.0 * MIN_PANE.x, 1000.0);
        assert!(!can_split(narrow, Side::Left) && !can_split(narrow, Side::Right));
        assert!(can_split(narrow, Side::Bottom));
        let short = area(1000.0, 2.0 * MIN_PANE.y);
        assert!(!can_split(short, Side::Bottom) && can_split(short, Side::Left));
    }

    #[test]
    fn drop_zones_are_the_left_right_and_bottom_edges_with_the_tab_bar_always_inside() {
        let r = area(800.0, 600.0);
        let bar = 28.0;
        let z = |x: f32, y: f32| drop_zone(r, bar, pos2(x, y));
        assert_eq!(z(10.0, 300.0), Zone::Split(Side::Left));
        assert_eq!(z(790.0, 300.0), Zone::Split(Side::Right));
        assert_eq!(z(400.0, 590.0), Zone::Split(Side::Bottom));
        assert_eq!(z(400.0, 300.0), Zone::Into, "中間是放進窗格");
        assert_eq!(z(400.0, 100.0), Zone::Into, "上方沒有切割區");
        assert_eq!(z(10.0, 5.0), Zone::Into, "分頁列一律是放進窗格");
        assert_eq!(z(790.0, 20.0), Zone::Into);
        // 左右與下方重疊的角落：左右優先
        assert_eq!(z(10.0, 590.0), Zone::Split(Side::Left));
    }

    #[test]
    fn drop_zones_disappear_when_the_pane_is_too_small_to_split() {
        let narrow = area(2.0 * MIN_PANE.x, 600.0);
        assert_eq!(drop_zone(narrow, 28.0, pos2(5.0, 300.0)), Zone::Into);
        assert_eq!(drop_zone(narrow, 28.0, pos2(narrow.width() / 2.0, 590.0)), Zone::Split(Side::Bottom));
    }

    #[test]
    fn preview_covers_the_half_the_new_pane_will_take() {
        let r = area(800.0, 600.0);
        assert_eq!(preview_rect(r, Zone::Split(Side::Left)).width(), 400.0);
        assert_eq!(preview_rect(r, Zone::Split(Side::Right)).min.x, 400.0);
        assert_eq!(preview_rect(r, Zone::Split(Side::Bottom)).min.y, 300.0);
        assert_eq!(preview_rect(r, Zone::Into), r);
    }

    #[test]
    fn layout_hands_out_unique_ids() {
        let mut l: Layout<()> = Layout::default();
        assert_eq!((l.focus, l.pane_count()), (0, 1));
        let (a, b) = (l.alloc_id(), l.alloc_id());
        assert_ne!(a, b);
        assert!(a != 0 && b != 0);
        l.root.split(0, Side::Right, a);
        assert_eq!(l.pane_count(), 2);
    }
}
