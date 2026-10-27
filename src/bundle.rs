//! Portable configuration documents with embedded custom images. Software,
//! scripts and launch targets remain external references, never copied/run.
use crate::{
    config::{Config, ItemIcon, Node},
    editing, icon_assets, storage,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    io::Read,
    path::Path,
};
const TOTAL: usize = 64 * 1024 * 1024;
const DOCUMENT: u64 = 144 * 1024 * 1024;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Bundle {
    format: String,
    version: u32,
    config: String,
    icons: Vec<Icon>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Icon {
    reference: String,
    extension: String,
    hex: String,
}
fn paths(c: &Config) -> HashSet<String> {
    fn visit(n: &Node, out: &mut HashSet<String>) {
        if let ItemIcon::Custom { path } = &n.icon {
            out.insert(path.clone());
        }
        for child in n.children.iter().flatten() {
            visit(child, out);
        }
    }
    let mut out = HashSet::new();
    for n in &c.universes {
        visit(n, &mut out);
    }
    out
}
fn decode(text: &str) -> Result<Vec<u8>, String> {
    if text.len() % 2 != 0 || text.len() > icon_assets::MAX_BYTES as usize * 2 {
        return Err("Invalid or oversized bundled image".into());
    }
    fn digit(b: u8) -> Result<u8, String> {
        match b {
            b'0'..=b'9' => Ok(b - b'0'),
            b'a'..=b'f' => Ok(b - b'a' + 10),
            _ => Err("Invalid image encoding".into()),
        }
    }
    text.as_bytes()
        .chunks_exact(2)
        .map(|p| Ok(digit(p[0])? * 16 + digit(p[1])?))
        .collect()
}
pub fn export(path: &Path, root: &Path, c: &Config) -> Result<(), String> {
    c.validate()?;
    let mut icons = Vec::new();
    let mut total = 0;
    let mut references: Vec<_> = paths(c).into_iter().collect();
    references.sort();
    for reference in references {
        let source = storage::resolve(root, &reference);
        let mut bytes = Vec::new();
        std::fs::File::open(&source)
            .map_err(|e| format!("Cannot bundle custom image {reference}: {e}"))?
            .take(icon_assets::MAX_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        total += bytes.len();
        if bytes.len() as u64 > icon_assets::MAX_BYTES || total > TOTAL {
            return Err(
                "Custom images exceed the bundle limit (8 MiB per image, 64 MiB total)".into(),
            );
        }
        let extension = source
            .extension()
            .and_then(|e| e.to_str())
            .ok_or("Custom image needs a supported extension")?
            .to_ascii_lowercase();
        icon_assets::validate_bytes(&extension, &bytes)?;
        let mut hex = String::with_capacity(bytes.len() * 2);
        const DIGITS: &[u8] = b"0123456789abcdef";
        for b in bytes {
            hex.push(DIGITS[(b >> 4) as usize] as char);
            hex.push(DIGITS[(b & 15) as usize] as char);
        }
        icons.push(Icon {
            reference,
            extension,
            hex,
        });
    }
    let document = Bundle {
        format: "constellation-bundle".into(),
        version: 1,
        config: serde_json::to_string(c).map_err(|e| e.to_string())?,
        icons,
    };
    let bytes = serde_json::to_vec(&document).map_err(|e| e.to_string())?;
    if bytes.len() as u64 > DOCUMENT {
        return Err("Bundle document is too large".into());
    }
    editing::export_bytes(path, &bytes)
}
pub fn import(path: &Path, root: &Path) -> Result<Config, String> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(|e| e.to_string())?
        .take(DOCUMENT + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > DOCUMENT {
        return Err("Bundle document is too large".into());
    }
    let document: Bundle =
        serde_json::from_slice(&bytes).map_err(|e| format!("Invalid bundle: {e}"))?;
    if document.format != "constellation-bundle" || document.version != 1 {
        return Err("Unsupported bundle format/version".into());
    }
    let mut config = storage::parse_config(document.config.as_bytes())?;
    let expected = paths(&config);
    let mut decoded = HashMap::new();
    let mut total = 0;
    for icon in document.icons {
        if !expected.contains(&icon.reference) || decoded.contains_key(&icon.reference) {
            return Err("Unexpected or duplicate bundled image".into());
        }
        let bytes = decode(&icon.hex)?;
        total += bytes.len();
        if total > TOTAL {
            return Err("Bundled images exceed 64 MiB".into());
        }
        icon_assets::validate_bytes(&icon.extension, &bytes)?;
        decoded.insert(icon.reference, (icon.extension, bytes));
    }
    if decoded.len() != expected.len() {
        return Err("Bundle is missing a custom image".into());
    }
    // No reference is treated as an output path. The importer allocates its own
    // safe content-addressed names under data/icons, preserving existing files.
    let mut mapped = HashMap::new();
    for (reference, (extension, bytes)) in decoded {
        mapped.insert(
            reference,
            icon_assets::import_bytes(root, &extension, &bytes)?,
        );
    }
    fn remap(n: &mut Node, map: &HashMap<String, String>) {
        if let ItemIcon::Custom { path } = &mut n.icon {
            *path = map[path].clone();
        }
        for child in n.children.iter_mut().flatten() {
            remap(child, map);
        }
    }
    for root in &mut config.universes {
        remap(root, &mapped);
    }
    config.validate()?;
    Ok(config)
}
