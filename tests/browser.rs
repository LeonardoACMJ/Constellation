use constellation::{
    config::{Browser, Config, ItemIcon, Target},
    editing::{self, History},
    storage,
};
fn sample() -> (Config, u64) {
    let mut c = Config::default();
    let mut node = c.universes[0]
        .children
        .as_ref()
        .unwrap()
        .iter()
        .find(|n| n.children.is_none())
        .unwrap()
        .clone();
    node.id = c.max_node_id() + 1;
    node.target = Some(Target::Url {
        url: "https://example.org/path?q=hello%20world&x=1".into(),
    });
    node.icon = ItemIcon::Automatic;
    let id = node.id;
    c.universes[0].children.as_mut().unwrap().push(node);
    (c, id)
}
#[test]
fn browser_inheritance_override_and_dispatch_preserve_one_url_argument() {
    let (mut c, id) = sample();
    let original = c.find(id).unwrap().target.clone().unwrap();
    assert_eq!(c.launch_target(id), Some(original.clone()));
    c.universes[0].browser = Some(Browser::Program {
        path: "browsers/edge.exe".into(),
        args: vec!["--profile-directory=Work profile".into()],
    });
    let target = c.launch_target(id).unwrap();
    let Target::Program { path, args, cwd } = target else {
        panic!()
    };
    assert_eq!(path, "browsers/edge.exe");
    assert_eq!(args.len(), 2);
    assert!(args[1].ends_with("&x=1"));
    assert_eq!(cwd, None);
    c.universes[0].find_mut(id).unwrap().browser = Some(Browser::System);
    assert_eq!(c.launch_target(id), Some(original));
    c.universes[0].find_mut(id).unwrap().browser = Some(Browser::Program {
        path: "other.exe".into(),
        args: vec![],
    });
    assert!(matches!(c.launch_target(id),Some(Target::Program{path,..}) if path=="other.exe"));
    c.validate().unwrap();
    let bytes = serde_json::to_vec(&c).unwrap();
    assert_eq!(storage::parse_config(&bytes).unwrap(), c);
}
#[test]
fn old_configs_stay_unchanged_and_browser_edits_copy_and_undo() {
    let (before, id) = sample();
    assert!(!serde_json::to_string(&before).unwrap().contains("browser"));
    let mut c = before.clone();
    c.universes[0].find_mut(id).unwrap().browser = Some(Browser::System);
    let mut history = History::default();
    history.record(&before, &c);
    assert_eq!(history.undo(&c).unwrap(), before);
    let copy = editing::duplicate_node(&mut c, 0, id).unwrap();
    assert_eq!(c.find(copy).unwrap().browser, Some(Browser::System));
}
#[test]
fn browser_scope_and_arguments_are_validated_without_probing_disk() {
    let (mut c, id) = sample();
    c.universes[0].browser = Some(Browser::Program {
        path: "missing-but-allowed.exe".into(),
        args: vec![],
    });
    c.validate().unwrap();
    c.universes[0].find_mut(id).unwrap().browser = Some(Browser::Program {
        path: "".into(),
        args: vec![],
    });
    assert!(c.validate().is_err());
    c.universes[0].find_mut(id).unwrap().browser = None;
    let branch = c.universes[0]
        .children
        .as_mut()
        .unwrap()
        .iter_mut()
        .find(|n| n.children.is_some())
        .unwrap();
    branch.browser = Some(Browser::System);
    assert!(c.validate().is_err());
}

#[test]
fn independent_universes_keep_their_own_browser_preferences() {
    let (mut c, id) = sample();
    c.universes[0].browser = Some(Browser::Program {
        path: "first.exe".into(),
        args: vec![],
    });
    let mut second = c.universes[0].clone();
    fn renew(n: &mut constellation::config::Node) {
        n.id += 1000;
        for c in n.children.iter_mut().flatten() {
            renew(c);
        }
    }
    renew(&mut second);
    second.browser = Some(Browser::Program {
        path: "second.exe".into(),
        args: vec![],
    });
    c.universes.push(second);
    c.validate().unwrap();
    assert!(matches!(c.launch_target(id),Some(Target::Program{path,..}) if path=="first.exe"));
    assert!(
        matches!(c.launch_target(id+1000),Some(Target::Program{path,..}) if path=="second.exe")
    );
}
