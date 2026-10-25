use constellation::{
    config::{Config, Mode, NOTE_MAX_CHARS, Node},
    editing::{self, History},
    engine::{Action, Engine},
    storage,
};

fn add_notes(node: &mut Node, depth: usize) {
    node.note = format!("Depth {depth}: needs ssh-agent; deletes the source 🐇");
    if let Some(children) = &mut node.children {
        for child in children {
            add_notes(child, depth + 1);
        }
    }
}

#[test]
fn old_config_omits_empty_notes_and_all_node_kinds_roundtrip() {
    let old = Config::default();
    let bytes = serde_json::to_vec(&old).unwrap();
    assert!(!String::from_utf8_lossy(&bytes).contains("\"note\""));
    let mut loaded = storage::parse_config(&bytes).unwrap();
    assert_eq!(loaded, old);
    let version = loaded.version;
    add_notes(&mut loaded.universes[0], 0);
    let exported = serde_json::to_vec_pretty(&loaded).unwrap();
    let imported = storage::parse_config(&exported).unwrap();
    assert_eq!(imported, loaded);
    assert_eq!(imported.version, version);
    assert!(imported.find(1).unwrap().note.starts_with("Depth 0"));
    assert!(imported.find(5).unwrap().note.starts_with("Depth 2"));
    assert!(imported.find(6).unwrap().note.starts_with("Depth 3"));
}

#[test]
fn note_limit_counts_unicode_characters_and_rejects_line_breaks() {
    let mut config = Config::default();
    config.universes[0].note = "🐇".repeat(NOTE_MAX_CHARS);
    assert!(config.validate().is_ok());
    config.universes[0].note.push('é');
    assert!(config.validate().unwrap_err().contains("140"));
    for bad in [
        "two\nlines",
        "two\rlines",
        "two\tcolumns",
        "null\0",
        "line\u{2028}break",
        "line\u{2029}break",
    ] {
        config.universes[0].find_mut(6).unwrap().note = bad.into();
        config.universes[0].note.clear();
        assert!(storage::parse_config(&serde_json::to_vec(&config).unwrap()).is_err());
    }
    let input = format!("needs\r\nssh-agent\t{}", "🐇".repeat(200));
    let bounded = editing::note_input(&input);
    assert_eq!(bounded.chars().count(), NOTE_MAX_CHARS);
    assert!(bounded.starts_with("needs  ssh-agent "));
    assert_eq!(editing::note_input(&bounded), bounded);
}

#[test]
fn note_edits_are_undoable_on_stars_constellations_and_root() {
    let mut config = Config::default();
    let mut history = History::default();
    for id in [1, 5, 6] {
        let before = config.clone();
        config.universes[0].find_mut(id).unwrap().note = "needs elevation and Codex closed".into();
        history.record(&before, &config);
        let after = config.clone();
        config = history.undo(&config).unwrap();
        assert_eq!(config, before);
        config = history.redo(&config).unwrap();
        assert_eq!(config, after);
        // Clearing an existing note is a real edit too.
        let mut cleared = config.clone();
        cleared.universes[0].find_mut(id).unwrap().note.clear();
        history.record(&config, &cleared);
        assert_eq!(history.undo(&cleared).unwrap(), config);
    }
}

#[test]
fn notes_do_not_change_immediate_hover_or_click_and_hold_launches() {
    for mode in [Mode::Click, Mode::Hold] {
        let mut plain = Config::default();
        plain.profiles[0].mode = mode;
        let mut noted = plain.clone();
        add_notes(&mut noted.universes[0], 0);
        let (mut a, mut b) = (Engine::default(), Engine::default());
        assert!(a.open(0, &plain) && b.open(0, &noted));
        for id in [2, 5, 6] {
            let p = a.sectors.iter().find(|s| s.id == id).unwrap().dot;
            assert_eq!(a.motion(&plain, p), b.motion(&noted, p));
            assert_eq!(a.path, b.path);
            assert_eq!(a.hovered, b.hovered);
            if id == 2 {
                assert_eq!(b.path, [2]);
            }
            if id == 5 {
                assert_eq!(b.path, [2, 5]);
            }
        }
        assert_eq!(a.select(&plain), Action::Launch(6));
        assert_eq!(b.select(&noted), Action::Launch(6));
        if mode == Mode::Hold {
            assert_eq!(b.release(&noted), Action::Launch(6));
        }
        assert_eq!(plain.find(6).unwrap().target, noted.find(6).unwrap().target);
        assert_eq!(
            editing::target_advice(
                std::path::Path::new("."),
                plain.find(6).unwrap().target.as_ref().unwrap()
            ),
            editing::target_advice(
                std::path::Path::new("."),
                noted.find(6).unwrap().target.as_ref().unwrap()
            )
        );
    }
}

#[test]
fn disk_save_and_backup_preserve_notes_without_another_index() {
    let root = std::env::current_dir()
        .unwrap()
        .join("target/note-storage-tests")
        .join(std::process::id().to_string());
    std::fs::create_dir_all(&root).unwrap();
    let old = Config::default();
    storage::save(&root, &old).unwrap();
    let mut noted = old.clone();
    add_notes(&mut noted.universes[0], 0);
    storage::save(&root, &noted).unwrap();
    assert_eq!(storage::read(&root).unwrap(), noted);
    let backup = std::fs::read(root.join("data/config.backup.json")).unwrap();
    assert_eq!(storage::parse_config(&backup).unwrap(), old);
    storage::save(&root, &noted).unwrap();
    let newer_backup = std::fs::read(root.join("data/config.backup.json")).unwrap();
    assert_eq!(storage::parse_config(&newer_backup).unwrap(), noted);
    assert!(!root.join("data/README.md").exists());
}
