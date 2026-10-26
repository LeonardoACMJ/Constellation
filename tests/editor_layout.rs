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

#[test]
fn instrument_row_fits_above_footer_and_advice_stays_above_layout() {
    for (w, h) in [(680, 500), (1380, 870), (1920, 1050), (2451, 1080)] {
        let layout = Layout::new(w, h, 17, 17);
        let instrument = layout.control(
            Rect {
                x: 405,
                y: 795,
                w: 260,
                h: 160,
            },
            508,
        );
        let save = layout.control(
            Rect {
                x: 20,
                y: 906,
                w: 100,
                h: 35,
            },
            401,
        );
        assert!(instrument.y + 28 < save.y);
        assert!(instrument.y + 28 < layout.panels()[1].y + layout.panels()[1].h);
        assert_eq!(instrument.h, 160);
        let advice = layout.control(
            Rect {
                x: 465,
                y: 585,
                w: 200,
                h: 48,
            },
            214,
        );
        let heading = layout.control(
            Rect {
                x: 305,
                y: 638,
                w: 370,
                h: 18,
            },
            0,
        );
        assert!(advice.y + advice.h < heading.y);
    }
}

/// The universe row sits above the tree and is anchored to the top, while the
/// tree is the control that absorbs extra height. They must not meet.
#[test]
fn the_universe_row_stays_above_a_growing_tree() {
    const TREE: i32 = 100;
    const UNIVERSE: i32 = 219;
    const ADD_UNIVERSE: i32 = 307;
    let combo = Rect {
        x: 20,
        y: 40,
        w: 182,
        h: 200,
    };
    let button = Rect {
        x: 210,
        y: 40,
        w: 70,
        h: 26,
    };
    let tree = Rect {
        x: 20,
        y: 98,
        w: 260,
        h: 417,
    };
    for height in [MIN_HEIGHT, MIN_HEIGHT + 400, 1440] {
        let l = Layout::new(MIN_WIDTH, height, 17, 17);
        let c = l.control(combo, UNIVERSE);
        let b = l.control(button, ADD_UNIVERSE);
        let t = l.control(tree, TREE);
        assert_eq!(c.y, 40, "the selector is anchored, not pushed by height");
        assert_eq!(b.y, 40);
        assert!(t.y > c.y + 26, "tree starts below the selector row");
        assert!(t.h >= tree.h, "the tree is what absorbs extra height");
        assert!(c.x + c.w <= b.x, "selector and button do not overlap");
        // A dropdown list keeps the height that opens it, unlike a plain field.
        assert_eq!(c.h, 200);
    }
}

#[test]
fn integration_controls_clear_help_text_and_stay_inside_sidebar() {
    use constellation::editor_layout::{START_MENU_CONTROL, STARTUP_CONTROL};
    for (width, height) in [(1050, 996), (2451, 1080), (3840, 2160)] {
        let layout = Layout::new(width, height, 17, 17);
        let startup = layout.control(STARTUP_CONTROL, 221);
        let menu = layout.control(START_MENU_CONTROL, 222);
        let help = layout.control(
            Rect {
                x: 20,
                y: 702,
                w: 260,
                h: 56,
            },
            0,
        );
        let panel = layout.panels()[0];
        assert!(help.y + help.h < startup.y);
        assert!(startup.y + startup.h < menu.y);
        assert!(menu.y + menu.h <= panel.y + panel.h);
        assert!(menu.x + menu.w <= panel.x + panel.w);
        let (_, max_scroll) = layout.scroll(i32::MAX, i32::MAX);
        assert!(menu.y + menu.h - max_scroll <= layout.page_height);
    }
}

/// Name/Kind and Parent/Icon share a row each. They must not overlap, must stay
/// inside the item column, and must leave the visibility list clear of Apply.
#[test]
fn paired_item_rows_stay_side_by_side_inside_their_column() {
    const NAME: i32 = 201;
    const KIND: i32 = 202;
    const PARENT: i32 = 207;
    const ICON_MODE: i32 = 215;
    const VISIBLE: i32 = 220;
    const APPLY: i32 = 301;
    for (w, h) in [(MIN_WIDTH, MIN_HEIGHT), (2451, 1080)] {
        let l = Layout::new(w, h, 17, 17);
        let left = |x, y, w, h, id| l.control(Rect { x, y, w, h }, id);
        let name = left(305, 66, 175, 28, NAME);
        let kind = left(490, 66, 175, 250, KIND);
        let parent = left(305, 444, 175, 250, PARENT);
        let icon = left(490, 444, 175, 180, ICON_MODE);
        let visible = left(305, 505, 175, 72, VISIBLE);
        let apply = left(305, 585, 150, 32, APPLY);
        for (a, b) in [(name, kind), (parent, icon)] {
            assert!(a.x + a.w <= b.x, "paired controls overlap at {w}x{h}");
            assert_eq!(a.y, b.y, "a pair shares its row");
        }
        assert!(
            kind.x + kind.w <= l.item_x + l.item_width,
            "the right half stays inside the item column"
        );
        assert!(visible.y + visible.h <= apply.y, "the list clears Apply");
        assert!(
            visible.y >= parent.y + 28,
            "the list sits below the parent row"
        );
    }
}

#[test]
fn create_and_duplicate_buttons_share_a_row_without_overlap() {
    use constellation::editor_layout::CREATE_CONTROLS;
    for (width, height) in [(680, 500), (1050, 996), (1920, 1080), (3840, 2160)] {
        let layout = Layout::new(width, height, 17, 17);
        let buttons: Vec<_> = CREATE_CONTROLS
            .iter()
            .map(|(id, _, base)| layout.control(*base, *id))
            .collect();
        for button in &buttons {
            assert!(button.x >= 20 && button.x + button.w <= 20 + layout.side_width);
            assert_eq!((button.y, button.h), (buttons[0].y, 30));
        }
        assert!(
            buttons
                .windows(2)
                .all(|pair| pair[0].x + pair[0].w < pair[1].x)
        );
    }
}
