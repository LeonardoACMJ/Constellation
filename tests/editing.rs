use constellation::{
    config::{Config, Target},
    editing::{self, History},
};
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
        id: 65,
        name: "Deep Star".into(),
        children: None,
        target: Some(Target::Folder { path: ".".into() }),
    };
    for id in (1..=64).rev() {
        node = Node {
            note: String::new(),
            icon: Default::default(),
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
