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
        // These notes are below the limiter threshold: no compression or
        // voice-count-dependent gain is allowed in their overlapping tails.
        assert!((actual as i32 - (a as i32 + b as i32)).abs() <= 2);
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
    let mut samples = vec![0; RATE as usize / 2];
    m.mix(&mut samples);
    assert!(samples.iter().all(|v| v.unsigned_abs() <= 32767));
    assert_eq!(m.active(), 0);
    assert!(!m.start(0, f64::NAN));
    assert!(!m.start(0, 0.));
}
#[test]
fn changing_output_chunk_sizes_cannot_change_the_pcm_stream() {
    let (mut continuous, mut chunked) = (Mixer::default(), Mixer::default());
    for note in 0..5 {
        continuous.start(note, 0.2);
        chunked.start(note, 0.2);
    }
    let mut expected = vec![0i16; RATE as usize / 2];
    let mut actual = vec![0i16; expected.len()];
    continuous.mix(&mut expected);
    for chunk in actual.chunks_mut(480) {
        chunked.mix(chunk);
    }
    assert_eq!(actual, expected);
}
#[test]
fn legacy_restore_preferences_migrate_and_new_profile_options_roundtrip() {
    let c = Config::default();
    let mut value = serde_json::to_value(&c).unwrap();
    let p = value["profiles"][0].as_object_mut().unwrap();
    p.remove("cursor_end");
    p.insert("restore_cursor".into(), false.into());
    p.remove("directional");
    p.remove("dot_cursor");
    let mut migrated = storage::parse_config(&serde_json::to_vec(&value).unwrap()).unwrap();
    assert_eq!(migrated.profiles[0].cursor_end, CursorEnd::Leave);
    assert!(migrated.profiles[0].directional);
    assert!(migrated.profiles[0].dot_cursor);
    migrated.profiles[0].dot_cursor = false;
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

#[test]
fn chimes_follow_new_lights_instead_of_every_hover_change() {
    for hover in [true, false] {
        let mut c = Config::default();
        c.profiles[0].hover = hover;
        let mut e = Engine::default();
        e.open(0, &c);
        let move_to = |e: &mut Engine, id| {
            let point = e.sectors.iter().find(|s| s.id == id).unwrap().dot;
            let (action, chime) = e.motion_with_feedback(&c, point);
            assert_eq!(e.hovered, Some(id));
            (action, chime)
        };
        assert_eq!(move_to(&mut e, 2), (Action::Changed, true));
        if !hover {
            assert_eq!(e.select(&c), Action::Changed);
        }
        assert_eq!(move_to(&mut e, 3), (Action::Changed, true));
        // MusicBee -> Media: its ancestor light never went out.
        assert_eq!(move_to(&mut e, 2), (Action::Changed, false));
        assert_eq!(move_to(&mut e, 2), (Action::None, false));
        // A Star actually loses its light when returning to the ancestor.
        assert_eq!(move_to(&mut e, 3), (Action::Changed, true));
        assert_eq!(move_to(&mut e, 4), (Action::Changed, true));
        assert_eq!(
            e.motion_with_feedback(&c, Point::default()),
            (Action::Changed, false)
        );
        assert_eq!(move_to(&mut e, 2), (Action::Changed, false));
        assert_eq!(move_to(&mut e, 5), (Action::Changed, true));
        if !hover {
            assert_eq!(e.select(&c), Action::Changed);
        }
        assert_eq!(move_to(&mut e, 6), (Action::Changed, true));
        assert_eq!(move_to(&mut e, 5), (Action::Changed, false));
        assert_eq!(move_to(&mut e, 2), (Action::Changed, false));
        // Both Constellations remain in the lit path, even when visiting the hub.
        e.motion_with_feedback(&c, Point::default());
        assert_eq!(move_to(&mut e, 5), (Action::Changed, false));
        // Switching to a root sibling collapses that path; re-entry is new.
        assert_eq!(move_to(&mut e, 8), (Action::Changed, true));
        assert_eq!(move_to(&mut e, 2), (Action::Changed, true));
        e.close();
        e.open(0, &c);
        assert_eq!(move_to(&mut e, 2), (Action::Changed, true));
    }
}
