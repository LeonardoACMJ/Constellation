use crate::config::Config;
use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

pub fn root() -> io::Result<PathBuf> {
    std::env::current_exe()?
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| io::Error::other("Executable folder unavailable"))
}
pub fn resolve(root: &Path, path: &str) -> PathBuf {
    let p = Path::new(path);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        root.join(p)
    }
}
/// Exact on-disk baseline; missing files are distinct from empty files.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Revision(Option<Vec<u8>>);
impl Revision {
    pub fn exists(&self) -> bool {
        self.0.is_some()
    }
}
pub fn revision(root: &Path) -> Result<Revision, String> {
    match fs::read(root.join("data/config.json")) {
        Ok(bytes) => Ok(Revision(Some(bytes))),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Revision(None)),
        Err(e) => Err(format!("Cannot inspect portable config: {e}")),
    }
}
pub fn recovery_notice(root: &Path) -> Option<String> {
    read(root)
        .err()
        .filter(|_| root.join("data/recovery.txt").is_file())
        .map(|_| "Recovered from backup. The original config is damaged; Save to repair it.".into())
}
pub fn save_checked(root: &Path, c: &Config, expected: &Revision) -> Result<Revision, String> {
    let mut c = c.clone();
    c.assign_profile_ids()?;
    let bytes = serde_json::to_vec_pretty(&c).map_err(|e| e.to_string())?;
    save_inner(root, &c, Some(expected))?;
    Ok(Revision(Some(bytes)))
}
pub fn read(root: &Path) -> Result<Config, String> {
    let bytes = fs::read(root.join("data/config.json"))
        .map_err(|e| format!("Cannot read portable config: {e}"))?;
    parse_config(&bytes)
}
/// A 64-level node tree also contains children arrays, exceeding Serde JSON's
/// default 128-container limit. Bound lexical nesting before using its deeper
/// deserializer, then enforce the stricter semantic node-depth limit.
pub fn parse_config(bytes: &[u8]) -> Result<Config, String> {
    use serde::Deserialize;
    let (mut depth, mut quoted, mut escaped) = (0usize, false, false);
    for &b in bytes {
        if quoted {
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                quoted = false;
            }
        } else if b == b'"' {
            quoted = true;
        } else if matches!(b, b'{' | b'[') {
            depth += 1;
            if depth > 256 {
                return Err("JSON nesting exceeds parser safety limit (256 containers)".into());
            }
        } else if matches!(b, b'}' | b']') {
            depth = depth.saturating_sub(1);
        }
    }
    let mut decoder = serde_json::Deserializer::from_slice(bytes);
    decoder.disable_recursion_limit();
    let mut c =
        Config::deserialize(&mut decoder).map_err(|e| format!("Invalid config JSON: {e}"))?;
    decoder
        .end()
        .map_err(|e| format!("Invalid config JSON: {e}"))?;
    c.assign_profile_ids()?;
    c.validate()?;
    Ok(c)
}

pub fn load_or_create(root: &Path) -> Result<Config, String> {
    let file = root.join("data/config.json");
    if file
        .try_exists()
        .map_err(|e| format!("Cannot access portable config: {e}"))?
    {
        match read(root) {
            Ok(c) => Ok(c),
            Err(original) => {
                let backup =
                    fs::read(root.join("data/config.backup.json")).map_err(|_| original.clone())?;
                let c = parse_config(&backup).map_err(|_| original.clone())?;
                // Preserve the broken original for repair; never write a recovered
                // backup over it without an explicit Save from configuration.
                let _ = fs::write(
                    root.join("data/recovery.txt"),
                    format!(
                        "{original}\nLoaded last valid backup; repair config.json or Save in configuration.\n"
                    ),
                );
                Ok(c)
            }
        }
    } else {
        let c = Config::default();
        save(root, &c)?;
        Ok(c)
    }
}
static SAVE_ID: AtomicU64 = AtomicU64::new(0);
pub fn save(root: &Path, c: &Config) -> Result<(), String> {
    save_inner(root, c, None)
}
fn save_inner(root: &Path, c: &Config, expected: Option<&Revision>) -> Result<(), String> {
    let check = || -> io::Result<()> {
        if let Some(expected) = expected {
            let current = revision(root).map_err(io::Error::other)?;
            if &current != expected {
                return Err(io::Error::other(
                    "Configuration changed outside this editor. Your draft is preserved. Export it before Reload, then reconcile the two versions.",
                ));
            }
        }
        Ok(())
    };
    check().map_err(|e| e.to_string())?;
    let mut c = c.clone();
    c.assign_profile_ids()?;
    c.validate()?;
    let dir = root.join("data");
    fs::create_dir_all(&dir).map_err(|e| format!("Portable folder is not writable: {e}"))?;
    let bytes = serde_json::to_vec_pretty(&c).map_err(|e| e.to_string())?;
    let path = dir.join("config.json");
    // Same-directory temp + atomic replacement; a failed replacement preserves the old file.
    let temp = dir.join(format!(
        "config-{}-{}.tmp",
        std::process::id(),
        SAVE_ID.fetch_add(1, Ordering::Relaxed)
    ));
    use std::io::Write;
    let result = (|| -> io::Result<()> {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        // A repaired Save must not replace a known-good backup with the
        // malformed file from which recovery was necessary.
        if path.exists() && read(root).is_ok() {
            fs::copy(&path, dir.join("config.backup.json"))?;
        }
        check()?;
        replace(&temp, &path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
        .map_err(|e| format!("Could not save inside portable folder; no external fallback: {e}"))?;
    let _ = fs::remove_file(dir.join("recovery.txt"));
    Ok(())
}
#[cfg(not(windows))]
pub(crate) fn replace(from: &Path, to: &Path) -> io::Result<()> {
    fs::rename(from, to)
}
#[cfg(windows)]
pub(crate) fn replace(from: &Path, to: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };
    let a: Vec<u16> = from.as_os_str().encode_wide().chain(Some(0)).collect();
    let b: Vec<u16> = to.as_os_str().encode_wide().chain(Some(0)).collect();
    if unsafe {
        MoveFileExW(
            a.as_ptr(),
            b.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    } == 0
    {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}
