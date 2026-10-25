//! Bounded asynchronous icon cache. Shell handlers and image decoding never
//! run in a paint/input handler; failures retain the ordinary star symbol.
#![allow(unsafe_op_in_unsafe_fn)]
use super::wide;
use crate::{
    config::{ItemIcon, Node, Target},
    storage,
};
use std::{
    collections::HashMap,
    mem::{size_of, zeroed},
    path::{Path, PathBuf},
    ptr::{null, null_mut},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{SyncSender, sync_channel},
    },
};
use windows_sys::Win32::{
    Foundation::*,
    Graphics::{Gdi::*, GdiPlus::*},
    Storage::FileSystem::{FILE_ATTRIBUTE_NORMAL, SearchPathW},
    System::Com::*,
    UI::{Shell::*, WindowsAndMessaging::*},
};

pub const READY: u32 = WM_APP + 41;
const SIZE: usize = 64;
const CAPACITY: usize = 128;
const PARGB: i32 = (11 | (32 << 8) | PixelFormatGDI | PixelFormatAlpha | PixelFormatPAlpha) as i32;
pub struct Pixels {
    pub bytes: Vec<u8>,
}
pub type Images = HashMap<u64, Arc<Pixels>>;
#[derive(Clone, Hash, PartialEq, Eq)]
pub enum Source {
    Program(String),
    File(String),
    Association(String),
    Custom(String),
}
fn source(node: &Node) -> Option<Source> {
    match &node.icon {
        ItemIcon::Symbol => None,
        ItemIcon::Software { path } => Some(Source::Program(path.clone())),
        ItemIcon::Custom { path } => Some(Source::Custom(path.clone())),
        ItemIcon::Automatic => match node.target.as_ref()? {
            Target::Program { path, .. } => Some(Source::Program(path.clone())),
            Target::Shortcut { path } | Target::Folder { path } | Target::Script { path, .. } => {
                Some(Source::File(path.clone()))
            }
            Target::Url { .. } => Some(Source::Association(".url".into())),
        },
    }
}
struct Entry {
    image: Option<Arc<Pixels>>,
    pending: bool,
    used: u64,
}
#[derive(Default)]
struct State {
    entries: HashMap<Source, Entry>,
    clock: u64,
}
pub struct Cache {
    state: Arc<Mutex<State>>,
    channel: Option<SyncSender<Source>>,
    alive: Arc<AtomicBool>,
    notified: Arc<AtomicBool>,
}
impl Cache {
    pub fn new(hwnd: HWND, root: PathBuf) -> Self {
        let state = Arc::new(Mutex::new(State::default()));
        let alive = Arc::new(AtomicBool::new(true));
        let notified = Arc::new(AtomicBool::new(false));
        let (tx, rx) = sync_channel::<Source>(CAPACITY);
        let (worker_state, worker_alive, worker_notified) =
            (state.clone(), alive.clone(), notified.clone());
        let window = hwnd as usize;
        let started = std::thread::Builder::new()
            .name("constellation-icons".into())
            .stack_size(512 * 1024)
            .spawn(move || unsafe {
                let com = CoInitializeEx(null(), COINIT_APARTMENTTHREADED as u32);
                let mut token = 0;
                let gdiplus = GdiplusStartup(
                    &mut token,
                    &GdiplusStartupInput {
                        GdiplusVersion: 1,
                        SuppressExternalCodecs: 1,
                        ..zeroed()
                    },
                    null_mut(),
                ) == Ok;
                while let Result::Ok(key) = rx.recv() {
                    if !worker_alive.load(Ordering::Acquire) {
                        break;
                    }
                    let image = load(&root, &key, gdiplus).map(Arc::new);
                    if let Result::Ok(mut state) = worker_state.lock() {
                        if let Some(entry) = state.entries.get_mut(&key) {
                            entry.image = image;
                            entry.pending = false;
                        }
                    }
                    if worker_alive.load(Ordering::Acquire)
                        && !worker_notified.swap(true, Ordering::AcqRel)
                    {
                        if PostMessageW(window as HWND, READY, 0, 0) == 0 {
                            worker_notified.store(false, Ordering::Release);
                        }
                    }
                }
                if gdiplus {
                    GdiplusShutdown(token);
                }
                if com >= 0 {
                    CoUninitialize();
                }
            })
            .is_ok();
        Self {
            state,
            channel: started.then_some(tx),
            alive,
            notified,
        }
    }
    pub fn acknowledge(&self) {
        self.notified.store(false, Ordering::Release);
    }
    pub fn failed(&self, node: &Node) -> bool {
        let Some(key) = source(node) else {
            return false;
        };
        self.state.try_lock().ok().is_some_and(|state| {
            state
                .entries
                .get(&key)
                .is_some_and(|entry| !entry.pending && entry.image.is_none())
        })
    }
    fn get(&self, node: &Node) -> Option<Arc<Pixels>> {
        let key = source(node)?;
        let mut state = self.state.try_lock().ok()?;
        state.clock = state.clock.wrapping_add(1);
        let used = state.clock;
        if let Some(entry) = state.entries.get_mut(&key) {
            entry.used = used;
            return entry.image.clone();
        }
        if state.entries.len() >= CAPACITY {
            let oldest = state
                .entries
                .iter()
                .filter(|(_, e)| !e.pending)
                .min_by_key(|(_, e)| e.used)
                .map(|(key, _)| key.clone());
            state.entries.remove(&oldest?);
        }
        self.channel.as_ref()?.try_send(key.clone()).ok()?;
        state.entries.insert(
            key,
            Entry {
                image: None,
                pending: true,
                used,
            },
        );
        None
    }
    pub fn collect<'a>(&self, nodes: impl IntoIterator<Item = &'a Node>) -> Images {
        nodes
            .into_iter()
            .take(CAPACITY)
            .filter_map(|node| self.get(node).map(|image| (node.id, image)))
            .collect()
    }
}
impl Drop for Cache {
    fn drop(&mut self) {
        self.alive.store(false, Ordering::Release);
        // Do not join a worker that could be inside an unresponsive Shell extension.
        self.channel.take();
    }
}

struct Dib {
    dc: HDC,
    bitmap: HBITMAP,
    old: HGDIOBJ,
    pixels: *mut u8,
}
impl Dib {
    unsafe fn new(background: u8) -> Option<Self> {
        let dc = CreateCompatibleDC(null_mut());
        if dc.is_null() {
            return None;
        }
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: SIZE as i32,
                biHeight: -(SIZE as i32),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB,
                ..zeroed()
            },
            ..zeroed()
        };
        let mut pixels = null_mut();
        let bitmap = CreateDIBSection(dc, &info, DIB_RGB_COLORS, &mut pixels, null_mut(), 0);
        if bitmap.is_null() || pixels.is_null() {
            if !bitmap.is_null() {
                DeleteObject(bitmap);
            }
            DeleteDC(dc);
            return None;
        }
        let old = SelectObject(dc, bitmap);
        if old.is_null() || old as isize == -1 {
            DeleteObject(bitmap);
            DeleteDC(dc);
            return None;
        }
        let pixels = pixels.cast::<u8>();
        std::slice::from_raw_parts_mut(pixels, SIZE * SIZE * 4).fill(background);
        Some(Self {
            dc,
            bitmap,
            old,
            pixels,
        })
    }
    unsafe fn bytes(&self) -> &[u8] {
        std::slice::from_raw_parts(self.pixels, SIZE * SIZE * 4)
    }
}
impl Drop for Dib {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.old);
            DeleteObject(self.bitmap);
            DeleteDC(self.dc);
        }
    }
}
struct Icon(HICON);
impl Drop for Icon {
    fn drop(&mut self) {
        unsafe {
            DestroyIcon(self.0);
        }
    }
}
unsafe fn icon_pixels(icon: Icon) -> Option<Pixels> {
    // Infer premultiplied alpha from black/white composites, handling legacy
    // AND-mask icons as well as modern alpha icons without opaque rectangles.
    let black = Dib::new(0)?;
    let white = Dib::new(255)?;
    for dc in [black.dc, white.dc] {
        if DrawIconEx(
            dc,
            0,
            0,
            icon.0,
            SIZE as i32,
            SIZE as i32,
            0,
            null_mut(),
            DI_NORMAL,
        ) == 0
        {
            return None;
        }
    }
    GdiFlush();
    let mut bytes = vec![0; SIZE * SIZE * 4];
    for ((b, w), out) in black
        .bytes()
        .chunks_exact(4)
        .zip(white.bytes().chunks_exact(4))
        .zip(bytes.chunks_exact_mut(4))
    {
        let difference = (0..3).map(|i| w[i].saturating_sub(b[i])).max().unwrap();
        let alpha = 255 - difference;
        for i in 0..3 {
            out[i] = b[i].min(alpha);
        }
        out[3] = alpha;
    }
    Some(Pixels { bytes })
}
unsafe fn shell_icon(path: &Path, association: bool) -> Option<Pixels> {
    let mut info: SHFILEINFOW = zeroed();
    let flags = SHGFI_ICON
        | SHGFI_LARGEICON
        | if association {
            SHGFI_USEFILEATTRIBUTES
        } else {
            0
        };
    if SHGetFileInfoW(
        wide(path).as_ptr(),
        FILE_ATTRIBUTE_NORMAL,
        &mut info,
        size_of::<SHFILEINFOW>() as u32,
        flags,
    ) == 0
        || info.hIcon.is_null()
    {
        return None;
    }
    icon_pixels(Icon(info.hIcon))
}
struct Image(*mut GpImage);
impl Drop for Image {
    fn drop(&mut self) {
        unsafe {
            GdipDisposeImage(self.0);
        }
    }
}
struct Graphics(*mut GpGraphics);
impl Drop for Graphics {
    fn drop(&mut self) {
        unsafe {
            GdipDeleteGraphics(self.0);
        }
    }
}
unsafe fn custom(path: &Path, gdiplus: bool) -> Option<Pixels> {
    if std::fs::metadata(path).ok()?.len() > crate::icon_assets::MAX_BYTES {
        return None;
    }
    if path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("ico"))
    {
        let icon = LoadImageW(
            null_mut(),
            wide(path).as_ptr(),
            IMAGE_ICON,
            SIZE as i32,
            SIZE as i32,
            LR_LOADFROMFILE,
        );
        return if icon.is_null() {
            None
        } else {
            icon_pixels(Icon(icon.cast()))
        };
    }
    if !gdiplus {
        return None;
    }
    let mut bitmap = null_mut();
    if GdipCreateBitmapFromFile(wide(path).as_ptr(), &mut bitmap) != Ok {
        return None;
    }
    let input = Image(bitmap.cast());
    let (mut width, mut height) = (0, 0);
    if GdipGetImageWidth(input.0, &mut width) != Ok
        || GdipGetImageHeight(input.0, &mut height) != Ok
        || width == 0
        || height == 0
        || width > 4096
        || height > 4096
    {
        return None;
    }
    let mut bytes = vec![0; SIZE * SIZE * 4];
    let mut output = null_mut();
    if GdipCreateBitmapFromScan0(
        SIZE as i32,
        SIZE as i32,
        (SIZE * 4) as i32,
        PARGB,
        bytes.as_mut_ptr(),
        &mut output,
    ) != Ok
    {
        return None;
    }
    let output = Image(output.cast());
    let mut graphics = null_mut();
    if GdipGetImageGraphicsContext(output.0, &mut graphics) != Ok {
        return None;
    }
    let graphics = Graphics(graphics);
    GdipSetInterpolationMode(graphics.0, InterpolationModeHighQualityBicubic);
    GdipGraphicsClear(graphics.0, 0);
    let scale = SIZE as f64 / width.max(height) as f64;
    let (w, h) = (
        (width as f64 * scale).round() as i32,
        (height as f64 * scale).round() as i32,
    );
    if GdipDrawImageRectI(
        graphics.0,
        input.0,
        (SIZE as i32 - w) / 2,
        (SIZE as i32 - h) / 2,
        w.max(1),
        h.max(1),
    ) != Ok
    {
        return None;
    }
    GdipFlush(graphics.0, FlushIntentionSync);
    Some(Pixels { bytes })
}
pub(super) unsafe fn load(root: &Path, key: &Source, gdiplus: bool) -> Option<Pixels> {
    match key {
        Source::Custom(path) => custom(&storage::resolve(root, path), gdiplus),
        Source::Association(ext) => shell_icon(Path::new(&format!("item{ext}")), true),
        Source::File(path) => {
            let path = storage::resolve(root, path);
            let value = path.to_string_lossy();
            let network = value.starts_with("\\\\") || value.starts_with("//");
            if !network && !path.exists() {
                return None;
            }
            shell_icon(&path, network)
        }
        Source::Program(path) => {
            let explicit = path.contains(['\\', '/']) || Path::new(path).is_absolute();
            if explicit {
                return load(root, &Source::File(path.clone()), gdiplus);
            }
            let local = root.join(path);
            if local.is_file() {
                return shell_icon(&local, false);
            }
            let mut result = [0u16; 32768];
            let len = SearchPathW(
                null(),
                wide(path).as_ptr(),
                wide(".exe").as_ptr(),
                result.len() as u32,
                result.as_mut_ptr(),
                null_mut(),
            );
            if len == 0 || len as usize >= result.len() {
                return None;
            }
            shell_icon(
                Path::new(&String::from_utf16_lossy(&result[..len as usize])),
                false,
            )
        }
    }
}

pub unsafe fn draw_gdi(dc: HDC, image: &Pixels, x: i32, y: i32, size: i32) {
    if size < 1 {
        return;
    }
    if let Some(source) = Dib::new(0) {
        std::ptr::copy_nonoverlapping(image.bytes.as_ptr(), source.pixels, image.bytes.len());
        GdiAlphaBlend(
            dc,
            x,
            y,
            size,
            size,
            source.dc,
            0,
            0,
            SIZE as i32,
            SIZE as i32,
            BLENDFUNCTION {
                BlendOp: AC_SRC_OVER as u8,
                BlendFlags: 0,
                SourceConstantAlpha: 255,
                AlphaFormat: AC_SRC_ALPHA as u8,
            },
        );
    }
}

#[cfg(test)]
mod cache_tests {
    use super::*;
    fn node(id: u64) -> Node {
        Node {
            id,
            visible_in: vec![],
            name: format!("App {id}"),
            children: None,
            target: Some(Target::Program {
                path: format!("app{id}.exe"),
                args: vec![],
                cwd: None,
            }),
            note: String::new(),
            icon: ItemIcon::Automatic,
        }
    }
    #[test]
    fn software_icon_source_is_independent_of_script_target() {
        let mut item = node(1);
        item.target = Some(Target::Script {
            path: "instances/hydrus-one.cmd".into(),
            runner: None,
            args: vec![],
            cwd: None,
        });
        item.icon = ItemIcon::Software {
            path: "software/hydrus_client.exe".into(),
        };
        assert!(
            matches!(source(&item), Some(Source::Program(path)) if path == "software/hydrus_client.exe")
        );
        item.icon = ItemIcon::Automatic;
        assert!(
            matches!(source(&item), Some(Source::File(path)) if path == "instances/hydrus-one.cmd")
        );
    }
    #[test]
    fn redraws_do_not_repeat_pending_or_failed_shell_requests() {
        let (tx, rx) = sync_channel(CAPACITY);
        let cache = Cache {
            state: Arc::new(Mutex::new(State::default())),
            channel: Some(tx),
            alive: Arc::new(AtomicBool::new(true)),
            notified: Arc::new(AtomicBool::new(false)),
        };
        let mut item = node(1);
        assert!(cache.collect([&item]).is_empty());
        assert!(rx.try_recv().is_ok());
        for _ in 0..10 {
            cache.collect([&item]);
        }
        assert!(rx.try_recv().is_err());
        cache
            .state
            .lock()
            .unwrap()
            .entries
            .values_mut()
            .for_each(|entry| entry.pending = false);
        assert!(cache.failed(&item));
        cache.collect([&item]);
        assert!(rx.try_recv().is_err());
        item.icon = ItemIcon::Symbol;
        cache.collect([&item]);
        assert!(rx.try_recv().is_err());
    }
    #[test]
    fn oversized_preview_has_a_stable_bounded_working_set() {
        let (tx, rx) = sync_channel(CAPACITY);
        let cache = Cache {
            state: Arc::new(Mutex::new(State::default())),
            channel: Some(tx),
            alive: Arc::new(AtomicBool::new(true)),
            notified: Arc::new(AtomicBool::new(false)),
        };
        let items: Vec<_> = (1..1000).map(node).collect();
        cache.collect(&items);
        assert_eq!(rx.try_iter().count(), CAPACITY);
        let image = Arc::new(Pixels {
            bytes: vec![0; SIZE * SIZE * 4],
        });
        for entry in cache.state.lock().unwrap().entries.values_mut() {
            entry.pending = false;
            entry.image = Some(image.clone());
        }
        for _ in 0..5 {
            assert_eq!(cache.collect(&items).len(), CAPACITY);
        }
        assert!(
            rx.try_recv().is_err(),
            "A large preview must not continuously evict and reload icons"
        );
        assert_eq!(cache.state.lock().unwrap().entries.len(), CAPACITY);
    }
}
