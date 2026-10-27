//! Repeatable release-mode core stress sample. Never opens a window or target.
use constellation::{
    config::{Config, Node, Target},
    engine::Engine,
    geometry,
};
use serde_json::json;
use std::time::Instant;
fn star(id: u64) -> Node {
    Node {
        browser: None,
        note: String::new(),
        icon: Default::default(),
        visible_in: vec![],
        id,
        name: format!("Star {id}"),
        children: None,
        target: Some(Target::Folder { path: ".".into() }),
    }
}
fn balanced(depth: usize, width: usize, id: &mut u64) -> Node {
    *id += 1;
    let own = *id;
    if depth == 0 {
        return star(own);
    }
    Node {
        browser: None,
        note: String::new(),
        icon: Default::default(),
        visible_in: vec![],
        id: own,
        name: format!("Branch {own}"),
        children: Some((0..width).map(|_| balanced(depth - 1, width, id)).collect()),
        target: None,
    }
}
fn main() {
    let mut rows = vec![];
    let mut cases = vec![];
    for count in [100, 1000, 10000] {
        let mut c = Config::default();
        c.universes[0].children = Some((2..count + 2).map(star).collect());
        c.layout.gap = 0.;
        cases.push((format!("flat-{count}"), c));
    }
    let mut c = Config::default();
    c.universes[0] = balanced(4, 8, &mut 0);
    cases.push(("balanced-4681".into(), c));
    let mut c = Config::default();
    c.universes[0] = balanced(64, 1, &mut 0);
    cases.push(("deep-64".into(), c));
    for (name, c) in cases {
        let then = Instant::now();
        c.validate().unwrap();
        let validate = then.elapsed();
        let then = Instant::now();
        let all = geometry::full(&c);
        let layout = then.elapsed();
        let then = Instant::now();
        let warnings = geometry::warnings(&c, 1920., 1080.);
        let warn = then.elapsed();
        let mut e = Engine::default();
        let then = Instant::now();
        assert!(e.open(0, &c));
        let open = then.elapsed();
        let roots = e.sectors.clone();
        let then = Instant::now();
        for i in 0..1000 {
            if let Some(s) = roots.get(i % roots.len().max(1)) {
                e.motion(&c, s.dot);
            }
        }
        let motion = then.elapsed();
        fn folders(node: &Node, ids: &mut std::collections::HashSet<u64>) {
            if let Some(children) = &node.children {
                ids.insert(node.id);
                for child in children {
                    folders(child, ids);
                }
            }
        }
        let mut folder_ids = std::collections::HashSet::new();
        folders(&c.universes[0], &mut folder_ids);
        let then = Instant::now();
        let mut deep = Engine::default();
        deep.open(0, &c);
        let mut traversed = 0;
        while let Some(s) = deep
            .sectors
            .iter()
            .rev()
            .find(|s| folder_ids.contains(&s.id))
            .cloned()
        {
            let before = deep.path.len();
            deep.motion(&c, s.dot);
            if deep.path.len() <= before {
                break;
            }
            traversed += 1;
            if traversed >= 64 {
                break;
            }
        }
        let traversal = then.elapsed();
        rows.push(json!({"case":name,"sectors":all.len(),"validate_ms":validate.as_secs_f64()*1000.,
            "layout_ms":layout.as_secs_f64()*1000.,"warnings_ms":warn.as_secs_f64()*1000.,
            "open_ms":open.as_secs_f64()*1000.,"motion_1000_ms":motion.as_secs_f64()*1000.,
            "deep_navigation_ms":traversal.as_secs_f64()*1000.,"traversed_levels":traversed,"warnings":warnings}));
    }
    let result = json!({"conditions":"Release build; pure Rust core; 1920x1080 warning area; no native rendering or launch latency measured","cases":rows});
    std::fs::create_dir_all(".local/stress").unwrap();
    std::fs::write(
        ".local/stress/metrics.json",
        serde_json::to_vec_pretty(&result).unwrap(),
    )
    .unwrap();
    println!("{}", serde_json::to_string_pretty(&result).unwrap());
}
