//! Event-driven configuration loading. No filesystem access on invocation,
//! no idle polling, and only the latest prepared snapshot is retained.
use super::wide;
use crate::{
    config::{Binding, Config},
    storage,
};
use std::{
    mem::zeroed,
    path::{Path, PathBuf},
    ptr::{null, null_mut},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::*,
    Storage::FileSystem::*,
    System::{IO::*, Threading::*},
    UI::WindowsAndMessaging::{PostMessageW, WM_APP},
};

pub const READY: u32 = WM_APP + 62;
pub struct Snapshot {
    pub config: Config,
    pub bindings: Vec<Binding>,
}
fn read(root: &Path) -> Result<Snapshot, String> {
    let config = storage::read(root)?;
    let bindings = config
        .profiles
        .iter()
        .map(|p| Binding::parse(&p.binding))
        .collect::<Result<_, _>>()?;
    Ok(Snapshot { config, bindings })
}

struct Handle(HANDLE);
impl Handle {
    fn event(manual: bool) -> Result<Self, String> {
        let handle = unsafe { CreateEventW(null(), manual as i32, 0, null()) };
        if handle.is_null() {
            Err("Cannot create configuration reload event".into())
        } else {
            Ok(Self(handle))
        }
    }
}
unsafe impl Send for Handle {}
unsafe impl Sync for Handle {}
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

#[derive(Default)]
struct Mailbox {
    result: Option<Result<Snapshot, String>>,
    notified: bool,
}
impl Mailbox {
    fn publish(&mut self, value: Result<Snapshot, String>) -> bool {
        self.result = Some(value);
        let notify = !self.notified;
        self.notified = true;
        notify
    }
    fn take(&mut self, gesture_active: bool) -> Option<Result<Snapshot, String>> {
        if gesture_active {
            return None;
        }
        self.notified = false;
        self.result.take()
    }
}
struct Shared {
    wake: Handle,
    alive: AtomicBool,
    mailbox: Mutex<Mailbox>,
}
pub struct Worker {
    shared: Arc<Shared>,
}
impl Worker {
    pub fn start(hwnd: HWND, root: PathBuf) -> Result<Self, String> {
        let window = hwnd as usize;
        Self::start_using(root, move || unsafe {
            PostMessageW(window as HWND, READY, 0, 0);
        })
    }
    fn start_using(root: PathBuf, notify: impl Fn() + Send + 'static) -> Result<Self, String> {
        let shared = Arc::new(Shared {
            wake: Handle::event(false)?,
            alive: AtomicBool::new(true),
            mailbox: Mutex::new(Mailbox::default()),
        });
        let background = shared.clone();
        std::thread::Builder::new()
            .name("constellation-config".into())
            // Reserve parsing headroom for the supported 64-level tree. Windows
            // commits stack pages on demand; this is not 2 MiB of idle RAM.
            .stack_size(2 * 1024 * 1024)
            .spawn(move || run(background, root, notify))
            .map_err(|e| format!("Cannot start configuration reload worker: {e}"))?;
        Ok(Self { shared })
    }
    pub fn request(&self) {
        unsafe {
            SetEvent(self.shared.wake.0);
        }
    }
    pub fn take_ready(&self, gesture_active: bool) -> Option<Result<Snapshot, String>> {
        self.shared.mailbox.lock().ok()?.take(gesture_active)
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.shared.alive.store(false, Ordering::Release);
        unsafe {
            SetEvent(self.shared.wake.0);
        }
        // Never join filesystem work from the resident/UI thread. All handles,
        // buffers and overlapped cancellation remain owned by the worker.
    }
}

struct Directory {
    file: Handle,
    event: Handle,
    overlapped: Box<OVERLAPPED>,
    buffer: Box<[u32; 4096]>,
    pending: bool,
}
impl Directory {
    unsafe fn open(path: &Path) -> Result<Self, String> {
        let file = CreateFileW(
            wide(path).as_ptr(),
            FILE_LIST_DIRECTORY,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OVERLAPPED,
            null_mut(),
        );
        if file == INVALID_HANDLE_VALUE {
            return Err(format!(
                "Cannot watch configuration directory: {}",
                std::io::Error::last_os_error()
            ));
        }
        let mut directory = Self {
            file: Handle(file),
            event: Handle::event(true)?,
            overlapped: Box::new(zeroed()),
            buffer: Box::new([0; 4096]),
            pending: false,
        };
        directory.arm()?;
        Ok(directory)
    }
    unsafe fn arm(&mut self) -> Result<(), String> {
        ResetEvent(self.event.0);
        *self.overlapped = zeroed();
        self.overlapped.hEvent = self.event.0;
        if ReadDirectoryChangesW(
            self.file.0,
            self.buffer.as_mut_ptr().cast(),
            size_of_val(self.buffer.as_ref()) as u32,
            0,
            FILE_NOTIFY_CHANGE_FILE_NAME | FILE_NOTIFY_CHANGE_LAST_WRITE | FILE_NOTIFY_CHANGE_SIZE,
            null_mut(),
            self.overlapped.as_mut(),
            None,
        ) == 0
        {
            return Err(format!(
                "Cannot subscribe to configuration changes: {}",
                std::io::Error::last_os_error()
            ));
        }
        self.pending = true;
        Ok(())
    }
    unsafe fn changed(&mut self) -> Result<bool, String> {
        let mut count = 0;
        let ok = GetOverlappedResult(self.file.0, self.overlapped.as_ref(), &mut count, 0);
        self.pending = false;
        if ok == 0 {
            return Err("Configuration directory notifications stopped".into());
        }
        let bytes = std::slice::from_raw_parts(self.buffer.as_ptr().cast::<u8>(), count as usize);
        let changed = touches_config(bytes);
        // Rearm before reading/parsing: a second Save must not be missed while
        // the first snapshot is being prepared.
        self.arm()?;
        Ok(changed)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        unsafe {
            if self.pending {
                CancelIoEx(self.file.0, self.overlapped.as_ref());
                let mut count = 0;
                GetOverlappedResult(self.file.0, self.overlapped.as_ref(), &mut count, 1);
            }
        }
    }
}
fn touches_config(mut bytes: &[u8]) -> bool {
    // An overflow/unknown record means re-read, never silently miss a change.
    if bytes.is_empty() {
        return true;
    }
    loop {
        if bytes.len() < 12 {
            return true;
        }
        let number =
            |offset| u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
        let (next, len) = (number(0), number(8));
        if len % 2 != 0 || len > bytes.len() - 12 {
            return true;
        }
        let name: Vec<_> = bytes[12..12 + len]
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        if String::from_utf16_lossy(&name).eq_ignore_ascii_case("config.json") {
            return true;
        }
        if next == 0 {
            return false;
        }
        if next < 12 + len || next >= bytes.len() {
            return true;
        }
        bytes = &bytes[next..];
    }
}
fn run(shared: Arc<Shared>, root: PathBuf, notify: impl Fn()) {
    unsafe {
        let publish = |result| {
            if !shared.alive.load(Ordering::Acquire) {
                return;
            }
            let signal = shared
                .mailbox
                .lock()
                .is_ok_and(|mut slot| slot.publish(result));
            if signal {
                notify();
            }
        };
        let mut directory = match Directory::open(&root.join("data")) {
            Ok(watch) => Some(watch),
            Err(error) => {
                publish(Err(format!(
                    "{error}. Settings Save still reloads; external edits require restarting Constellation."
                )));
                None
            }
        };
        // Close the startup load→watch gap, entirely in the background.
        let mut load = directory.is_some();
        let mut retry: Option<(Instant, u8)> = None;
        while shared.alive.load(Ordering::Acquire) {
            if load {
                match read(&root) {
                    Ok(snapshot) => {
                        publish(Ok(snapshot));
                        retry = None;
                    }
                    Err(error) => {
                        let attempt = retry.map_or(0, |(_, attempt)| attempt);
                        if attempt < 3 {
                            // Only bounded recovery from an incomplete external write;
                            // no idle polling and no delay in opening/navigation.
                            retry = Some((Instant::now() + Duration::from_millis(75), attempt + 1));
                        } else {
                            publish(Err(error));
                            retry = None;
                        }
                    }
                }
            }
            load = false;
            let mut handles = vec![shared.wake.0];
            if let Some(watch) = &directory {
                handles.push(watch.event.0);
            }
            let timeout = retry.map_or(INFINITE, |(deadline, _)| {
                deadline
                    .saturating_duration_since(Instant::now())
                    .as_millis()
                    .min(u32::MAX as u128 - 1) as u32
            });
            match WaitForMultipleObjects(handles.len() as u32, handles.as_ptr(), 0, timeout) {
                WAIT_OBJECT_0 => {
                    load = true;
                    retry = None;
                }
                value if value == WAIT_OBJECT_0 + 1 => {
                    match directory.as_mut().unwrap().changed() {
                        Ok(true) => {
                            load = true;
                            retry = None;
                        }
                        Ok(false) => {}
                        Err(error) => {
                            directory = None;
                            publish(Err(format!(
                                "{error}. Settings Save still reloads; external edits require restarting Constellation."
                            )));
                        }
                    }
                }
                WAIT_TIMEOUT => load = true,
                _ => {
                    publish(Err(
                        "Configuration reload wait failed; restart Constellation".into(),
                    ));
                    break;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    static FIXTURE_SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    fn fixture_root() -> PathBuf {
        // SystemTime can have coarse resolution on hosted Windows runners;
        // parallel tests also need a process-local uniqueness guarantee.
        let sequence = FIXTURE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "constellation-config-worker-{}-{}-{sequence}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }
    fn receive_until(
        receiver: &std::sync::mpsc::Receiver<()>,
        worker: &Worker,
        mut accept: impl FnMut(&Result<Snapshot, String>) -> bool,
    ) -> Result<Snapshot, String> {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let timeout = deadline.saturating_duration_since(Instant::now());
            receiver
                .recv_timeout(timeout)
                .expect("configuration worker did not publish the expected state");
            let Some(result) = worker.take_ready(false) else {
                continue;
            };
            if accept(&result) {
                return result;
            }
        }
    }
    fn record(name: &str) -> Vec<u8> {
        let name: Vec<_> = name.encode_utf16().collect();
        let mut bytes = vec![0; 12];
        bytes[8..12].copy_from_slice(&(name.len() as u32 * 2).to_le_bytes());
        for c in name {
            bytes.extend(c.to_le_bytes());
        }
        bytes
    }
    #[test]
    fn filters_logs_backups_and_icons_but_keeps_renames_and_overflow() {
        for name in [
            "input-state.log",
            "audio-trace.log",
            "config.backup.json",
            "config-123.tmp",
        ] {
            assert!(!touches_config(&record(name)));
        }
        assert!(touches_config(&record("CONFIG.JSON")));
        let mut first = record("old.tmp");
        let offset = first.len() as u32;
        first[..4].copy_from_slice(&offset.to_le_bytes());
        first.extend(record("config.json"));
        assert!(touches_config(&first));
        assert!(touches_config(&[]));
        assert!(touches_config(&[0; 5]));
    }
    #[test]
    fn latest_snapshot_waits_for_gesture_close_without_unbounded_queueing() {
        let mut slot = Mailbox::default();
        assert!(slot.publish(Err("old error".into())));
        assert!(!slot.publish(Ok(Snapshot {
            config: Config::default(),
            bindings: vec![]
        })));
        assert!(slot.take(true).is_none());
        assert!(slot.notified);
        assert!(slot.take(false).unwrap().is_ok());
        assert!(slot.take(false).is_none());
        assert!(slot.publish(Err("new error".into())));
    }
    #[test]
    fn watches_atomic_saves_recovers_invalid_edits_and_accepts_explicit_refresh() {
        use std::sync::mpsc;
        let root = fixture_root();
        std::fs::create_dir_all(&root).unwrap();
        let mut config = Config::default();
        storage::save(&root, &config).unwrap();
        let (tx, rx) = mpsc::channel();
        let worker = Worker::start_using(root.clone(), move || {
            let _ = tx.send(());
        })
        .unwrap();
        let receive = || {
            rx.recv_timeout(Duration::from_secs(5)).unwrap();
            worker.take_ready(false).unwrap()
        };
        assert_eq!(receive().unwrap().config, config);
        std::fs::write(root.join("data/input-state.log"), "unrelated").unwrap();
        assert!(rx.recv_timeout(Duration::from_millis(100)).is_err());
        config.universes[0].name = "Reloaded atomically".into();
        storage::save(&root, &config).unwrap();
        assert_eq!(
            receive_until(&rx, &worker, |result| {
                result
                    .as_ref()
                    .is_ok_and(|snapshot| snapshot.config == config)
            })
            .unwrap()
            .config,
            config
        );
        std::fs::write(root.join("data/config.json"), b"{broken").unwrap();
        assert!(receive_until(&rx, &worker, |result| result.is_err()).is_err());
        storage::save(&root, &config).unwrap();
        assert_eq!(
            receive_until(&rx, &worker, |result| {
                result
                    .as_ref()
                    .is_ok_and(|snapshot| snapshot.config == config)
            })
            .unwrap()
            .config,
            config
        );
        loop {
            match rx.recv_timeout(Duration::from_millis(100)) {
                Ok(()) => {
                    let _ = worker.take_ready(false);
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => break,
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    panic!("configuration worker closed its notification channel")
                }
            }
        }
        worker.request();
        assert_eq!(
            receive_until(&rx, &worker, |result| {
                result
                    .as_ref()
                    .is_ok_and(|snapshot| snapshot.config == config)
            })
            .unwrap()
            .config,
            config
        );
        drop(worker);
        // The worker owns cancellation; no user data or native UI is touched.
        // Fixture files can remain if its asynchronous shutdown still holds a handle.
        let _ = std::fs::remove_file(root.join("data/config.json"));
        let _ = std::fs::remove_file(root.join("data/config.backup.json"));
        let _ = std::fs::remove_file(root.join("data/input-state.log"));
        let _ = std::fs::remove_dir(root.join("data"));
        let _ = std::fs::remove_dir(root);
    }
    #[test]
    fn explicit_save_still_reloads_when_directory_watch_is_unavailable() {
        let root = fixture_root();
        let (tx, rx) = std::sync::mpsc::channel();
        let worker = Worker::start_using(root.clone(), move || {
            let _ = tx.send(());
        })
        .unwrap();
        rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(worker.take_ready(false).unwrap().is_err());
        let config = Config::default();
        storage::save(&root, &config).unwrap();
        worker.request();
        rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(worker.take_ready(false).unwrap().unwrap().config, config);
        drop(worker);
        let _ = std::fs::remove_file(root.join("data/config.json"));
        let _ = std::fs::remove_dir(root.join("data"));
        let _ = std::fs::remove_dir(root);
    }
    #[test]
    fn worker_stack_supports_the_maximum_valid_tree_depth() {
        let root = fixture_root();
        let mut config = Config::default();
        let mut base = config.universes[0].clone();
        base.children = Some(vec![]);
        let mut branch = base.clone();
        branch.id = 1000;
        for id in (1001..1064).rev() {
            let mut parent = base.clone();
            parent.id = id;
            parent.children = Some(vec![branch]);
            branch = parent;
        }
        config.universes[0].children = Some(vec![branch]);
        storage::save(&root, &config).unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let worker = Worker::start_using(root.clone(), move || {
            let _ = tx.send(());
        })
        .unwrap();
        rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(worker.take_ready(false).unwrap().unwrap().config, config);
        drop(worker);
        let _ = std::fs::remove_file(root.join("data/config.json"));
        let _ = std::fs::remove_dir(root.join("data"));
        let _ = std::fs::remove_dir(root);
    }
}
