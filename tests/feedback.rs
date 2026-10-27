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

#[test]
fn instrument_defaults_persist_and_participate_in_history() {
    use constellation::{config::Instrument, editing::History};
    let original = Config::default();
    let old_json = serde_json::to_value(&original).unwrap();
    assert!(old_json["sound"].get("instrument").is_none());
    assert_eq!(
        storage::parse_config(&serde_json::to_vec(&old_json).unwrap())
            .unwrap()
            .sound
            .instrument,
        Instrument::Mallet
    );
    let mut changed = original.clone();
    changed.sound.instrument = Instrument::ClassicBell;
    let saved = serde_json::to_vec(&changed).unwrap();
    assert_eq!(storage::parse_config(&saved).unwrap(), changed);
    let mut history = History::default();
    history.record(&original, &changed);
    assert_eq!(history.undo(&changed).unwrap(), original);
    assert_eq!(history.redo(&original).unwrap(), changed);
    let mut invalid = serde_json::to_value(&changed).unwrap();
    invalid["sound"]["instrument"] = "missing_bank".into();
    assert!(storage::parse_config(&serde_json::to_vec(&invalid).unwrap()).is_err());
}

#[test]
fn switching_instruments_changes_new_notes_but_keeps_existing_tails() {
    use constellation::config::Instrument;
    let (mut mixed, mut old, mut classic) = (
        Mixer::default(),
        Mixer::default(),
        Mixer::new(Instrument::ClassicBell),
    );
    mixed.start(0, 0.2);
    old.start(0, 0.2);
    mixed.mix(&mut [0; 500]);
    old.mix(&mut [0; 500]);
    mixed.set_instrument(Instrument::ClassicBell);
    assert_eq!(mixed.active(), 1);
    let (mut a, mut b) = ([0; 128], [0; 128]);
    mixed.mix(&mut a);
    old.mix(&mut b);
    assert_eq!(
        a, b,
        "Changing the bank cannot cut or retune an existing note"
    );
    mixed.start(2, 0.2);
    classic.start(2, 0.2);
    let (mut old_tail, mut new_tone, mut actual) = ([0; 128], [0; 128], [0; 128]);
    old.mix(&mut old_tail);
    classic.mix(&mut new_tone);
    mixed.mix(&mut actual);
    assert_ne!(old_tail, new_tone);
    for i in 0..128 {
        assert!((actual[i] as i32 - old_tail[i] as i32 - new_tone[i] as i32).abs() <= 2);
    }
    mixed.set_instrument(Instrument::Mallet);
    mixed.mix(&mut vec![0; RATE as usize / 2]);
    assert_eq!(mixed.active(), 0);
}

#[test]
fn all_four_instruments_roundtrip_and_keep_tails_on_switch() {
    use constellation::config::Instrument;
    let instruments = [
        Instrument::Mallet,
        Instrument::ClassicBell,
        Instrument::PluckedStrings,
        Instrument::WindChimes,
    ];
    let mut c = Config::default();
    let mut mixed = Mixer::default();
    for instrument in instruments {
        c.sound.instrument = instrument;
        assert_eq!(
            storage::parse_config(&serde_json::to_vec(&c).unwrap()).unwrap(),
            c
        );
        mixed.set_instrument(instrument);
        mixed.start(2, 0.2);
        mixed.mix(&mut [0; 480]);
    }
    assert_eq!(mixed.active(), 4);
    mixed.mix(&mut vec![0; RATE as usize]);
    assert_eq!(mixed.active(), 0);
}

#[test]
fn none_sound_preserves_legacy_mute_volume_and_last_instrument() {
    use constellation::{config::Instrument, editing::History};
    let mut legacy = Config::default();
    legacy.sound.enabled = false;
    legacy.sound.volume = 0.37;
    legacy.sound.instrument = Instrument::WindChimes;
    let mut loaded = storage::parse_config(&serde_json::to_vec(&legacy).unwrap()).unwrap();
    assert_eq!(loaded.sound.selected_instrument(), None);
    loaded.sound.select_instrument(None);
    assert_eq!(loaded, legacy);
    assert_eq!(
        storage::parse_config(&serde_json::to_vec(&loaded).unwrap()).unwrap(),
        legacy
    );
    loaded
        .sound
        .select_instrument(Some(Instrument::PluckedStrings));
    assert_eq!(
        loaded.sound.selected_instrument(),
        Some(Instrument::PluckedStrings)
    );
    assert_eq!(loaded.sound.volume, 0.37);
    let mut history = History::default();
    history.record(&legacy, &loaded);
    assert_eq!(history.undo(&loaded).unwrap(), legacy);
    assert_eq!(history.redo(&legacy).unwrap(), loaded);
    loaded.sound.select_instrument(None);
    assert!(!loaded.sound.enabled);
    assert_eq!(loaded.sound.instrument, Instrument::PluckedStrings);
    assert_eq!(loaded.sound.volume, 0.37);
}

// Six root sectors reproduce the Yeetus/Web overlap: the child fan's inner
// hole crosses an older sibling even though its outward direction is correct.
fn guarded_fan_fixture() -> Config {
    use constellation::config::Node;
    let mut c = Config::default();
    let star = c.find(3).unwrap().clone();
    let nodes = (0..6)
        .map(|i| {
            let mut child = star.clone();
            child.id = 100 + i;
            Node {
                browser: None,
                id: 10 + i,
                name: format!("Branch {i}"),
                note: String::new(),
                icon: Default::default(),
                visible_in: vec![],
                target: None,
                children: Some(vec![child]),
            }
        })
        .collect();
    c.universes[0].children = Some(nodes);
    c
}

#[test]
fn open_fan_guards_both_edges_without_delays_or_stale_launches() {
    use constellation::config::Mode;
    for directional in [false, true] {
        for hover in [false, true] {
            for side in [-1., 1.] {
                let mut c = guarded_fan_fixture();
                c.profiles[0].directional = directional;
                c.profiles[0].hover = hover;
                let mut e = Engine::default();
                e.open(0, &c);
                let parent = e.sectors.iter().find(|s| s.id == 14).unwrap().clone();
                e.motion(&c, parent.dot);
                if !hover {
                    e.select(&c);
                }
                let child = e.sectors.iter().find(|s| s.id == 104).unwrap().clone();
                e.motion(&c, child.dot);
                assert_eq!(e.select(&c), Action::Launch(104));
                // Both inside the angular corridor and 4° beyond its edge.
                for offset in [-4_f64, 4.] {
                    let p = child.center.polar(
                        60.,
                        parent.angle + side * (c.layout.spread / 2. + offset).to_radians(),
                    );
                    let raw = geometry::hit(&e.sectors, p).unwrap();
                    assert_eq!(raw.depth, 0);
                    assert_ne!(raw.id, parent.id);
                    let (_, chime) = e.motion_with_feedback(&c, p);
                    assert!(!chime);
                    assert_eq!(e.path, vec![14]);
                    assert_eq!(e.hovered, None);
                    assert_eq!(e.select(&c), Action::None);
                }
                // 11° beyond the edge crosses the 8° margin immediately.
                let outside = child.center.polar(
                    60.,
                    parent.angle + side * (c.layout.spread / 2. + 11.).to_radians(),
                );
                let sibling = geometry::hit(&e.sectors, outside).unwrap().id;
                let (_, chime) = e.motion_with_feedback(&c, outside);
                assert!(chime);
                assert_eq!(e.hovered, Some(sibling));
                if hover {
                    assert_eq!(e.path, vec![sibling]);
                } else {
                    assert_eq!(e.path, vec![14]);
                    e.select(&c);
                    assert_eq!(e.path, vec![sibling]);
                }
                e.root(&c);
                e.motion(&c, parent.dot);
                if !hover {
                    e.select(&c);
                }
                e.motion(&c, Point::default());
                assert_eq!(e.hovered, None);
                e.back(&c);
                assert!(e.path.is_empty());
                // A blocked corridor never launches the child on Hold release.
                c.profiles[0].mode = Mode::Hold;
                e.motion(&c, parent.dot);
                if !hover {
                    e.select(&c);
                }
                e.motion(&c, child.dot);
                let p = child.center.polar(
                    60.,
                    parent.angle + side * (c.layout.spread / 2. + 4.).to_radians(),
                );
                e.motion(&c, p);
                assert_eq!(e.release(&c), Action::Changed);
                assert!(e.profile.is_none());
            }
        }
    }
}

#[test]
fn empty_fans_do_not_guard_older_siblings() {
    let mut c = guarded_fan_fixture();
    c.universes[0].find_mut(14).unwrap().children = Some(vec![]);
    let mut e = Engine::default();
    e.open(0, &c);
    let parent = e.sectors.iter().find(|s| s.id == 14).unwrap().clone();
    e.motion(&c, parent.dot);
    assert_eq!(e.path, vec![14]);
    let center = parent
        .center
        .polar(c.layout.radius * c.layout.distance, parent.angle);
    let p = center.polar(60., parent.angle + (c.layout.spread / 2. + 4.).to_radians());
    let sibling = geometry::hit(&e.sectors, p).unwrap().id;
    e.motion(&c, p);
    assert_eq!(e.hovered, Some(sibling));
    assert_eq!(e.path, vec![sibling]);
}

#[test]
fn nested_fan_protects_against_ancestor_stars() {
    let mut c = guarded_fan_fixture();
    c.layout.distance = 0.2;
    let mut nested = c.find(14).unwrap().clone();
    nested.id = 104;
    for node in nested.children.as_mut().unwrap() {
        node.id = 204;
    }
    c.universes[0].find_mut(14).unwrap().children = Some(vec![nested]);
    // Root siblings are Stars, so a stray hover would collapse both fans.
    for node in c.universes[0].children.as_mut().unwrap() {
        if node.id != 14 {
            node.target = node.children.as_ref().unwrap()[0].target.clone();
            node.children = None;
        }
    }
    let mut e = Engine::default();
    e.open(0, &c);
    for id in [14, 104, 204] {
        let p = e.sectors.iter().find(|s| s.id == id).unwrap().dot;
        e.motion(&c, p);
    }
    assert_eq!(e.path, vec![14, 104]);
    let parent = e.sectors.iter().find(|s| s.id == 104).unwrap().clone();
    let child = e.sectors.iter().find(|s| s.id == 204).unwrap().clone();
    let p = child
        .center
        .polar(60., parent.angle + (c.layout.spread / 2. + 4.).to_radians());
    let raw = geometry::hit(&e.sectors, p).unwrap();
    assert_eq!(raw.depth, 0);
    assert!(c.find(raw.id).unwrap().target.is_some());
    assert_eq!(e.motion_with_feedback(&c, p), (Action::Changed, false));
    assert_eq!(e.path, vec![14, 104]);
    assert_eq!(e.select(&c), Action::None);
    e.back(&c);
    assert_eq!(e.path, vec![14]);
    e.root(&c);
    assert!(e.path.is_empty());
}
