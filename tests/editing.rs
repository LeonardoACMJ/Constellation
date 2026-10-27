use constellation::{
    config::{Config, Target},
    editing::{self, History},
};

#[test]
fn ctrl_backspace_boundaries_cover_words_paths_whitespace_and_unicode() {
    for (text, remaining) in [
        ("first second", "first "),
        ("first second  ", "first "),
        ("C:/Work/msedge.exe", "C:/Work/msedge."),
        ("https://example.org/page", "https://example.org/"),
        ("one_two", ""),
        ("nome café", "nome "),
        ("hello 😀", "hello "),
        ("hello 😀😀  ", "hello "),
        ("hello...", "hello"),
        ("   ", ""),
        ("", ""),
        ("first\r\nsecond", "first\r\n"),
    ] {
        let utf16: Vec<_> = text.encode_utf16().collect();
        let start = editing::previous_word_start(&utf16, utf16.len());
        assert_eq!(
            String::from_utf16(&utf16[..start]).unwrap(),
            remaining,
            "{text:?}"
        );
    }
    let text: Vec<_> = "alpha beta".encode_utf16().collect();
    assert_eq!(editing::previous_word_start(&text, 3), 0);
    assert_eq!(editing::previous_word_start(&text, 0), 0);
    assert_eq!(editing::previous_word_start(&text, usize::MAX), 6);
}
use std::{
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
fn sandbox() -> PathBuf {
    let p = std::env::current_dir()
        .unwrap()
        .join("target/editor-tests")
        .join(format!(
            "{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
    std::fs::create_dir_all(&p).unwrap();
    p
}
#[test]
fn ordinary_arguments_preserve_spaces_empty_values_quotes_and_paths() {
    assert_eq!(
        editing::parse_arguments(r#"--profile "Work profile" "" C:\Tools\app & literal"#).unwrap(),
        vec![
            "--profile",
            "Work profile",
            "",
            r"C:\Tools\app",
            "&",
            "literal"
        ]
    );
    assert!(editing::parse_arguments("--profile \"unfinished").is_err());
    let args = vec![
        "".into(),
        "space at end ".into(),
        "a\"b".into(),
        "C:\\folder with spaces\\".into(),
        "日本語 🐇".into(),
        "line\nfeed".into(),
    ];
    assert_eq!(
        editing::parse_arguments(&editing::format_arguments(&args)).unwrap(),
        args
    );
    let alphabet = ['a', ' ', '\t', '\\', '"', 'é', '🦌'];
    let mut seed = 17u64;
    for _ in 0..1000 {
        let mut args = vec![];
        for _ in 0..4 {
            let mut arg = String::new();
            for _ in 0..12 {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                arg.push(alphabet[seed as usize % alphabet.len()]);
            }
            args.push(arg);
        }
        assert_eq!(
            editing::parse_arguments(&editing::format_arguments(&args)).unwrap(),
            args
        );
    }
}
#[test]
fn undo_redo_retains_valid_snapshots_and_a_new_edit_replaces_redo() {
    let original = Config::default();
    let mut history = History::default();
    let mut changed = original.clone();
    changed.universes[0].name = "Renamed".into();
    history.record(&original, &changed);
    let mut removed = changed.clone();
    removed.universes[0].children.as_mut().unwrap().clear();
    history.record(&changed, &removed);
    let undo = history.undo(&removed).unwrap();
    assert_eq!(undo, changed);
    assert_eq!(history.undo(&undo).unwrap(), original);
    let restored = history.redo(&original).unwrap();
    assert_eq!(restored, changed);
    let mut other = restored.clone();
    other.sound.enabled = false;
    history.record(&restored, &other);
    assert!(history.redo(&other).is_none());
    assert_eq!(history.undo(&other).unwrap(), changed);
}
#[test]
fn drop_infers_targets_and_preserves_portable_paths() {
    let root = sandbox();
    let folder = root.join("Folder");
    std::fs::create_dir(&folder).unwrap();
    for filename in ["tool.exe", "open.lnk", "do.ps1", "notes.txt"] {
        std::fs::write(root.join(filename), "").unwrap();
    }
    assert!(
        matches!(editing::star_from_path(&root,&folder,1).unwrap().target,Some(Target::Folder {path}) if path=="Folder")
    );
    assert!(
        matches!(editing::star_from_path(&root,&root.join("tool.exe"),2).unwrap().target,Some(Target::Program {path,..}) if path=="tool.exe")
    );
    assert!(matches!(
        editing::star_from_path(&root, &root.join("do.ps1"), 3)
            .unwrap()
            .target,
        Some(Target::Script { .. })
    ));
    assert!(matches!(
        editing::star_from_path(&root, &root.join("open.lnk"), 4)
            .unwrap()
            .target,
        Some(Target::Shortcut { .. })
    ));
    assert!(matches!(
        editing::star_from_path(&root, &root.join("notes.txt"), 5)
            .unwrap()
            .target,
        Some(Target::Shortcut { .. })
    ));
    assert!(editing::star_from_path(&root, &root.join("missing"), 6).is_err());
    assert!(
        !editing::target_advice(
            &root,
            &Target::Folder {
                path: "missing".into()
            }
        )
        .is_empty()
    );
    assert!(
        editing::target_advice(
            &root,
            &Target::Folder {
                path: "Folder".into()
            }
        )
        .is_empty()
    );
}
#[test]
fn export_roundtrip_and_failed_import_never_replace_live_data() {
    let root = sandbox();
    let c = Config::default();
    constellation::storage::save(&root, &c).unwrap();
    let before = std::fs::read(root.join("data/config.json")).unwrap();
    let export = root.join("export.json");
    editing::export(&export, &c).unwrap();
    assert_eq!(editing::read_import(&export).unwrap(), c);
    let mut invalid = c.clone();
    invalid.layout.radius = -1.;
    assert!(editing::export(&export, &invalid).is_err());
    assert_eq!(editing::read_import(&export).unwrap(), c);
    std::fs::write(root.join("bad.json"), "{ broken").unwrap();
    assert!(editing::read_import(&root.join("bad.json")).is_err());
    assert_eq!(
        std::fs::read(root.join("data/config.json")).unwrap(),
        before
    );
    assert!(editing::export(&root.join("absent/export.json"), &c).is_err());
    assert!(!root.join("absent").exists());
}

#[test]
fn deepest_supported_tree_survives_json_import_storage_and_backup_recovery() {
    use constellation::config::Node;
    let mut node = Node {
        note: String::new(),
        icon: Default::default(),
        visible_in: vec![],
        id: 65,
        name: "Deep Star".into(),
        children: None,
        target: Some(Target::Folder { path: ".".into() }),
    };
    for id in (1..=64).rev() {
        node = Node {
            note: String::new(),
            icon: Default::default(),
            visible_in: vec![],
            id,
            name: format!("Level {id}"),
            children: Some(vec![node]),
            target: None,
        };
    }
    let mut c = Config::default();
    c.universes[0] = node;
    c.validate().unwrap();
    let root = sandbox();
    constellation::storage::save(&root, &c).unwrap();
    assert_eq!(constellation::storage::read(&root).unwrap(), c);
    let exported = root.join("deep.json");
    editing::export(&exported, &c).unwrap();
    assert_eq!(editing::read_import(&exported).unwrap(), c);
    constellation::storage::save(&root, &c).unwrap();
    std::fs::write(root.join("data/config.json"), "broken").unwrap();
    assert_eq!(constellation::storage::load_or_create(&root).unwrap(), c);
    let excessive = format!("{}0{}", "[".repeat(300), "]".repeat(300));
    assert!(
        constellation::storage::parse_config(excessive.as_bytes())
            .unwrap_err()
            .contains("safety limit")
    );
    c.universes[0].name = format!("{}\\\"{}", "[".repeat(1000), "]".repeat(1000));
    let json = serde_json::to_vec(&c).unwrap();
    assert_eq!(constellation::storage::parse_config(&json).unwrap(), c);
    let mut trailing = json;
    trailing.extend_from_slice(b" true");
    assert!(constellation::storage::parse_config(&trailing).is_err());
}

#[test]
fn ten_thousand_stars_remain_individually_selectable_without_paging() {
    use constellation::{config::Node, engine::Engine, geometry};
    let mut c = Config::default();
    c.layout.gap = 0.;
    c.universes[0].children = Some(
        (2..10002)
            .map(|id| Node {
                note: String::new(),
                icon: Default::default(),
                visible_in: vec![],
                id,
                name: format!("Star {id}"),
                children: None,
                target: Some(Target::Folder { path: ".".into() }),
            })
            .collect(),
    );
    c.validate().unwrap();
    let full = geometry::full(&c);
    assert_eq!(full.len(), 10000);
    let mut e = Engine::default();
    assert!(e.open(0, &c));
    for index in [0, 5000, 9999] {
        let s = &full[index];
        e.motion(&c, s.dot);
        assert_eq!(e.hovered, Some(s.id));
    }
    assert!(!geometry::warnings(&c, 1920., 1080.).is_empty());
}

#[test]
fn item_paths_follow_ids_through_duplicate_names_and_moves() {
    let mut c = Config::default();
    c.universes[0].name = "Same".into();
    c.universes[0].find_mut(2).unwrap().name = "Same".into();
    c.universes[0].find_mut(5).unwrap().name = "Same".into();
    let path = editing::node_path(&c.universes[0], 6);
    assert_eq!(
        path.iter().map(|n| n.id).collect::<Vec<_>>(),
        vec![1, 2, 5, 6]
    );
    assert_eq!(
        path.iter().map(|n| n.name.as_str()).collect::<Vec<_>>()[..3],
        ["Same", "Same", "Same"]
    );
    assert!(editing::node_path(&c.universes[0], u64::MAX).is_empty());
    let moved = c.universes[0]
        .find_mut(5)
        .unwrap()
        .children
        .as_mut()
        .unwrap()
        .remove(0);
    c.universes[0].children.as_mut().unwrap().push(moved);
    assert_eq!(
        editing::node_path(&c.universes[0], 6)
            .iter()
            .map(|n| n.id)
            .collect::<Vec<_>>(),
        vec![1, 6]
    );
    assert_eq!(editing::node_path(&c.universes[0], 1).len(), 1);
}

#[test]
fn duplicate_star_preserves_launch_fields_and_uses_global_ids() {
    use constellation::config::ItemIcon;
    let mut c = Config::default();
    let profile = c.profiles[0].id;
    let n = c.universes[0].find_mut(3).unwrap();
    n.target = Some(Target::Program {
        path: "C:/Program Files/Edge/msedge.exe".into(),
        args: vec!["https://work.example/".into()],
        cwd: Some("C:/Work".into()),
    });
    n.note = "Work browser".into();
    n.icon = ItemIcon::Software {
        path: "icons/browser.exe".into(),
    };
    n.visible_in = vec![profile];
    let original = n.clone();
    let mut other = c.universes[0].clone();
    other.id = 10000;
    other.children = Some(vec![]);
    other.visible_in.clear();
    c.universes.push(other);
    let id = editing::duplicate_node(&mut c, 0, 3).unwrap();
    assert_eq!(id, 10001);
    let mut expected = original.clone();
    expected.id = id;
    expected.name.push_str(" (copy)");
    assert_eq!(c.find(id).unwrap(), &expected);
    assert_eq!(c.find(3).unwrap(), &original);
    let siblings = c.find(2).unwrap().children.as_ref().unwrap();
    assert_eq!(
        siblings.iter().map(|n| n.id).collect::<Vec<_>>(),
        vec![3, id, 4, 5]
    );
    if let Some(Target::Program { args, .. }) = &mut c.universes[0].find_mut(id).unwrap().target {
        args[0] = "https://another.example/".into();
    }
    assert_eq!(
        c.find(3).unwrap(),
        &original,
        "Editing a copy altered its source"
    );
    c.validate().unwrap();
}

#[test]
fn duplicate_constellation_copies_every_descendant_and_roundtrips_history() {
    use constellation::config::Node;
    let mut c = Config::default();
    c.universes[0].find_mut(5).unwrap().note = "Nested note".into();
    let before = c.clone();
    let maximum = c.max_node_id();
    let original = c.find(2).unwrap().clone();
    let id = editing::duplicate_node(&mut c, 0, 2).unwrap();
    let mut ids = std::collections::HashSet::new();
    fn compare(a: &Node, b: &Node, max: u64, ids: &mut std::collections::HashSet<u64>, top: bool) {
        assert!(b.id > max && ids.insert(b.id));
        let mut expected = a.clone();
        expected.id = b.id;
        if top {
            expected.name.push_str(" (copy)");
        }
        expected.children = b.children.clone();
        assert_eq!(&expected, b);
        let old = a.children.as_deref().unwrap_or_default();
        let new = b.children.as_deref().unwrap_or_default();
        assert_eq!(old.len(), new.len());
        for (a, b) in old.iter().zip(new) {
            compare(a, b, max, ids, false);
        }
    }
    compare(&original, c.find(id).unwrap(), maximum, &mut ids, true);
    assert_eq!(c.find(2).unwrap(), &original);
    let mut history = History::default();
    history.record(&before, &c);
    assert_eq!(history.undo(&c).unwrap(), before);
    assert_eq!(history.redo(&before).unwrap(), c);
    let json = serde_json::to_vec(&c).unwrap();
    assert_eq!(constellation::storage::parse_config(&json).unwrap(), c);
    let child = c.find(id).unwrap().children.as_ref().unwrap()[0].id;
    c.universes[0].find_mut(child).unwrap().name = "Independent".into();
    assert_eq!(c.find(2).unwrap(), &original);
}

#[test]
fn failed_duplication_never_changes_the_draft() {
    let mut c = Config::default();
    let mut other = c.universes[0].clone();
    other.id = u64::MAX - 1;
    other.children = Some(vec![]);
    c.universes.push(other);
    let before = c.clone();
    // There is room for one ID but not the complete branch: failure is atomic.
    assert!(editing::duplicate_node(&mut c, 0, 2).is_err());
    assert_eq!(c, before);
    for (universe, node) in [(0, 1), (0, 999), (999, 2)] {
        assert!(editing::duplicate_node(&mut c, universe, node).is_err());
        assert_eq!(c, before);
    }
}

#[test]
fn deletion_protects_nested_profile_entries_without_mutating_draft() {
    let mut c = Config::default();
    let branch = c.universes[0]
        .children
        .as_ref()
        .unwrap()
        .iter()
        .find(|n| n.children.is_some())
        .unwrap()
        .id;
    c.profiles[0].root_id = Some(branch);
    let before = c.clone();
    assert!(
        editing::check_delete(&c, branch)
            .unwrap_err()
            .contains(&c.profiles[0].name)
    );
    assert!(editing::check_delete(&c, c.universes[0].id).is_err());
    let leaf = c.universes[0]
        .children
        .as_ref()
        .unwrap()
        .iter()
        .find(|n| n.children.is_none())
        .unwrap()
        .id;
    assert!(editing::check_delete(&c, leaf).is_ok());
    assert_eq!(c, before);
}
#[test]
fn tree_structure_distinguishes_rename_from_reparent_or_kind_change() {
    let mut c = Config::default();
    let before = editing::tree_rows(&c.universes[0]);
    c.universes[0].name = "Renamed".into();
    assert!(editing::same_tree_structure(
        &before,
        &editing::tree_rows(&c.universes[0])
    ));
    c.universes[0].children.as_mut().unwrap().reverse();
    assert!(!editing::same_tree_structure(
        &before,
        &editing::tree_rows(&c.universes[0])
    ));
}

#[test]
fn preview_ignores_notes_sound_and_other_universes_but_not_icons_or_geometry() {
    let original = Config::default();
    let mut c = original.clone();
    c.universes[0].note = "Internal only".into();
    c.sound.volume = 0.4;
    assert!(editing::preview_equal(&original, &c, 0));
    c.universes[0].icon = constellation::config::ItemIcon::Symbol;
    assert!(!editing::preview_equal(&original, &c, 0));
    c = original.clone();
    c.layout.radius += 1.;
    assert!(!editing::preview_equal(&original, &c, 0));
}
