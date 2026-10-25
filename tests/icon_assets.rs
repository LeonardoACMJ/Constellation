use constellation::{
    config::{Config, ItemIcon},
    icon_assets, storage,
};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
fn sandbox() -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "constellation-icons-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&path).unwrap();
    path
}
#[test]
fn old_configuration_defaults_to_automatic_and_modes_roundtrip() {
    let mut config = Config::default();
    let bytes = serde_json::to_vec(&config).unwrap();
    assert!(!String::from_utf8_lossy(&bytes).contains("\"icon\""));
    let loaded = storage::parse_config(&bytes).unwrap();
    assert_eq!(loaded.universes[0].icon, ItemIcon::Automatic);
    config.universes[0].icon = ItemIcon::Custom {
        path: "data/icons/group.png".into(),
    };
    config.universes[0].children.as_mut().unwrap()[0].icon = ItemIcon::Symbol;
    assert_eq!(
        storage::parse_config(&serde_json::to_vec(&config).unwrap()).unwrap(),
        config
    );
    config.universes[0].icon = ItemIcon::Custom { path: "\0".into() };
    assert!(config.validate().is_err());
}
#[test]
fn imported_images_are_relative_deduplicated_and_survive_relocation() {
    let original = sandbox();
    let moved = sandbox();
    let source = original.join("outside.png");
    let png = include_bytes!("../assets/app-icon.png");
    fs::write(&source, png).unwrap();
    let app = original.join("app");
    fs::create_dir(&app).unwrap();
    let path = icon_assets::import(&app, &source).unwrap();
    assert!(path.starts_with("data/icons/"));
    assert_eq!(path, icon_assets::import(&app, &source).unwrap());
    assert_eq!(fs::read(storage::resolve(&app, &path)).unwrap(), png);
    fs::create_dir_all(moved.join("data/icons")).unwrap();
    fs::copy(
        storage::resolve(&app, &path),
        storage::resolve(&moved, &path),
    )
    .unwrap();
    fs::remove_file(source).unwrap();
    assert_eq!(fs::read(storage::resolve(&moved, &path)).unwrap(), png);
}
#[test]
fn invalid_or_oversize_import_does_not_overwrite_existing_asset() {
    let root = sandbox();
    let source = root.join("good.png");
    fs::write(&source, include_bytes!("../assets/app-icon.png")).unwrap();
    let imported = icon_assets::import(&root, &source).unwrap();
    let before = fs::read(storage::resolve(&root, &imported)).unwrap();
    let bad = root.join("bad.png");
    fs::write(&bad, b"not an image").unwrap();
    assert!(icon_assets::import(&root, &bad).is_err());
    let huge = root.join("large.png");
    let mut file = fs::File::create(&huge).unwrap();
    use std::io::Write;
    file.write_all(b"\x89PNG\r\n\x1a\n").unwrap();
    file.set_len(icon_assets::MAX_BYTES + 1).unwrap();
    drop(file);
    assert!(icon_assets::import(&root, &huge).is_err());
    assert_eq!(
        fs::read(storage::resolve(&root, &imported)).unwrap(),
        before
    );
}

#[test]
fn software_icon_roundtrip_preserves_script_launch_definition() {
    use constellation::config::Target;
    let mut config = Config::default();
    let item = &mut config.universes[0].children.as_mut().unwrap()[0];
    item.children = None;
    let target = Target::Script {
        path: "instances/hydrus-one.cmd".into(),
        runner: Some("cmd.exe".into()),
        args: vec!["one".into()],
        cwd: Some("instances".into()),
    };
    item.target = Some(target.clone());
    item.icon = ItemIcon::Software {
        path: "software/hydrus_client.exe".into(),
    };
    let loaded = storage::parse_config(&serde_json::to_vec(&config).unwrap()).unwrap();
    assert_eq!(loaded, config);
    assert_eq!(
        loaded.universes[0].children.as_ref().unwrap()[0].target,
        Some(target)
    );
    for path in ["", " ", "bad\0.exe"] {
        config.universes[0].icon = ItemIcon::Software { path: path.into() };
        assert!(config.validate().is_err());
    }
}
