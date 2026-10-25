use constellation::{
    config::{Binding, Config, Input, Mode, Node, Target},
    engine::{Action, Engine},
    geometry::{self, Point},
    storage,
};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
static COUNTER: AtomicU64 = AtomicU64::new(0);
fn sandbox() -> PathBuf {
    let p = std::env::current_dir()
        .unwrap()
        .join("target/test-data")
        .join(format!(
            "{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
    std::fs::create_dir_all(&p).unwrap();
    p
}

#[test]
fn rejects_ambiguous_bindings_and_invalid_nodes() {
    let mut c = Config::default();
    c.profiles.push(c.profiles[0].clone());
    assert!(c.validate().unwrap_err().contains("Duplicate shortcut"));
    c.profiles.pop();
    c.universes[0].children.as_mut().unwrap()[0].id = c.universes[0].id;
    assert!(c.validate().unwrap_err().contains("Duplicate node"));
    assert!(Binding::parse("Ctrl+F12").is_err());
    assert!(Binding::parse("Mouse1").is_err());
    assert!(Binding::parse("Ctrl").is_err());
}
#[test]
fn hold_release_keys_are_exact() {
    let b = Binding::parse("Ctrl+Shift+G").unwrap();
    assert!(b.requires(Input::Key(b'G' as u16)));
    assert!(b.requires(Input::Key(0xa2)));
    assert!(b.requires(Input::Key(0xa1)));
    assert!(!b.requires(Input::Key(b'Q' as u16)));
    let mouse = Binding::parse("Mouse4").unwrap();
    assert!(mouse.requires(Input::Mouse(4)));
    assert!(!mouse.requires(Input::Mouse(3)));
}
#[test]
fn subdivision_keeps_every_target_without_pages() {
    let mut c = Config::default();
    c.universes[0].children = Some(
        (10..210)
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
    c.layout.gap = 0.;
    let sectors = geometry::visible(&c, &[]);
    assert_eq!(sectors.len(), 200);
    for s in sectors {
        assert_eq!(
            geometry::hit(&geometry::visible(&c, &[]), s.dot)
                .unwrap()
                .id,
            s.id
        );
    }
}
#[test]
fn child_expansion_does_not_move_existing_sectors() {
    let c = Config::default();
    let before = geometry::visible(&c, &[]);
    let after = geometry::visible(&c, &[2, 5]);
    for s in before {
        let t = after.iter().find(|t| t.id == s.id).unwrap();
        assert_eq!(s.center, t.center);
        assert_eq!(s.dot, t.dot);
        assert_eq!(s.from, t.from);
        assert_eq!(s.outer, t.outer);
    }
    assert!(after.iter().any(|s| s.id == 6 && s.depth == 2));
}
#[test]
fn hover_opens_immediately_gaps_preserve_and_ancestors_switch() {
    let mut c = Config::default();
    c.profiles[0].directional = false;
    let mut e = Engine::default();
    assert!(e.open(0, &c));
    let folder = e.sectors.iter().find(|s| s.id == 2).unwrap().dot;
    assert_eq!(e.motion(&c, folder), Action::Changed);
    assert_eq!(e.path, vec![2]);
    e.motion(
        &c,
        Point {
            x: 10000.,
            y: 10000.,
        },
    );
    assert_eq!(e.path, vec![2]);
    let star = e.sectors.iter().find(|s| s.id == 8).unwrap().dot;
    e.motion(&c, star);
    assert!(e.path.is_empty());
    assert_eq!(e.select(&c), Action::Launch(8));
}
#[test]
fn click_to_open_profile_does_not_open_on_hover() {
    let mut c = Config::default();
    c.profiles[0].hover = false;
    let mut e = Engine::default();
    e.open(0, &c);
    let p = e.sectors.iter().find(|s| s.id == 2).unwrap().dot;
    e.motion(&c, p);
    assert!(e.path.is_empty());
    assert_eq!(e.select(&c), Action::Changed);
    assert_eq!(e.path, vec![2]);
    let ancestor = e.sectors.iter().find(|s| s.id == 8).unwrap().dot;
    e.motion(&c, ancestor);
    assert!(
        e.path.is_empty(),
        "Ancestor Star must collapse deeper fans even with click-to-open folders"
    );
}
#[test]
fn hold_selects_only_a_star_and_cancels_on_empty() {
    let mut c = Config::default();
    c.profiles[0].mode = Mode::Hold;
    let mut e = Engine::default();
    e.open(0, &c);
    let p = e.sectors.iter().find(|s| s.id == 8).unwrap().dot;
    e.motion(&c, p);
    assert_eq!(e.release(&c), Action::Launch(8));
    e.close();
    e.open(0, &c);
    assert_eq!(e.release(&c), Action::Changed);
    assert!(e.profile.is_none());
}
#[test]
fn other_profile_invocation_cannot_replace_active_state() {
    let mut c = Config::default();
    let mut p = c.profiles[0].clone();
    p.binding = "Mouse4".into();
    c.profiles.push(p);
    let mut e = Engine::default();
    e.open(0, &c);
    let position = e.sectors[0].dot;
    e.motion(&c, position);
    let path = e.path.clone();
    assert!(!e.open(1, &c));
    assert_eq!(e.profile, Some(0));
    assert_eq!(e.path, path);
}
#[test]
fn warning_only_layout_can_be_saved_and_reloaded() {
    let mut c = Config::default();
    c.layout.radius = 4000.;
    c.layout.distance = 10.;
    c.layout.gap = 1e6;
    assert!(c.validate().is_ok());
    assert!(!geometry::warnings(&c, 1920., 1040.).is_empty());
    let root = sandbox();
    storage::save(&root, &c).unwrap();
    let loaded = storage::read(&root).unwrap();
    assert_eq!(loaded.layout.radius, 4000.);
}
#[test]
fn portable_save_backup_and_relocation() {
    let root = sandbox();
    let mut c = storage::load_or_create(&root).unwrap();
    c.universes[0].name = "Renamed".into();
    storage::save(&root, &c).unwrap();
    assert!(root.join("data/config.backup.json").is_file());
    let other = sandbox();
    std::fs::create_dir_all(other.join("data")).unwrap();
    std::fs::copy(
        root.join("data/config.json"),
        other.join("data/config.json"),
    )
    .unwrap();
    assert_eq!(storage::read(&other).unwrap().universes[0].name, "Renamed");
    assert_eq!(
        storage::resolve(&other, "scripts/test.ps1"),
        other.join("scripts/test.ps1")
    );
}
#[test]
fn invalid_save_preserves_last_valid_file() {
    let root = sandbox();
    let mut c = storage::load_or_create(&root).unwrap();
    let original = std::fs::read(root.join("data/config.json")).unwrap();
    c.layout.radius = -1.;
    assert!(storage::save(&root, &c).is_err());
    assert_eq!(
        std::fs::read(root.join("data/config.json")).unwrap(),
        original
    );
}
#[test]
fn unwritable_portable_data_never_creates_an_alternate_location() {
    let root = sandbox();
    std::fs::write(root.join("data"), "not a directory").unwrap();
    assert!(storage::save(&root, &Config::default()).is_err());
    assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
}
#[test]
fn full_preview_enumerates_hidden_sibling_branches() {
    let c = Config::default();
    let all = geometry::full(&c);
    let closed = geometry::visible(&c, &[]);
    assert!(all.len() > closed.len());
    for id in [2, 3, 4, 5, 6, 7, 8, 9] {
        assert!(all.iter().any(|s| s.id == id));
    }
}
#[test]
fn json_roundtrip_preserves_targets_profiles_and_order() {
    let c = Config::default();
    let json = serde_json::to_string(&c).unwrap();
    let loaded: Config = serde_json::from_str(&json).unwrap();
    loaded.validate().unwrap();
    assert_eq!(serde_json::to_string(&loaded).unwrap(), json);
}

#[test]
fn corrupted_config_can_recover_backup_without_overwriting_original() {
    let root = sandbox();
    let mut c = storage::load_or_create(&root).unwrap();
    c.universes[0].name = "Newer".into();
    storage::save(&root, &c).unwrap();
    std::fs::write(root.join("data/config.json"), b"broken").unwrap();
    let loaded = storage::load_or_create(&root).unwrap();
    assert_eq!(loaded.universes[0].name, "Your universe");
    assert_eq!(
        std::fs::read(root.join("data/config.json")).unwrap(),
        b"broken"
    );
    assert!(root.join("data/recovery.txt").is_file());
    storage::save(&root, &loaded).unwrap();
    let backup: Config =
        serde_json::from_slice(&std::fs::read(root.join("data/config.backup.json")).unwrap())
            .unwrap();
    backup.validate().unwrap();
    assert_eq!(backup.universes[0].name, "Your universe");
}

#[test]
fn legacy_profiles_get_stable_ids_across_rename_reorder_and_relocation() {
    let root = sandbox();
    let mut c = Config::default();
    c.profiles[0].id = 0;
    let mut second = c.profiles[0].clone();
    second.binding = "F8".into();
    c.profiles.push(second);
    std::fs::create_dir_all(root.join("data")).unwrap();
    std::fs::write(
        root.join("data/config.json"),
        serde_json::to_vec(&c).unwrap(),
    )
    .unwrap();
    let mut loaded = storage::read(&root).unwrap();
    let id = loaded.profiles[0].id;
    assert_ne!(id, 0);
    assert_ne!(id, loaded.profiles[1].id);
    loaded.profiles[0].name = "Renamed".into();
    loaded.profiles.reverse();
    let other = sandbox();
    storage::save(&other, &loaded).unwrap();
    let moved = storage::read(&other).unwrap();
    assert_eq!(moved.profiles[1].id, id);
    assert_eq!(moved.profiles[1].name, "Renamed");
    let mut invalid = moved.clone();
    invalid.profiles[0].id = id;
    assert!(
        storage::save(&other, &invalid)
            .unwrap_err()
            .contains("Duplicate profile ID")
    );
    assert_eq!(storage::read(&other).unwrap().profiles[1].id, id);
}
#[test]
fn overlap_warning_is_advisory_and_ignores_normal_ancestor_stacking() {
    let star = |id| Node {
        note: String::new(),
        icon: Default::default(),
        id,
        name: format!("Star {id}"),
        children: None,
        target: Some(Target::Folder { path: ".".into() }),
    };
    let mut c = Config::default();
    c.universes[0].children = Some(vec![Node {
        note: String::new(),
        icon: Default::default(),
        id: 2,
        name: "One".into(),
        children: Some(vec![star(3)]),
        target: None,
    }]);
    assert!(
        !geometry::warnings(&c, 1920., 1080.)
            .iter()
            .any(|s| s.contains("overlap"))
    );
    c.universes[0].children.as_mut().unwrap().push(Node {
        note: String::new(),
        icon: Default::default(),
        id: 4,
        name: "Two".into(),
        children: Some(vec![star(5)]),
        target: None,
    });
    c.layout.distance = 0.;
    c.layout.spread = 360.;
    c.layout.gap = 0.;
    assert!(
        geometry::warnings(&c, 1920., 1080.)
            .iter()
            .any(|s| s.contains("overlap"))
    );
    let root = sandbox();
    storage::save(&root, &c).unwrap();
    assert_eq!(storage::read(&root).unwrap().layout.distance, 0.);
}

/// A configuration written before universes existed keeps the `root` shape when
/// it is saved again, so opening the editor does not rewrite a working file.
#[test]
fn single_universe_round_trips_in_the_legacy_root_shape() {
    let root = sandbox();
    std::fs::create_dir_all(root.join("data")).unwrap();
    let legacy = r#"{"version":1,"root":{"id":1,"name":"Universe","children":[]},
        "profiles":[{"id":1,"name":"Default","binding":"Ctrl+Q","mode":"click",
        "hover":true,"cursor_end":"leave","directional":true,"dot_cursor":true}]}"#;
    std::fs::write(root.join("data/config.json"), legacy).unwrap();
    let c = storage::read(&root).unwrap();
    assert_eq!(c.universes.len(), 1);
    assert_eq!(c.universes[0].name, "Universe");
    assert_eq!(c.profiles[0].universe, 0);
    assert_eq!(c.profiles[0].root_id, None);
    storage::save(&root, &c).unwrap();
    let written = std::fs::read_to_string(root.join("data/config.json")).unwrap();
    assert!(written.contains("\"root\""), "{written}");
    assert!(!written.contains("universes"), "{written}");
    // Untouched profiles must not grow keys they never had.
    assert!(!written.contains("\"universe\""), "{written}");
    assert!(!written.contains("root_id"), "{written}");
    assert_eq!(storage::read(&root).unwrap(), c);
}

#[test]
fn a_second_universe_switches_the_file_shape_and_survives_a_reload() {
    let root = sandbox();
    let mut c = storage::load_or_create(&root).unwrap();
    let second = Node {
        note: String::new(),
        icon: Default::default(),
        id: c.max_node_id() + 1,
        name: "Work".into(),
        children: Some(vec![]),
        target: None,
    };
    c.universes.push(second.clone());
    c.profiles[0].universe = second.id;
    storage::save(&root, &c).unwrap();
    let written = std::fs::read_to_string(root.join("data/config.json")).unwrap();
    assert!(written.contains("universes"), "{written}");
    let reloaded = storage::read(&root).unwrap();
    assert_eq!(reloaded, c);
    assert_eq!(reloaded.universe_of(&reloaded.profiles[0]).unwrap().id, second.id);
}

#[test]
fn a_profile_opens_its_own_universe_and_entry_node() {
    let mut c = Config::default();
    let branch = c.universes[0]
        .children
        .as_ref()
        .unwrap()
        .iter()
        .find(|n| n.children.is_some())
        .unwrap()
        .clone();
    // Entry point: the radial opens inside the tree, without copying anything.
    c.profiles[0].root_id = Some(branch.id);
    assert_eq!(c.entry_of(&c.profiles[0]).unwrap().id, branch.id);
    let mut engine = Engine::default();
    assert!(engine.open(0, &c));
    assert_eq!(engine.entry(&c).unwrap().id, branch.id);
    let shown: Vec<u64> = engine.sectors.iter().filter(|s| s.depth == 0).map(|s| s.id).collect();
    let expected: Vec<u64> = branch.children.as_ref().unwrap().iter().map(|n| n.id).collect();
    assert_eq!(shown, expected);
}

#[test]
fn validate_rejects_entry_points_that_are_not_constellations_of_that_universe() {
    let mut c = Config::default();
    let star = c.universes[0]
        .children
        .as_ref()
        .unwrap()
        .iter()
        .find(|n| n.target.is_some())
        .unwrap()
        .id;
    c.profiles[0].root_id = Some(star);
    assert!(c.validate().is_err(), "a Star has nothing to fan out");
    c.profiles[0].root_id = Some(c.max_node_id() + 50);
    assert!(c.validate().is_err(), "unknown node");
    c.profiles[0].root_id = None;
    c.profiles[0].universe = c.max_node_id() + 50;
    assert!(c.validate().is_err(), "unknown universe");
    c.profiles[0].universe = 0;
    assert!(c.validate().is_ok());
    c.universes.clear();
    assert!(c.validate().is_err(), "a configuration needs a universe");
}
