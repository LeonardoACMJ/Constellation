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
    let parent = path.parent().ok_or("Choose an export file")?;
    let temp: PathBuf = parent.join(format!(
        ".constellation-export-{}-{}.tmp",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos()
    ));
    let bytes = serde_json::to_vec_pretty(c).map_err(|e| e.to_string())?;
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
