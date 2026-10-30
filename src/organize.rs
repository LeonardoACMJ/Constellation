//! Transactional batch operations, independent of native windows.
use crate::config::{Browser, Config, ItemIcon, Node, Target};
use std::collections::HashSet;
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub id: u64,
    pub universe: usize,
    pub path: String,
    pub branch: bool,
}
pub fn rows(config: &Config) -> Vec<Row> {
    fn walk(n: &Node, u: usize, path: &str, out: &mut Vec<Row>) {
        let path = if path.is_empty() {
            n.name.clone()
        } else {
            format!("{path} › {}", n.name)
        };
        out.push(Row {
            id: n.id,
            universe: u,
            path: path.clone(),
            branch: n.children.is_some(),
        });
        for child in n.children.iter().flatten() {
            walk(child, u, &path, out);
        }
    }
    let mut out = vec![];
    for (u, root) in config.universes.iter().enumerate() {
        walk(root, u, "", &mut out);
    }
    out
}
pub fn move_nodes(config: &mut Config, ids: &[u64], destination: u64) -> Result<usize, String> {
    let chosen: HashSet<_> = ids.iter().copied().collect();
    if chosen.is_empty() {
        return Err("Choose at least one item".into());
    }
    let dest_universe = config
        .universes
        .iter()
        .position(|u| u.find(destination).is_some())
        .ok_or("Choose a destination")?;
    if config
        .find(destination)
        .and_then(|n| n.children.as_ref())
        .is_none()
    {
        return Err("Destination must be a constellation".into());
    }
    for id in &chosen {
        let node = config.find(*id).ok_or("Selected item no longer exists")?;
        if config.universes.iter().any(|u| u.id == *id) {
            return Err("Universe roots cannot be moved".into());
        }
        if node.find(destination).is_some() {
            return Err("Cannot move an item inside itself or its descendants".into());
        }
        let old = config
            .universes
            .iter()
            .position(|u| u.find(*id).is_some())
            .unwrap();
        if old != dest_universe {
            crate::editing::check_delete(config, *id)
                .map_err(|e| format!("Cannot move this entry across universes: {e}"))?;
        }
    }
    fn detach(node: &mut Node, ids: &HashSet<u64>, out: &mut Vec<Node>) {
        if let Some(children) = &mut node.children {
            let mut i = 0;
            while i < children.len() {
                if ids.contains(&children[i].id) {
                    out.push(children.remove(i));
                } else {
                    detach(&mut children[i], ids, out);
                    i += 1;
                }
            }
        }
    }
    let mut candidate = config.clone();
    let mut moving = vec![];
    for root in &mut candidate.universes {
        detach(root, &chosen, &mut moving);
    }
    let count = moving.len();
    candidate.universes[dest_universe]
        .find_mut(destination)
        .unwrap()
        .children
        .as_mut()
        .unwrap()
        .extend(moving);
    candidate
        .validate()
        .map_err(|e| format!("Move would break a profile or visibility restriction: {e}"))?;
    *config = candidate;
    Ok(count)
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PathRef {
    pub node: u64,
    pub label: String,
    pub field: String,
    pub value: String,
    pub command: bool,
}
pub fn paths(config: &Config, universe: usize) -> Vec<PathRef> {
    fn walk(n: &Node, label: &str, out: &mut Vec<PathRef>) {
        let label = if label.is_empty() {
            n.name.clone()
        } else {
            format!("{label} › {}", n.name)
        };
        let mut add = |field: &str, value: &str, command: bool| {
            if !value.is_empty() {
                out.push(PathRef {
                    node: n.id,
                    label: label.clone(),
                    field: field.into(),
                    value: value.into(),
                    command,
                });
            }
        };
        match &n.target {
            Some(Target::Program { path, cwd, .. }) => {
                add("Program", path, true);
                if let Some(cwd) = cwd {
                    add("Working directory", cwd, false);
                }
            }
            Some(Target::Script {
                path, runner, cwd, ..
            }) => {
                add("Script", path, false);
                if let Some(r) = runner {
                    add("Script runner", r, true);
                }
                if let Some(c) = cwd {
                    add("Working directory", c, false);
                }
            }
            Some(Target::Shortcut { path }) => add("Shortcut", path, false),
            Some(Target::Folder { path }) => add("Folder", path, false),
            _ => {}
        }
        match &n.icon {
            ItemIcon::Custom { path } => add("Custom image", path, false),
            ItemIcon::Software { path } => add("Software icon", path, true),
            _ => {}
        }
        if let Some(Browser::Program { path, .. }) = &n.browser {
            add("Browser", path, true);
        }
        for child in n.children.iter().flatten() {
            walk(child, &label, out);
        }
    }
    let mut out = vec![];
    if let Some(root) = config.universes.get(universe) {
        walk(root, "", &mut out);
    }
    out
}
fn replace_prefix(value: &str, old: &str, new: &str) -> Option<String> {
    if old.trim().is_empty() {
        return None;
    }
    let prefix = value.get(..old.len())?;
    if !prefix.eq_ignore_ascii_case(old) {
        return None;
    }
    let tail = &value[old.len()..];
    if !tail.is_empty() && !old.ends_with(['\\', '/']) && !tail.starts_with(['\\', '/']) {
        return None;
    }
    Some(format!("{}{tail}", new.trim_end_matches(['\\', '/'])))
}
pub fn relink_preview(
    config: &Config,
    universe: usize,
    old: &str,
    new: &str,
) -> Result<Vec<(PathRef, String)>, String> {
    if old.trim().is_empty() || new.trim().is_empty() || old.contains('\0') || new.contains('\0') {
        return Err("Enter both folder prefixes".into());
    }
    // Use directory-boundary matches; never replace substrings in URLs/args/notes.
    Ok(paths(config, universe)
        .into_iter()
        .filter_map(|p| {
            replace_prefix(&p.value, old.trim_end_matches(['\\', '/']), new)
                .filter(|v| *v != p.value)
                .map(|v| (p, v))
        })
        .collect())
}
pub fn relink(config: &mut Config, universe: usize, old: &str, new: &str) -> Result<usize, String> {
    let changes = relink_preview(config, universe, old, new)?;
    let mut candidate = config.clone();
    for (reference, value) in &changes {
        let n = candidate.universes[universe]
            .find_mut(reference.node)
            .ok_or("Item disappeared")?;
        match reference.field.as_str() {
            "Custom image" => {
                if let ItemIcon::Custom { path } = &mut n.icon {
                    *path = value.clone();
                }
            }
            "Software icon" => {
                if let ItemIcon::Software { path } = &mut n.icon {
                    *path = value.clone();
                }
            }
            "Browser" => {
                if let Some(Browser::Program { path, .. }) = &mut n.browser {
                    *path = value.clone();
                }
            }
            "Working directory" => match &mut n.target {
                Some(Target::Program { cwd, .. }) | Some(Target::Script { cwd, .. }) => {
                    *cwd = Some(value.clone())
                }
                _ => {}
            },
            "Script runner" => {
                if let Some(Target::Script { runner, .. }) = &mut n.target {
                    *runner = Some(value.clone());
                }
            }
            _ => match &mut n.target {
                Some(Target::Program { path, .. })
                | Some(Target::Script { path, .. })
                | Some(Target::Shortcut { path })
                | Some(Target::Folder { path }) => *path = value.clone(),
                _ => {}
            },
        }
    }
    candidate.validate()?;
    *config = candidate;
    Ok(changes.len())
}
pub fn browser_summary(config: &Config, universe: usize, id: u64) -> String {
    let Some(root) = config.universes.get(universe) else {
        return String::new();
    };
    let Some(n) = root.find(id) else {
        return String::new();
    };
    if id != root.id && !matches!(n.target, Some(Target::Url { .. })) {
        return String::new();
    }
    let inherited = n.id != root.id && n.browser.is_none();
    let browser = n.browser.as_ref().or(if inherited {
        root.browser.as_ref()
    } else {
        None
    });
    let value = match browser {
        Some(Browser::Program { path, .. }) => std::path::Path::new(path)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(path),
        _ => "System default",
    };
    format!(
        "Browser: {value}{}",
        if inherited {
            " · inherited from universe"
        } else {
            ""
        }
    )
}
