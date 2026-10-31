use constellation::{
    config::{Config, IconStyle, ItemIcon},
    editing::{self, History},
    icon_composition::compose,
};
#[test]
fn original_alpha_and_decoration_bounds_are_preserved() {
    let mut source = vec![0u8; 64 * 64 * 4];
    for y in 12..52 {
        for x in 12..52 {
            let i = (y * 64 + x) * 4;
            source[i..i + 4].copy_from_slice(&[100, 50, 20, 128]);
        }
    }
    assert_eq!(compose(&source, 64, IconStyle::Original).unwrap(), source);
    let mut rendered = vec![];
    for style in IconStyle::ALL.into_iter().skip(1) {
        let out = compose(&source, 64, style).unwrap();
        assert_eq!(out.len(), source.len());
        assert_eq!(&out[..4], &[0, 0, 0, 0]);
        assert!(out.chunks_exact(4).any(|p| p[3] > 128));
        assert!(out.chunks_exact(4).any(|p| p[3] > 0 && p[3] < 128));
        assert!(
            out.chunks_exact(4)
                .all(|p| p[..3].iter().all(|v| *v <= p[3]))
        );
        assert_ne!(out, source);
        rendered.push(out);
    }
    assert_ne!(rendered[0], rendered[1]);
    assert_ne!(rendered[1], rendered[2]);
    assert!(compose(&[], 0, IconStyle::Folder).is_none());
    assert!(compose(&source, 65, IconStyle::Folder).is_none());
    assert!(compose(&vec![0; 257 * 257 * 4], 257, IconStyle::Folder).is_none());
}
#[test]
fn appearance_round_trips_independently_and_invalidates_preview_with_one_undo() {
    let original = Config::default();
    let raw = serde_json::to_string(&original).unwrap();
    assert!(!raw.contains("icon_style"));
    assert_eq!(serde_json::from_str::<Config>(&raw).unwrap(), original);
    for style in IconStyle::ALL.into_iter().skip(1) {
        let mut changed = original.clone();
        changed.universes[0].icon_style = style;
        changed.universes[0].icon = ItemIcon::Software {
            path: "software/player.exe".into(),
        };
        changed.validate().unwrap();
        let loaded: Config =
            serde_json::from_str(&serde_json::to_string(&changed).unwrap()).unwrap();
        assert_eq!(loaded, changed);
        let mut only_style = original.clone();
        only_style.universes[0].icon_style = style;
        assert!(!editing::preview_equal(&original, &only_style, 0));
        let mut history = History::default();
        history.record(&original, &changed);
        assert_eq!(history.undo(&changed).unwrap(), original);
        assert_eq!(history.redo(&original).unwrap(), changed);
        assert_eq!(
            changed.universes[0].children,
            original.universes[0].children
        );
    }
    let invalid = raw.replacen("\"name\":", "\"icon_style\":\"invalid\",\"name\":", 1);
    assert!(serde_json::from_str::<Config>(&invalid).is_err());
}

#[test]
#[ignore = "device-free visual authoring; requires explicitly supplied local review assets"]
fn export_offscreen_composition_review() {
    let root = std::path::PathBuf::from(
        std::env::var_os("CONSTELLATION_ICON_REVIEW_DIR").expect("review asset folder"),
    );
    for name in ["musicbee", "hydrus"] {
        let source = std::fs::read(root.join(format!("{name}-source.bgra"))).unwrap();
        for (label, style) in ["original", "folder", "cluster", "frame"]
            .into_iter()
            .zip(IconStyle::ALL)
        {
            let out = compose(&source, 64, style).unwrap();
            std::fs::write(root.join(format!("{name}-{label}-rust.bgra")), out).unwrap();
        }
    }
}
