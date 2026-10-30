use constellation::{palettes, theme::Palette};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};
fn sandbox() -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let p = std::env::current_dir()
        .unwrap()
        .join("target/palette-tests")
        .join(format!(
            "{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
    std::fs::create_dir_all(&p).unwrap();
    p
}
#[test]
fn named_palettes_survive_reopening_and_portable_copy_without_activating() {
    let root = sandbox();
    assert!(palettes::load(&root).unwrap().is_empty());
    palettes::save_named(&root, "My colors", Palette::preset(2)).unwrap();
    let entries = palettes::load(&root).unwrap();
    assert_eq!(entries[0].name, "My colors");
    assert_eq!(entries[0].palette, Palette::preset(2));
    assert!(!root.join("data/config.json").exists());
    let copied = sandbox();
    std::fs::create_dir_all(copied.join("data")).unwrap();
    std::fs::copy(
        root.join("data/palettes.json"),
        copied.join("data/palettes.json"),
    )
    .unwrap();
    assert_eq!(palettes::load(&copied).unwrap(), entries);
}
#[test]
fn replacing_one_name_preserves_other_palettes_and_saved_config() {
    let root = sandbox();
    palettes::save_named(&root, "Custom", Palette::preset(1)).unwrap();
    palettes::save_named(&root, "Another", Palette::preset(2)).unwrap();
    std::fs::write(root.join("data/config.json"), "saved config sentinel").unwrap();
    let entries = palettes::save_named(&root, " custom ", Palette::preset(3)).unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].palette, Palette::preset(3));
    assert_eq!(entries[1].name, "Another");
    assert_eq!(entries[1].palette, Palette::preset(2));
    assert_eq!(
        std::fs::read_to_string(root.join("data/config.json")).unwrap(),
        "saved config sentinel"
    );
    assert!(!root.join("data/palettes.tmp").exists());
}
#[test]
fn malformed_or_unsupported_libraries_and_invalid_names_are_preserved() {
    let root = sandbox();
    palettes::save_named(&root, "Keep", Palette::default()).unwrap();
    let path = root.join("data/palettes.json");
    let before = std::fs::read(&path).unwrap();
    for name in ["", "   ", "Forest", "FOREST", "bad\nname", &"x".repeat(65)] {
        assert!(palettes::save_named(&root, name, Palette::default()).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }
    for bytes in [
        b"{malformed".as_slice(),
        br#"{"version":99,"palettes":[]}"#.as_slice(),
        br#"{"version":1,"palettes":[{"name":"A","palette":{}},{"name":"a","palette":{}}]}"#
            .as_slice(),
    ] {
        std::fs::write(&path, bytes).unwrap();
        assert!(palettes::load(&root).is_err());
        assert!(palettes::save_named(&root, "New", Palette::default()).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }
}
