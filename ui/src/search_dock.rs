//! 尋找面板的停靠位置：只能停在視窗四個邊緣之一，並且佔滿那一邊（左右邊佔滿整個高度、上下邊佔滿整個寬度），
//! 剩下的空間才是其他窗格（編輯區、預覽…）的。它不在窗格版面樹裡，預設停在右邊。
//!
//! 拖面板的標題可以換邊：拖曳中畫出放開後的位置，放開就停靠過去；面板與其他窗格之間的分隔條可以拖來調整大小。

use crate::pane_ops::CARD_INSET;
use crate::panes::{DIVIDER, MIN_PANE};
use crate::{Action, App};
use eframe::egui::{self, Rect, pos2};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DockEdge {
    Left,
    Right,
    Top,
    Bottom,
}

/// 左右邊停靠時面板至少多寬、上下邊停靠時至少多高（再小面板裡的東西就放不下了）。
pub const MIN_WIDTH: f32 = 260.0;
pub const MIN_HEIGHT: f32 = 170.0;
/// 預設的寬度（左右邊停靠）與高度（上下邊停靠）。
pub const DEFAULT_WIDTH: f32 = 380.0;
pub const DEFAULT_HEIGHT: f32 = 280.0;

impl DockEdge {
    /// 設定檔裡的名稱。
    pub fn name(self) -> &'static str {
        match self {
            DockEdge::Left => "left",
            DockEdge::Right => "right",
            DockEdge::Top => "top",
            DockEdge::Bottom => "bottom",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        [DockEdge::Left, DockEdge::Right, DockEdge::Top, DockEdge::Bottom]
            .into_iter()
            .find(|e| e.name().eq_ignore_ascii_case(s.trim()))
    }
}

pub struct SearchDock {
    pub edge: DockEdge,
    /// 停在左右邊時的寬度、停在上下邊時的高度（各自記住，換邊時不用重新調）。
    pub width: f32,
    pub height: f32,
    /// 正在拖標題換邊。
    pub dragging: bool,
}

impl Default for SearchDock {
    fn default() -> Self {
        Self { edge: DockEdge::Right, width: DEFAULT_WIDTH, height: DEFAULT_HEIGHT, dragging: false }
    }
}

impl SearchDock {
    /// 用設定檔記住的位置與大小。
    pub fn from_settings(s: &crate::settings::Settings) -> Self {
        Self { edge: s.search_dock_edge, width: s.search_dock_width, height: s.search_dock_height, dragging: false }
    }
}

/// 面板與它和其他窗格之間的分隔條所在的位置。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DockGeom {
    pub rect: Rect,
    pub divider: Rect,
}

impl SearchDock {
    /// 面板停在 `edge` 時，在 `area` 裡佔的範圍、分隔條，以及剩給其他窗格的範圍。
    pub fn place(&self, edge: DockEdge, area: Rect) -> (DockGeom, Rect) {
        let width = self.width.clamp(MIN_WIDTH, (area.width() - MIN_PANE.x - DIVIDER).max(MIN_WIDTH));
        let height = self.height.clamp(MIN_HEIGHT, (area.height() - MIN_PANE.y - DIVIDER).max(MIN_HEIGHT));
        let (rect, divider, rest) = match edge {
            DockEdge::Right => {
                let rect = Rect::from_min_max(pos2(area.right() - width, area.top()), area.max);
                let divider = Rect::from_min_max(pos2(rect.left() - DIVIDER, area.top()), pos2(rect.left(), area.bottom()));
                (rect, divider, Rect::from_min_max(area.min, pos2(divider.left(), area.bottom())))
            }
            DockEdge::Left => {
                let rect = Rect::from_min_max(area.min, pos2(area.left() + width, area.bottom()));
                let divider = Rect::from_min_max(pos2(rect.right(), area.top()), pos2(rect.right() + DIVIDER, area.bottom()));
                (rect, divider, Rect::from_min_max(pos2(divider.right(), area.top()), area.max))
            }
            DockEdge::Bottom => {
                let rect = Rect::from_min_max(pos2(area.left(), area.bottom() - height), area.max);
                let divider = Rect::from_min_max(pos2(area.left(), rect.top() - DIVIDER), pos2(area.right(), rect.top()));
                (rect, divider, Rect::from_min_max(area.min, pos2(area.right(), divider.top())))
            }
            DockEdge::Top => {
                let rect = Rect::from_min_max(area.min, pos2(area.right(), area.top() + height));
                let divider = Rect::from_min_max(pos2(area.left(), rect.bottom()), pos2(area.right(), rect.bottom() + DIVIDER));
                (rect, divider, Rect::from_min_max(pos2(area.left(), divider.bottom()), area.max))
            }
        };
        (DockGeom { rect, divider }, rest)
    }

    /// 滑鼠在 `area` 裡的哪個位置，就停靠在離它最近的那一邊。
    pub fn nearest_edge(area: Rect, p: egui::Pos2) -> DockEdge {
        let w = area.width().max(1.0);
        let h = area.height().max(1.0);
        let candidates = [
            (DockEdge::Left, (p.x - area.left()) / w),
            (DockEdge::Right, (area.right() - p.x) / w),
            (DockEdge::Top, (p.y - area.top()) / h),
            (DockEdge::Bottom, (area.bottom() - p.y) / h),
        ];
        candidates.into_iter().min_by(|a, b| a.1.total_cmp(&b.1)).map_or(DockEdge::Right, |c| c.0)
    }

    /// 分隔條被拖到 `p`：換算成面板的新寬度（或高度）。
    fn resize_to(&mut self, area: Rect, p: egui::Pos2) {
        let half = DIVIDER / 2.0;
        match self.edge {
            DockEdge::Right => self.width = area.right() - p.x - half,
            DockEdge::Left => self.width = p.x - area.left() - half,
            DockEdge::Bottom => self.height = area.bottom() - p.y - half,
            DockEdge::Top => self.height = p.y - area.top() - half,
        }
    }
}

impl App {
    /// 把窗格區域 `area` 分成尋找面板與剩下的空間（面板沒開就整塊都是剩下的）。
    pub(crate) fn split_search_dock(&self, area: Rect) -> (Option<DockGeom>, Rect) {
        if !self.folder.open {
            return (None, area);
        }
        let (geom, rest) = self.dock.place(self.dock.edge, area);
        (Some(geom), rest)
    }

    /// 畫尋找面板（卡片、內容、與其他窗格之間的分隔條），以及拖標題換邊時的放置預覽。
    /// `full` 是分出面板之前的整個窗格區域。回傳面板裡產生的動作。
    pub(crate) fn show_search_dock(&mut self, ui: &mut egui::Ui, geom: Option<DockGeom>, full: Rect) -> Option<Action> {
        let ctx = ui.ctx().clone();
        let mut action = None;
        if let Some(g) = geom {
            let (fill, stroke) = {
                let v = ui.visuals();
                (v.panel_fill, egui::Stroke::new(1.0, v.widgets.noninteractive.bg_stroke.color))
            };
            let radius = egui::CornerRadius::same(8);
            ui.painter().rect_filled(g.rect, radius, fill);
            ui.painter().rect_stroke(g.rect, radius, stroke, egui::StrokeKind::Inside);
            let mut child = ui.new_child(
                egui::UiBuilder::new()
                    .id_salt("search_dock")
                    .max_rect(g.rect)
                    .layout(egui::Layout::top_down(egui::Align::Min)),
            );
            child.set_clip_rect(g.rect.intersect(ui.clip_rect()));
            if let Some(a) = self.show_search_pane(&mut child, g.rect) {
                action = Some(a);
            }

            // 與其他窗格之間的分隔條：拖曳調整面板的大小
            let resp = ui.interact(g.divider, egui::Id::unique("search_dock_divider"), egui::Sense::drag());
            let vertical_bar = matches!(self.dock.edge, DockEdge::Left | DockEdge::Right);
            if resp.hovered() || resp.dragged() {
                ctx.set_cursor_icon(if vertical_bar {
                    egui::CursorIcon::ResizeHorizontal
                } else {
                    egui::CursorIcon::ResizeVertical
                });
                let grip = if vertical_bar {
                    egui::vec2(3.0, (g.divider.height() * 0.3).clamp(24.0, 80.0))
                } else {
                    egui::vec2((g.divider.width() * 0.3).clamp(24.0, 80.0), 3.0)
                };
                ui.painter().rect_filled(
                    Rect::from_center_size(g.divider.center(), grip),
                    egui::CornerRadius::same(2),
                    ui.visuals().selection.bg_fill,
                );
            }
            if resp.dragged() {
                if let Some(p) = resp.interact_pointer_pos() {
                    self.dock.resize_to(full, p);
                }
                ctx.request_repaint();
            }
        }

        if self.dock.dragging {
            ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
            let edge = ctx
                .input(|i| i.pointer.interact_pos())
                .map_or(self.dock.edge, |p| SearchDock::nearest_edge(full, p));
            let (target, _) = self.dock.place(edge, full);
            let accent = ui.visuals().selection.bg_fill;
            let layer = egui::LayerId::new(egui::Order::Foreground, egui::Id::unique("search_dock_drag"));
            let painter = ctx.layer_painter(layer);
            let preview = target.rect.shrink(CARD_INSET / 2.0);
            painter.rect_filled(preview, egui::CornerRadius::same(6), accent.gamma_multiply(0.35));
            painter.rect_stroke(preview, egui::CornerRadius::same(6), egui::Stroke::new(2.0, accent), egui::StrokeKind::Inside);
            if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                self.dock.dragging = false;
            } else if !ctx.input(|i| i.pointer.any_down()) {
                self.dock.edge = edge;
                self.dock.dragging = false;
            }
            ctx.request_repaint();
        }
        action
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area() -> Rect {
        Rect::from_min_max(pos2(0.0, 0.0), pos2(1000.0, 600.0))
    }

    #[test]
    fn the_dock_takes_a_whole_edge_and_the_rest_is_left_for_the_panes() {
        let dock = SearchDock::default();
        for edge in [DockEdge::Left, DockEdge::Right, DockEdge::Top, DockEdge::Bottom] {
            let (geom, rest) = dock.place(edge, area());
            // 面板與剩下的空間加上分隔條剛好鋪滿整個區域，沒有重疊
            let total = geom.rect.area() + geom.divider.area() + rest.area();
            assert!((total - area().area()).abs() < 1.0, "{edge:?}");
            // 三塊緊鄰（只共用邊界），沒有互相重疊
            let overlap = geom.rect.intersect(rest);
            assert!(overlap.width() <= 0.0 || overlap.height() <= 0.0, "{edge:?}");
            match edge {
                DockEdge::Left | DockEdge::Right => assert_eq!(geom.rect.height(), 600.0, "{edge:?} 佔滿整個高度"),
                _ => assert_eq!(geom.rect.width(), 1000.0, "{edge:?} 佔滿整個寬度"),
            }
        }
        assert_eq!(dock.place(DockEdge::Right, area()).0.rect.right(), 1000.0);
        assert_eq!(dock.place(DockEdge::Left, area()).0.rect.left(), 0.0);
    }

    #[test]
    fn the_dock_keeps_a_minimum_size_and_leaves_the_panes_theirs() {
        let mut dock = SearchDock::default();
        dock.width = 10.0;
        assert_eq!(dock.place(DockEdge::Right, area()).0.rect.width(), MIN_WIDTH);
        dock.width = 5000.0;
        let (geom, rest) = dock.place(DockEdge::Right, area());
        assert!(rest.width() >= MIN_PANE.x - 0.5, "其他窗格至少留得下最小寬度：{}", rest.width());
        assert!(geom.rect.width() >= MIN_WIDTH);
    }

    #[test]
    fn dragging_the_divider_changes_the_size_for_every_edge() {
        let mut dock = SearchDock::default();
        dock.edge = DockEdge::Right;
        dock.resize_to(area(), pos2(600.0, 300.0));
        assert!((dock.width - (400.0 - DIVIDER / 2.0)).abs() < 0.01);
        dock.edge = DockEdge::Left;
        dock.resize_to(area(), pos2(300.0, 300.0));
        assert!((dock.width - (300.0 - DIVIDER / 2.0)).abs() < 0.01);
        dock.edge = DockEdge::Bottom;
        dock.resize_to(area(), pos2(500.0, 400.0));
        assert!((dock.height - (200.0 - DIVIDER / 2.0)).abs() < 0.01);
        dock.edge = DockEdge::Top;
        dock.resize_to(area(), pos2(500.0, 150.0));
        assert!((dock.height - (150.0 - DIVIDER / 2.0)).abs() < 0.01);
    }

    #[test]
    fn edge_names_round_trip() {
        for e in [DockEdge::Left, DockEdge::Right, DockEdge::Top, DockEdge::Bottom] {
            assert_eq!(DockEdge::parse(e.name()), Some(e));
        }
        assert_eq!(DockEdge::parse(" TOP "), Some(DockEdge::Top));
        assert_eq!(DockEdge::parse("middle"), None);
    }

    #[test]
    fn the_pointer_picks_the_nearest_window_edge() {
        let a = area();
        assert_eq!(SearchDock::nearest_edge(a, pos2(20.0, 300.0)), DockEdge::Left);
        assert_eq!(SearchDock::nearest_edge(a, pos2(980.0, 300.0)), DockEdge::Right);
        assert_eq!(SearchDock::nearest_edge(a, pos2(500.0, 10.0)), DockEdge::Top);
        assert_eq!(SearchDock::nearest_edge(a, pos2(500.0, 590.0)), DockEdge::Bottom);
    }
}
