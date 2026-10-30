//! Isolated small-window experiment: fixed footer and independently clipped panes.
use crate::editor_layout::{Layout, MIN_HEIGHT, Rect};
pub const ITEM: i32 = 9000;
pub const SIDE: i32 = 9001;
pub const TOGGLE: i32 = 715;
pub fn form(width: i32, height: i32, barw: i32, barh: i32, preview: bool) -> Layout {
    let min = if preview { 1050 } else { 700 };
    let horizontal = width < min;
    let page_width = width.max(1);
    let page_height = (height - if horizontal { barh } else { 0 }).max(1);
    let width = page_width.max(min);
    let mut f = Layout::new(width, MIN_HEIGHT.max(page_height), barw, barh);
    f.page_width = page_width;
    f.page_height = page_height;
    f.width = width;
    f.height = MIN_HEIGHT.max(page_height);
    f.horizontal = horizontal;
    f.vertical = false;
    if !preview {
        f.side_width = 260;
        f.item_x = 305;
        f.item_width = (width - 340).max(360);
        f.preview_x = width;
    }
    f
}
pub fn pane(f: &Layout, item: bool) -> Rect {
    Rect {
        x: if item { f.item_x - 10 } else { 10 },
        y: 8,
        w: if item {
            f.item_width + 25
        } else {
            f.side_width + 20
        },
        h: (f.page_height - 110).max(40),
    }
}
pub fn footer(f: &Layout, base: Rect) -> Rect {
    let available = (f.page_width - 40).max(500);
    Rect {
        x: 20 + (base.x - 20) * available / 980,
        y: (f.page_height - 76).max(8),
        w: base.w * available / 980,
        h: base.h,
    }
}
pub fn content(f: &Layout, item: bool) -> i32 {
    f.height - if item { 113 } else { 118 }
}
pub fn maximum(f: &Layout, item: bool) -> i32 {
    (content(f, item) - pane(f, item).h).max(0)
}
pub fn item(base: Rect) -> bool {
    (295..690).contains(&base.x) && base.y < 906
}
