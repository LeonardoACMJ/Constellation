//! Headless layout arithmetic only. No windows, input or native UI calls.
use constellation::editor_layout::{Layout, MIN_HEIGHT, MIN_WIDTH, Rect};
#[test]
fn maximized_content_uses_both_dimensions_without_dead_scrollbars() {
    let big = Layout::new(2451, 1080, 17, 17);
    assert!(!big.horizontal && !big.vertical);
    assert_eq!((big.page_width, big.page_height), (2451, 1080));
    assert_eq!(big.scroll(500, 500), (0, 0));
    let small = Layout::new(1380, 870, 17, 17);
    assert!(big.preview().w > small.preview().w && big.preview().h > small.preview().h);
    assert!(big.side_width > small.side_width && big.item_width > small.item_width);
}
#[test]
fn scrollbars_resolve_their_mutual_space_and_hide_at_the_fit_boundary() {
    let fits = Layout::new(MIN_WIDTH, MIN_HEIGHT, 17, 17);
    assert!(!fits.horizontal && !fits.vertical);
    let short = Layout::new(MIN_WIDTH, MIN_HEIGHT - 1, 17, 17);
    assert!(short.horizontal && short.vertical);
    assert_eq!(short.scroll(1000, 1000), (17, 18));
    let narrow = Layout::new(MIN_WIDTH - 1, MIN_HEIGHT, 17, 17);
    assert!(narrow.horizontal && narrow.vertical);
    assert_eq!(narrow.scroll(-10, -10), (0, 0));
}
#[test]
fn layout_keeps_controls_inside_panels_and_retains_combo_dropdown_height() {
    let tree = Rect {
        x: 20,
        y: 45,
        w: 260,
        h: 470,
    };
    let add = Rect {
        x: 20,
        y: 525,
        w: 80,
        h: 30,
    };
    let combo = Rect {
        x: 305,
        y: 129,
        w: 360,
        h: 250,
    };
    let monitor = Rect {
        x: 700,
        y: 40,
        w: 320,
        h: 250,
    };
    for (w, h) in [
        (680, 500),
        (1050, 895),
        (1380, 870),
        (1920, 1050),
        (2451, 1080),
        (3840, 2160),
    ] {
        let layout = Layout::new(w, h, 17, 17);
        let list = layout.control(tree, 100);
        let button = layout.control(add, 302);
        assert!(list.y + list.h < button.y);
        assert_eq!(list.w, layout.side_width);
        let kind = layout.control(combo, 202);
        assert_eq!(kind.h, 250);
        assert_eq!(kind.x, layout.item_x);
        assert!(kind.x + kind.w < layout.preview_x);
        let display = layout.control(monitor, 507);
        assert_eq!(display.x, layout.preview().x);
        assert_eq!(display.w, layout.preview().w);
        assert_eq!(display.h, 250);
        for panel in layout.panels() {
            assert!(
                panel.x >= 0
                    && panel.y >= 0
                    && panel.x + panel.w <= layout.width
                    && panel.y + panel.h <= layout.height
            );
        }
    }
}
