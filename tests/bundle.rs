use constellation::{
    bundle,
    config::{Config, ItemIcon},
    editing::History,
    icon_assets, storage,
};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
fn sandbox() -> PathBuf {
    let root = std::env::current_dir()
        .unwrap()
        .join("target/bundle-tests")
        .join(format!(
            "{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
    fs::create_dir_all(&root).unwrap();
    root
}
fn configured(root: &std::path::Path) -> Config {
    let mut c = Config::default();
    let path = icon_assets::import_bytes(root, "png", b"\x89PNG\r\n\x1a\nfixture").unwrap();
    c.universes[0].icon = ItemIcon::Custom { path: path.clone() };
    c.universes[0].children.as_mut().unwrap()[0].icon = ItemIcon::Custom { path };
    c.universes[0].note = "Internal note".into();
    c
}
#[test]
fn portable_bundle_roundtrips_shared_assets_and_history_without_original_folder() {
    let source = sandbox();
    let destination = sandbox();
    let c = configured(&source);
    let file = source.join("export.constellation");
    bundle::export(&file, &source, &c).unwrap();
    let document: serde_json::Value = serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
    assert_eq!(document["icons"].as_array().unwrap().len(), 1);
    let imported = bundle::import(&file, &destination).unwrap();
    assert_eq!(imported, c);
    let ItemIcon::Custom { path } = &imported.universes[0].icon else {
        panic!()
    };
    assert!(destination.join(path).is_file());
    assert!(!destination.join("data/config.json").exists());
    let original = Config::default();
    let mut history = History::default();
    history.record(&original, &imported);
    assert_eq!(history.undo(&imported).unwrap(), original);
    assert_eq!(history.redo(&original).unwrap(), imported);
    fs::remove_file(source.join(path)).unwrap();
    assert!(destination.join(path).is_file());
}
#[test]
fn malformed_bundle_never_replaces_live_config_or_writes_requested_paths() {
    let source = sandbox();
    let destination = sandbox();
    let c = configured(&source);
    let file = source.join("export.constellation");
    bundle::export(&file, &source, &c).unwrap();
    storage::save(&destination, &Config::default()).unwrap();
    let original = fs::read(destination.join("data/config.json")).unwrap();
    let document: serde_json::Value = serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
    for variant in 0..5 {
        let mut bad = document.clone();
        match variant {
            0 => bad["icons"][0]["reference"] = "../../outside.png".into(),
            1 => bad["icons"][0]["extension"] = "../../outside.png".into(),
            2 => bad["icons"][0]["hex"] = "zz".into(),
            3 => bad["icons"] = serde_json::json!([]),
            _ => bad["icons"]
                .as_array_mut()
                .unwrap()
                .push(document["icons"][0].clone()),
        }
        fs::write(&file, serde_json::to_vec(&bad).unwrap()).unwrap();
        assert!(bundle::import(&file, &destination).is_err());
        assert_eq!(
            fs::read(destination.join("data/config.json")).unwrap(),
            original
        );
        assert!(!destination.join("data/icons").exists());
    }
}
#[test]
fn unavailable_asset_preserves_existing_export_and_json_remains_available() {
    let source = sandbox();
    let mut c = Config::default();
    let file = source.join("export.constellation");
    fs::write(&file, b"original").unwrap();
    c.universes[0].icon = ItemIcon::Custom {
        path: "missing.png".into(),
    };
    assert!(bundle::export(&file, &source, &c).is_err());
    assert_eq!(fs::read(&file).unwrap(), b"original");
    let json = source.join("export.json");
    constellation::editing::export(&json, &c).unwrap();
    assert_eq!(constellation::editing::read_import(&json).unwrap(), c);
}

#[test]
fn original_asset_references_never_become_destination_paths() {
    let source = sandbox();
    let destination = sandbox();
    let c = configured(&source);
    let file = source.join("export.constellation");
    bundle::export(&file, &source, &c).unwrap();
    let mut doc: serde_json::Value = serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
    let mut config: Config = serde_json::from_str(doc["config"].as_str().unwrap()).unwrap();
    config.universes[0].icon = ItemIcon::Custom {
        path: "../../outside.png".into(),
    };
    config.universes[0].children.as_mut().unwrap()[0].icon = config.universes[0].icon.clone();
    doc["config"] = serde_json::to_string(&config).unwrap().into();
    doc["icons"][0]["reference"] = "../../outside.png".into();
    fs::write(&file, serde_json::to_vec(&doc).unwrap()).unwrap();
    let imported = bundle::import(&file, &destination).unwrap();
    let ItemIcon::Custom { path } = &imported.universes[0].icon else {
        panic!()
    };
    assert!(path.starts_with("data/icons/"));
    assert!(!path.contains(".."));
    assert!(destination.join(path).is_file());
}
