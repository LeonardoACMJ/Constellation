//! Bounded resident lifecycle diagnostics. No polling and no input recording.
use std::{
    io::Write,
    path::Path,
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

const MAX_BYTES: u64 = 64 * 1024;
static WRITER: Mutex<()> = Mutex::new(());

pub(crate) fn record(root: &Path, event: &str, detail: &str) {
    // A failing diagnostic must never stop the launcher, including in a panic.
    let Ok(_guard) = WRITER.lock() else { return };
    let data = root.join("data");
    if std::fs::create_dir_all(&data).is_err() {
        return;
    }
    let path = data.join("lifecycle.log");
    if std::fs::metadata(&path).is_ok_and(|m| m.len() >= MAX_BYTES) {
        // One previous segment retains the preceding session across rotation.
        if crate::storage::replace(&path, &data.join("lifecycle.previous.log")).is_err() {
            return;
        }
    }
    let safe = |text: &str| {
        text.chars()
            .filter(|c| !c.is_control())
            .take(240)
            .collect::<String>()
    };
    let time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(
            file,
            "{time} pid={} version={} event={} {}",
            std::process::id(),
            env!("CARGO_PKG_VERSION"),
            safe(event),
            safe(detail)
        );
    }
}

pub(crate) fn install_panic_hook(root: std::path::PathBuf) {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current();
        let location = info
            .location()
            .map(|p| format!("{}:{}", p.file(), p.line()))
            .unwrap_or_default();
        // Deliberately omit the panic payload: it can contain personal config.
        record(
            &root,
            "panic",
            &format!(
                "thread={} location={location}",
                thread.name().unwrap_or("unnamed")
            ),
        );
        previous(info);
    }));
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounded_rotation_retains_context_and_unwritable_logs_are_inert() {
        let root = std::env::current_dir()
            .unwrap()
            .join("target/lifecycle-tests")
            .join(std::process::id().to_string());
        std::fs::create_dir_all(root.join("data")).unwrap();
        let path = root.join("data/lifecycle.log");
        std::fs::write(&path, vec![b'x'; MAX_BYTES as usize]).unwrap();
        record(&root, "start", "no\nforged\rline");
        let first = std::fs::read_to_string(&path).unwrap();
        assert_eq!(first.lines().count(), 1);
        assert!(first.contains("event=start noforgedline"));
        assert_eq!(
            std::fs::metadata(root.join("data/lifecycle.previous.log"))
                .unwrap()
                .len(),
            MAX_BYTES
        );
        record(&root, "exit_requested", "reason=tray_quit");
        assert_eq!(std::fs::read_to_string(&path).unwrap().lines().count(), 2);
        // A file where a directory is expected must not panic or alter that file.
        let blocked = root.join("blocked");
        std::fs::write(&blocked, b"preserve").unwrap();
        record(&blocked, "start", "");
        assert_eq!(std::fs::read(blocked).unwrap(), b"preserve");
    }
}
