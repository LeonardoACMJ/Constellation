//! Portable custom-image import. Assets stay in the app folder, including
//! when a draft is discarded, so undo/backups never lose referenced images.
use std::{
    collections::hash_map::DefaultHasher,
    fs,
    hash::{Hash, Hasher},
    io::{Read, Write},
    path::Path,
};
pub const MAX_BYTES: u64 = 8 * 1024 * 1024;
pub fn import(root: &Path, source: &Path) -> Result<String, String> {
    let ext = source
        .extension()
        .unwrap_or_default()
        .to_string_lossy()
        .to_ascii_lowercase();
    if !["png", "jpg", "jpeg", "bmp", "ico"].contains(&ext.as_str()) {
        return Err("Choose a PNG, JPEG, BMP or ICO image.".into());
    }
    let mut bytes = Vec::new();
    fs::File::open(source)
        .map_err(|e| format!("Cannot read icon: {e}"))?
        .take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err("Custom icons must be at most 8 MiB.".into());
    }
    let valid = match ext.as_str() {
        "png" => bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
        "jpg" | "jpeg" => bytes.starts_with(&[255, 216, 255]),
        "bmp" => bytes.starts_with(b"BM"),
        "ico" => bytes.starts_with(&[0, 0, 1, 0]),
        _ => false,
    };
    if !valid {
        return Err("The file does not match a supported image format.".into());
    }
    let mut hash = DefaultHasher::new();
    bytes.hash(&mut hash);
    let dir = root.join("data/icons");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    for suffix in 0..1000 {
        let name = format!("{:016x}-{suffix}.{ext}", hash.finish());
        let destination = dir.join(&name);
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&destination)
        {
            Ok(mut file) => {
                if let Err(error) = file.write_all(&bytes).and_then(|_| file.sync_all()) {
                    drop(file);
                    let _ = fs::remove_file(&destination);
                    return Err(format!("Cannot store icon: {error}"));
                }
                return Ok(format!("data/icons/{name}"));
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                if fs::read(&destination).ok().as_deref() == Some(bytes.as_slice()) {
                    return Ok(format!("data/icons/{name}"));
                }
            }
            Err(e) => return Err(format!("Cannot store icon: {e}")),
        }
    }
    Err("Cannot allocate a custom icon filename.".into())
}
