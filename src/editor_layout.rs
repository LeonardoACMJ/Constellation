//! Pure editor layout, independent of native windows and their current pixels.
pub const MIN_WIDTH: i32 = 1050;
pub const MIN_HEIGHT: i32 = 996;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}
/// Shared with the native editor and layout checks.
pub const STARTUP_CONTROL: Rect = Rect {
    x: 20,
    y: 766,
    w: 260,
    h: 28,
};
pub const START_MENU_CONTROL: Rect = Rect {
    x: 20,
    y: 802,
    w: 260,
    h: 28,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layout {
    pub page_width: i32,
    pub page_height: i32,
    pub width: i32,
    pub height: i32,
    pub horizontal: bool,
    pub vertical: bool,
    pub side_width: i32,
    pub item_x: i32,
    pub item_width: i32,
    pub preview_x: i32,
}
impl Layout {
    /// Start with the client area as if both scrollbars were hidden. Resolve
    /// their interdependence before sizing anything; never reserve unused bars.
    pub fn new(full_width: i32, full_height: i32, bar_width: i32, bar_height: i32) -> Self {
        let (mut horizontal, mut vertical) = (false, false);
        for _ in 0..3 {
            horizontal = full_width - if vertical { bar_width } else { 0 } < MIN_WIDTH;
            vertical = full_height - if horizontal { bar_height } else { 0 } < MIN_HEIGHT;
        }
        let page_width = (full_width - if vertical { bar_width } else { 0 }).max(1);
        let page_height = (full_height - if horizontal { bar_height } else { 0 }).max(1);
        let width = page_width.max(MIN_WIDTH);
        let height = page_height.max(MIN_HEIGHT);
        let side_width = (width / 5).clamp(260, 360);
        let item_width = (width * 8 / 25).clamp(360, 520);
        let item_x = 20 + side_width + 25;
        let preview_x = item_x + item_width + 35;
        Self {
            page_width,
            page_height,
            width,
            height,
            horizontal,
            vertical,
            side_width,
            item_x,
            item_width,
            preview_x,
        }
    }
    pub fn scroll(&self, x: i32, y: i32) -> (i32, i32) {
        (
            x.clamp(0, self.width - self.page_width),
            y.clamp(0, self.height - self.page_height),
        )
    }
    pub fn preview(&self) -> Rect {
        Rect {
            x: self.preview_x,
            y: 85,
            w: (self.width - self.preview_x - 30).max(1),
            h: self.height - 295,
        }
    }
    pub fn panels(&self) -> [Rect; 3] {
        [
            Rect {
                x: 10,
                y: 8,
                w: self.side_width + 20,
                h: self.height - 118,
            },
            Rect {
                x: self.item_x - 10,
                y: 8,
                w: self.item_width + 25,
                h: self.height - 118,
            },
            Rect {
                x: self.preview_x - 10,
                y: 8,
                w: self.width - self.preview_x - 10,
                h: self.height - 103,
            },
        ]
    }
    /// Coordinates always derive from the original form, never a window's
    /// previously moved bounds. Combo heights retain their dropdown extent.
    pub fn control(&self, base: Rect, id: i32) -> Rect {
        let extra = self.height - MIN_HEIGHT;
        let mut r = base;
        if base.y >= 906 {
            let available = self.item_x + self.item_width - 20;
            r.x = 20 + (base.x - 20) * available / 650;
            r.w = base.w * available / 650;
            r.y += extra;
        } else if base.x < 295 {
            r.x = 20 + (base.x - 20) * self.side_width / 260;
            r.w = (base.w * self.side_width / 260).min(20 + self.side_width - r.x);
            if id == 100 {
                r.h += extra + 38;
            } else if base.y >= 525 {
                r.y += extra + 38;
            }
        } else if base.x < 690 {
            r.x = self.item_x + (base.x - 305) * self.item_width / 360;
            r.w = base.w * self.item_width / 360;
            if base.y >= 638 {
                r.y += extra;
            }
            if id == 214 {
                r.h += extra;
            }
        } else {
            r.x = self.preview_x;
            r.w = self.width - self.preview_x - 30;
        }
        r
    }
}
