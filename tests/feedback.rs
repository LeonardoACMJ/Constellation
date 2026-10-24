use constellation::{
    config::{Config, CursorEnd},
    engine::{Action, Engine},
    geometry::{self, Point},
    sound::{Mixer, RATE, VOICES},
    storage,
};
#[test]
fn notes_overlap_without_restarting_or_discarding_the_old_tail() {
    let (mut old, mut mixed, mut new) = (Mixer::default(), Mixer::default(), Mixer::default());
    old.start(0, 0.3);
    mixed.start(0, 0.3);
    let mut discarded = [0; 500];
    old.mix(&mut discarded);
    mixed.mix(&mut discarded);
    assert!(mixed.start(1, 0.3));
    new.start(1, 0.3);
    let (mut a, mut b, mut actual) = ([0i16; 128], [0i16; 128], [0i16; 128]);
    old.mix(&mut a);
    new.mix(&mut b);
    mixed.mix(&mut actual);
    assert_eq!(mixed.active(), 2);
    for ((a, b), actual) in a.into_iter().zip(b).zip(actual) {
        let inverse = |x: i16| x as f64 / (32767. - (x as f64).abs());
        let sum = inverse(a) + inverse(b);
        let expected = 32767. * sum / (1. + sum.abs());
        assert!((actual as f64 - expected).abs() < 5.);
    }
    let mut tail = vec![0; (RATE as usize) / 2];
    mixed.mix(&mut tail);
    assert_eq!(mixed.active(), 0);
    assert!(tail[tail.len() - 100..].iter().all(|v| *v == 0));
}
#[test]
fn mixer_saturation_keeps_existing_voices_and_bounds_output() {
    let mut m = Mixer::default();
    for i in 0..VOICES {
        assert!(m.start(i % 5, 1.));
    }
    assert!(!m.start(0, 1.));
    assert_eq!(m.active(), VOICES);
    let mut samples = vec![0; 5000];
    m.mix(&mut samples);
    assert!(samples.iter().all(|v| v.unsigned_abs() < 32767));
    assert_eq!(m.active(), 0);
    assert!(!m.start(0, f64::NAN));
    assert!(!m.start(0, 0.));
}
#[test]
fn legacy_restore_preferences_migrate_and_new_profile_options_roundtrip() {
    let c = Config::default();
    let mut value = serde_json::to_value(&c).unwrap();
    let p = value["profiles"][0].as_object_mut().unwrap();
    p.remove("cursor_end");
    p.insert("restore_cursor".into(), false.into());
    p.remove("directional");
    let mut migrated = storage::parse_config(&serde_json::to_vec(&value).unwrap()).unwrap();
    assert_eq!(migrated.profiles[0].cursor_end, CursorEnd::Leave);
    assert!(migrated.profiles[0].directional);
    migrated.profiles[0].cursor_end = CursorEnd::Center;
    assert_eq!(
        storage::parse_config(&serde_json::to_vec(&migrated).unwrap()).unwrap(),
        migrated
    );
}
#[test]
fn extended_direction_selects_stars_and_branches_but_preserves_hub_and_gaps() {
    let mut c = Config::default();
    let mut e = Engine::default();
    e.open(0, &c);
    let star = e
        .sectors
        .iter()
        .find(|s| c.find(s.id).unwrap().target.is_some())
        .unwrap()
        .clone();
    let far = star.center.polar(star.outer * 5., star.angle);
    assert!(geometry::hit(&e.sectors, far).is_none());
    e.motion(&c, far);
    assert_eq!(e.hovered, Some(star.id));
    assert_eq!(e.select(&c), Action::Launch(star.id));
    c.profiles[0].directional = false;
    e.motion(&c, Point::default());
    e.motion(&c, far);
    assert_eq!(e.hovered, None);
    c.profiles[0].directional = true;
    let folder = e.sectors.iter().find(|s| s.id == 2).unwrap().clone();
    e.motion(&c, folder.center.polar(folder.outer * 5., folder.angle));
    assert_eq!(e.path, vec![2]);
    let child = e.sectors.iter().find(|s| s.depth == 1).unwrap().clone();
    let child_far = child.center.polar(child.outer * 5., child.angle);
    e.motion(&c, child_far);
    assert_eq!(e.hovered, Some(child.id));
    e.motion(&c, Point::default());
    assert_eq!(e.hovered, None);
    let root = geometry::visible(&c, &[]);
    let s = &root[0];
    let gap = s
        .center
        .polar(s.outer * 5., s.from - c.layout.gap / c.layout.radius * 0.25);
    assert!(geometry::directional_hit(&root, gap).is_none());
}
