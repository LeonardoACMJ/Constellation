//! Portable editor operations; none run in the resident event loop.
use crate::config::{Config, Node, Target};
use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
};

/// Normalize pasted/typed note text immediately, before it reaches Save.
pub fn note_input(text: &str) -> String {
    text.chars()
        .map(|c| if crate::config::note_break(c) { ' ' } else { c })
        .take(crate::config::NOTE_MAX_CHARS)
        .collect()
}

/// Ctrl+Backspace boundary in the UTF-16 offsets used by native EDIT controls.
/// Consume trailing whitespace, then a word or punctuation run. A surrogate
/// pair is one character, so emoji are never split into invalid UTF-16.
pub fn previous_word_start(text: &[u16], caret: usize) -> usize {
    let caret = caret.min(text.len());
    let prefix = String::from_utf16_lossy(&text[..caret]);
    let mut chars = prefix.chars().rev().peekable();
    let mut start = caret;
    while chars.peek().is_some_and(|c| c.is_whitespace()) {
        start -= chars.next().unwrap().len_utf16();
    }
    let word = |c: char| c.is_alphanumeric() || c == '_';
    if let Some(&last) = chars.peek() {
        let kind = word(last);
        while chars
            .peek()
            .is_some_and(|c| !c.is_whitespace() && word(*c) == kind)
        {
            start -= chars.next().unwrap().len_utf16();
        }
    }
    start
}

/// Double quotes group arguments; backslashes before quotes use Windows/CRT
/// escaping. This is a string-vector editor, never a shell command evaluator.
pub fn parse_arguments(text: &str) -> Result<Vec<String>, String> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = vec![];
    let mut i = 0;
    while i < chars.len() {
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        if i == chars.len() {
            break;
        }
        let mut arg = String::new();
        let mut quoted = false;
        while i < chars.len() && (quoted || !chars[i].is_whitespace()) {
            if chars[i] == '\\' {
                let start = i;
                while i < chars.len() && chars[i] == '\\' {
                    i += 1;
                }
                let count = i - start;
                if i < chars.len() && chars[i] == '"' {
                    arg.extend(std::iter::repeat_n('\\', count / 2));
                    if count % 2 == 1 {
                        arg.push('"');
                        i += 1;
                    } else {
                        quoted = !quoted;
                        i += 1;
                    }
                } else {
                    arg.extend(std::iter::repeat_n('\\', count));
                }
            } else if chars[i] == '"' {
                if quoted && chars.get(i + 1) == Some(&'"') {
                    arg.push('"');
                    i += 2;
                } else {
                    quoted = !quoted;
                    i += 1;
                }
            } else {
                arg.push(chars[i]);
                i += 1;
            }
        }
        if quoted {
            return Err(
                "Close the double quote in Arguments. Example: --profile \"Work profile\"".into(),
            );
        }
        if arg.contains('\0') {
            return Err("Arguments cannot contain a null character".into());
        }
        out.push(arg);
    }
    Ok(out)
}
pub fn format_arguments(args: &[String]) -> String {
    args.iter()
        .map(|arg| {
            if !arg.is_empty() && !arg.chars().any(|c| c.is_whitespace() || c == '"') {
                return arg.clone();
            }
            let mut result = String::from("\"");
            let mut slashes = 0;
            for c in arg.chars() {
                if c == '\\' {
                    slashes += 1;
                    continue;
                }
                result.extend(std::iter::repeat_n(
                    '\\',
                    if c == '"' { slashes * 2 + 1 } else { slashes },
                ));
                result.push(c);
                slashes = 0;
            }
            result.extend(std::iter::repeat_n('\\', slashes * 2));
            result.push('"');
            result
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[derive(Default)]
pub struct History {
    undo: VecDeque<(Config, usize)>,
    redo: VecDeque<(Config, usize)>,
}
impl History {
    const LIMIT: usize = 64;
    const BYTES: usize = 16 * 1024 * 1024;
    fn push(queue: &mut VecDeque<(Config, usize)>, value: &Config) {
        let size = serde_json::to_vec(value).map_or(0, |v| v.len());
        queue.push_back((value.clone(), size));
        while queue.len() > Self::LIMIT
            || queue.len() > 1 && queue.iter().map(|(_, n)| n).sum::<usize>() > Self::BYTES
        {
            queue.pop_front();
        }
    }
    pub fn record(&mut self, before: &Config, after: &Config) {
        if before == after {
            return;
        }
        Self::push(&mut self.undo, before);
        self.redo.clear();
    }
    pub fn undo(&mut self, current: &Config) -> Option<Config> {
        let (previous, _) = self.undo.pop_back()?;
        Self::push(&mut self.redo, current);
        Some(previous)
    }
    pub fn redo(&mut self, current: &Config) -> Option<Config> {
        let (next, _) = self.redo.pop_back()?;
        Self::push(&mut self.undo, current);
        Some(next)
    }
}

pub fn portable_path(root: &Path, file: &Path) -> String {
    file.strip_prefix(root)
        .unwrap_or(file)
        .to_string_lossy()
        .into_owned()
}
pub fn star_from_path(root: &Path, path: &Path, id: u64) -> Result<Node, String> {
    let value = portable_path(root, path);
    let target = if path.is_dir() {
        Target::Folder { path: value }
    } else if path.is_file() {
        match path
            .extension()
            .unwrap_or_default()
            .to_string_lossy()
            .to_ascii_lowercase()
            .as_str()
        {
            "exe" | "com" => Target::Program {
                path: value,
                args: vec![],
                cwd: None,
            },
            "ps1" | "py" | "cmd" | "bat" | "sh" => Target::Script {
                path: value,
                runner: None,
                args: vec![],
                cwd: None,
            },
            _ => Target::Shortcut { path: value },
        }
    } else {
        return Err(format!("Dropped path is unavailable: {}", path.display()));
    };
    Ok(Node {
        browser: None,
        note: String::new(),
        icon: Default::default(),
        visible_in: vec![],
        id,
        name: if path.is_dir() {
            path.file_name()
        } else {
            path.file_stem()
        }
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned(),
        children: None,
        target: Some(target),
    })
}

/// Advisory checks only. Skip network paths to avoid stalling the editor on an
/// unavailable UNC server. Actual availability is checked again at dispatch.
pub fn target_advice(root: &Path, target: &Target) -> Vec<String> {
    fn check(root: &Path, value: &str, folder: bool, program: bool) -> Option<String> {
        if value.starts_with("\\\\") || value.starts_with("//") {
            return Some("Network path: availability is checked at launch.".into());
        }
        let p = Path::new(value);
        if program && p.components().count() == 1 && !value.contains(['\\', '/']) {
            if std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()).any(|d| {
                let v = d.to_string_lossy();
                !v.starts_with("\\\\")
                    && (d.join(value).is_file()
                        || cfg!(windows) && d.join(format!("{value}.exe")).is_file())
            }) {
                return None;
            }
        }
        let p = crate::storage::resolve(root, value);
        if (folder && p.is_dir()) || (!folder && p.is_file()) {
            None
        } else {
            Some(format!("Not found: {value}. You can still save it."))
        }
    }
    let mut result = vec![];
    let (path, folder, program, cwd, runner) = match target {
        Target::Program { path, cwd, .. } => (path, false, true, cwd.as_deref(), None),
        Target::Shortcut { path } => (path, false, false, None, None),
        Target::Folder { path } => (path, true, false, None, None),
        Target::Script {
            path, cwd, runner, ..
        } => (path, false, false, cwd.as_deref(), runner.as_deref()),
        Target::Url { .. } => return result,
    };
    if let Some(s) = check(root, path, folder, program) {
        result.push(s);
    }
    if let Some(cwd) = cwd {
        if let Some(s) = check(root, cwd, true, false) {
            result.push(s);
        }
    }
    if let Some(runner) = runner {
        if let Some(s) = check(root, runner, false, true) {
            result.push(s);
        }
    }
    result
}

pub fn read_import(path: &Path) -> Result<Config, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("Cannot read import: {e}"))?;
    crate::storage::parse_config(&bytes).map_err(|e| format!("Invalid import: {e}"))
}
pub fn export(path: &Path, c: &Config) -> Result<(), String> {
    c.validate()?;
    let bytes = serde_json::to_vec_pretty(c).map_err(|e| e.to_string())?;
    export_bytes(path, &bytes)
}
pub(crate) fn export_bytes(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path.parent().ok_or("Choose an export file")?;
    let temp: PathBuf = parent.join(format!(
        ".constellation-export-{}-{}.tmp",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos()
    ));
    let result = (|| -> std::io::Result<()> {
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        crate::storage::replace(&temp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result.map_err(|e| format!("Cannot export configuration: {e}"))
}

/// Full ancestry by stable ID, including the selected item. Names may repeat.
pub fn node_path(root: &Node, id: u64) -> Vec<&Node> {
    fn walk<'a>(node: &'a Node, id: u64, path: &mut Vec<&'a Node>) -> bool {
        path.push(node);
        if node.id == id {
            return true;
        }
        if node
            .children
            .iter()
            .flatten()
            .any(|child| walk(child, id, path))
        {
            return true;
        }
        path.pop();
        false
    }
    let mut path = Vec::new();
    walk(root, id, &mut path);
    path
}

/// Duplicate a node beside its source, with fresh IDs throughout its subtree.
/// Work on a candidate so validation/ID exhaustion never partly changes a draft.
pub fn duplicate_node(config: &mut Config, universe: usize, id: u64) -> Result<u64, String> {
    let root = config.universes.get(universe).ok_or("Choose a universe")?;
    let path = node_path(root, id);
    if path.len() < 2 {
        return Err("Select a Star or Constellation inside the universe to duplicate".into());
    }
    let parent = path[path.len() - 2].id;
    let mut copy = (*path.last().unwrap()).clone();
    let mut next = config.max_node_id();
    fn renew(node: &mut Node, next: &mut u64) -> Result<(), String> {
        *next = next.checked_add(1).ok_or("Item ID space exhausted")?;
        node.id = *next;
        for child in node.children.iter_mut().flatten() {
            renew(child, next)?;
        }
        Ok(())
    }
    renew(&mut copy, &mut next)?;
    copy.name.push_str(" (copy)");
    let new_id = copy.id;
    let mut candidate = config.clone();
    let children = candidate.universes[universe]
        .find_mut(parent)
        .and_then(|n| n.children.as_mut())
        .ok_or("Parent is not a constellation")?;
    let index = children
        .iter()
        .position(|n| n.id == id)
        .ok_or("Select an item")?;
    children.insert(index + 1, copy);
    candidate.validate()?;
    *config = candidate;
    Ok(new_id)
}

/// Refuse deletion before a draft can acquire dangling profile entries.
pub fn check_delete(config: &Config, id: u64) -> Result<(), String> {
    let node = config.find(id).ok_or("Item no longer exists")?;
    let affected: Vec<_> = config
        .profiles
        .iter()
        .filter(|p| p.root_id.is_some_and(|entry| node.find(entry).is_some()))
        .map(|p| p.name.as_str())
        .collect();
    if !affected.is_empty() {
        return Err(format!(
            "Used as an entry by: {}. Change Opens at in Shortcut profiles before deleting this branch.",
            affected.join(", ")
        ));
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeRow {
    pub id: u64,
    pub parent: Option<u64>,
    pub name: String,
    pub branch: bool,
}
pub fn tree_rows(root: &Node) -> Vec<TreeRow> {
    fn walk(n: &Node, parent: Option<u64>, out: &mut Vec<TreeRow>) {
        out.push(TreeRow {
            id: n.id,
            parent,
            name: n.name.clone(),
            branch: n.children.is_some(),
        });
        if let Some(children) = &n.children {
            for child in children {
                walk(child, Some(n.id), out);
            }
        }
    }
    let mut out = Vec::new();
    walk(root, None, &mut out);
    out
}
pub fn same_tree_structure(a: &[TreeRow], b: &[TreeRow]) -> bool {
    a.len() == b.len()
        && a.iter()
            .zip(b)
            .all(|(a, b)| (a.id, a.parent, a.branch) == (b.id, b.parent, b.branch))
}

/// The full preview has no profile filter and never displays notes or sound.
/// Keep metadata edits from rerasterizing the whole fan-out.
pub fn preview_equal(a: &Config, b: &Config, universe: usize) -> bool {
    a.layout == b.layout && a.theme == b.theme && preview_tree_equal(a, b, universe)
}
pub fn preview_tree_equal(a: &Config, b: &Config, universe: usize) -> bool {
    fn nodes(a: &Node, b: &Node) -> bool {
        a.id == b.id
            && a.name == b.name
            && a.icon == b.icon
            && (!matches!(a.icon, crate::config::ItemIcon::Automatic) || a.target == b.target)
            && match (&a.children, &b.children) {
                (None, None) => true,
                (Some(a), Some(b)) => {
                    a.len() == b.len() && a.iter().zip(b).all(|(a, b)| nodes(a, b))
                }
                _ => false,
            }
    }
    a.universes
        .get(universe)
        .zip(b.universes.get(universe))
        .is_some_and(|(a, b)| nodes(a, b))
}

#[derive(Debug, PartialEq, Eq)]
pub enum EditorShortcut {
    Save,
    Duplicate,
    Rename,
    Undo,
    Redo,
}
#[derive(Clone, Copy)]
pub enum EditorFocus {
    Tree,
    Text,
    Other,
}
pub fn editor_shortcut(
    key: u32,
    ctrl: bool,
    shift: bool,
    alt: bool,
    focus: EditorFocus,
) -> Option<EditorShortcut> {
    if alt || shift {
        return None;
    }
    use EditorFocus::*;
    match (key, ctrl, focus) {
        (0x53, true, _) => Some(EditorShortcut::Save),
        (0x44, true, Tree) => Some(EditorShortcut::Duplicate),
        (0x71, false, Tree) => Some(EditorShortcut::Rename),
        (0x5a, true, Tree | Other) => Some(EditorShortcut::Undo),
        (0x59, true, Tree | Other) => Some(EditorShortcut::Redo),
        _ => None,
    }
}
