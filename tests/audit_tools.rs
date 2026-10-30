use constellation::{
    config::{Browser, Config, ItemIcon, Node, Target},
    draft::{self, Draft, Writer},
    editing::History,
    editor_layout::Dpi,
    organize, preview,
    theme::{Color, Palette},
};
use std::{
    collections::BTreeMap,
    sync::Arc,
    time::{Duration, Instant},
};

fn root(id: u64) -> Node {
    Node {
        id,
        name: format!("Universe {id}"),
        note: String::new(),
        browser: None,
        icon: ItemIcon::Automatic,
        visible_in: vec![],
        children: Some(vec![]),
        target: None,
    }
}
fn folder(id: u64, path: &str) -> Node {
    Node {
        target: Some(Target::Folder { path: path.into() }),
        children: None,
        ..root(id)
    }
}
fn two() -> Config {
    let mut c = Config::default();
    c.universes.push(root(100));
    c
}

#[test]
fn moves_preserve_ids_subtrees_and_are_one_history_operation() {
    let mut c = two();
    let before = c.clone();
    let count = organize::move_nodes(&mut c, &[2, 3, 4], 100).unwrap();
    assert_eq!(count, 1);
    assert!(c.universes[0].find(2).is_none());
    assert_eq!(c.universes[1].find(2), before.find(2));
    let mut history = History::default();
    history.record(&before, &c);
    assert_eq!(history.undo(&c), Some(before.clone()));
    assert_eq!(history.redo(&before), Some(c));
}
#[test]
fn bad_moves_never_partially_mutate_the_draft() {
    let mut c = two();
    let before = c.clone();
    for (ids, dest) in [
        (vec![2], 5),
        (vec![1], 100),
        (vec![2], 3),
        (vec![2, 999], 100),
    ] {
        assert!(organize::move_nodes(&mut c, &ids, dest).is_err());
        assert_eq!(c, before);
    }
    c.profiles[0].root_id = Some(2);
    let before = c.clone();
    assert!(organize::move_nodes(&mut c, &[2], 100).is_err());
    assert_eq!(c, before);
}
#[test]
fn relink_matches_directory_boundaries_and_only_path_fields() {
    let mut c = Config::default();
    let n = c.universes[0].children.as_mut().unwrap();
    n.clear();
    let mut a = folder(2, r"O:\Work\app");
    a.note = r"O:\Work is a note".into();
    a.icon = ItemIcon::Software {
        path: r"O:\Work\icon.exe".into(),
    };
    n.extend([
        a,
        folder(3, r"O:\Worker\other"),
        folder(4, r"O:\Work"),
        folder(5, r"o:\work\lower"),
    ]);
    let changes = organize::relink_preview(&c, 0, r"O:\Work\", r"P:\Moved\").unwrap();
    assert_eq!(changes.len(), 4);
    assert!(changes.iter().all(|(p, _)| p.node != 3));
    let before = c.clone();
    assert_eq!(
        organize::relink(&mut c, 0, r"O:\Work", r"P:\Moved").unwrap(),
        4
    );
    assert_eq!(c.find(2).unwrap().note, before.find(2).unwrap().note);
    assert_eq!(c.find(3), before.find(3));
    assert!(organize::relink_preview(&c, 0, "", "x").is_err());
}
#[test]
fn browser_summary_distinguishes_inheritance_and_override() {
    let mut c = Config::default();
    c.universes[0].browser = Some(Browser::Program {
        path: "edge.exe".into(),
        args: vec![],
    });
    assert!(organize::browser_summary(&c, 0, 7).contains("edge.exe · inherited"));
    c.universes[0].find_mut(7).unwrap().browser = Some(Browser::System);
    assert_eq!(
        organize::browser_summary(&c, 0, 7),
        "Browser: System default"
    );
    assert!(organize::browser_summary(&c, 0, 3).is_empty());
}
#[test]
fn palette_defaults_are_omitted_and_custom_colors_round_trip() {
    let mut c = Config::default();
    let old = serde_json::to_value(&c).unwrap();
    assert!(old.get("theme").is_none());
    c.theme = Palette::preset(3);
    let restored = constellation::storage::parse_config(&serde_json::to_vec(&c).unwrap()).unwrap();
    assert_eq!(restored, c);
    assert_eq!(Color::parse(" #8ee2cd ").unwrap().hex(), "#8EE2CD");
    for invalid in ["#12345", "red", "#12345678", "#GG0000"] {
        assert!(Color::parse(invalid).is_err());
    }
    let mut bad = serde_json::to_value(&c).unwrap();
    bad["theme"]["typo"] = true.into();
    assert!(serde_json::from_value::<Config>(bad).is_err());
}
fn sample() -> Draft {
    Draft {
        version: 1,
        baseline: draft::fingerprint(&Config::default()),
        config: Config::default(),
        universe: 1,
        selected: 3,
        fields: BTreeMap::from([
            ("main:203".into(), "unfinished \"argument".into()),
            ("json".into(), "{ incomplete".into()),
        ]),
    }
}
fn sandbox() -> std::path::PathBuf {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let p = std::env::current_dir()
        .unwrap()
        .join("target/audit-tools")
        .join(format!(
            "{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
    std::fs::create_dir_all(&p).unwrap();
    p
}
fn until(mut check: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(8);
    while !check() {
        assert!(Instant::now() < deadline, "background operation timed out");
        std::thread::yield_now();
    }
}
#[test]
fn draft_roundtrip_retains_invalid_fields_and_external_baseline() {
    let p = sandbox();
    let w = Writer::new(p.clone()).unwrap();
    let d = sample();
    w.submit(d.clone());
    until(|| draft::read(&p).ok().flatten().is_some());
    let read = draft::read(&p).unwrap().unwrap();
    assert_eq!(read.config, d.config);
    assert_eq!(read.fields, d.fields);
    assert_eq!(read.baseline, d.baseline);
    assert!(!p.join("data/config.json").exists());
    let mut external = d.config.clone();
    external.universes[0].name = "External".into();
    assert_ne!(draft::fingerprint(&external), read.baseline);
    w.clear().unwrap();
    assert!(draft::read(&p).unwrap().is_none());
}
#[test]
fn cleared_or_superseded_recovery_cannot_resurrect() {
    let p = sandbox();
    let w = Writer::new(p.clone()).unwrap();
    let mut d = sample();
    d.fields.insert("large".into(), "x".repeat(1_000_000));
    for _ in 0..12 {
        w.submit(d.clone());
        w.clear().unwrap();
    }
    let mut latest = sample();
    latest.fields.insert("latest".into(), "yes".into());
    w.submit(latest.clone());
    until(|| {
        draft::read(&p)
            .ok()
            .flatten()
            .is_some_and(|d| d.fields.contains_key("latest"))
    });
    assert_eq!(draft::read(&p).unwrap().unwrap().fields, latest.fields);
}
#[test]
fn oversized_or_malformed_recovery_is_rejected_without_touching_saved_data() {
    let p = sandbox();
    std::fs::create_dir_all(p.join("data")).unwrap();
    std::fs::write(p.join("data/editor-draft.json"), b"{broken").unwrap();
    assert!(draft::read(&p).is_err());
    let w = Writer::new(p.clone()).unwrap();
    let mut huge = sample();
    huge.fields
        .insert("huge".into(), "x".repeat(draft::LIMIT as usize));
    w.submit(huge);
    until(|| w.error().is_some());
    assert_eq!(
        std::fs::read(p.join("data/editor-draft.json")).unwrap(),
        b"{broken"
    );
    assert!(!p.join("data/config.json").exists());
}
#[test]
fn preview_owns_matching_geometry_and_model_when_authoring_changes() {
    let c = Arc::new(Config::default());
    let request = preview::Request {
        config: c.clone(),
        universe: 0,
        layout: c.layout,
        theme: c.theme,
        width: 1920.,
        height: 1080.,
    };
    let prepared = preview::prepare(&request);
    assert!(
        prepared
            .sectors
            .iter()
            .all(|s| prepared.config.find(s.id).is_some())
    );
    let mut next = request.clone();
    next.layout.radius += 25.;
    assert!(request != next);
    let updated = preview::prepare(&next);
    assert_eq!(prepared.config.layout, c.layout);
    assert_eq!(updated.config.layout, next.layout);
}
#[test]
fn dpi_keeps_scroll_positions_logical_and_scales_all_coordinates() {
    for dpi in [96, 120, 144, 192] {
        let d = Dpi(dpi);
        for logical in [-100, 0, 24, 1050] {
            assert!((d.logical(d.pixels(logical)) - logical).abs() <= 1);
        }
        assert_eq!(d.pixels(96), dpi as i32);
    }
}

#[test]
#[ignore = "device-free large-tree performance probe, opt in explicitly"]
fn measure_large_tree_preview() {
    fn branch(depth: usize, width: usize, id: &mut u64) -> Node {
        *id += 1;
        let mut n = root(*id);
        if depth == 0 {
            n.target = Some(Target::Folder { path: ".".into() });
            n.children = None;
        } else {
            n.children = Some((0..width).map(|_| branch(depth - 1, width, id)).collect());
        }
        n
    }
    for (name, depth, width) in [("flat", 1, 10000), ("balanced", 4, 8), ("deep", 63, 1)] {
        let mut id = 0;
        let mut c = Config::default();
        c.universes = vec![branch(depth, width, &mut id)];
        let config = Arc::new(c);
        let result = preview::prepare(&preview::Request {
            config: config.clone(),
            universe: 0,
            layout: config.layout,
            theme: config.theme,
            width: 1920.,
            height: 1080.,
        });
        println!(
            "{name}: {id} nodes, {} sectors, {:.1}ms background preparation",
            result.sectors.len(),
            result.elapsed_ms
        );
    }
}
