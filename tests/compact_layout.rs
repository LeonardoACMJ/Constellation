use constellation::{compact_layout as c, editor_layout::Rect};
#[test]
fn keyboard_reveal_and_wheel_routing_preserve_independent_context() {
    assert_eq!(c::reveal(200, 240, 28, 400, 600), 200);
    assert_eq!(c::reveal(200, 80, 28, 400, 600), 80);
    assert_eq!(c::reveal(0, 700, 28, 400, 600), 328);
    assert_eq!(c::reveal(0, 900, 28, 400, 400), 400);
    let f = c::form(1400, 650, 17, 17, true);
    for item in [false, true] {
        let pane = c::pane(&f, item);
        assert_eq!(c::hit_pane(&f, 0, pane.x + 10, pane.y + 10), Some(item));
        assert_eq!(c::hit_pane(&f, 50, pane.x - 40, pane.y + 10), Some(item));
    }
    assert_eq!(c::hit_pane(&f, 0, f.preview_x + 20, 100), None);
    assert_eq!(c::hit_pane(&f, 0, 20, f.page_height - 30), None);
}
#[test]
fn footer_stays_visible_at_small_and_large_sizes() {
    for (w, h) in [(700, 500), (1050, 700), (1920, 1050)] {
        let f = c::form(w, h, 17, 17, false);
        for (x, width) in [
            (20, 100),
            (130, 100),
            (240, 250),
            (500, 170),
            (690, 150),
            (850, 150),
        ] {
            let r = c::footer(
                &f,
                Rect {
                    x,
                    y: 906,
                    w: width,
                    h: 35,
                },
            );
            assert!(r.y >= 0 && r.y + r.h <= f.page_height);
            assert!(r.x >= 0 && r.x + r.w <= f.page_width);
            assert!(c::pane(&f, true).y + c::pane(&f, true).h < r.y);
        }
    }
}
#[test]
fn preview_can_collapse_without_unnecessary_horizontal_scroll() {
    let full = c::form(800, 650, 17, 17, true);
    let collapsed = c::form(800, 650, 17, 17, false);
    assert!(full.horizontal);
    assert!(!collapsed.horizontal);
    assert!(!collapsed.vertical);
    assert!(collapsed.item_width > full.item_width);
}
#[test]
fn panes_have_independent_logical_ranges() {
    let small = c::form(1050, 650, 17, 17, true);
    assert!(c::maximum(&small, true) > 0);
    assert!(c::maximum(&small, false) > 0);
    let large = c::form(1920, 1200, 17, 17, true);
    assert_eq!(c::maximum(&large, true), 0);
    assert_eq!(c::maximum(&large, false), 0);
    assert!(c::item(Rect {
        x: 305,
        y: 100,
        w: 200,
        h: 20
    }));
    assert!(!c::item(Rect {
        x: 690,
        y: 906,
        w: 150,
        h: 35
    }));
}
