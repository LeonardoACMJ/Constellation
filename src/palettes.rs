//! Named user palettes are editor assets, separate from the active configuration.
use crate::{storage, theme::Palette};
use serde::{Deserialize, Serialize};
use std::{io::Read, path::Path};

pub const BUILTINS: [&str; 4] = ["Constellation (default)", "Forest", "Rose dusk", "Daylight"];
const MAX_BYTES: u64 = 256 * 1024;
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NamedPalette {
    pub name: String,
    pub palette: Palette,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Library {
    version: u32,
    palettes: Vec<NamedPalette>,
}
fn key(name: &str) -> String {
    name.trim().to_lowercase()
}
fn validate_name(name: &str) -> Result<(), String> {
    if name.is_empty() || name.chars().count() > 64 || name.chars().any(crate::config::note_break) {
        return Err("Use a palette name of 1–64 characters on one line".into());
    }
    if BUILTINS.iter().any(|builtin| key(builtin) == key(name)) {
        return Err("Choose a name different from the built-in palettes".into());
    }
    Ok(())
}
pub fn load(root: &Path) -> Result<Vec<NamedPalette>, String> {
    let file = match std::fs::File::open(root.join("data/palettes.json")) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(e.to_string()),
    };
    let mut bytes = vec![];
    file.take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err("Palette library exceeds 256 KiB".into());
    }
    let library: Library =
        serde_json::from_slice(&bytes).map_err(|e| format!("Cannot read palette library: {e}"))?;
    if library.version != 1 || library.palettes.len() > 128 {
        return Err("Unsupported palette library".into());
    }
    let mut names = std::collections::HashSet::new();
    for entry in &library.palettes {
        validate_name(&entry.name)?;
        if !names.insert(key(&entry.name)) {
            return Err("Duplicate names in palette library".into());
        }
    }
    Ok(library.palettes)
}
pub fn save_named(root: &Path, name: &str, palette: Palette) -> Result<Vec<NamedPalette>, String> {
    let name = name.trim();
    validate_name(name)?;
    if palette.colors().iter().any(|c| c.0 > 0xffffff) {
        return Err("Palette colors must be six-digit RGB values".into());
    }
    // Re-read so saving one preset preserves other presets changed since opening.
    let mut palettes = load(root)?;
    let entry = NamedPalette {
        name: name.into(),
        palette,
    };
    if let Some(existing) = palettes.iter_mut().find(|p| key(&p.name) == key(name)) {
        *existing = entry;
    } else {
        if palettes.len() >= 128 {
            return Err("Palette library already contains 128 palettes".into());
        }
        palettes.push(entry);
    }
    let bytes = serde_json::to_vec_pretty(&Library {
        version: 1,
        palettes: palettes.clone(),
    })
    .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err("Palette library exceeds 256 KiB".into());
    }
    std::fs::create_dir_all(root.join("data")).map_err(|e| e.to_string())?;
    let temporary = root.join("data/palettes.tmp");
    let result = (|| -> Result<(), String> {
        use std::io::Write;
        let mut file = std::fs::File::create(&temporary).map_err(|e| e.to_string())?;
        file.write_all(&bytes).map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
        drop(file);
        storage::replace(&temporary, &root.join("data/palettes.json")).map_err(|e| e.to_string())
    })();
    let _ = std::fs::remove_file(&temporary);
    result?;
    Ok(palettes)
}
